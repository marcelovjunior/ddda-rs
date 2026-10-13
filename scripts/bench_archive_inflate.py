"""Prepare a PRIVATE single-leaf A/B benchmark; does not build any shard.

python scripts/bench_archive_inflate.py
Then cargo run --manifest-path C:/ddda-targets/L109/inflate-bench/Cargo.toml
 --release, with CARGO_TARGET_DIR=C:/ddda-targets/L109, JOBS=4, INCREMENTAL=0.
Extracts only the existing generated leaf and its split helpers; never
commits that generated excerpt or the private oracle vectors.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
OUT = Path('C:/ddda-targets/L109/inflate-bench')
source = (ROOT/'crates/ddlift/lift_hot_00/src/generated/lifted_001.rs').read_text()
start = source.index('pub fn f_00cf2cb0(')
end = source.index('// Translated from', start)
leaf = source[start:end]
assert not re.search(r'call_(?:ext|lifted|local)', leaf), 'benchmark leaf acquired a callee'
(OUT/'original/src').mkdir(parents=True, exist_ok=True)
(OUT/'src').mkdir(exist_ok=True)
(OUT/'original/src/lib.rs').write_text('pub use ddnative_base::{mem,x86};\nuse mem::Mem;\nuse x86::{Cpu,Flags};\nuse ddnative_base::emit;\n'+leaf)
(OUT/'original/Cargo.toml').write_text(f'''[package]
name = "l109-original-leaf"
version = "0.0.0"
edition = "2024"
[dependencies]
ddnative-base = {{path = "{ROOT.as_posix()}/crates/ddnative-base"}}
''')
(OUT/'Cargo.toml').write_text(f'''[package]
name = "l109-inflate-bench"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
ddnative-base = {{path = "{ROOT.as_posix()}/crates/ddnative-base"}}
ddruntime = {{path = "{ROOT.as_posix()}/crates/ddruntime", default-features = false}}
l109-original-leaf = {{path = "original"}}
[profile.release]
lto = false
codegen-units = 64
[profile.release.package.l109-original-leaf]
opt-level = 1
''')
(OUT/'src/main.rs').write_text(r'''
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::memory::{GuestMemory,READ,WRITE};
use std::{io::{Read,Cursor},time::{Duration,Instant}};
fn word(c:&mut Cursor<Vec<u8>>) -> u32 {let mut b=[0;4];c.read_exact(&mut b).unwrap();u32::from_le_bytes(b)}
fn record(c:&mut Cursor<Vec<u8>>) -> (Cpu,Vec<(u32,Vec<u8>)>) {
 let r:[u32;8]=std::array::from_fn(|_|word(c));
 let xmm=std::array::from_fn(|_|std::array::from_fn(|_|word(c)));
 let cpu=Cpu{eax:r[0],ecx:r[1],edx:r[2],ebx:r[3],esp:r[4],ebp:r[5],esi:r[6],edi:r[7],xmm,..Cpu::default()};
 let windows=(0..word(c)).map(|_|{let a=word(c);let mut b=vec![0;word(c) as usize];c.read_exact(&mut b).unwrap();(a,b)}).collect();
 (cpu,windows)
}
fn main(){
 let mut c=Cursor::new(std::fs::read("E:/ddda-re/notes/native-L109/vectors/inflate_fast.bin").unwrap());
 c.set_position(8);let count=word(&mut c); assert!(count>=4);
 for index in 0..4 {
  let (cpu,windows)=record(&mut c);let (_,expected)=record(&mut c);let _flags=word(&mut c);
  let mut mem=GuestMemory::new();
  for (a,b) in &windows {let p=a&!4095;let end=(a+b.len() as u32+4095)&!4095;mem.map(p,end-p,READ|WRITE).unwrap();}
  for round in 0..3 {
   for implementation in if round%2==0 {[0,1]}else{[1,0]} {
    let f=if implementation==0 {l109_original_leaf::f_00cf2cb0}else{ddruntime::archive_inflate::inflate_fast};
    let mut elapsed=Duration::ZERO;
    for iteration in 0..1000 {
     for (a,b) in &windows {ddruntime::fastmem::write_bulk(&mut mem,*a,b);}
     let mut cpu=cpu.clone(); let start=Instant::now();f(&mut mem,&mut cpu);elapsed+=start.elapsed();
     std::hint::black_box(cpu.eax);
     if iteration==0 {for (a,b) in &expected {for (k,v) in b.iter().enumerate(){assert_eq!(mem.u8(*a+k as u32),*v);}}}
    }
    println!("case={index} round={round} implementation={implementation} ns_per_call={}",elapsed.as_nanos()/1000);
   }
  }
 }
}
''')
print(f'Prepared {OUT}; original leaf opt1, readable/runtime opt3, no shard dependencies.')
