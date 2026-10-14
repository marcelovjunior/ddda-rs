"""Compile only the three lifted bodies and ABI replay/benchmark, never shards."""
import os
from pathlib import Path
import subprocess

EXTRA=r'''
fn lookup(a:u32)->Option<LiftFn>{match a {0x67fb90=>Some(|m,c|x86::call_ext(m,c,0x67fb90)),0xc6cbd0=>Some(|m,c|x86::call_ext(m,c,0xc6cbd0)),_=>None}}
#[test] fn production_pages_and_cost(){
    use ddnative_base::mem::{READ,WRITE,ATOMIC_ACCESSES};
    use std::{collections::BTreeSet,sync::atomic::Ordering::Relaxed,time::Instant,hint::black_box,cell::Cell};
    ddnative_host::install();x86::set_lifted_lookup(lookup);
    let mut m=guest_memory::GuestMemory::new();let mut mapped=BTreeSet::new();
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for (entry,bodies) in [(0xff1920,[lifted::f_00ff1920 as LiftFn,mtcore::semantic_draw_a::shadow_draws]),(0xff14d0,[lifted::f_00ff14d0 as LiftFn,mtcore::semantic_draw_a::replay_draw_passes]),(0xfefb90,[lifted::f_00fefb90 as LiftFn,mtcore::semantic_draw_a::bind_render_target])] {
            let data=vectors(entry).unwrap();
            for case in data["cases"].as_array().unwrap() {
                let initial=pairs(&case["initial"]);
                for &a in initial.keys(){if mapped.insert(a/4096){m.map(a&!4095,4096,READ|WRITE).unwrap();}}
                for body in bodies {
                    for (&a,&b) in &initial {m.set_u8(a,b);}
                    let mut c=cpu(&case["before"]);let cursor=install_calls(case);x86::ldmxcsr(c.mxcsr.0);body(&mut m,&mut c);x86::set_ext_calls(None);
                    assert_eq!(*cursor.borrow(),case["events"].as_array().unwrap().len());check(&c,&m,case);
                }
                if !atomic || std::env::var_os("L173_BENCH").is_none() || n(&case["count"])!=12 || ![10,14,18,30].contains(&n(&case["variant"])) {continue;}
                let events:Vec<_>=case["events"].as_array().unwrap().chunks(2).map(|pair|(n(&pair[0]["target"]),regs(&pair[1]),pairs(&pair[1]["effects"]))).collect();
                let cursor=Rc::new(Cell::new(0usize));let position=cursor.clone();
                x86::set_ext_calls(Some(Box::new(move|m,c,target|{
                    let k=position.get();let (want,r,effects)=&events[k];assert_eq!(target,*want);
                    for (&a,&b) in effects {m.set_u8(a,b);}c.eax=r[0];c.ecx=r[1];c.edx=r[2];c.esp=r[4];position.set(k+1);
                })));
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;let body=black_box(bodies[port]);let mut elapsed=0u128;
                    for _ in 0..200 {
                        for (&a,&b) in &initial {m.set_u8(a,b);}
                        let mut c=cpu(&case["before"]);cursor.set(0);x86::ldmxcsr(c.mxcsr.0);
                        let t=Instant::now();body(&mut m,&mut c);elapsed+=t.elapsed().as_nanos();black_box(c.eax);
                    }
                    eprintln!("cost {entry:08x} variant={} round={round} port={port} ns={:.2}",case["variant"],elapsed as f64/200.0);
                }}
                x86::set_ext_calls(None);
            }
        }
    }
    ATOMIC_ACCESSES.store(false,Relaxed);
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
}
'''

def main():
    repo=Path(__file__).resolve().parents[1]
    out=Path('E:/ddda-re/notes/native-L173/isolated');out.mkdir(parents=True,exist_ok=True)
    source=(repo/'crates/ddlift/lift_31/src/generated/lifted_037.rs').read_text()
    bodies=[]
    for entry in ('00ff1920','00ff14d0','00fefb90'):
        start=source.index('pub fn f_'+entry+'(');end=source.find('\n// Translated from ',start);end=len(source) if end<0 else end
        bodies.append(source[start:end].replace('crate::mem::','ddnative_base::mem::'))
    (out/'Cargo.toml').write_text(f'''[package]
name="l173-isolated"
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
opt-level=2
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
    guest=out/'guest-memory';guest.mkdir(exist_ok=True)
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
    memory=(repo/'crates/ddruntime/src/memory.rs').read_text().split('#[cfg(test)]')[0]
    (guest/'lib.rs').write_text(memory+'\nmod threads {pub fn preempt_entry(){panic!("unexpected preemption");}}\n')
    test=(repo/'crates/mtcore/tests/semantic_draw_a.rs').read_text().replace('//!','//')
    test+='\nmod lifted {\n#![allow(warnings)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'+'\n'.join(bodies)+'\n}\n'
    test+='''
#[test] fn lifted_shadow(){x86::set_lifted_lookup(lookup);verify(0xff1920,lifted::f_00ff1920);}
#[test] fn lifted_passes(){verify(0xff14d0,lifted::f_00ff14d0);}
#[test] fn lifted_target(){verify(0xfefb90,lifted::f_00fefb90);}
mod platform { pub fn memcpy(m:&mut dyn ddnative_base::mem::Mem,c:&mut ddnative_base::x86::Cpu){ddnative_base::x86::call_ext(m,c,0x12efea0);} }
'''
    (out/'isolated.rs').write_text(test+EXTRA)
    env=dict(os.environ,CARGO_INCREMENTAL='0',CARGO_TARGET_DIR='C:/ddda-targets/L173',CARGO_BUILD_JOBS='4',DDNATIVE_REQUIRE_VECTORS='1')
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release','--','--nocapture','--test-threads=1'],env=env,check=True)

if __name__=='__main__':main()



