"""Isolated original/lifted/port material specialization experiment, no shard build or game run."""
import argparse
import os
import re
from pathlib import Path
import subprocess

CHECK = r'''
#[test]
fn original_lifted_material_vectors() {
    install();verify(lifted::f_00ea5b50,0xea5b50);
}
#[test]
fn material_fault_and_access_order() {
    install();
    use std::panic::{catch_unwind,AssertUnwindSafe};
    std::panic::set_hook(Box::new(|_|{}));
    let data=vectors().unwrap(); let mut total=0;
    for case in data["cases"].as_array().unwrap().iter().step_by(37) {
        for fault in 1..=1800 {
            let run=|body:LiftFn| {
                let mut m=Memory{bytes:pairs(&case["initial"]),fail_at:Some(fault),..Default::default()};
                let mut c=cpu(&case["before"]); ALLOCATIONS.with(|v| *v.borrow_mut()=0);
                let failed=catch_unwind(AssertUnwindSafe(||body(&mut m,&mut c))).is_err();
                (failed,regs(&c),c.fl.snapshot().0,m.writes,m.accesses.into_inner())
            };
            assert_eq!(run(lifted::f_00ea5b50),run(material_fast::configure_material),"seed {} fault {fault}",case["seed"]);
            total+=1;
        }
    }
    eprintln!("material: {total} paired access/fault positions");
}
#[test]
fn page_vectors_and_optional_cost() {
    use ddnative_base::mem::{READ,WRITE,ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    install();
    let data=vectors().unwrap();
    let mut m=guest_memory::GuestMemory::new();
    let mut mapped=std::collections::BTreeSet::new();
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for case in data["cases"].as_array().unwrap() {
            let bodies:[LiftFn;2]=[lifted::f_00ea5b50,material_fast::configure_material];
            for body in bodies {
                for(a,b) in pairs(&case["initial"]) {
                    if mapped.insert(a/4096) { m.map(a&!4095,4096,READ|WRITE).unwrap(); }
                    m.set_u8(a,b);
                }
                let mut c=cpu(&case["before"]); ALLOCATIONS.with(|v| *v.borrow_mut()=0);body(&mut m,&mut c);
                let expected=&case["after"];
                assert_eq!(regs(&c),std::array::from_fn(|i|n(&expected["regs"][i])));
                let mask=n(&expected["flag_mask"]);
                assert_eq!((c.fl.snapshot().0|if c.df {0x400} else {0})&mask,n(&expected["flags"])&mask);
                for(a,b) in pairs(&expected["memory"]) {assert_eq!(m.u8(a),b);}
            }
            if atomic && std::env::var_os("L167_MATERIAL_BENCH").is_some()
                && (n(&case["seed"])%61==0 || [1,7,8,15,31,63,127,255,511].contains(&n(&case["seed"]))) {
                TRACE_CALLS.with(|v|v.set(false));
                let initial=cpu(&case["before"]);
                let bytes=pairs(&case["initial"]);
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;let body=black_box(bodies[port]);
                    let mut total=0u128;
                    for _ in 0..2000 {
                        for (&a,&b) in &bytes {m.set_u8(a,b);}
                        let mut c=initial.clone();ALLOCATIONS.with(|v| *v.borrow_mut()=0);
                        let start=Instant::now();body(&mut m,&mut c);total+=start.elapsed().as_nanos();black_box(c.eax);
                    }
                    eprintln!("material-cost seed={} round={round} port={port} ns={:.3}",
                        case["seed"],total as f64/2000.0);
                }}
                TRACE_CALLS.with(|v|v.set(true));
            }
        }
    }
}
'''


FAST_HELPERS = r'''
mod emit {
    use super::*;
    pub use ddnative_base::emit::xorps;
    #[inline] pub fn push(v:u32,m:&mut dyn Mem,p:Pages,c:&mut Cpu) { c.esp=c.esp.wrapping_sub(4);p.set_u32(m,c.esp,v); }
    #[inline] pub fn pop(m:&mut dyn Mem,p:Pages,c:&mut Cpu)->u32 { let v=p.u32(m,c.esp);c.esp=c.esp.wrapping_add(4);v }
    #[inline] pub fn load64(m:&mut dyn Mem,p:Pages,a:u32)->[u32;4] { [p.u32(m,a),p.u32(m,a.wrapping_add(4)),0,0] }
    #[inline] pub fn store64(v:[u32;4],m:&mut dyn Mem,p:Pages,a:u32) {p.set_u32(m,a,v[0]);p.set_u32(m,a.wrapping_add(4),v[1]);}
}
'''
SEAMS = r'''
thread_local! {
    static CALLS: RefCell<Vec<(u32,[u32;8],[[u32;4];8])>> = const {RefCell::new(Vec::new())};
    static TRACE_CALLS: std::cell::Cell<bool> = const {std::cell::Cell::new(true)};
    static ALLOCATIONS: RefCell<u32> = const {RefCell::new(0)};
}
fn seam(m:&mut dyn Mem,c:&mut Cpu,target:u32) {
    if TRACE_CALLS.with(|v|v.get()) { CALLS.with(|v|v.borrow_mut().push((target,regs(c),c.xmm))); }
    let cleanup=match target {
        0xfed890 => {
            let count=m.u32(c.esp+4).wrapping_add(4095)&!4095;
            let dest=ALLOCATIONS.with(|v| { let mut n=v.borrow_mut();*n+=1;0xe080000+*n*4096 });
            m.set_u32(c.ecx+0x24,dest);m.set_u32(c.ecx+0x28,dest+count);c.eax=dest;4
        },
        0x12efea0 => {
            let dst=m.u32(c.esp+4);let src=m.u32(c.esp+8);let count=m.u32(c.esp+12);
            for i in 0..count {let b=m.u8(src+i);m.set_u8(dst+i,b);}c.eax=dst;0
        },
        0xfe66a0|0xe100000=>16,
        0xff1310=>4,
        _=>panic!("unexpected seam {target:x}"),
    };
    c.esp=c.esp.wrapping_add(4+cleanup);
}
fn install() { x86::set_ext_calls(Some(Box::new(seam))); }
#[test] fn original_material_candidate() {
    install();verify(material_fast::configure_material,0xea5b50);
}
'''

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bench', action='store_true')
    p.add_argument('--flatten', action='store_true', help='remove only generated part-dispatch boundaries')
    args = p.parse_args()
    repo = Path(__file__).resolve().parents[1]
    out = Path('E:/ddda-re/notes/native-L167/material-isolated')
    out.mkdir(parents=True, exist_ok=True)
    source = (repo/'crates/ddlift/lift_29/src/generated/lifted_010.rs').read_text()
    bodies = []
    for entry in ('00ea5b50',):
        start = source.index('pub fn f_'+entry+'(')
        end = source.index('\n// Translated from ', start)
        bodies.append(source[start:end])
    manifest = f'''[package]
name="l167-material-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
material-fast={{path="material-fast"}}
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
[profile.release.package.material-fast]
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
    fast=out/'material-fast'
    fast.mkdir(exist_ok=True)
    (fast/'Cargo.toml').write_text(f'''[package]
name="material-fast"
version="0.1.0"
edition="2021"
[lib]
path="lib.rs"
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
''')
    body=bodies[0]
    if args.flatten:
        blocks=[line for line in body.splitlines() if re.match(r'\s+0x[0-9A-F]+ =>',line)]
        assert len(blocks)==112
        body='pub fn f_00ea5b50(m: &mut dyn Mem, cpu: &mut Cpu) { let pt = m.page_table(); let mut pc=0xEA5B50; loop { match pc {\n'+'\n'.join(blocks).replace('return None','return')+'\n_=>unreachable!(), } } }'
    candidate=body.replace('f_00ea5b50','configure_material').replace('let pt = m.page_table();','let pt = Pages::of(m);')
    candidate=candidate.replace('Option<std::ptr::NonNull<crate::mem::PageTable>>','Pages')
    for name in ('u8','u16','u32','set_u32'):
        candidate=candidate.replace('m.fast_'+name+'(pt,','pt.'+name+'(m,')
    candidate=candidate.replace('x86::call_local(m, cpu, 0x012EFEA0, crate::platform::memcpy)','x86::call_ext(m, cpu, 0x012EFEA0)')
    (fast/'lib.rs').write_text('use ddnative_base::{mem::{Mem,Pages},x86::{self,Cpu}};\n'+candidate+FAST_HELPERS)
    test=(repo/'crates/mtcore/tests/job_shadow.rs').read_text().replace('//!','//')
    test=test[:test.index('// Explicit unowned')]
    test=test.replace('DDNATIVE_JOB_SHADOW_VECTORS','DDNATIVE_SHADOW_MATERIAL_VECTORS').replace('native-L167/job_shadow.json','native-L167/shadow_material.json').replace('>= 432','>= 576').replace('job_shadow','shadow_material')
    test=test.replace('CALLS.with(|v| v.borrow_mut().clear());','CALLS.with(|v| v.borrow_mut().clear()); ALLOCATIONS.with(|v| *v.borrow_mut()=0);')
    # Replace the call comparison with target/register/SIMD boundary checks.
    start=test.index('        CALLS.with(|v| {')
    stop=test.index('        assert_eq!(\n            regs(&c)',start)
    test=test[:start]+'''        CALLS.with(|v| {
            let calls=v.borrow();let want=case["calls"].as_array().unwrap();
            assert_eq!(calls.len(),want.len());
            for (got,w) in calls.iter().zip(want) {
                assert_eq!(got.0,n(&w["target"]));
                assert_eq!(got.1,std::array::from_fn(|i|n(&w["regs"][i])));
                assert_eq!(got.2,std::array::from_fn(|i|std::array::from_fn(|j|n(&w["xmm"][i][j]))));
            }
        });
'''+test[stop:]
    test+=SEAMS
    test+='\nmod mem { pub use ddnative_base::mem::*; }\nmod lifted {\n#![allow(warnings)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'
    test+='\n'.join(bodies)+'\n}\nmod platform { pub fn memcpy(m: &mut dyn ddnative_base::mem::Mem,c: &mut ddnative_base::x86::Cpu) { ddnative_base::x86::call_ext(m,c,0x12efea0); } }\n'+CHECK
    (out/'isolated.rs').write_text(test)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR='C:/ddda-targets/L167',
               CARGO_BUILD_JOBS='4', DDNATIVE_REQUIRE_VECTORS='1')
    if args.bench: env['L167_MATERIAL_BENCH']='1'
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release',
                    '--','--nocapture','--test-threads=1'],env=env,check=True)


if __name__ == '__main__': main()

