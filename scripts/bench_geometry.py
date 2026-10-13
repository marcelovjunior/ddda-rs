"""Prepare a private isolated FE9580 original-vector replay (no ddlift shards).

Emits the whole function twice at opt-level 1, with/without L109c fusion,
and compares the readable port at opt-level 3.
Generated source and vectors remain private. Run cargo with the task's
target dir, INCREMENTAL=0 and JOBS=4. This is not an end-to-end game timing.
"""
from pathlib import Path
import re
import sys
from unittest.mock import patch

from scan_float_compare import functions
import lift

ROOT = Path(__file__).resolve().parents[1]
OUT = Path('C:/ddda-targets/L109c/geometry-bench')


def main():
    function = next(f for f in functions(Path(lift.N) / 'patterns/lift.json') if f['addr'] == 0xFE9580)
    for variant in ('before', 'after'):
        with patch.object(lift, 'fuse_float_test_branches',
                          (lambda arm: arm) if variant == 'before' else lift.fuse_float_test_branches):
            source = lift.rust_fn(function['addr'], function['name'], function['arms'])
        source = source.replace('crate::platform::memset(m, cpu)', 'x86::call_ext(m, cpu, 0x12EFE20)')
        source = re.sub(r'super::lifted::f_([0-9a-f]{8})\(m, cpu\)',
                        lambda m: f'x86::call_ext(m, cpu, 0x{m[1]})', source)
        directory = OUT / variant / 'src'
        directory.mkdir(parents=True, exist_ok=True)
        (directory / 'lib.rs').write_text('#![allow(warnings)]\npub use ddnative_base::mem;\nuse ddnative_base::{mem::Mem,x86::{self,Cpu,Flags},emit};\n' + source)
        (directory.parent / 'Cargo.toml').write_text(f'''[package]
name = "geometry-{variant}"
version = "0.0.0"
edition = "2024"
[dependencies]
ddnative-base = {{path = "{ROOT.as_posix()}/crates/ddnative-base"}}
''')
    (OUT / 'src').mkdir(exist_ok=True)
    (OUT / 'port').mkdir(exist_ok=True)
    (OUT / 'port/Cargo.toml').write_text(f'''[package]
name = "geometry-port"
version = "0.0.0"
edition = "2024"
[lib]
path = "{ROOT.as_posix()}/crates/ddruntime/src/procedural_geometry.rs"
[dependencies]
ddnative-base = {{path = "{ROOT.as_posix()}/crates/ddnative-base"}}
''')
    (OUT / 'Cargo.toml').write_text(f'''[package]
name = "l109c-geometry-bench"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
ddnative-base = {{path = "{ROOT.as_posix()}/crates/ddnative-base"}}
ddnative-host = {{path = "{ROOT.as_posix()}/crates/ddnative-host"}}
serde_json = "1"
geometry-before = {{path = "before"}}
geometry-after = {{path = "after"}}
geometry-port = {{path = "port"}}
[profile.release]
lto = false
[profile.release.package.geometry-before]
opt-level = 1
[profile.release.package.geometry-after]
opt-level = 1
''')
    (OUT / 'src/main.rs').write_text(RUNNER)
    print(f'Prepared {OUT}; original-vector replay, no shards.')


RUNNER = r'''
use ddnative_base::{mem::{self,Mem,PageTable},x86::{self,Cpu}};
use serde_json::Value;
use std::{cell::Cell,rc::Rc,time::{Duration,Instant}};
struct Memory(Box<PageTable>);
impl Mem for Memory {
 fn u8(&self,a:u32)->u8 {unsafe{mem::load_at(self.0.page((a>>12) as usize).add((a&4095) as usize),1) as u8}}
 fn set_u8(&mut self,a:u32,b:u8) {let p=(a>>12) as usize;self.0.backed_page(p);self.0.set_perm(p,3);unsafe{mem::store_at(self.0.page(p).add((a&4095) as usize),1,b as u32)}}
 fn page_table(&self)->Option<std::ptr::NonNull<PageTable>> {Some(std::ptr::NonNull::from(self.0.as_ref()))}
}
fn number(v:&Value)->u32 {v.as_u64().unwrap() as u32}
fn bytes(v:&Value)->Vec<u8> {let s=v.as_str().unwrap();(0..s.len()).step_by(2).map(|k|u8::from_str_radix(&s[k..k+2],16).unwrap()).collect()}
fn write(m:&mut dyn Mem,a:u32,b:&[u8]) {for (i,v) in b.iter().enumerate(){m.set_u8(a+i as u32,*v)}}
fn machine(v:&Value)->Cpu {
 let r=&v["regs"];let mut c=Cpu{eax:number(&r["eax"]),ecx:number(&r["ecx"]),edx:number(&r["edx"]),ebx:number(&r["ebx"]),esp:number(&r["esp"]),ebp:number(&r["ebp"]),esi:number(&r["esi"]),edi:number(&r["edi"]),..Cpu::default()};
 for i in 0..8 {for j in 0..4 {c.xmm[i][j]=number(&v["xmm"][i][j]);}}
 c.fl=x86::Flags::new(x86::RAW,0,0,number(&v["flags"]),32);c
}
fn check(m:&dyn Mem,c:&Cpu,v:&Value,stack:u32) {
 let e=machine(v);
 assert_eq!([c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi],[e.eax,e.ecx,e.edx,e.ebx,e.esp,e.ebp,e.esi,e.edi]);
 assert_eq!(c.xmm,e.xmm);
 let (f,valid)=c.fl.snapshot(); assert_eq!(f&valid&0x8c5,number(&v["flags"])&valid&0x8c5);
 assert_eq!(x86::stmxcsr(),number(&v["mxcsr"]));
 for (i,b) in bytes(&v["stack"]).iter().enumerate(){assert_eq!(m.u8(stack+i as u32),*b,"stack {i:x}");}
 // The recorder executes original SSE arithmetic/comparison on hardware.
}
fn main(){
 ddnative_host::install();
 let vectors:Value=serde_json::from_slice(&std::fs::read("E:/ddda-re/notes/native-L109c/vectors/geometry.json").unwrap()).unwrap();
 for (case,v) in vectors.as_array().unwrap().iter().enumerate(){
  for (name,function) in [("before",geometry_before::f_00fe9580 as x86::LiftFn),("after",geometry_after::f_00fe9580),("port",geometry_port::build_geometry)] {
   let mut m=Memory(Box::default());
   for w in v["memory_before"].as_array().unwrap(){write(&mut m,number(&w[0]),&bytes(&w[1]));}
   let stack=number(&v["stack_base"]);write(&mut m,stack,&bytes(&v["before"]["stack"]));
   let mut c=machine(&v["before"]);let calls=v["calls"].as_array().unwrap().clone();let count=calls.len();
   let index=Rc::new(Cell::new(0));let cursor=index.clone();let overhead=Rc::new(Cell::new(Duration::ZERO));let clock=overhead.clone();
   x86::set_ext_calls(Some(Box::new(move |m,c,target|{
    let start=Instant::now();let k=cursor.get();let call=&calls[k];
    assert_eq!(target,number(&call["target"]),"call {k}");
    check(m,c,&call["before"],stack);
    for w in call["writes"].as_array().unwrap(){write(m,number(&w[0]),&bytes(&w[1]));}
    *c=machine(&call["after"]);write(m,stack,&bytes(&call["after"]["stack"]));
    cursor.set(k+1);clock.set(clock.get()+start.elapsed());
   })));
   x86::ldmxcsr(number(&v["before"]["mxcsr"]));let start=Instant::now();function(&mut m,&mut c);let elapsed=start.elapsed()-overhead.get();
   assert_eq!(index.get(),count);check(&m,&c,&v["after"],stack);
   for w in v["memory_after"].as_array().unwrap(){let a=number(&w[0]);for(i,b)in bytes(&w[1]).iter().enumerate(){assert_eq!(m.u8(a+i as u32),*b,"memory {a:x}+{i:x}");}}
   x86::set_ext_calls(None);println!("case={case} {name} calls={count} checked_ns={}",elapsed.as_nanos());
  }
 }
}
'''


if __name__ == '__main__':
    main()
