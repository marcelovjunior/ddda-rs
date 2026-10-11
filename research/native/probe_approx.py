"""Measure host/Unicorn reciprocal differences; this never admits fixtures.

Synthetic operands/instructions only. The host uses original legacy SSE
instructions with MXCSR=0x1f80. Reports stay under private LIFT_DATA.
"""
import json
import os
from pathlib import Path
import random
import subprocess
import tempfile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,__version__
from unicorn.x86_const import UC_X86_REG_XMM0,UC_X86_REG_XMM1,UC_X86_REG_MXCSR


def probe(root):
    rng=random.Random(13)
    values=[[0x3f800000,0x40000000,0x40400000,0x40800000]]
    values += [[rng.randrange(0x00800000,0x7f000000) for _ in range(4)] for _ in range(127)]
    operations={'rcpps':'0f53c1','rcpss':'f30f53c1','rsqrtps':'0f52c1'}
    source='use std::arch::{asm,x86_64::__m128i}; fn main(){\n'
    for op in operations:
        for v in values:
            source+=('{ let mut a:__m128i=unsafe{std::mem::transmute([1u32,2,3,4])};'
                     f'let b:__m128i=unsafe{{std::mem::transmute({v!r}.map(|x|x as u32))}};'
                     'let mut old=0u32;let default=0x1f80u32;unsafe{'
                     f'asm!("stmxcsr [{{old}}]","ldmxcsr [{{default}}]","{op} {{a}}, {{b}}","ldmxcsr [{{old}}]",'
                     'a=inout(xmm_reg)a,b=in(xmm_reg)b,old=in(reg)&mut old,default=in(reg)&default,options(nostack));'
                     'let out:[u32;4]=std::mem::transmute(a);println!("{:?}",out);}}\n')
    source+='}'
    with tempfile.TemporaryDirectory(dir=root,prefix='approx-') as tmp:
        path=Path(tmp);(path/'probe.rs').write_text(source)
        exe=path/('probe.exe' if os.name=='nt' else 'probe')
        subprocess.run(['rustc','--edition=2021',str(path/'probe.rs'),'-o',str(exe)],check=True,capture_output=True)
        host=iter(map(json.loads,subprocess.check_output([str(exe)],text=True).splitlines()))
    rows=[]
    uc=Uc(UC_ARCH_X86,UC_MODE_32);uc.ctl_set_tcg_buffer_size(32<<20);uc.mem_map(0x400000,4096)
    for op,raw in operations.items():
        code=bytes.fromhex(raw);uc.mem_write(0x400000,code);uc.ctl_remove_cache(0x400000,0x401000)
        for v in values:
            uc.reg_write(UC_X86_REG_MXCSR,0x1f80)
            uc.reg_write(UC_X86_REG_XMM0,sum(x<<(32*k) for k,x in enumerate([1,2,3,4])))
            uc.reg_write(UC_X86_REG_XMM1,sum(x<<(32*k) for k,x in enumerate(v)))
            uc.emu_start(0x400000,0x400000+len(code))
            out=uc.reg_read(UC_X86_REG_XMM0);emulated=[(out>>(32*k))&0xffffffff for k in range(4)]
            native=next(host);rows.append(dict(operation=op,input=v,host=native,unicorn=emulated,equal=native==emulated))
    result=dict(unicorn_version=__version__,mxcsr=0x1f80,rows=rows,
                differing={op:sum(r['operation']==op and not r['equal'] for r in rows) for op in operations})
    (root/'approx-probe.json').write_text(json.dumps(result,indent=2));print(result['differing'])


if __name__=='__main__':
    root=Path(os.environ['LIFT_DATA'])
    if root.resolve()==Path('E:/ddda-re/notes/native').resolve():raise SystemExit('private LIFT_DATA required')
    probe(root)
