"""Compile/test the standalone corpus wrapper and shared recorder, no runtime/shards."""
import os
import json
from pathlib import Path
import subprocess
from worker_corpus import materialize


def main():
    root=Path(__file__).resolve().parents[1];out=Path('C:/ddda-targets/L165/capture-check');out.mkdir(parents=True,exist_ok=True)
    # Public repository source only; never copy research/game data into repo.
    recorder=subprocess.check_output(['git','show','711b9576:crates/ddruntime/src/shadow_fee320.rs'],cwd=root,text=True)
    (out/'shadow_fee320.rs').write_text(recorder,encoding='utf-8')
    (out/'Cargo.toml').write_text(f'''[package]
name="l165-capture-check"
version="0.0.0"
edition="2021"
[workspace]
[features]
game=[]
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{root.as_posix()}/crates/ddnative-host"}}
[[test]]
name="capture"
path="capture.rs"
''',encoding='utf-8')
    (out/'capture.rs').write_text(f'''mod threads {{pub fn current_tid()->u32{{0x1000}}}}
mod shadow_fee320;
#[path="{root.as_posix()}/crates/ddruntime/src/codegen_corpus.rs"]mod codegen_corpus;
''',encoding='utf-8')
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L165',CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='4',DDNATIVE_CORPUS_SMOKE_DIR=str(out))
    old=set(out.glob('smoke-*/*.json'))
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--','--include-ignored','--test-threads=1'],env=env,check=True)
    fresh=set(out.glob('smoke-*/*.json'))-old;assert len(fresh)==2
    for path in fresh:
        sample=json.loads(path.read_text(encoding='utf-8'));assert sample['origin']=='synthetic-lift'
        for call in sample['records']:materialize(call)
    print('Two synthetic wrapper records accepted by strict snapshot converter; zero real-data coverage.',flush=True)


if __name__=='__main__':main()
