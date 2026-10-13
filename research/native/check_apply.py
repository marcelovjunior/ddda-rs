"""Build just the four private lifted bodies and the L125 tests, never shards.

Extracts existing generated bodies without changing them. The private test
workspace depends on mtdraw as a library (no shard dev-dependencies). Run with
the system Python; CARGO_TARGET_DIR/JOBS/INCREMENTAL follow agent policy.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',default='E:/ddda-re/notes/native-L125/check')
    a=p.parse_args()
    repo=Path(__file__).resolve().parents[2]
    out=Path(a.out)
    (out/'src').mkdir(parents=True,exist_ok=True)
    (out/'tests').mkdir(exist_ok=True)
    addresses=['00da6fe0','00daa5f0','00daa7c0','00daa8b0']
    functions={}
    for path in (repo/'crates/ddlift/lift_27/src/generated').glob('*.rs'):
        text=path.read_text()
        for address in addresses:
            match=re.search(r'pub fn f_'+address+r'\(.*?\n\}',text,re.S)
            if match:
                parts=re.findall(r'(?:pub )?fn f_'+address+r'_p\d+\(.*?\n\}',text,re.S)
                functions[address]=match.group()+'\n'+'\n'.join(parts)
    assert len(functions)==4,functions.keys()
    source='#![allow(warnings,clippy::all)]\nuse ddnative_base::{mem::Mem,x86::{self,Cpu,Flags},emit};\n'
    source+='\n'.join(functions.values())
    # The one local helper stays a local call; its leaf dispatches to the
    # same recorded mock as the readable adapter's call_lifted boundary.
    source+='\n'
    (out/'src/lib.rs').write_text('pub use ddnative_base::mem;\nmod lifted { pub fn f_00da68a0(m:&mut dyn ddnative_base::mem::Mem,c:&mut ddnative_base::x86::Cpu) { ddnative_base::x86::call_ext(m,c,0xda68a0); } }\npub mod reference;\n')
    (out/'src/reference.rs').write_text(source)
    deps='\n'.join(f'{n} = {{ path = "{(repo / "crates" / n).as_posix()}" }}' for n in ['mtdraw','ddnative-base','ddnative-host'])
    (out/'Cargo.toml').write_text('[package]\nname="apply-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'+deps+'\nserde_json="1"\n')
    test=(repo/'crates/mtdraw/tests/apply.rs').read_text()
    choices=','.join(f'"{kind}"=>apply_check::reference::f_{addr} as LiftFn' for kind,addr in zip(['targets','fixed','streams','shader'],addresses))
    test=test.replace('// L125_REFERENCE_RUN',f'run(match kind {{ {choices}, _=>unreachable!() }},case,&format!("{{kind}} {{i}} lifted"));')
    (out/'tests/apply.rs').write_text(test)
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L125',CARGO_BUILD_JOBS='4',CARGO_INCREMENTAL='0',MTDRAW_REQUIRE_APPLY='1')
    raise SystemExit(subprocess.call(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--','--nocapture'],env=env))


if __name__=='__main__': main()

