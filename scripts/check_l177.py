"""L177 state-commit isolation: ABI vectors, serial/atomic pages, paired CPU costs.

Extracted generated bodies and build products stay private. No shard build.
"""
import argparse
import os
from pathlib import Path
import subprocess

CHECK = r'''
struct PageMemory { table: Box<ddnative_base::mem::PageTable> }
impl Mem for PageMemory {
    fn page_table(&self)->Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {Some(std::ptr::NonNull::from(&*self.table))}
    fn u8(&self,a:u32)->u8 {let p=self.table.page((a/4096) as usize);assert!(!p.is_null());unsafe{ddnative_base::mem::load_at(p.add((a%4096) as usize),1) as u8}}
    fn set_u8(&mut self,a:u32,b:u8) {let p=self.table.page((a/4096) as usize);assert!(!p.is_null());unsafe{ddnative_base::mem::store_at(p.add((a%4096) as usize),1,b as u32)}}
}
fn cpu(v:&Value)->Cpu {
    let mut c=Cpu::default();
    [c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi]=std::array::from_fn(|i|n(&v["regs"][i]));
    c
}
fn install_calls(calls:Vec<Value>,_lift:bool) {
    // Decode the transcript outside timing. JSON traversal and BTreeMap
    // allocation per mocked memcpy would otherwise dominate the caller.
    struct Call {target:u32,owner:u32,stack:u32,args:Vec<u32>,effects:Vec<(u32,u8)>,result:u32,returned:u32}
    let calls:Vec<Call>=calls.iter().map(|v|Call {
        target:n(&v["target"]),owner:n(&v["regs"][1]),stack:n(&v["regs"][4]),
        args:v["args"].as_array().unwrap().iter().map(n).collect(),
        effects:pairs(&v["effects"]).into_iter().collect(),result:n(&v["result"]),returned:n(&v["return_esp"]),
    }).collect();
    let cursor=Cell::new(0usize);
    x86::set_ext_calls(Some(Box::new(move|m,c,target|{
        let call=&calls[cursor.get()%calls.len()];cursor.set(cursor.get()+1);
        assert_eq!(target,call.target);
        if target!=0x12efea0 {assert_eq!(c.ecx,call.owner);}assert_eq!(c.esp,call.stack);
        let p=ddnative_base::mem::Pages::of(m);
        for(i,&v) in call.args.iter().enumerate(){assert_eq!(p.u32(m,c.esp+4*i as u32),v);}
        for &(a,b) in &call.effects {p.set_u8(m,a,b);}
        c.eax=call.result;c.esp=call.returned;c.ecx=0xc1a0;c.edx=0xd1a0;
    })));
}
#[test]
fn original_lifted_vectors() {
    x86::set_lifted_lookup(resolve);
    for (entry,body) in [(0xfee320,lifted::f_00fee320 as LiftFn),(0xffc350,lifted::f_00ffc350),(0x1145bf0,lifted::f_01145bf0)] {
        verify(&format!("E:/ddda-re/notes/native-L177/{entry:08x}.json"),entry,body);
    }
}
#[test]
fn pages_and_cost() {
    use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
    use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
    ddnative_host::install();
    x86::set_lifted_lookup(resolve);
    let mut m=PageMemory{table:Box::new(PageTable::default())};
    for atomic in [false,true] {
        ATOMIC_ACCESSES.store(atomic,Relaxed);
        for(entry,bodies) in [
            (0xfee320,[lifted::f_00fee320 as LiftFn,mtcore::semantic_draw_c::commit_primitive_state]),
            (0xffc350,[lifted::f_00ffc350 as LiftFn,mtcore::semantic_draw_c::draw_model_mesh]),
            (0x1145bf0,[lifted::f_01145bf0 as LiftFn,mtcore::semantic_draw_c::apply_standard_material]),
        ] {
            let path=format!("E:/ddda-re/notes/native-L177/{entry:08x}.json");
            let data:Value=serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            for case in data["cases"].as_array().unwrap().iter().filter(|v|n(&v["entry"])==entry) {
                let seed=n(&case["seed"]);
                let initial=pairs(&case["initial"]);
                let mut expected=initial.clone();expected.extend(pairs(&case["after"]["memory"]));
                let calls=case["calls"].as_array().cloned().unwrap_or_default();
                // Both execution modes replay original bytes, including all heap writes.
                for(port,body) in bodies.iter().enumerate() {
                    for(&a,&b) in &initial {m.table.set_perm((a/4096) as usize,READ|WRITE);m.table.backed_page((a/4096) as usize);m.set_u8(a,b);}
                    install_calls(calls.clone(),port==0);
                    let mut c=cpu(&case["before"]);let _floating=floating_control(&mut c,&case["before"]);body(&mut m,&mut c);
                    assert_eq!([c.eax,c.ebx,c.esp,c.ebp,c.esi,c.edi],[0,3,4,5,6,7].map(|i|n(&case["after"]["regs"][i])),"entry {entry:x} seed {seed} port {port}");
                    for(&a,&b) in &expected {if observable(a,c.esp) {assert_eq!(m.u8(a),b,"entry {entry:x} seed {seed} port {port} byte {a:x}");}}
                }
                if !atomic || std::env::var_os("L177_BENCH").is_none() || ![1,2,3,31,32,39,63,64,99,127,143].contains(&seed) {continue;}
                // Restore written input dwords each iteration; included symmetrically
                // in timings. This bounds allocations/state logs without timing JSON.
                let mut reset=BTreeMap::new();
                for(a,_) in pairs(&case["after"]["memory"]) {if observable(a,n(&case["after"]["regs"][4])) {reset.insert(a&!3,());}}
                let stack=n(&case["before"]["regs"][4]);
                for a in (stack..stack+28).step_by(4) {if (0..4).all(|i|initial.contains_key(&(a+i))) {reset.insert(a,());}}
                let reset:Vec<(u32,u32)>=reset.keys().map(|&a|(a,u32::from_le_bytes(std::array::from_fn(|i|*initial.get(&(a+i as u32)).unwrap_or(&0))))).collect();
                for round in 0..7 {for step in 0..2 {
                    let port=(round+step)%2;
                    for(&a,&b) in &initial {m.set_u8(a,b);}
                    install_calls(calls.clone(),port==0);
                    let mut c=cpu(&case["before"]);let ecx=c.ecx;let esp=c.esp;
                    let _floating=floating_control(&mut c,&case["before"]);
                    let body=black_box(bodies[port]);let start=Instant::now();
                    for _ in 0..10000 {
                        for &(a,v) in &reset {ddnative_base::mem::Pages::of(&m).set_u32(&mut m,a,v);}
                        c.ecx=ecx;c.esp=esp;body(&mut m,&mut c);black_box(c.eax);
                    }
                    eprintln!("L177-cost entry={entry:08x} seed={seed} round={round} port={port} ns={:.3}",start.elapsed().as_nanos() as f64/10000.0);
                }}
            }
            eprintln!("L177 pages entry={entry:08x} atomic={atomic} PASS");
        }
    }
    x86::set_ext_calls(None);ATOMIC_ACCESSES.store(false,Relaxed);
}
fn resolve(_target:u32)->Option<LiftFn> {None}
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bench',action='store_true')
    args=parser.parse_args()
    repo=Path(__file__).resolve().parents[1]
    out=Path('E:/ddda-re/notes/native-L177/isolated');out.mkdir(parents=True,exist_ok=True)
    # Locate each body from source; shard allocation can change after regeneration.
    bodies=[]
    for entry in [0xfee320,0xffc350,0x1145bf0]:
        needle=f'pub fn f_{entry:08x}('
        matches=subprocess.check_output(['rg','-l','-F',needle,str(repo/'crates/ddlift')],text=True).splitlines()
        assert len(matches)==1,(entry,matches)
        source=Path(matches[0]).read_text();start=source.index(needle)
        end=source.find('\n// Translated from ',start)
        body=source[start:end if end>=0 else len(source)]
        body=body.replace('super::lifted::f_00fed940','crate::platform::view_matrix')
        bodies.append(body)
    # Route the local memcpy binding through the same recorded seam as the
    # semantic implementation; retain the lifted caller's instructions.
    seam='mod platform {pub fn memcpy(m:&mut dyn crate::Mem,c:&mut crate::Cpu){crate::x86::call_ext(m,c,0x12efea0);} pub fn view_matrix(m:&mut dyn crate::Mem,c:&mut crate::Cpu){crate::x86::call_ext(m,c,0xfed940);}}\n'
    (out/'lifted.rs').write_text('#![allow(warnings)]\npub use ddnative_base::mem;\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'+seam+'\n'.join(bodies))
    manifest=f'''[package]
name="l177-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{repo.as_posix()}/crates/ddnative-host"}}
mtcore={{path="{repo.as_posix()}/crates/mtcore"}}
serde_json="1"
lifted={{path="baseline"}}
[profile.release]
opt-level=3
lto=false
codegen-units=1
[profile.release.package.lifted]
opt-level=2
[[test]]
name="isolated"
path="isolated.rs"
'''
    (out/'baseline').mkdir(exist_ok=True)
    (out/'baseline/Cargo.toml').write_text(f'''[package]
name="lifted"
version="0.1.0"
edition="2021"
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
[lib]
path="../lifted.rs"
''')
    (out/'Cargo.toml').write_text(manifest)
    test=(repo/'crates/mtcore/tests/semantic_draw_c.rs').read_text().replace('//!','//')
    (out/'isolated.rs').write_text(test+CHECK)
    env=dict(os.environ,CARGO_INCREMENTAL='0',CARGO_TARGET_DIR='C:/ddda-targets/L177',CARGO_BUILD_JOBS='4',DDNATIVE_REQUIRE_VECTORS='1')
    if args.bench:env['L177_BENCH']='1'
    subprocess.run(['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release','--','--nocapture','--test-threads=1'],env=env,check=True)

if __name__=='__main__':main()
