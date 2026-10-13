"""Isolated original/lifted/port shadow gate, no shard build or game run."""
import argparse
import os
from pathlib import Path
import subprocess

CHECK = r'''
#[test]
fn original_lifted_shadow_vectors() {
    install();verify(lifted::f_00fed480,0xfed480);
}
#[test]
fn shadow_fault_and_access_order() {
    install();
    use std::panic::{catch_unwind,AssertUnwindSafe};
    std::panic::set_hook(Box::new(|_|{}));
    let data=vectors().unwrap(); let mut total=0;
    for case in data["cases"].as_array().unwrap().iter().step_by(7) {
        for fault in 1..=1100 {
            let run=|body:LiftFn| {
                let mut m=Memory{bytes:pairs(&case["initial"]),fail_at:Some(fault),..Default::default()};
                let mut c=cpu(&case["before"]);
                let failed=catch_unwind(AssertUnwindSafe(||body(&mut m,&mut c))).is_err();
                (failed,regs(&c),c.fl.snapshot().0,m.writes,m.accesses.into_inner())
            };
            assert_eq!(run(lifted::f_00fed480),run(mtcore::job_shadow::save_state),"seed {} fault {fault}",case["seed"]);
            total+=1;
        }
    }
    eprintln!("shadow: {total} paired access/fault positions");
}
#[test]
fn page_vectors_and_optional_cost() {
    use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    install();
    let data=vectors().unwrap();
    let mut m=guest_memory::GuestMemory::new();
    let mut mapped=std::collections::BTreeSet::new();
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for case in data["cases"].as_array().unwrap() {
            let entry=n(&case["entry"]);
            let bodies:[LiftFn;2]=[lifted::f_00fed480,mtcore::job_shadow::save_state];
            for body in bodies {
                for(a,b) in pairs(&case["initial"]) {
                    if mapped.insert(a/4096) { m.map(a&!4095,4096,READ|WRITE).unwrap(); }
                    m.set_u8(a,b);
                }
                let mut c=cpu(&case["before"]);body(&mut m,&mut c);
                let expected=&case["after"];
                assert_eq!(regs(&c),std::array::from_fn(|i|n(&expected["regs"][i])));
                let mask=n(&expected["flag_mask"]);
                assert_eq!((c.fl.snapshot().0|if c.df {0x400} else {0})&mask,n(&expected["flags"])&mask);
                for(a,b) in pairs(&expected["memory"]) {assert_eq!(m.u8(a),b);}
            }
            if atomic && std::env::var_os("L167_SHADOW_BENCH").is_some()
                && case["full"]==0 && case["index"]==0 && case["alias"]==false {
                let mut c=cpu(&case["before"]); let initial=c.clone();
                let index=initial.ecx.wrapping_add(0x30874);
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;let body=black_box(bodies[port]);
                    let start=Instant::now();
                    for _ in 0..10000 {
                        c=initial.clone(); m.set_u32(index,0);body(&mut m,&mut c);black_box(c.eax);
                    }
                    eprintln!("shadow-cost df={} alignment={} round={round} port={port} ns={:.3}",
                        case["df"],case["alignment"],start.elapsed().as_nanos() as f64/10000.0);
                }}
            }
        }
    }
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bench', action='store_true')
    args = p.parse_args()
    repo = Path(__file__).resolve().parents[1]
    out = Path('E:/ddda-re/notes/native-L167/shadow-isolated')
    out.mkdir(parents=True, exist_ok=True)
    source = (repo/'crates/ddlift/lift_31/src/generated/lifted_036.rs').read_text()
    bodies = []
    for entry in ('00fed480',):
        start = source.index('pub fn f_'+entry+'(')
        end = source.index('\n// Translated from ', start)
        bodies.append(source[start:end])
    manifest = f'''[package]
name="l167-shadow-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
mtcore={{path="{repo.as_posix()}/crates/mtcore"}}
serde_json="1"
guest-memory={{path="guest-memory"}}
[profile.release]
opt-level=1
lto=false
codegen-units=1
[profile.release.package.guest-memory]
opt-level=3
[profile.release.package.ddnative-host]
opt-level=3
[profile.release.package.mtcore]
opt-level=3
[profile.release.package.ddnative-base]
opt-level=3
[[test]]
name="isolated"
path="isolated.rs"
'''
    (out/'Cargo.toml').write_text(manifest)
    # Compile the production memory implementation at production opt3. Only
    # the inactive serial-preemption seam is a fail-closed harness stub.
    guest = out/'guest-memory'
    guest.mkdir(exist_ok=True)
    (guest/'Cargo.toml').write_text(f'''[package]
name="guest-memory"
version="0.1.0"
edition="2021"
[lib]
path="lib.rs"
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{repo.as_posix()}/crates/ddnative-host"}}
''')
    memory = (repo/'crates/ddruntime/src/memory.rs').read_text().split('#[cfg(test)]')[0]
    memory += '\nmod threads { pub fn preempt_entry() { panic!("unexpected preemption in isolated replay"); } }\n'
    (guest/'lib.rs').write_text(memory)
    test = (repo/'crates/mtcore/tests/job_shadow.rs').read_text().replace('//!','//')
    test += '\nmod lifted {\n#![allow(warnings)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'
    test += '\n'.join(bodies) + '\n}\nmod platform { pub fn memcpy(m: &mut dyn ddnative_base::mem::Mem,c: &mut ddnative_base::x86::Cpu) { super::memcpy(m,c); } }\n' + CHECK
    (out/'isolated.rs').write_text(test)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR='C:/ddda-targets/L167',
               CARGO_BUILD_JOBS='4', DDNATIVE_REQUIRE_VECTORS='1')
    if args.bench: env['L167_SHADOW_BENCH']='1'
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release',
                    '--','--nocapture','--test-threads=1'],env=env,check=True)


if __name__ == '__main__': main()
