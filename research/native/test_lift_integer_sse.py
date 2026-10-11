"""Synthetic x86 bytes, recorded in Unicorn and replayed by emitted Rust."""
from pathlib import Path
import os
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import harness as h
import lift


class IntegerSseTests(unittest.TestCase):
    def test_traps_and_undefined_flag_reads_are_not_vectors(self):
        for code in ('31d2b80100000031c9f7f1c3',  # div zero
                     'b8ffffffffbaffffffffb901000000f7f1c3',  # quotient overflow
                     'b803000000b902000000f7e17400c3',  # mul; jz (undefined)
                     'b803000000b902000000f7e19fc3',  # mul; lahf (undefined)
                     '31c90fbcc1c3'):  # bsf zero: destination undefined
            data=bytes.fromhex(code)
            with patch.object(h,'image',return_value=(0x400000,[(0x400000,data.ljust(4096,b'\x90'))])):
                with self.assertRaises(h.Rejected):
                    h.run_sparse(0x400000,17,zero_inputs=True,size=len(data))

    def test_emitted_integer_and_sse(self):
        programs=[]
        # Carry and borrow chains, all widths and subregister preservation.
        for op in ('19c8','11c8','6619c8','6611c8','18c8','10c8'):
            programs.append('b8ffffffffb90100000039c8'+op+'9fc3')
        # MUL/IMUL and DIV/IDIV: consume only defined overflow flags.
        for op in ('f7e1','f7e9','66f7e1','66f7e9','f6e1','f6e9'):
            programs.append('b807003412ba1100cdabb903000000'+op+'0f92c331c0c3')
        for op in ('f7f1','f7f9','66f7f1','66f7f9','f6f1','f6f9'):
            programs.append('b81700000031d2b903000000'+op+'c3')
        for op in ('d1c0','d1c8','0fa4c803','0facc803','0fbcc1','0fbdc1','0f44c1','98','6698','6699'):
            programs.append('b878563412b90300000039c8'+op+'c3')
        for op in ('0fa3c8','0fabc8','0fb3c8','0fbbc8'):
            programs.append('b878563412b923000000'+op+'0f92c231c0c3')
        programs.append('b800204000b9030000008908870831c0c3')  # xchg [eax],ecx
        programs.append('b800204000b9002040008908870131c0c3')  # xchg [ecx],eax
        programs.append('b804002000b9dfffffff0fa3080f92c231c0c3')  # negative bit-string index across page
        # REP in both directions, zero count, and overlapping source/destination.
        for setup in ('b904000000fc','b904000000fd','31c9fd'):
            for op in ('f3a4','f366a5','f3a5','f3aa','f366ab','f3ab'):
                programs.append('be10104000bf12200000b878563412'+setup+op+'fcc3')
        # Full XMM stores compare every lane and make source width observable.
        load='0f100500104000'  # movups xmm0,[401000]
        store='0f110500200000c3'
        for op in ('0f50c0', '660f50c0', '660fd7c0'):
            programs.append(load+op+'c3')
        for op in ('660f2f0510104000', '660f2e0510104000'):
            programs.append(load+op+'9fc3')  # m64 despite Capstone's m128 detail
        for op in ('0f580510104000','0f590510104000','0f5c0510104000',
                   '0f5e0510104000','0f5d0510104000','0f5f0510104000',
                   '0f5a0510104000','660f5a0510104000','f30f5a0510104000','f20f5a0510104000',
                   '660f6e0510104000','f20f100510104000','0f160510104000','0f120510104000',
                   '660f7005101040001b','0fc2051010400002','0fc605101040001b',
                   '0f140510104000','660f760510104000','660ffe0510104000','0f2b0500300000'):
            programs.append(load+op+store)
        root=Path(__file__).resolve().parents[2]
        source='#![allow(warnings)]\n'+''.join(f'#[path="{(root/f"crates/ddnative-base/src/{m}.rs").as_posix()}"] mod {m};\n' for m in ('mem','x86','x87'))
        source+='use mem::Mem; use x86::{Cpu,Flags};\n'
        for k,hexcode in enumerate(programs):
            code=bytes.fromhex(hexcode)
            data=code.ljust(4096,b'\x90')+struct.pack('<8f',1.5,-2,3.25,4,2,3,4,5)
            with patch.object(h,'image',return_value=(0x400000,[(0x400000,data)])), patch.object(h,'executable',return_value=True):
                try:
                    case=h.run_sparse(0x400000,17,zero_inputs=True,size=len(code))
                except h.Rejected as error:
                    raise AssertionError(f"fixture {k}: {hexcode}: {error}") from error
                arms=lift.lift(0x400000,len(code))
            source+=lift.rust_fn(0x400000,'synthetic integer/SSE',arms).replace('f_00400000',f'fixture_{k}')
            source+=f'#[test] fn case_{k}() {{ let mut m=mem::Sparse::default(); let mut c=Cpu::default();\n'
            for reg,value in case['regs_in'].items():source+=f'c.{reg}={value};\n'
            for addr,raw in case['reads']:
                for offset,byte in enumerate(bytes.fromhex(raw)):source+=f'm.bytes.insert({addr+offset},{byte});\n'
            source+=f'fixture_{k}(&mut m,&mut c);\n'
            for reg,value in case['regs_out'].items():source+=f'assert_eq!(c.{reg},{value},"{reg}");\n'
            source+=f'assert_eq!(c.xmm[0][0],{case["xmm0_out"]}); let mut expected=std::collections::BTreeMap::new();\n'
            for addr,raw in case['writes']:
                for offset,byte in enumerate(bytes.fromhex(raw)):source+=f'expected.insert({addr+offset},{byte});\n'
            source+='assert_eq!(m.written,expected); }\n'
        (root/'target').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root/'target',prefix='integer-sse-') as folder:
            path=Path(folder);(path/'test.rs').write_text(source)
            exe=path/('test.exe' if os.name=='nt' else 'test')
            result=subprocess.run(['rustc','--edition=2021','--test',str(path/'test.rs'),'-o',str(exe)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            result=subprocess.run([str(exe)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)

if __name__=='__main__':unittest.main()
