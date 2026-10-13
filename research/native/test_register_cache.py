"""Public synthetic proofs of cache boundaries; no game data required."""
import unittest
import os
import subprocess
import tempfile
from pathlib import Path
from unittest.mock import patch

import register_cache as rc


class RegisterCacheTests(unittest.TestCase):
    def test_switch_and_restartable_steps(self):
        import lift
        arms = ['0x1000 => { eax = m.u32(esp); fl = Flags::new(x86::LOGIC, eax, eax, eax, 32); return; }']
        with patch.dict(os.environ, {'LIFT_REG_LOCALS': '0'}):
            baseline = lift.rust_fn(0x1000, 'synthetic', arms)
            step = lift.rust_step(0x1000, 'synthetic', arms)
        with patch.dict(os.environ, {'LIFT_REG_LOCALS': '1'}):
            self.assertEqual(lift.rust_fn(0x1000, 'synthetic', arms), rc.cache_functions(baseline))
            self.assertEqual(lift.rust_step(0x1000, 'synthetic', arms), step)
        with patch.dict(os.environ):
            os.environ.pop('LIFT_REG_LOCALS', None)
            self.assertEqual(lift.rust_fn(0x1000, 'synthetic', arms), baseline)
        self.assertIn('let mut l161_fl = cpu.fl;', rc.cache_functions(baseline))

    def test_helpers_flush_flags(self):
        for helper in ('x86::pushfd_image(cpu)', 'x86::popfd(cpu, v)',
                       'x86::string(m, cpu, 4, true, true)', 'crate::platform::cpuid(cpu)'):
            out = rc.cache_body(' cpu.fl = Flags::default(); cpu.eax = 7; { let v = 0; '+helper+'; } return;')
            self.assertIn('cpu.fl = l161_fl; let l161_value = '+helper, out)
            self.assertIn('l161_fl = cpu.fl; l161_value', out)

    def test_original_templates_calls_flags_memory_and_split_opt012(self):
        import lift
        import lean_emission as le
        import rust_base
        from test_lean_emission import FIXTURE, alu, push, pop
        arms = ['0x1000 => { '+push('esp')+alu('add','eax','ecx')+
                'pc = if fl.cond("e") { 0x1020 } else { 0x1010 }; continue; }',
                '0x1010 => { '+alu('xor','edi','eax')+
                'x86::call_ext(m, cpu, 0x402000); pc = 0x1020; continue; }',
                '0x1020 => { '+alu('sub','edx','eax')+pop('ebp')+
                'esp = esp.wrapping_add(4); '
                '{ let v = x86::pushfd_image(cpu); x86::popfd(cpu, v); } '
                'ecx = 3; esi = 0x2000; edi = 0x3000; '
                'x86::string(m, cpu, 4, true, true); '
                'x86::string_from(m, cpu, 1, true, false, 0x10); '
                'x86::scan(m, cpu, 4, 0); eax = ecx ^ esi ^ edi; return; }']
        with patch.dict(os.environ, {'LIFT_REG_LOCALS':'0'}):
            plain = lift.rust_fn(0x1000,'synthetic',arms)
        with patch.dict(os.environ, {'LIFT_REG_LOCALS':'1'}):
            lean = lift.rust_fn(0x1000,'synthetic',arms)
            with patch.object(le,'SPLIT_BYTES',200):
                split = lift.rust_fn(0x1000,'synthetic',arms).replace('f_00001000','g_00001000')
        self.assertIn('l161_fl',lean)
        self.assertIn('_p0(',split)
        root = Path(__file__).resolve().parents[2]
        source = '#![allow(warnings)]\n'+rust_base.modules(root)+'\nuse mem::Mem; use x86::{Cpu,Flags};\n'
        fixture = FIXTURE.replace('c.esp = c.esp.wrapping_add(4);',
                                  'c.esp = c.esp.wrapping_add(4); c.fl = Flags::partial(c.eax, 0x8d5); c.edi ^= c.eax;')
        source += f'mod plain {{ use super::*; {plain} }} mod lean {{ use super::*; {lean} }} mod split {{ use super::*; {split} }}'+fixture
        with tempfile.TemporaryDirectory(dir=os.environ['CARGO_TARGET_DIR'],prefix='reg-locals-') as directory:
            src,exe=Path(directory)/'test.rs',Path(directory)/'test.exe'
            src.write_text(source)
            for opt in (0,1,2):
                for command in (['rustc','--edition=2021','--test',f'-Copt-level={opt}',str(src),'-o',str(exe)], [str(exe),'--test-threads=1']):
                    r=subprocess.run(command,capture_output=True,text=True,timeout=60)
                    self.assertEqual(r.returncode,0,r.stdout+r.stderr)

    def test_ordered_faults_and_panicking_cpu_boundary_opt012(self):
        import rust_base
        body = ('let pt=m.page_table(); cpu.eax=cpu.eax.wrapping_add(1); '
                'emit::push(cpu.esp,m,pt,cpu); '
                'cpu.ecx=m.fast_u32(pt,cpu.esp); cpu.edx=emit::pop(m,pt,cpu); '
                'cpu.fl=Flags::new(x86::ADD,cpu.eax,cpu.ecx,cpu.eax.wrapping_add(cpu.ecx),32); '
                'x86::call_ext(m, cpu, 0x402000); cpu.edi=cpu.eax; return;')
        baseline='fn f_00001000(m:&mut dyn Mem,cpu:&mut Cpu) {'+body+'}'
        candidate=rc.cache_functions(baseline).replace('f_00001000','f_00002000')
        self.assertIn('l161_fl',candidate)
        root=Path(__file__).resolve().parents[2]
        source='#![allow(warnings)]\n'+rust_base.modules(root)+'\nuse mem::Mem; use x86::{Cpu,Flags};\n'+baseline+candidate+r'''
struct Faults { events:std::cell::RefCell<Vec<(bool,u32,u8)>>, fail:usize }
impl Mem for Faults {
 fn u8(&self,a:u32)->u8 {
  let v=a as u8; let mut e=self.events.borrow_mut(); e.push((false,a,v));
  if e.len()==self.fail {panic!("read fault")}; v
 }
 fn set_u8(&mut self,a:u32,v:u8) {
  let mut e=self.events.borrow_mut(); e.push((true,a,v));
  if e.len()==self.fail {panic!("write fault")};
 }
}
#[test] fn faults_and_calls() {
 std::panic::set_hook(Box::new(|_|{}));
 for fail in 0..14 {for trap in [false,true] {for initial in [0,1,0x7fffffff,0xffffffff] {
  let run=|f:x86::LiftFn| {
   let calls=std::rc::Rc::new(std::cell::RefCell::new(Vec::new())); let log=calls.clone();
   x86::set_ext_calls(Some(Box::new(move |_,c,_| {
    log.borrow_mut().push(format!("{:?}",c)); c.eax^=123; c.fl=Flags::partial(1,0x8d5);
    if trap {panic!("callee trap")}
   })));
   let mut m=Faults {events:Default::default(),fail}; let mut c=Cpu::default();
   c.eax=initial; c.esp=0x8000;
   let panic=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||f(&mut m,&mut c))).is_err();
   x86::set_ext_calls(None);
   let call_state=calls.borrow().clone();
   // Whole-function memory faults are not restart checkpoints. CPU state
   // must match at successful exits and at CPU-taking callee unwinds.
   let state=if !panic || !call_state.is_empty() {Some(format!("{:?}",c))} else {None};
   (panic,m.events.into_inner(),call_state,state)
  };
  assert_eq!(run(f_00001000),run(f_00002000));
 }}}
}
'''
        with tempfile.TemporaryDirectory(dir=os.environ['CARGO_TARGET_DIR'],prefix='reg-faults-') as directory:
            src,exe=Path(directory)/'test.rs',Path(directory)/'test.exe'
            src.write_text(source)
            for opt in (0,1,2):
                for command in (['rustc','--edition=2021','--test',f'-Copt-level={opt}',str(src),'-o',str(exe)], [str(exe),'--test-threads=1']):
                    r=subprocess.run(command,capture_output=True,text=True,timeout=60)
                    self.assertEqual(r.returncode,0,r.stdout+r.stderr)

    def test_leaf_and_exit(self):
        out = rc.cache_body(' cpu.eax = m.fast_u32(pt, cpu.esp); return; ')
        self.assertIn('let mut l161_eax = cpu.eax;', out)
        self.assertIn('l161_eax = m.fast_u32(pt, l161_esp);', out)
        self.assertIn('cpu.eax = l161_eax; cpu.esp = l161_esp; return }', out)

    def test_call_flushes_arguments_and_reloads_outputs(self):
        out = rc.cache_body(' cpu.eax = 4; x86::call_ext(m, cpu, cpu.eax); cpu.edx = cpu.eax; return; ')
        self.assertIn('cpu.eax = l161_eax; cpu.edx = l161_edx; x86::call_ext(m, cpu, cpu.eax); '
                      'l161_eax = cpu.eax; l161_edx = cpu.edx;', out)
        self.assertIn('l161_edx = l161_eax;', out)

    def test_more_than_ten_boundaries(self):
        out = rc.cache_body(' cpu.eax = 1; ' + 'x86::call_ext(m, cpu, 1); ' * 12 + 'return;')
        self.assertNotIn('L161_BOUNDARY', out)
        self.assertEqual(out.count('x86::call_ext(m, cpu, 1)'), 12)

    def test_unknown_cpu_observer_rejects(self):
        for text in (' cpu.eax = 1; unknown(cpu); return;',
                     ' cpu.eax = 1; let v = cpu.exit(); return;',
                     ' cpu.eax = 1; x86::popfd(cpu, x86::pushfd_image(cpu)); return;',
                     ' cpu.eax = 1; let v = x86::call_ext(m, cpu, 1); return;',
                     ' cpu.eax = 1; foo(x86::call_ext(m, cpu, 1)); return;'):
            self.assertEqual(rc.cache_body(text), text)

    def test_unknown_return_rejects(self):
        text = ' cpu.eax = 1; return Some(next);'
        self.assertEqual(rc.cache_body(text), text)

    def test_strings_are_not_state(self):
        out = rc.cache_body(' cpu.eax = 1; let s = "cpu unknown(cpu) return;"; return;')
        self.assertIn('"cpu unknown(cpu) return;"', out)

    def test_push_pop_esp_order(self):
        out = rc.cache_body(' emit::push(cpu.esp, m, pt, cpu); cpu.esp = emit::pop(m, pt, cpu); return;')
        self.assertIn('let l161_push = l161_esp; l161_esp = l161_esp.wrapping_sub(4);', out)
        self.assertIn('l161_esp = { let l161_pop = m.fast_u32(pt, l161_esp); '
                      'l161_esp = l161_esp.wrapping_add(4); l161_pop }', out)

    def test_split_part_exits(self):
        source = ('fn f_00001000_p0(m: &mut dyn Mem, cpu: &mut Cpu) -> Option<u32> {\n'
                  ' cpu.eax = 1; if cpu.edx == 0 { return None; } return Some(pc);\n}\n')
        out = rc.cache_functions(source)
        self.assertEqual(out.count('cpu.eax = l161_eax;'), 2)
        self.assertIn('let mut l161_eax', out)

    def test_match_arm_return_with_comma(self):
        out = rc.cache_body(' cpu.eax = 1; match pc { _ => return Some(pc), }')
        self.assertIn('_ => { cpu.eax = l161_eax; return Some(pc) },', out)


if __name__ == '__main__':
    unittest.main()
