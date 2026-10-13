"""Build one existing lifted function with the L130 oracle test, never shards."""
import argparse
import os
from pathlib import Path
import re
import subprocess


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',default='E:/ddda-re/notes/native-L130/check')
    p.add_argument('--bench',action='store_true')
    a=p.parse_args()
    repo=Path(__file__).resolve().parents[2]
    out=Path(a.out)
    (out/'src').mkdir(parents=True,exist_ok=True)
    (out/'tests').mkdir(exist_ok=True)
    source=(repo/'crates/ddlift/lift_28/src/generated/lifted_004.rs').read_text()
    match=re.search(r'pub fn f_00e0ec50\(.*?\n\}',source,re.S)
    assert match
    prelude='#![allow(warnings)]\nuse ddnative_base::{mem::Mem,x86::{self,Cpu,Flags},emit};\n'
    (out/'src/lib.rs').write_text(prelude+match.group())
    deps='\n'.join(f'{n}={{path="{(repo/"crates"/n).as_posix()}"}}' for n in ('mtcore','ddnative-base','ddnative-host'))
    (out/'Cargo.toml').write_text('[package]\nname="main-thread-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'+deps+'\nserde_json="1"\n[profile.release]\nopt-level=1\n[profile.release.package.mtcore]\nopt-level=3\n[profile.release.package.ddnative-base]\nopt-level=3\n[profile.release.package.ddnative-host]\nopt-level=3\n')
    test=(repo/'crates/mtcore/tests/hot.rs').read_text().replace('// L130_REFERENCE_RUN','run(main_thread_check::f_00e0ec50,case);')
    test,count=re.subn(r'vec!\[\("port",\s*mtcore::hot::expand_dependencies\)\]; // L130_COST_BODIES', 'vec![("lifted",main_thread_check::f_00e0ec50),("port",mtcore::hot::expand_dependencies)];', test)
    assert count==1, 'cost-body insertion marker missing'
    (out/'tests/hot.rs').write_text(test)
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L130',CARGO_BUILD_JOBS='4',CARGO_INCREMENTAL='0',DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--test','hot']
    command+=['--release','dependencies_cost','--','--ignored','--nocapture'] if a.bench else ['--','--nocapture']
    raise SystemExit(subprocess.call(command,env=env))


if __name__=='__main__': main()
