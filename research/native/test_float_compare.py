"""L109c float-idiom emission: synthetic x86 and native SSE oracle, no shards."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import harness as h
import lift
import rust_base

CC = ('o', 'no', 'b', 'ae', 'e', 'ne', 'be', 'a',
      's', 'ns', 'p', 'np', 'l', 'ge', 'le', 'g')


def arm(ordered=False, mask=0x44, cc='p'):
    return (f'fl = x86::comiss_hw::<{str(ordered).lower()}>(xmm0[0], xmm1[0]); '
            'eax = (eax & !0xff00) | ((fl.lahf() as u32) << 8); '
            f'{{ let r = fl.alu_and(((eax >> 8) & 0xFF), 0x{mask:X}u32, 8); }} '
            f'pc = if fl.cond("{cc}") {{ 0x10 }} else {{ 0x20 }};')


class FloatCompareTests(unittest.TestCase):
    def test_literal_decoder_and_restartable_boundaries(self):
        code = bytes.fromhex('0f2ec19ff6c4447a0190c3')
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000, code.ljust(4096, b'\x90'))])), \
             patch.object(h, 'executable', return_value=True):
            arms = lift.lift(0x400000, len(code))
        self.assertIn('float_cmp', lift.cpu_arm(arms[0]))
        self.assertNotIn('float_cmp', lift.cpu_arm(arms[0], fast=False))
        with patch.object(lift, 'LEAN', False):
            self.assertNotIn('float_cmp', lift.cpu_arm(arms[0]))
        for changed in (arm().replace('; eax =', '; ecx = 1; eax ='),
                        arm().replace('; { let r', '; } 0x1234 => { { let r'),
                        arm().replace('0x44u32', 'edx'),
                        arm().replace('8); }', '16); }')):
            self.assertEqual(lift.fuse_float_test_branches(changed), changed)

    def test_every_mask_and_condition_truth_table(self):
        for mask in range(256):
            for cc in CC:
                fused = lift.fuse_float_test_branches(arm(mask=mask, cc=cc))
                self.assertIn('let float_cmp', fused)
                for result, ah in enumerate((0x42, 2, 3, 0x47)):
                    value = ah & mask
                    # Independent x86 condition decoder with CF=OF=0.
                    flags = (0x40 if value == 0 else 0) | (value & 0x80)
                    flags |= 4 if bin(value).count('1') % 2 == 0 else 0
                    expected = condition(cc, flags)
                    predicate = fused.split('pc = if ')[1].split(' { 0x10')[0]
                    actual = predicate == 'true' or (predicate.startswith('matches!') and
                             str(result) in predicate.split(', ')[1].rstrip(')').split(' | '))
                    self.assertEqual(actual, expected, (mask, cc, result))

    def test_native_oracle_registers_flags_branches_and_mxcsr(self):
        root = Path(__file__).resolve().parents[2]
        source = '#![allow(warnings)]\n' + rust_base.modules(root)
        source += 'use x86::{Cpu, Flags};\n'
        calls = []
        for ordered in (False, True):
            for mask in (0, 1, 4, 0x40, 0x44, 0x80, 0xff):
                for index, cc in enumerate(CC):
                    name = f'case_{len(calls)}'
                    body = lift.cpu_arm(arm(ordered, mask, cc))
                    source += f'fn {name}(cpu: &mut Cpu) -> u32 {{ let pc; {body} pc }}\n'
                    calls.append(f'({name} as fn(&mut Cpu)->u32, {str(ordered).lower()}, {mask}u8, {index})')
        source += r'''
fn original(a:u32,b:u32,eax:u32,mask:u8,ordered:bool)->(u32,u32) {
    let mut accumulator=eax; let flags:u64;
    unsafe {
        if ordered {
            std::arch::asm!("movd xmm0, {a:e}", "movd xmm1, {b:e}",
                "comiss xmm0, xmm1", "lahf", "test ah, cl", "pushfq", "pop {flags}",
                a=in(reg) a,b=in(reg) b,inout("eax") accumulator,in("cl") mask,
                out("xmm0") _,out("xmm1") _,flags=out(reg) flags);
        } else {
            std::arch::asm!("movd xmm0, {a:e}", "movd xmm1, {b:e}",
                "ucomiss xmm0, xmm1", "lahf", "test ah, cl", "pushfq", "pop {flags}",
                a=in(reg) a,b=in(reg) b,inout("eax") accumulator,in("cl") mask,
                out("xmm0") _,out("xmm1") _,flags=out(reg) flags);
        }
    }
    (accumulator,flags as u32)
}
fn branch(cc:usize,f:u32)->bool {
    let c=f&1!=0; let p=f&4!=0; let z=f&64!=0; let s=f&128!=0; let o=f&2048!=0;
    [o,!o,c,!c,z,!z,c||z,!c&&!z,s,!s,p,!p,s!=o,s==o,z||(s!=o),!z&&(s==o)][cc]
}
#[test] fn native_oracle() {
    ddnative_host::install();
    let saved=x86::stmxcsr();
    let values=[0,0x80000000,1,0x80000001,0x007fffff,0x00800000,
        0x3f800000,0xbf800000,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,
        0x7fc00000,0xffc12345,0x7f800001,0xff800001];
'''
        source += 'let cases = [' + ','.join(calls) + '];\n'
        source += r'''
    for mode in [0x1f80,0x1fc0,0x9f80,0x9fc0,0x3f80,0x5f80,0x7f80] {
        for a in values { for b in values {
            for (f,ordered,mask,cc) in cases {
                let mut cpu=Cpu::default(); cpu.eax=0xdeadbeef;
                cpu.xmm[0]=[a,17,23,42]; cpu.xmm[1]=[b,99,88,77];
                x86::ldmxcsr(mode);
                let (eax,flags)=original(a,b,cpu.eax,mask,ordered);
                let mxcsr=x86::stmxcsr();
                x86::ldmxcsr(mode); let target=f(&mut cpu);
                assert_eq!(x86::stmxcsr(),mxcsr,"MXCSR {a:x} {b:x}");
                assert_eq!(cpu.eax,eax); assert_eq!(target,if branch(cc,flags){0x10}else{0x20});
                let (actual,defined)=cpu.fl.snapshot();
                assert_eq!(actual&0x8c5,flags&0x8c5); assert_eq!(defined&0x8c5,0x8c5);
                assert_eq!(cpu.xmm[0],[a,17,23,42]); assert_eq!(cpu.xmm[1],[b,99,88,77]);
            }
        }}
    }
    x86::ldmxcsr(saved);
}
'''
        target = Path(os.environ.get('CARGO_TARGET_DIR', root / 'target'))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='float-compare-', dir=target) as tmp:
            src, exe = Path(tmp) / 'test.rs', Path(tmp) / 'test.exe'
            src.write_text(source)
            for command in ([os.environ.get('RUSTC', 'rustc'), '--edition=2021', '-C', 'opt-level=1',
                             '--test', str(src), '-o', str(exe)], [str(exe), '--test-threads=1']):
                result = subprocess.run(command, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


def condition(cc, flags):
    c, p, z, s, o = (bool(flags & bit) for bit in (1, 4, 64, 128, 2048))
    return (o, not o, c, not c, z, not z, c or z, not c and not z,
            s, not s, p, not p, s != o, s == o, z or s != o, not z and s == o)[CC.index(cc)]


if __name__ == '__main__':
    unittest.main()
