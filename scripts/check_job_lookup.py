"""Isolated original/lifted/port lookup gate, no shard build or game run."""
import argparse
import os
from pathlib import Path
import subprocess

CHECK = r'''
#[test]
fn original_lifted_lookup_vectors() {
    verify(lifted::f_01008200, 0x01008200);
    verify(lifted::f_01008260, 0x01008260);
}
#[test]
fn lookup_fault_and_access_order() {
    use std::panic::{catch_unwind,AssertUnwindSafe};
    let data=vectors().unwrap();
    let mut total=0;
    for case in data["cases"].as_array().unwrap().iter().filter(|v| n(&v["count"]) == 2) {
        let (lift,port):(LiftFn,LiftFn)=if n(&case["entry"])==0x1008200 {
            (lifted::f_01008200,mtcore::job_lookup::single_key)
        } else {(lifted::f_01008260,mtcore::job_lookup::pair_key)};
        for fault in 1..=64 {
            let run=|body:LiftFn| {
                let mut m=Memory{bytes:pairs(&case["initial"]),fail_at:Some(fault),..Default::default()};
                let mut c=cpu(&case["before"]);
                let failed=catch_unwind(AssertUnwindSafe(||body(&mut m,&mut c))).is_err();
                (failed,regs(&c),c.fl.snapshot().0,m.writes,m.accesses.into_inner())
            };
            assert_eq!(run(lift),run(port),"seed {} fault {fault}",case["seed"]);
            total+=1;
        }
    }
    eprintln!("lookup: {total} paired access/fault positions");
}
struct PageMemory { table: Box<ddnative_base::mem::PageTable> }
impl Mem for PageMemory {
    fn page_table(&self)->Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&*self.table))
    }
    fn u8(&self,a:u32)->u8 {
        let p=self.table.page((a/4096) as usize);assert!(!p.is_null());
        unsafe {ddnative_base::mem::load_at(p.add((a%4096) as usize),1) as u8}
    }
    fn set_u8(&mut self,a:u32,b:u8) {
        let p=self.table.page((a/4096) as usize);assert!(!p.is_null());
        unsafe {ddnative_base::mem::store_at(p.add((a%4096) as usize),1,b as u32);}
    }
}
#[test]
fn page_vectors_and_optional_cost() {
    use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    let data=vectors().unwrap();
    let mut m=PageMemory{table:Box::new(PageTable::default())};
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for case in data["cases"].as_array().unwrap() {
            let entry=n(&case["entry"]);
            let bodies:[LiftFn;2]=if entry==0x1008200 {
                [lifted::f_01008200,mtcore::job_lookup::single_key]
            } else {[lifted::f_01008260,mtcore::job_lookup::pair_key]};
            for body in bodies {
                for(a,b) in pairs(&case["initial"]) {
                    m.table.set_perm((a/4096) as usize,READ|WRITE);
                    m.table.backed_page((a/4096) as usize);m.set_u8(a,b);
                }
                let mut c=cpu(&case["before"]);body(&mut m,&mut c);
                let expected=&case["after"];
                assert_eq!(regs(&c),std::array::from_fn(|i|n(&expected["regs"][i])));
                let mask=n(&expected["flag_mask"]);
                assert_eq!((c.fl.snapshot().0|if c.df {0x400} else {0})&mask,n(&expected["flags"])&mask);
                for(a,b) in pairs(&expected["memory"]) {assert_eq!(m.u8(a),b);}
            }
            // Select one miss per chain length/function, normal alignment.
            if atomic && std::env::var_os("L158_LOOKUP_BENCH").is_some()
                && case["mode"]=="normal" && case["hit"] == -1
                && case["seed"].as_u64().unwrap()%30==1 {
                let mut c=cpu(&case["before"]);
                let ecx=c.ecx;let esp=c.esp;
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;let body=black_box(bodies[port]);
                    let start=Instant::now();
                    for _ in 0..20000 {c.ecx=ecx;c.esp=esp;body(&mut m,&mut c);black_box(c.eax);}
                    eprintln!("lookup-cost entry={entry:x} count={} round={round} port={port} ns={:.3}",
                        case["count"],start.elapsed().as_nanos() as f64/20000.0);
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
    out = Path('E:/ddda-re/notes/native-L158/lookup-isolated')
    out.mkdir(parents=True, exist_ok=True)
    source = (repo/'crates/ddlift/lift_00/src/generated/lifted_004.rs').read_text()
    bodies = []
    for entry in ('01008200', '01008260'):
        start = source.index('pub fn f_'+entry+'(')
        end = source.index('\n// Translated from ', start)
        bodies.append(source[start:end])
    manifest = f'''[package]
name="l158-lookup-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
mtcore={{path="{repo.as_posix()}/crates/mtcore"}}
serde_json="1"
[profile.release]
opt-level=1
lto=false
codegen-units=1
[profile.release.package.mtcore]
opt-level=3
[profile.release.package.ddnative-base]
opt-level=3
[[test]]
name="isolated"
path="isolated.rs"
'''
    (out/'Cargo.toml').write_text(manifest)
    test = (repo/'crates/mtcore/tests/job_lookup.rs').read_text().replace('//!','//')
    test += '\nmod lifted {\n#![allow(warnings)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu}};\n'
    test += '\n'.join(bodies) + '\n}\n' + CHECK
    (out/'isolated.rs').write_text(test)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR='C:/ddda-targets/L158',
               CARGO_BUILD_JOBS='4', DDNATIVE_REQUIRE_VECTORS='1')
    if args.bench: env['L158_LOOKUP_BENCH']='1'
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release',
                    '--','--nocapture','--test-threads=1'],env=env,check=True)


if __name__ == '__main__': main()
