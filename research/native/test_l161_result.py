"""32-bit adjacent-result ZF predicates preserve full CPU/flags/accesses."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import lean_emission as le
import lift
import rust_base


def producer(op):
    if op in ('inc','dec'):
        method='add' if op=='inc' else 'sub'
        return ('{ let a = cpu.eax & u32::MAX; let r = a.wrapping_' + method + '(1) & u32::MAX; '
                f'cpu.fl.inc_dec(x86::{op.upper()}, a, r, 32); cpu.eax = r; }} ')
    return f'cpu.eax = cpu.fl.alu_{op}(m.fast_u32(pt, 0x8000), cpu.ecx, 32); '


@patch.dict(os.environ, {'LIFT_RESULT_ZF':'1'})
class ResultZf(unittest.TestCase):
    def test_supported_and_barriers(self):
        for op in ('add','sub','and','or','xor','inc','dec'):
            for condition,relation in [('cpu.fl.zf()','=='),('!cpu.fl.zf()','!='),('(!cpu.fl.zf())','!=')]:
                arm='0x1000 => { '+producer(op)+f'pc = if {condition} {{ 0x1010 }} else {{ 0x1020 }}; continue; }}'
                result=le.result_zf(arm)
                self.assertIn(f'pc = if cpu.eax {relation} 0',result)
                self.assertTrue(result.startswith('0x1000 => { '+producer(op)))
                with patch.dict(os.environ,{'LIFT_RESULT_ZF':'0'}):
                    self.assertEqual(le.result_zf(arm),arm)
                for barrier in ('cpu.eax = 1; ', 'x86::call_ext(m, cpu, 42); ', 'cpu.fl.sahf(0); ', '} { '):
                    blocked=arm.replace('pc = if',barrier+'pc = if')
                    self.assertEqual(le.result_zf(blocked),blocked)
                if op not in ('inc','dec'):
                    narrow=arm.replace(', 32);',', 16);')
                    self.assertEqual(le.result_zf(narrow),narrow)
        text='"'+producer('add')+'pc = if cpu.fl.zf() {"'
        self.assertEqual(le.result_zf(text),text)

    def test_steps_unchanged(self):
        arms=['0x1000 => { eax = fl.alu_and(eax, ecx, 32); pc = if fl.cond("z") { 0x1010 } else { 0x1020 }; continue; }']
        with patch.dict(os.environ,{'LIFT_RESULT_ZF':'0'}):
            before=lift.rust_step(0x1000,'synthetic',arms)
        self.assertEqual(before,lift.rust_step(0x1000,'synthetic',arms))

    def test_independent_fixed_point_switch(self):
        arm='0x1000 => { '+producer('and')+'pc = if cpu.fl.zf() { 0x1010 } else { 0x1020 }; continue; }'
        arms=[arm,'0x1010 => { return; }','0x1020 => { return; }']
        for fixed in ('0','1'):
            with patch.dict(os.environ,{'LIFT_FLAG_FIXED_POINT':fixed}):
                self.assertIn('pc = if cpu.eax == 0',le.function_flags(arms)[0])
                with patch.dict(os.environ,{'LIFT_RESULT_ZF':'0'}):
                    self.assertEqual(le.function_flags(arms),arms)

    def test_all_flags_values_and_accesses_opt0_opt1(self):
        functions=[]
        for op in ('add','sub','and','or','xor','inc','dec'):
            for negate in (False,True):
                arm='0x1000 => { '+producer(op)+'pc = if '+('!' if negate else '')+'cpu.fl.zf() { 0x1010 } else { 0x1020 }; continue; }'
                body='let pt=m.page_table(); let mut pc=0x1000; loop { match pc { ARM 0x1010 => {cpu.edx=1;return},0x1020=>{cpu.edx=0;return},_=>unreachable!() } }'
                for variant,code in [('base',arm),('new',le.result_zf(arm))]:
                    functions.append(f'fn {op}_{int(negate)}_{variant}(m:&mut dyn Mem,cpu:&mut Cpu) {{'+body.replace('ARM',code)+'}')
        pairs=','.join(f'({op}_{int(n)}_base as x86::LiftFn,{op}_{int(n)}_new as x86::LiftFn)' for op in ('add','sub','and','or','xor','inc','dec') for n in (False,True))
        root=Path(__file__).resolve().parents[2]
        source='#![allow(warnings)]\n'+rust_base.modules(root)+'\nuse mem::Mem; use x86::Cpu;\n'+'\n'.join(functions)+'''
struct Observed {value:u32,reads:std::cell::Cell<u32>,fault:bool}
impl Mem for Observed {
 fn u8(&self,_:u32)->u8 {panic!("byte")}
 fn set_u8(&mut self,_:u32,_:u8){panic!("write")}
 fn u32(&self,a:u32)->u32 {assert_eq!(a,0x8000);self.reads.set(self.reads.get()+1);if self.fault {panic!("fault")};self.value}
}
#[test] fn differential() {
 std::panic::set_hook(Box::new(|_|{}));
 let values=[0u32,1,2,0x7fffffff,0x80000000,0x80000001,0xfffffffe,0xffffffff,0x12345678];
 for (base,new) in [PAIRS] {for a in values {for b in values {for flags in [0,1,0x8d5] {for defined in [true,false] {for fault in [false,true] {
  let run=|f:x86::LiftFn| {
   let mut m=Observed {value:a,reads:Default::default(),fault};
   let mut c=Cpu::default();c.eax=a;c.ecx=b;
   c.fl=x86::Flags::partial(flags,if defined {0x8d5} else {0x8d4});
   let panicked=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||f(&mut m,&mut c))).is_err();
   (c.exit(),c.fl.snapshot(),m.reads.get(),panicked)
  };
  assert_eq!(run(base),run(new));
 }}}}}}
}
'''.replace('PAIRS',pairs)
        with tempfile.TemporaryDirectory(dir=os.environ['CARGO_TARGET_DIR'],prefix='result-zf-') as directory:
            src,exe=Path(directory)/'test.rs',Path(directory)/'test.exe';src.write_text(source)
            for opt in (0,1):
                for command in (['rustc','--edition=2021','--test',f'-Copt-level={opt}',str(src),'-o',str(exe)], [str(exe),'--test-threads=1']):
                    r=subprocess.run(command,capture_output=True,text=True,timeout=60)
                    self.assertEqual(r.returncode,0,r.stdout+r.stderr)


if __name__=='__main__':
    unittest.main()
