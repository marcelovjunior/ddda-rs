"""Synthetic codegen regressions, without private game data.

The runtime fixture compiles only mem.rs, x86.rs and synthetic functions
with rustc; it does not regenerate or rebuild the private lifted batch.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

import lift


class CodegenTests(unittest.TestCase):
    def test_whole_identifiers_and_strings(self):
        self.assertEqual(lift.cpu_arm('eax = edx; xmm7[0] = xmm0[1]; fl.cf_in = true; '
                                      'st.push(0); let eax_extra = "eax st \\\"fl";'),
                         'cpu.eax = cpu.edx; cpu.xmm[7][0] = cpu.xmm[0][1]; cpu.fl.cf_in = true; '
                         'cpu.st.push(0); let eax_extra = "eax st \\\"fl";')

    def test_save_load_lowering_keeps_calls_and_cache(self):
        arm = f'0x1000 => {{ {lift.SAVE} super::lifted::f_00002000(m, cpu); {lift.LOAD} return; }}'
        before = [arm]
        rust = lift.rust_fn(0x1000, 'synthetic', before)
        self.assertEqual(before, [arm])
        self.assertEqual(lift.callees(before), [0x2000])
        self.assertIn('super::lifted::f_00002000(m, cpu);', rust)
        self.assertNotIn('std::mem::take', rust)
        self.assertNotIn('*cpu =', rust)
        legacy = [arm.replace(', df: cpu.df', '').replace(', df: _', '')]
        legacy_rust = lift.rust_fn(0x1000, 'pre-DF cache', legacy)
        self.assertNotIn('std::mem::take', legacy_rust)
        self.assertNotIn('*cpu =', legacy_rust)

    def test_l60_calls_go_through_overridable_dispatch(self):
        # in-shard: call_local (a bit test); cross-shard: call_lifted; a
        # platform model: call_local at the model's address
        import harness as h
        model, name = next(iter(sorted(h.PLATFORM.items())))
        same, other = 0x00400010, 0x00400000 + (1 << lift.SHARD_SHIFT)
        arm = (f'0x400000 => {{ super::lifted::f_{same:08x}(m, cpu); super::lifted::f_{other:08x}(m, cpu); '
               f'crate::platform::{name}(m, cpu); return; }}')
        rust = lift.shard_fn(dict(addr=0x400000, name='synthetic', arms=[arm]))
        self.assertIn(f'x86::call_local(m, cpu, 0x{same:08X}, super::lifted::f_{same:08x}))(m, cpu)', rust)
        self.assertIn(f'x86::call_lifted(m, cpu, 0x{other:08X}))(m, cpu)', rust)
        self.assertIn(f'x86::call_local(m, cpu, 0x{model:08X}, crate::platform::{name})', rust)
        self.assertNotIn(f'super::lifted::f_{same:08x}(m, cpu)', rust)

    def test_inc_dec_carry_cf_lazily(self):
        # L46: the cached template reads CF; after DIV it is undefined
        arm = ("{ let a = eax & u32::MAX; let r = a.wrapping_add(1) & u32::MAX; let cf = fl.cf(); "
               "fl = Flags::new(x86::INC, a, 1, r, 32); fl.cf_in = cf; eax = r; }")
        out = lift.cpu_arm(arm, start=0x1000)
        self.assertIn("cpu.fl.inc_dec(x86::INC, a, r, 32);", out)
        self.assertNotIn("cf()", out)

    def test_binary_templates_and_conservative_fallback(self):
        for bits, mask in ((8, '0xFF'), (16, '0xFFFF'), (32, 'u32::MAX')):
            for op, expression, kind in (
                    ('add', f'(a.wrapping_add(c) & {mask})', 'ADD'),
                    ('sub', f'(a.wrapping_sub(c) & {mask})', 'SUB'),
                    ('and', f'((a & c) & {mask})', 'LOGIC'),
                    ('or', f'((a | c) & {mask})', 'LOGIC'),
                    ('xor', f'((a ^ c) & {mask})', 'LOGIC')):
                arm = (f'{{ let a = m.u32(esp) & {mask}; let c = eax & {mask}; '
                       f'let r = {expression}; fl = Flags::new(x86::{kind}, a, c, r, {bits}); '
                       'm.set_u32(esp, r); }')
                self.assertEqual(lift.compact_alu(arm),
                                 f'{{ let r = fl.alu_{op}(m.u32(esp), eax, {bits}); m.set_u32(esp, r); }}')
                # A changed formula or an inconsistent width must not be
                # silently treated as one of the supported templates.
                for bad in (arm.replace('let r = ', 'let r = 1 + '),
                            arm.replace(f'r, {bits})', 'r, 7)')):
                    self.assertEqual(lift.compact_alu(bad), bad)

    def test_straight_line_requires_entry_and_no_dispatch_control(self):
        arm = f'0x1000 => {{ eax = 7; {lift.SAVE} return; }}'
        self.assertNotIn('match pc', lift.rust_fn(0x1000, 'synthetic', [arm]))
        for arms in ([arm.replace('0x1000', '0x2000')],
                     ['0x1000 => { pc = 0x1000; continue; }'],
                     ['0x1000 => { if eax == 0 { continue; } return; }'],
                     ['0x1000 => { eax = 7; }'], [arm, arm]):
            self.assertIn('match pc', lift.rust_fn(0x1000, 'synthetic', arms))

    def test_runtime_matches_local_register_model(self):
        # Exercises operand-before-push ordering, full state through both
        # normal and tail calls, condition flags, SSE lanes and x87 state.
        arms = [f'''0x1000 => {{
            {{ let t = m.u32(esp); esp = esp.wrapping_sub(4); m.set_u32(esp, 0x1005);
               {lift.SAVE} x86::call_ext(m, cpu, t); {lift.LOAD} }}
            {{ let a = m.u32(128) & 0xFFFF; let c = ecx & 0xFFFF;
               let r = (a.wrapping_sub(c) & 0xFFFF);
               fl = Flags::new(x86::SUB, a, c, r, 16); m.set_u16(132, r as u16); }}
            {{ let a = edx & 0xFF; let c = (m.u8(140) as u32) & 0xFF;
               let r = ((a ^ c) & 0xFF);
               fl = Flags::new(x86::LOGIC, a, c, r, 8); edx = (edx & !0xFF) | (r & 0xFF); }}
            {{ let a = eax & u32::MAX; let c = edx & u32::MAX;
               let r = (a.wrapping_add(c) & u32::MAX);
               fl = Flags::new(x86::ADD, a, c, r, 32); eax = r; }}
            xmm7 = [eax, xmm0[1], xmm2[2], xmm7[3]];
            st.push(eax); {{ let v = st.top_f32().unwrap(); st.register::<0xd8dd>(); m.set_u32(128, v); }}
            if fl.cond("b") {{ {lift.SAVE} callee(m, cpu); return; }}
            esp = esp.wrapping_sub(4); m.set_u32(esp, 0x1009);
            {lift.SAVE} callee(m, cpu); {lift.LOAD}
            edx = fl.cond("e") as u32; esp = esp.wrapping_add(4);
            {lift.SAVE} return;
        }}''']
        reference = '''fn f_00001000(m: &mut dyn Mem, cpu: &mut Cpu) {
            let Cpu { mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi,
                xmm: [mut xmm0, mut xmm1, mut xmm2, mut xmm3, mut xmm4, mut xmm5, mut xmm6, mut xmm7],
                mut fl, mut st, mut fs_base, mut seg, mut sys_flags, mut mxcsr, .. } = std::mem::take(cpu);
            let pc = 0x1000; loop { match pc { ARMS _ => unreachable!() } }
        }'''.replace('ARMS', '\n'.join(arms).replace(lift.SAVE, lift.SAVE.replace('df: cpu.df', 'df: cpu.df, fs_base, seg, sys_flags, mxcsr'))
                    .replace(lift.LOAD, lift.LOAD.replace('df: _', 'df: _, fs_base, seg, sys_flags, mxcsr')))
        generated = lift.rust_fn(0x1000, 'synthetic', arms)
        root = Path(__file__).resolve().parents[2]
        source = f'''#![allow(warnings)]
            #[path = "{(root / 'crates/ddnative-base/src/mem.rs').as_posix()}"] mod mem;
            #[path = "{(root / 'crates/ddnative-base/src/x86.rs').as_posix()}"] mod x86;
            #[path = "{(root / 'crates/ddnative-base/src/x87.rs').as_posix()}"] mod x87;
            use mem::Mem; use x86::{{Cpu, Flags}};
            mod reference {{ use super::*; pub(super) fn run(m: &mut dyn Mem, c: &mut Cpu) {{ f_00001000(m,c); }} {reference} }}
            mod generated {{ use super::*; {generated} }}
        ''' + r'''
            #[derive(Default, Debug)] struct Trace {
                bytes: std::collections::BTreeMap<u32, u8>,
                events: std::cell::RefCell<Vec<(bool, u32, u8)>>,
            }
            impl Mem for Trace {
                fn u8(&self, a: u32) -> u8 {
                    let v = self.bytes.get(&a).copied().unwrap_or(a as u8);
                    self.events.borrow_mut().push((false,a,v)); v
                }
                fn set_u8(&mut self, a: u32, v: u8) {
                    self.events.borrow_mut().push((true,a,v)); self.bytes.insert(a,v);
                }
            }
            fn callee(m: &mut dyn Mem, c: &mut Cpu) {
                let ret = m.u32(c.esp);
                m.set_u32(180, ret ^ c.xmm[7][3] ^ c.st.top_f32().unwrap());
                c.fl = Flags::new(x86::SUB, c.eax, c.edx, c.eax.wrapping_sub(c.edx), 32);
                c.eax = c.eax.wrapping_add(7); c.xmm[3][2] ^= c.ebx;
                c.st.push(c.esi); c.esp = c.esp.wrapping_add(4);
                c.fs_base = c.fs_base.wrapping_add(0x1000);
            }
            #[test] fn same_state_and_ordered_memory_events() {
                for value in [0u32, 1, 0x7fffffff, 0x80000000, 0xffffffff] {
                    let mut initial = Cpu::default();
                    initial.eax = value; initial.edx = value; initial.esp = 64;
                    initial.ebx = 11; initial.esi = 13; initial.edi = 17; initial.fs_base = 0x7ffde000;
                    initial.fl = Flags::new(x86::SUB, 0, 1, u32::MAX, 32);
                    initial.st.push(31); initial.st.push(37);
                    for i in 0..8 { for j in 0..4 { initial.xmm[i][j] = (i*4+j) as u32; } }
                    let mut results = Vec::new();
                    for run in [reference::run as fn(&mut dyn Mem, &mut Cpu), generated::f_00001000] {
                        let snapshots = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                        let log = snapshots.clone();
                        x86::set_ext_calls(Some(Box::new(move |m,c,t| {
                            log.borrow_mut().push((t, format!("{:?}",c)));
                            callee(m,c);
                            c.ecx ^= 0xdeadbeef; c.edx = value;
                        })));
                        let mut c = initial.clone(); let mut m = Trace::default();
                        run(&mut m,&mut c);
                        x86::set_ext_calls(None);
                        results.push((format!("{:?}",c), m.bytes, m.events.into_inner(), snapshots.borrow().clone()));
                    }
                    assert_eq!(results[0], results[1]);
                }
            }
        '''
        target = root / 'target'
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='codegen-test-', dir=target) as tmp:
            src = Path(tmp) / 'fixture.rs'
            exe = Path(tmp) / 'fixture.exe'
            src.write_text(source)
            for command in (['rustc', '--edition=2021', '--test', str(src), '-o', str(exe)], [str(exe)]):
                result = subprocess.run(command, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_memory_fast_path_matches_the_trait(self):
        # Lowered accessors (m.fast_u32(pt, ..)) with a page table give the
        # same state and memory as through the trait (pt None): accesses
        # within a page, page-crossing ones, unbacked pages, stack pushes
        # and a store whose value is a load (two-phase borrow).
        self.assertEqual(lift.fast_memory('m.set_u32(a, m.u32(b)); let s = "m.u8(";'),
                         'm.fast_set_u32(pt, a, m.fast_u32(pt, b)); let s = "m.u8(";')
        arms = [f'''0x1000 => {{
            m.set_u32(0xFFE, eax); m.set_u16(0x2FFF, ecx as u16); m.set_u8(0x3000, 7);
            eax = m.u32(0xFFC) ^ m.u32(0x1FFE) ^ (m.u16(0x2FFF) as u32) ^ (m.u8(0x3000) as u32);
            {{ let v = m.u32(esp); esp = esp.wrapping_sub(4); m.set_u32(esp, v ^ 0x55); }}
            m.set_u32(0x4000, m.u32(0xFFE)); edx = m.u32(0x5000);
            {lift.SAVE} return;
        }}''']
        generated = lift.rust_fn(0x1000, 'synthetic', arms)
        self.assertIn('let pt = m.page_table();', generated)
        self.assertNotIn('m.u32(', generated)
        root = Path(__file__).resolve().parents[2]
        source = f'''#![allow(warnings)]
            #[path = "{(root / 'crates/ddnative-base/src/mem.rs').as_posix()}"] mod mem;
            #[path = "{(root / 'crates/ddnative-base/src/x86.rs').as_posix()}"] mod x86;
            #[path = "{(root / 'crates/ddnative-base/src/x87.rs').as_posix()}"] mod x87;
            use mem::Mem; use x86::{{Cpu, Flags}};
            mod generated {{ use super::*; {generated} }}
        ''' + r'''
            struct Paged { t: Box<mem::PageTable>, fast: bool, calls: std::cell::Cell<u32> }
            impl Paged {
                fn new(fast: bool) -> Paged {
                    let t = mem::PageTable::default();
                    for p in 0..mem::PAGES {
                        t.set_perm(p, mem::READ | mem::WRITE);
                    }
                    Paged { t: Box::new(t), fast, calls: std::cell::Cell::new(0) }
                }
                fn page(&self, p: usize) -> Vec<u8> {
                    let page = self.t.page(p);
                    if page.is_null() { return Vec::new(); }
                    (0..mem::PAGE as usize).map(|k| unsafe { mem::load_at(page.add(k), 1) as u8 }).collect()
                }
            }
            impl Mem for Paged {
                fn u8(&self, a: u32) -> u8 {
                    self.calls.set(self.calls.get() + 1);
                    let page = self.t.page((a >> 12) as usize);
                    if page.is_null() { 0 } else { unsafe { mem::load_at(page.add((a & 0xFFF) as usize), 1) as u8 } }
                }
                fn set_u8(&mut self, a: u32, v: u8) {
                    self.calls.set(self.calls.get() + 1);
                    let page = self.t.backed_page((a >> 12) as usize);
                    unsafe { mem::store_at(page.add((a & 0xFFF) as usize), 1, v as u32) }
                }
                fn page_table(&self) -> Option<std::ptr::NonNull<mem::PageTable>> {
                    if self.fast { Some(std::ptr::NonNull::from(&*self.t)) } else { None }
                }
            }
            #[test] fn fast_and_trait_paths_agree() {
                for value in [0u32, 1, 0x7fffffff, 0x80000000, 0xffffffff] {
                    let mut results = Vec::new();
                    let mut calls = Vec::new();
                    for fast in [false, true] {
                        let mut m = Paged::new(fast);
                        m.set_u32(64, value ^ 0x1234);
                        m.set_u32(0x1FFC, value.rotate_left(7));
                        let mut c = Cpu::default();
                        c.eax = value; c.ecx = !value; c.esp = 64;
                        m.calls.set(0);
                        generated::f_00001000(&mut m, &mut c);
                        calls.push(m.calls.get());
                        results.push((format!("{:?}", c), (0..6).map(|p| m.page(p)).collect::<Vec<_>>()));
                    }
                    assert_eq!(results[0], results[1]);
                    assert!(calls[1] < calls[0], "the fast path made {} trait calls, the trait path {}", calls[1], calls[0]);
                }
            }
        '''
        target = root / 'target'
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='codegen-test-', dir=target) as tmp:
            src = Path(tmp) / 'fixture.rs'
            exe = Path(tmp) / 'fixture.exe'
            src.write_text(source)
            for command in (['rustc', '--edition=2021', '-O', '--test', str(src), '-o', str(exe)], [str(exe)]):
                result = subprocess.run(command, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

if __name__ == '__main__':
    unittest.main()
