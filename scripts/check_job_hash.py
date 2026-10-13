"""Compile ONLY the E0E5E0 lifted body for L158 differential/CPU checks.

Generated source stays private. Never builds shards or runs the game.
"""
import argparse
import os
from pathlib import Path
import subprocess

BENCH = r'''
struct PageMemory { table: Box<ddnative_base::mem::PageTable> }
impl Mem for PageMemory {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&*self.table))
    }
    fn u8(&self, a:u32)->u8 {
        let p=self.table.page((a/4096) as usize);
        assert!(!p.is_null());
        unsafe { ddnative_base::mem::load_at(p.add((a%4096) as usize),1) as u8 }
    }
    fn set_u8(&mut self,a:u32,b:u8) {
        let p=self.table.page((a/4096) as usize);
        assert!(!p.is_null());
        unsafe { ddnative_base::mem::store_at(p.add((a%4096) as usize),1,b as u32); }
    }
}
#[test]
fn page_table_vectors() {
    use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
    let mut m=PageMemory {table:Box::new(PageTable::default())};
    let data=vectors().unwrap();
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,std::sync::atomic::Ordering::Relaxed);
        for case in data["cases"].as_array().unwrap() {
            for body in [lifted::f_00e0e5e0 as LiftFn,mtcore::job_hash::dependency_hash] {
                for (a,b) in pairs(&case["initial"]) {
                    m.table.set_perm((a/4096) as usize,READ|WRITE);
                    m.table.backed_page((a/4096) as usize);
                    m.set_u8(a,b);
                }
                let mut c=cpu(&case["before"]);
                body(&mut m,&mut c);
                let expected=&case["after"];
                assert_eq!(regs(&c),std::array::from_fn(|i|n(&expected["regs"][i])));
                let mask=n(&expected["flag_mask"]);
                assert_eq!((c.fl.snapshot().0|if c.df {0x400} else {0})&mask,n(&expected["flags"])&mask);
                for(a,b) in pairs(&expected["memory"]) {assert_eq!(m.u8(a),b);}
            }
        }
    }
}
#[test]
fn access_order_and_fault_state() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let data=vectors().unwrap();
    let mut checked=0;
    for case in data["cases"].as_array().unwrap().iter().filter(|v| n(&v["length"])==16) {
        for fault in 1..=56 {
            let run=|body:LiftFn| {
                let mut m=Memory {bytes:pairs(&case["initial"]),fail_at:Some(fault),..Default::default()};
                let mut c=cpu(&case["before"]);
                let failed=catch_unwind(AssertUnwindSafe(|| body(&mut m,&mut c))).is_err();
                let flags=c.fl.snapshot().0;
                (failed,regs(&c),flags,m.writes,m.accesses.into_inner())
            };
            assert_eq!(run(lifted::f_00e0e5e0),run(mtcore::job_hash::dependency_hash),"fault {fault}");
            checked+=1;
        }
    }
    eprintln!("job_hash: {checked} paired fault/access cases");
}
#[test]
#[ignore = "bounded paired CPU microbenchmark; no game"]
fn paired_cost() {
    use ddnative_base::mem::{PageTable, READ, WRITE, ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    ATOMIC_ACCESSES.store(true,Relaxed);
    let mut m=PageMemory {table:Box::new(PageTable::default())};
    for page in [8,16,17,18] {m.table.set_perm(page,READ|WRITE);m.table.backed_page(page);}
    for a in 0x10000..0x13000 {m.set_u8(a,(a as u8).wrapping_mul(17));}
    let mut c=Cpu::default();
    m.set_u32(0x8800,0xdead000);
    m.set_u32(0x880c,0x811c9dc5);
    for length in [16u32,64,256,1024,4096] {
        for offset in [0u32,1,4093] {
            m.set_u32(0x8804,0x10000+offset);m.set_u32(0x8808,length);
            let iterations=(2_000_000/length).max(500);
            for round in 0..7 {
                for step in 0..2 {
                    let port=(round+step)%2==1;
                    let body=black_box(if port {mtcore::job_hash::dependency_hash as LiftFn} else {lifted::f_00e0e5e0});
                    let start=Instant::now();
                    for _ in 0..iterations {c.esp=0x8800;body(&mut m,&mut c);black_box(c.eax);}
                    let ns=start.elapsed().as_nanos() as f64/iterations as f64;
                    eprintln!("hash-cost length={length} offset={offset} round={round} port={} ns={ns:.3}",u8::from(port));
                }
            }
        }
    }
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, default=Path('E:/ddda-re/notes/native-L158/isolated'))
    p.add_argument('--bench', action='store_true')
    a = p.parse_args()
    repo = Path(__file__).resolve().parents[1]
    source = (repo / 'crates/ddlift/lift_28/src/generated/lifted_004.rs').read_text()
    start = source.index('pub fn f_00e0e5e0(')
    end = source.index('\n// Translated from ', start)
    body = source[start:end]
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out/'Cargo.toml').write_text(f'''[package]
name="l158-isolated"
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
''')
    test = f'''include!("{repo.as_posix()}/crates/mtcore/tests/job_hash.rs");
mod lifted {{
#![allow(unused_parens)]
use ddnative_base::{{mem::Mem,emit,x86::{{self,Cpu,Flags}}}};
{body}
}}
#[test]
fn original_lifted_vectors() {{ verify(lifted::f_00e0e5e0); }}
'''
    # Strip the included test's inner doc comment (include! disallows it).
    test = (repo/'crates/mtcore/tests/job_hash.rs').read_text().replace('//!','//') + test[test.index('\nmod lifted'):]
    (a.out/'isolated.rs').write_text(test + BENCH)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR='C:/ddda-targets/L158',
               CARGO_BUILD_JOBS='4', DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--manifest-path',str(a.out/'Cargo.toml'),'--release']
    subprocess.run(command,env=env,check=True)
    if a.bench:
        subprocess.run(command+['paired_cost','--','--ignored','--nocapture','--test-threads=1'],env=env,check=True)


if __name__ == '__main__':
    main()
