"""Isolated original lifted worker bodies vs opt-in ports; never builds shards."""
import argparse
import os
import re
import statistics
from pathlib import Path
import subprocess


def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--bench',action='store_true');args=ap.parse_args()
    root=Path(__file__).resolve().parents[1];out=Path('C:/ddda-targets/L165/workers-check')
    (out/'original/src').mkdir(parents=True,exist_ok=True)
    source=(root/'crates/ddlift/lift_00/src/generated/lifted_026.rs').read_text(encoding='utf-8')
    start=source.index('pub fn f_01054700(');end=source.index('// Translated from',start)
    bodies=source[start:end]
    source=(root/'crates/ddlift/lift_31/src/generated/lifted_036.rs').read_text(encoding='utf-8')
    start=source.index('pub fn f_00fedd60(');end=source.index('// Translated from',start)
    bodies+=source[start:end]
    boundary='mod platform { pub fn memcpy(m:&mut dyn crate::mem::Mem,c:&mut crate::x86::Cpu){crate::x86::call_ext(m,c,0x012efea0)} }\n'
    (out/'original/src/lib.rs').write_text('#![allow(warnings)]\npub use ddnative_base::{mem,x86};\nuse mem::Mem;use x86::{Cpu,Flags};use ddnative_base::emit;\n'+boundary+bodies,encoding='utf-8')
    (out/'original/Cargo.toml').write_text(f'''[package]
name="l165-original-workers"
version="0.0.0"
edition="2021"
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
''',encoding='utf-8')
    (out/'Cargo.toml').write_text(f'''[package]
name="l165-workers-check"
version="0.0.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
mtcore={{path="{root.as_posix()}/crates/mtcore"}}
l165-original-workers={{path="original"}}
serde_json="1"
[[test]]
name="workers"
path="workers.rs"
[profile.release]
opt-level=3
lto=false
[profile.release.package.l165-original-workers]
opt-level=1
''',encoding='utf-8')
    test=(root/'crates/mtcore/tests/job_workers.rs').read_text(encoding='utf-8').replace('//!','//')
    test+='\n#[test]fn original_lifted_joint_vectors(){verify(l165_original_workers::f_01054700,false);}\n'
    test+='\n#[test]fn original_lifted_joint_pages(){verify(l165_original_workers::f_01054700,true);}\n'
    test+='\n#[test]fn original_lifted_sort_vectors(){verify_kind(l165_original_workers::f_00fedd60,false,"sort",800);}\n'
    test+='\n#[test]fn original_lifted_sort_pages(){verify_kind(l165_original_workers::f_00fedd60,true,"sort",800);}\n'
    (out/'workers.rs').write_text(test+BENCH,encoding='utf-8')
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L165',CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='4',DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--release','--manifest-path',str(out/'Cargo.toml')]
    subprocess.run(command+['--','--nocapture','--test-threads=1'],env=env,check=True)
    if args.bench:
        result=subprocess.run(command+['paired_cost','--','--ignored','--nocapture','--test-threads=1'],env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        print(result.stdout,flush=True)
        samples={}
        pattern=r'joint-cost count=(\d+) mode=(\d+) skipped=(\w+) round=(\d+) port=(\d+) ns=([\d.]+)'
        for count,mode,skipped,round_,port,ns in re.findall(pattern,result.stdout):
            samples.setdefault((int(count),int(mode),skipped),{}).setdefault(int(round_),{})[int(port)]=float(ns)
        assert len(samples)==20
        for key,rounds in sorted(samples.items()):
            assert len(rounds)==7 and all(set(pair)=={0,1} for pair in rounds.values())
            ratios=[pair[0]/pair[1]for pair in rounds.values()]
            median=statistics.median(ratios)
            print(f'joint-speed count/mode/skipped={key}: median={median:.3f}x min={min(ratios):.3f}x max={max(ratios):.3f}x',flush=True)
            assert median>=1.5,(key,median)


BENCH=r'''
struct PagesMemory{table:Box<ddnative_base::mem::PageTable>}
impl Mem for PagesMemory {
 fn page_table(&self)->Option<std::ptr::NonNull<ddnative_base::mem::PageTable>>{Some(std::ptr::NonNull::from(&*self.table))}
 fn u8(&self,a:u32)->u8 {let p=self.table.page((a/4096)as usize);assert!(!p.is_null());unsafe{*p.add((a%4096)as usize)}}
 fn set_u8(&mut self,a:u32,b:u8){let p=self.table.page((a/4096)as usize);assert!(!p.is_null());unsafe{*p.add((a%4096)as usize)=b}}
}
#[test]
#[ignore="isolated synthetic dispatcher CPU cost, original callees excluded"]
fn paired_cost(){
 use std::{hint::black_box,time::Instant,sync::atomic::Ordering::Relaxed};
 use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
 ATOMIC_ACCESSES.store(true,Relaxed);
 let mut m=PagesMemory{table:Box::new(PageTable::default())};
 for page in 8..64{m.table.set_perm(page,READ|WRITE);m.table.backed_page(page);}
 let model=0x10000;let bank=0x14000;let indices=0x12000;let sp=0x9000;
 m.set_u32(model+0x364,bank);m.set_u32(model+0x378,0x08000000);m.set_u8(model+0xe19,0);
 for i in 0..256 {m.set_u8(indices+i,i as u8);m.set_u8(bank+i*0xa0+0x96,1);}
 for count in [1,8,32,64,128] {for mode in [0,1] {for skipped in [false,true] {
  for i in 0..256{m.set_u8(bank+i*0xa0+0x90,if skipped{0x20}else{0});}
  m.set_u32(sp+4,indices);m.set_u32(sp+8,count);m.set_u32(sp+12,mode);
  for round in 0..7 {for step in 0..2 {
   let port=(round+step)%2==1;
   let f=black_box(if port{mtcore::job_joints::evaluate_joint_indices as LiftFn}else{l165_original_workers::f_01054700});
   let mut c=Cpu::default();let iterations=1000000/count;let t=Instant::now();
   for _ in 0..iterations {m.set_u32(sp+12,mode);c.esp=sp;c.ecx=model;f(&mut m,&mut c);black_box(c.eax);}
   eprintln!("joint-cost count={count} mode={mode} skipped={skipped} round={round} port={} ns={:.3}",u8::from(port),t.elapsed().as_nanos()as f64/iterations as f64);
  }}
 }}}
}
'''

if __name__=='__main__':main()
