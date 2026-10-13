"""L169: compile only DB63E0, never the ddlift shards or the game."""
import argparse
import os
from pathlib import Path
import subprocess

FAULTS = r'''
#[test]
fn access_order_and_faults() {
    use std::panic::{catch_unwind,AssertUnwindSafe};
    let data=vectors().unwrap();
    let hook=std::panic::take_hook();
    std::panic::set_hook(Box::new(|_|{}));
    let result=catch_unwind(AssertUnwindSafe(|| {
        let mut checked=0;
        for case in data["cases"].as_array().unwrap().iter().filter(|v| n(&v["width"])==9 && n(&v["groups"])==1 && n(&v["filter"])==0) {
            let run=|body:LiftFn,fault:Option<usize>| {
                let mut m=Memory {bytes:pairs(&case["initial"]),fail_at:fault,..Default::default()};
                let mut c=cpu(&case["before"]);
                let events=case["events"].as_array().unwrap().clone();
                let pos=Rc::new(RefCell::new(0usize));
                let position=pos.clone();
                let calls=Rc::new(RefCell::new(Vec::new()));
                let log=calls.clone();
                x86::set_ext_calls(Some(Box::new(move |m,c,target| {
                    log.borrow_mut().push((target,regs(c),c.fl.snapshot().0));
                    let i=*position.borrow();
                    assert_eq!(target,n(&events[i]["target"]));
                    for(a,b) in pairs(&events[i+1]["effects"]) {m.set_u8(a,b);}
                    *c=cpu(&events[i+1]);
                    *position.borrow_mut()+=2;
                })));
                let failed=catch_unwind(AssertUnwindSafe(||body(&mut m,&mut c))).is_err();
                x86::set_ext_calls(None);
                let trace=calls.borrow().clone();
                (failed,regs(&c),c.fl.snapshot().0,m.writes,m.accesses.into_inner(),trace)
            };
            let length=run(lifted::f_00db63e0,None).4.len();
            for fault in 1..=length+1 {
                assert_eq!(run(lifted::f_00db63e0,Some(fault)),run(mtcore::shadow_traversal::traverse,Some(fault)),"seed {} fault {fault}",case["seed"]);
                checked+=1;
            }
        }
        eprintln!("shadow traversal: {checked} fault/access pairs match");
    }));
    std::panic::set_hook(hook);
    result.unwrap();
}
'''

PAGES = r'''
#[test]
fn page_vectors_and_cost() {
    use ddnative_base::mem::{READ,WRITE,ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    let data=vectors().unwrap();
    let mut m=guest_memory::GuestMemory::new();
    let mut mapped=std::collections::BTreeSet::new();
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for case in data["cases"].as_array().unwrap() {
            let initial=pairs(&case["initial"]);
            let before=cpu(&case["before"]);
            let events=case["events"].as_array().unwrap().clone();
            let bodies:[LiftFn;2]=[lifted::f_00db63e0,mtcore::shadow_traversal::traverse];
            for body in bodies {
                for(&a,&b) in &initial {
                    if mapped.insert(a/4096) {m.map(a&!4095,4096,READ|WRITE).unwrap();}
                    m.set_u8(a,b);
                }
                let mut c=before.clone();
                let pos=Rc::new(RefCell::new(0usize));
                let cursor=pos.clone();let trace=events.clone();
                x86::set_ext_calls(Some(Box::new(move |m,c,target| {
                    let i=*cursor.borrow();
                    assert_eq!(target,n(&trace[i]["target"]));
                    check(m,c,&trace[i],"page callee");
                    for(a,b) in pairs(&trace[i+1]["effects"]) {m.set_u8(a,b);}
                    *c=cpu(&trace[i+1]);*cursor.borrow_mut()+=2;
                })));
                body(&mut m,&mut c);
                x86::set_ext_calls(None);
                assert_eq!(*pos.borrow(),events.len());
                check(&m,&c,&case["after"],"page return");
            }
            if atomic && std::env::var_os("L169_BENCH").is_some()
                && n(&case["variant"])==0 && n(&case["filter"])==0 && n(&case["width"])!=0 {
                // Replay explicit synthetic callees; no JSON, allocations or
                // validation inside the timed loop. Includes dispatch cost.
                let returns:Vec<_>=events.iter().skip(1).step_by(2).map(cpu).collect();
                let cursor=Rc::new(std::cell::Cell::new(0usize));let pos=cursor.clone();
                x86::set_ext_calls(Some(Box::new(move |_,c,_| {
                    let i=pos.get();*c=returns[i].clone();pos.set(i+1);
                })));
                let queue_count=u32::from_le_bytes(std::array::from_fn(|i|*initial.get(&(0xe00408c+i as u32)).unwrap_or(&0)));
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;let body=black_box(bodies[port]);
                    let start=Instant::now();
                    for _ in 0..50000 {
                        let mut c=before.clone();cursor.set(0);
                        m.set_u32(0xe00408c,queue_count);
                        body(&mut m,&mut c);black_box(c.eax);
                    }
                    eprintln!("traversal-cost groups={} width={} mode={} round={round} port={port} ns={:.3}",
                        case["groups"],case["width"],case["mode"],start.elapsed().as_nanos() as f64/50000.0);
                }}
                x86::set_ext_calls(None);
            }
        }
    }
}
'''


def pass_checks():
    """Reuse the same fault/page contracts for EFD240's corpus schema."""
    faults=FAULTS.replace('let data=vectors().unwrap();','install(); let data=vectors().unwrap();')
    faults=faults.replace('.filter(|v| n(&v["width"])==9 && n(&v["groups"])==1 && n(&v["filter"])==0)', '.step_by(8)')
    faults=faults.replace('*c=cpu(&events[i+1]);','release(c); *c=cpu(&events[i+1]);')
    faults=faults.replace('(failed,regs(&c),c.fl.snapshot().0,m.writes,m.accesses.into_inner(),trace)',
        'let state=(regs(&c),c.fl.snapshot().0,c.xmm,c.mxcsr.0,c.st.clone().state); release(&mut c);\n                (failed,state,m.writes,m.accesses.into_inner(),trace)')
    faults=faults.replace('.4.len()', '.3.len()')
    pages=PAGES.replace('let data=vectors().unwrap();','install(); let data=vectors().unwrap();')
    pages=pages.replace('*c=cpu(&trace[i+1]);','release(c); *c=cpu(&trace[i+1]);')
    pages=pages.replace('check(&m,&c,&case["after"],"page return");','check(&m,&c,&case["after"],"page return"); release(&mut c);')
    pages=pages.replace('&& n(&case["variant"])==0 && n(&case["filter"])==0 && n(&case["width"])!=0',
        '&& n(&case["alignment"])==0')
    pages=pages.replace('let i=pos.get();*c=returns[i].clone();', 'let i=pos.get();release(c); *c=returns[i].clone();')
    pages=pages.replace('let queue_count=u32::from_le_bytes(std::array::from_fn(|i|*initial.get(&(0xe00408c+i as u32)).unwrap_or(&0)));',
        'let reset:Vec<_>=[0x24u32,0x28,0x5c,0x6c,0x154,0x3848].into_iter().map(|off| {let a=0xe010000+off; (a,u32::from_le_bytes(std::array::from_fn(|i|*initial.get(&(a+i as u32)).unwrap_or(&0))))}).collect();')
    pages=pages.replace('m.set_u32(0xe00408c,queue_count);','for &(a,b) in &reset {m.set_u32(a,b);}')
    pages=pages.replace('body(&mut m,&mut c);black_box(c.eax);','body(&mut m,&mut c);black_box(c.eax);release(&mut c);')
    pages=pages.replace('traversal-cost groups={} width={} mode={}', 'pass-cost recording={} matched={} variant={}')
    pages=pages.replace('case["groups"],case["width"],case["mode"]', 'case["recording"],case["matched"],case["variant"]')
    return (faults+pages).replace('lifted::f_00db63e0','lifted::f_00efd240').replace('mtcore::shadow_traversal::traverse','mtcore::shadow_pass::setup')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L169/isolated'))
    parser.add_argument('--bench',action='store_true')
    parser.add_argument('--pass-setup',action='store_true',help='EFD240 isolated oracle')
    args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1]
    source_path='crates/ddlift/lift_29/src/generated/lifted_035.rs' if args.pass_setup else 'crates/ddlift/lift_27/src/generated/lifted_021.rs'
    entry='00efd240' if args.pass_setup else '00db63e0'
    topic='shadow_pass' if args.pass_setup else 'shadow_traversal'
    source=(repo/source_path).read_text()
    start=source.index('pub fn f_'+entry+'(')
    body=source[start:source.index('\n// Translated from ',start)]
    args.out.mkdir(parents=True,exist_ok=True)
    (args.out/'Cargo.toml').write_text(f'''[package]
name="l169-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{repo.as_posix()}/crates/ddnative-host"}}
mtcore={{path="{repo.as_posix()}/crates/mtcore"}}
serde_json="1"
guest-memory={{path="guest-memory"}}
[profile.release]
opt-level=1
lto=false
codegen-units=1
[profile.release.package.mtcore]
opt-level=3
[profile.release.package.ddnative-base]
opt-level=3
[profile.release.package.ddnative-host]
opt-level=3
[profile.release.package.guest-memory]
opt-level=3
[[test]]
name="isolated"
path="isolated.rs"
''')
    guest=args.out/'guest-memory'
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
    # Production memory accessors at opt3; unexpected preemption fails closed.
    memory=(repo/'crates/ddruntime/src/memory.rs').read_text().split('#[cfg(test)]')[0]
    memory+='\nmod threads {pub fn preempt_entry(){panic!("unexpected isolated preemption");}}\n'
    (guest/'lib.rs').write_text(memory)
    tests=(repo/f'crates/mtcore/tests/{topic}.rs').read_text().replace('//!','//')
    tests+='\nmod lifted {\n#![allow(unused_parens)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'+body+'\n}\n'
    tests+=f'#[test]\nfn original_lifted_vectors() {{verify(lifted::f_{entry});}}\n'
    (args.out/'isolated.rs').write_text(tests+(pass_checks() if args.pass_setup else FAULTS+PAGES))
    env=dict(os.environ,CARGO_INCREMENTAL='0',CARGO_TARGET_DIR='C:/ddda-targets/L169',CARGO_BUILD_JOBS='4',DDNATIVE_REQUIRE_VECTORS='1')
    if args.bench: env['L169_BENCH']='1'
    subprocess.run(['cargo','test','--manifest-path',str(args.out/'Cargo.toml'),'--release','--','--nocapture','--test-threads=1'],env=env,check=True)


if __name__=='__main__': main()
