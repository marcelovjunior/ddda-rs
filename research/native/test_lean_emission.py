"""L73 lean emission: text rules of the dead-flag and idiom passes, and a
rustc differential test (same Cpu, memory and memory-event order with and
without the passes) over lift.py's cached templates. No private data.
"""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

import lean_emission as le
import lift

ADD = "{ let r = cpu.fl.alu_add(cpu.eax, 0x1u32, 32); cpu.eax = r; }"
CMP = "{ let r = cpu.fl.alu_sub(cpu.eax, 0x5u32, 32); }"
JCC = 'pc = if cpu.fl.cond("e") { 0x2000 } else { 0x1010 }; continue;'


class DeadFlags(unittest.TestCase):
    def test_overwritten_write_becomes_plain_arithmetic(self):
        self.assertEqual(le.dead_flags(f"0x1000 => {{ {ADD} {CMP} {JCC} }}"),
                         f"0x1000 => {{ cpu.eax = cpu.eax.wrapping_add(0x1u32); {CMP} {JCC} }}")

    def test_last_write_of_the_arm_stays(self):
        arm = f"0x1000 => {{ {ADD} pc = 0x1010; continue; }}"
        self.assertEqual(le.dead_flags(arm), arm)
        self.assertEqual(le.dead_flags(f"{ADD} return;"), f"{ADD} return;")

    def test_readers_keep_the_write(self):
        for reader in ('cpu.edx = cpu.fl.cond("b") as u32;',
                       "x86::call_ext(m, cpu, 0x401000);",
                       "super::lifted::f_00401000(m, cpu);",
                       "{ let v = x86::pushfd_image(cpu); }",
                       "cpu.eax = (cpu.eax & !0xff00) | ((cpu.fl.lahf() as u32) << 8);",
                       "{ let (a, b) = (cpu.eax, cpu.ebx); let r = cpu.fl.alu_carry(a, b, 32, false); cpu.eax = r; }",
                       'if cpu.fl.cond("e") { return; }'):
            arm = f"0x1000 => {{ {ADD} {reader} {CMP} {JCC} }}"
            self.assertEqual(le.dead_flags(arm), arm, reader)

    def test_conditional_write_does_not_kill_but_can_die(self):
        shift = ("{ let a = cpu.ecx & u32::MAX; let n = cpu.ecx & 31; if n != 0 { let r = (a << n) & u32::MAX; "
                 "cpu.fl = Flags::new(x86::SHL, a, n, r, 32); cpu.ecx = r; } }")
        out = le.dead_flags(f"0x1000 => {{ {ADD} {shift} {CMP} {JCC} }}")
        self.assertIn("cpu.eax = cpu.eax.wrapping_add(0x1u32);", out)  # killed by CMP, not by the shift
        self.assertNotIn("Flags::new(x86::SHL", out)
        self.assertIn("if n != 0 { let r = (a << n) & u32::MAX; cpu.ecx = r; }", out)
        # a conditional write alone never removes the earlier write
        arm = f"0x1000 => {{ {ADD} {shift} {JCC} }}"
        self.assertEqual(le.dead_flags(arm), arm)

    def test_inc_dec_reads_the_carry(self):
        inc = "{ let a = cpu.esi & u32::MAX; let r = a.wrapping_add(1) & u32::MAX; cpu.fl.inc_dec(x86::INC, a, r, 32); cpu.esi = r; }"
        arm = f"0x1000 => {{ {ADD} {inc} {JCC} }}"
        self.assertEqual(le.dead_flags(arm), arm)
        out = le.dead_flags(f"0x1000 => {{ {inc} {CMP} {JCC} }}")
        self.assertIn("let r = a.wrapping_add(1) & u32::MAX; cpu.esi = r; }", out)
        self.assertNotIn("inc_dec", out)

    def test_loop_body_is_a_reader(self):
        loop = "for k in 0..4 { cpu.xmm[0][k] = cpu.fl.r; }"
        arm = f"0x1000 => {{ {ADD} {loop} {CMP} {JCC} }}"
        self.assertEqual(le.dead_flags(arm), arm)

    def test_strings_are_ignored(self):
        arm = f'0x1000 => {{ {ADD} panic!("cpu.fl cpu {{"); }}'
        self.assertEqual(le.dead_flags(arm), arm)

    def test_widths_operators_and_order(self):
        cases = {
            ("add", "8"): "(cpu.eax & 0xFF).wrapping_add(0x1u32) & 0xFF",
            ("sub", "16"): "(cpu.eax & 0xFF).wrapping_sub(0x1u32) & 0xFFFF",
            ("and", "8"): "(cpu.eax & 0xFF) & 0x1u32 & 0xFF",
            ("or", "8"): "((cpu.eax & 0xFF) | 0x1u32) & 0xFF",
            ("xor", "32"): "(cpu.eax & 0xFF) ^ 0x1u32",
        }
        for (op, bits), want in cases.items():
            arm = f"{{ let r = cpu.fl.alu_{op}((cpu.eax & 0xFF), 0x1u32, {bits}); m.fast_set_u8(pt, cpu.ebx, (r) as u8); }} {CMP} {JCC}"
            out = le.dead_flags(arm)
            self.assertTrue(out.startswith(f"{{ let r = {want}; m.fast_set_u8"), (op, bits, out))

    def test_non_postfix_operands_are_parenthesized(self):
        arm = f"{{ let r = cpu.fl.alu_add(m.fast_u32(pt, cpu.esp), (cpu.seg.fs as u32), 32); cpu.ecx = r; }} {CMP} {JCC}"
        self.assertTrue(le.dead_flags(arm).startswith(
            "cpu.ecx = m.fast_u32(pt, cpu.esp).wrapping_add((cpu.seg.fs as u32));"))

    def test_fold_skips_if_blocks(self):
        arm = "if n != 0 { let r = a >> n; cpu.ebx = r; }"
        self.assertEqual(le.fold_results(arm), arm)
        self.assertEqual(le.fold_results("{ let r = a >> n; cpu.ebx = r; }"), "cpu.ebx = a >> n;")


class Idioms(unittest.TestCase):
    def test_direct_predicates(self):
        for codes, expression in le.CONDITIONS.items():
            for cc in codes:
                result = le.predicates(f'cpu.fl.cond("{cc}")')
                self.assertIn(expression.replace('f.', 'cpu.fl.'), result)
        self.assertEqual(le.predicates('cpu.fl.cond("unknown")'), 'cpu.fl.cond("unknown")')

    def test_sse_lane_loop_needs_its_destination_as_first_operand(self):
        arm = "for k in 0..4 { cpu.xmm[1][k] = x86::mulss(cpu.xmm[2][k], cpu.xmm[3][k]); }"
        self.assertEqual(le.idioms(arm), arm)

    def test_push_pop_return_address_and_direct_calls(self):
        arm = ("{ let v = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8)); cpu.esp = cpu.esp.wrapping_sub(4); m.fast_set_u32(pt, cpu.esp, v); } "
               "cpu.esp = cpu.esp.wrapping_sub(4); m.fast_set_u32(pt, cpu.esp, 0x401005);  "
               "(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_lifted(m, cpu, 0x00E60FB0))(m, cpu);  "
               "{ let v = m.fast_u32(pt, cpu.esp); cpu.esp = cpu.esp.wrapping_add(4); cpu.ebp = v; } "
               "{ let v = m.fast_u32(pt, cpu.esp); cpu.esp = cpu.esp.wrapping_add(4); m.fast_set_u32(pt, cpu.esp, v); }")
        self.assertEqual(le.idioms(arm),
                         "emit::push(m.fast_u32(pt, cpu.ebp.wrapping_add(0x8)), m, pt, cpu); "
                         "emit::push(0x401005, m, pt, cpu);  x86::call_lifted(m, cpu, 0x00E60FB0);  "
                         "cpu.ebp = emit::pop(m, pt, cpu); "
                         "{ let v = emit::pop(m, pt, cpu); m.fast_set_u32(pt, cpu.esp, v); }")

    def test_conditional_push_template_untouched(self):
        arm = "if c { let v = cpu.eax; cpu.esp = cpu.esp.wrapping_sub(4); m.fast_set_u32(pt, cpu.esp, v); }"
        self.assertEqual(le.idioms(arm), arm)


def alu(mn, dst, src, bits=32, mem=None):
    """lift.py's cached binary-ALU template (before compact_alu)."""
    mask = {8: "0xFF", 16: "0xFFFF", 32: "u32::MAX"}[bits]
    if mem is None:
        load = {8: f"(({dst} >> 0) & 0xFF)", 16: f"({dst} & 0xFFFF)", 32: dst}[bits]
        store = {8: f"{dst} = ({dst} & !(0xFF << 0)) | (((r) & 0xFF) << 0);",
                 16: f"{dst} = ({dst} & !0xFFFF) | ((r) & 0xFFFF);", 32: f"{dst} = r;"}[bits]
    else:
        load, store = f"m.u32({mem})", f"m.set_u32({mem}, r);"
    op = {"add": "wrapping_add", "sub": "wrapping_sub", "cmp": "wrapping_sub"}.get(mn)
    sym = {"and": "&", "test": "&", "or": "|", "xor": "^"}.get(mn)
    r = f"(a.{op}(c) & {mask})" if op else f"((a {sym} c) & {mask})"
    kind = "ADD" if mn == "add" else "SUB" if op else "LOGIC"
    return (f"{{ let a = {load} & {mask}; let c = {src} & {mask}; let r = {r}; "
            f"fl = Flags::new(x86::{kind}, a, c, r, {bits});" + ("" if mn in ("cmp", "test") else " " + store) + " }")


def inc(x):
    return (f"{{ let a = {x} & u32::MAX; let r = a.wrapping_add(1) & u32::MAX; let cf = fl.cf(); "
            f"fl = Flags::new(x86::INC, a, 1, r, 32); fl.cf_in = cf; {x} = r; }}")


def shl(x, count):
    return (f"{{ let a = {x} & u32::MAX; let n = {count} & 31; if n != 0 {{ let r = (a << n) & u32::MAX; "
            f"fl = Flags::new(x86::SHL, a, n, r, 32); {x} = r; }} }}")


def push(v):
    return f"{{ let v = {v}; esp = esp.wrapping_sub(4); m.set_u32(esp, v); }}"


def pop(x):
    return f"{{ let v = m.u32(esp); esp = esp.wrapping_add(4); {x} = v; }}"


FIXTURE = r'''
    #[derive(Default, Debug)] struct Trace {
        bytes: std::collections::BTreeMap<u32, u8>,
        events: std::cell::RefCell<Vec<(bool, u32, u8)>>,
    }
    impl Mem for Trace {
        fn u8(&self, a: u32) -> u8 {
            let v = self.bytes.get(&a).copied().unwrap_or((a as u8).wrapping_mul(37));
            self.events.borrow_mut().push((false, a, v)); v
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.events.borrow_mut().push((true, a, v)); self.bytes.insert(a, v);
        }
    }
    #[test] fn same_state_memory_and_event_order() {
        let values = [0u32, 1, 2, 5, 0x7f, 0x80, 0xff, 0x7fff_ffff, 0x8000_0000, 0xffff_fffe, 0xffff_ffff];
        for &value in &values {
            for &other in &values {
                let mut initial = Cpu::default();
                initial.eax = value; initial.ecx = other; initial.edx = value ^ other;
                initial.ebx = other.rotate_left(3); initial.esi = 13; initial.edi = value;
                initial.ebp = 0x1234_5678 ^ other; initial.esp = 0x1000;
                initial.fl = Flags::new(x86::SUB, other, value, other.wrapping_sub(value), 32);
                for i in 0..8 { for j in 0..4 {
                    initial.xmm[i][j] = [value, other, 0x3f80_0000, 0x7f7f_ffff, 1, 0xff80_0000][(i + j) % 6] ^ (i as u32);
                } }
                let mut results = Vec::new();
                for run in [plain::f_00001000 as fn(&mut dyn Mem, &mut Cpu), lean::f_00001000, split::g_00001000] {
                    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                    let log = calls.clone();
                    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
                        log.borrow_mut().push((t, format!("{:?}", c)));
                        c.eax = c.eax.rotate_left(5) ^ m.u32(c.esp); c.esp = c.esp.wrapping_add(4);
                    })));
                    let mut c = initial.clone(); let mut m = Trace::default();
                    run(&mut m, &mut c);
                    x86::set_ext_calls(None);
                    results.push((format!("{:?}", c), m.bytes, m.events.into_inner(), calls.borrow().clone()));
                }
                assert_eq!(results[0], results[1], "lean: eax {value:#x} ecx {other:#x}");
                assert_eq!(results[0], results[2], "split: eax {value:#x} ecx {other:#x}");
            }
        }
    }
'''


# lift.py's packed SSE templates: 16/8-byte loads, lane loops, 16-byte store
SSE = ["{ let a = esp.wrapping_add(0x10); xmm1 = [m.u32(a), m.u32(a.wrapping_add(4)), m.u32(a.wrapping_add(8)), "
       "m.u32(a.wrapping_add(12))]; }",
       "for k in 0..4 { xmm1[k] = x86::mulss(xmm1[k], xmm2[k]); }",
       "for k in 0..4 { xmm3[k] = x86::addss(xmm3[k], xmm3[k]); }",
       "{ for k in 0..4 { let (d, s) = (xmm2[k], xmm1[k]); xmm2[k] = !d & s; } }",
       "{ let a = ebx & 0xFF0u32; let s = [m.u32(a), m.u32(a.wrapping_add(4)), m.u32(a.wrapping_add(8)), "
       "m.u32(a.wrapping_add(12))]; for k in 0..4 { let (d, s) = (xmm3[k], s[k]); xmm3[k] = d ^ s; } }",
       "{ let a = esp.wrapping_add(0x20); xmm4 = [m.u32(a), m.u32(a.wrapping_add(4)), 0, 0]; }",
       "{ let a = esp.wrapping_sub(0x20); for k in 0..4u32 { m.set_u32(a.wrapping_add(4 * k), xmm2[k as usize]); } }",
       "{ let a = esp.wrapping_sub(0x30); m.set_u32(a, xmm4[0]); m.set_u32(a.wrapping_add(4), xmm4[1]); }",
       "edx = m.u32(0u32.wrapping_add(0x2468));"]


class Differential(unittest.TestCase):
    def test_lean_and_literal_templates_agree(self):
        S, L = lift.SAVE, lift.LOAD
        arms = [
            "0x1000 => { " + " ".join([
                push("ebp"), alu("add", "eax", "0x1u32"), alu("sub", "ecx", "eax"), alu("and", "edx", "0x7Fu32", 8),
                shl("ebx", "ecx"), shl("esi", "0x2u32"), alu("xor", "edi", "esi"), inc("edi"),
                alu("sub", "", "0x4u32", mem="esp.wrapping_add(0x4)"), alu("or", "ebp", "eax", 16), *SSE, alu("cmp", "eax", "ecx"),
                'pc = if fl.cond("e") { 0x1020 } else { 0x1010 }; continue; }']),
            "0x1010 => { " + " ".join([
                alu("or", "eax", "0x10u32"),
                "{ let (a, b) = (edx, 0x0u32); let r = fl.alu_carry(a, b, 32, false); edx = r; }",
                alu("add", "ecx", "0x3u32"), inc("ecx"), alu("test", "eax", "eax"), 'ebx = fl.cond("ne") as u32;',
                alu("add", "esi", "ebx"), pop("ebp"), f"esp = esp.wrapping_add(4 + 0); {S} return; }}"]),
            "0x1020 => { " + " ".join([
                alu("add", "eax", "ecx"), push("eax"),
                f"esp = esp.wrapping_sub(4); m.set_u32(esp, 0x1030); {S} "
                f"(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_ext(m, cpu, 0x402000))(m, cpu); {L}",
                alu("sub", "edx", "eax"), alu("xor", "eax", "edx"), shl("eax", "0x3u32"),
                f"{{ let t = edx; esp = esp.wrapping_sub(4); m.set_u32(esp, 0x1040); {S} x86::call_ext(m, cpu, t); {L} }}",
                f"{{ let t = 0x403000u32; esp = esp.wrapping_sub(4); m.set_u32(esp, 0x1050); {S} x86::call_ext(m, cpu, t); {L} }}",
                alu("cmp", "eax", "0x5u32"), pop("ecx"), f"esp = esp.wrapping_add(4 + 0); {S} return; }}"]),
        ]
        saved_split = le.SPLIT_BYTES
        try:
            lift.LEAN = False
            plain = lift.rust_fn(0x1000, "synthetic", arms)
            lift.LEAN = True
            lean = lift.rust_fn(0x1000, "synthetic", arms)
            le.SPLIT_BYTES = 900  # three parts of one arm each
            split = lift.rust_fn(0x1000, "synthetic", arms).replace("f_00001000", "g_00001000")
        finally:
            lift.LEAN, le.SPLIT_BYTES = True, saved_split
        for wanted in ("emit::push(", "emit::pop(", "cpu.eax = cpu.eax.wrapping_add(0x1u32);",
                       "x86::call_ext(m, cpu, 0x402000);", "cpu.xmm[1] = emit::load128(m, pt, cpu.esp.wrapping_add(0x10));",
                       "cpu.xmm[4] = emit::load64(m, pt, cpu.esp.wrapping_add(0x20));",
                       "emit::store128(cpu.xmm[2], m, pt, cpu.esp.wrapping_sub(0x20));",
                       "emit::store64(cpu.xmm[4], m, pt, cpu.esp.wrapping_sub(0x30));",
                       "cpu.esi = cpu.esi << 0x2u32;", "cpu.edi = cpu.edi.wrapping_add(1);",
                       "emit::push(0x1050, m, pt, cpu); x86::call_ext(m, cpu, 0x403000u32);",
                       "let n = 0x3u32; let r = (a << n) & u32::MAX; cpu.fl = Flags::new(x86::SHL", "cpu.xmm[1] = x86::mulps(cpu.xmm[1], cpu.xmm[2]);",
                       "cpu.xmm[3] = x86::addps(cpu.xmm[3], cpu.xmm[3]);",
                       "cpu.xmm[2] = emit::andnps(cpu.xmm[2], cpu.xmm[1]);",
                       "cpu.xmm[3] = emit::xorps(cpu.xmm[3], emit::load128(m, pt, cpu.ebx & 0xFF0u32));", "m.fast_u32(pt, 0x2468u32)"):
            self.assertIn(wanted, lean)
        self.assertNotIn("_p0(", lean)
        for wanted in ("0x1000..=0x1000 => match g_00001000_p0(m, cpu, pt, pc) { Some(next) => next, None => return },",
                       "0x1020..=0x1020 => match g_00001000_p2(", "_ if !(0x1010..=0x1010).contains(&pc) => return Some(pc),",
                       "return None; }"):
            self.assertIn(wanted, split)
        self.assertNotIn("x86::SHL, a, n, r, 32); cpu.ebx", lean)  # dies: the xor overwrites it
        self.assertIn("x86::SHL, a, n, r, 32); cpu.eax", lean)  # live: cmp is past a call
        root = Path(__file__).resolve().parents[2]
        base = root / "crates/ddnative-base/src"
        source = f'''#![allow(warnings)]
            #[path = "{(base / 'mem.rs').as_posix()}"] mod mem;
            #[path = "{(base / 'x86.rs').as_posix()}"] mod x86;
            #[path = "{(base / 'x87.rs').as_posix()}"] mod x87;
            #[path = "{(base / 'emit.rs').as_posix()}"] mod emit;
            #[path = "{(base / 'hooks.rs').as_posix()}"] mod hooks;
            use mem::Mem; use x86::{{Cpu, Flags}};
            mod plain {{ use super::*; {plain} }}
            mod lean {{ use super::*; {lean} }}
            mod split {{ use super::*; {split} }}
        ''' + FIXTURE
        target = Path(os.environ.get("CARGO_TARGET_DIR", "C:/ddda-targets/L129"))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="lean-test-", dir=target) as tmp:
            src, exe = Path(tmp) / "fixture.rs", Path(tmp) / "fixture.exe"
            src.write_text(source)
            for command in (["rustc", "--edition=2021", "--test", str(src), "-o", str(exe)], [str(exe)]):
                result = subprocess.run(command, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
