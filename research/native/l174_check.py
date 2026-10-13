"""Extract only L174's two lifted bodies for ABI replay/paired benchmarks.

Private generated source stays under --out. No ddlift shard is compiled.
The isolated lifted bodies use opt2, the semantic ports/base use opt3.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L174/check'))
    p.add_argument('--bench',action='store_true')
    args=p.parse_args()
    repo=Path(__file__).resolve().parents[2]
    source=(repo/'crates/ddlift/lift_00/src/generated/lifted_031.rs').read_text()
    bodies=[]
    for address in (0x10655a0,0x1066c60):
        matches=re.findall(rf'(?:pub )?fn f_{address:08x}(?:_p\d+)?\(.*?\n\}}',source,re.S)
        assert matches,hex(address)
        body=re.sub(r'x86::call_local\(m, cpu, (0x[0-9A-F]+), super::lifted::f_[0-9a-f]+\)',r'x86::call_ext(m, cpu, \1)','\n'.join(matches))
        body=body.replace('x86::call_lifted(', 'x86::call_ext(')
        body=body.replace('crate::mem::','ddnative_base::mem::')
        assert 'super::' not in body
        bodies.append(body)
    out=args.out
    (out/'src').mkdir(parents=True,exist_ok=True)
    (out/'tests').mkdir(exist_ok=True)
    (out/'src/lib.rs').write_text('#![allow(warnings)]\nuse ddnative_base::{mem::Mem,x86::{self,Cpu,Flags},emit};\n'+'\n'.join(bodies))
    dependencies='\n'.join(f'{n}={{path="{(repo/"crates"/n).as_posix()}"}}' for n in ('mtcore','ddnative-base','ddnative-host'))
    (out/'Cargo.toml').write_text('[package]\nname="l174-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'+dependencies+'\nserde_json="1"\n[profile.release]\nopt-level=2\n'+''.join(f'[profile.release.package.{n}]\nopt-level=3\n' for n in ('mtcore','ddnative-base','ddnative-host')))
    test=(repo/'crates/mtcore/tests/semantic_draw_b.rs').read_text()
    test=test.replace('// L174_BASELINES', '0x10655a0 => Some(l174_check::f_010655a0),\n0x1066c60 => Some(l174_check::f_01066c60),')
    test+='\n#[test]\nfn isolated_lifted_originals() { ddnative_host::install();\n'
    for address in (0x10655a0,0x1066c60):
        test+=f'verify("E:/ddda-re/notes/native-L174/{address:08x}.json",l174_check::f_{address:08x});\n'
    test+='}\n'
    (out/'tests/abi.rs').write_text(test)
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L174',CARGO_BUILD_JOBS='4',CARGO_INCREMENTAL='0',DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--test','abi','--release']
    if args.bench: command+=['draw_benchmark','--','--ignored','--nocapture','--test-threads=1']
    else: command+=['--','--nocapture','--test-threads=1']
    raise SystemExit(subprocess.call(command,env=env))


if __name__=='__main__': main()
