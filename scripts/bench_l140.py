"""Prepare private original-leaf/native Adler benchmark; no shard build.

Generated leaf stays under C:/ddda-targets/L140. Build the resulting manifest
with JOBS=4, INCREMENTAL=0, TARGET_DIR=C:/ddda-targets/L140, --release.
"""
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
out = Path('C:/ddda-targets/L140/adler-bench')
source = (root / 'crates/ddlift/lift_hot_00/src/generated/lifted_002.rs').read_text()
start = source.index('pub fn f_00cf34d0(')
end = source.index('// Translated from', start)
leaf = source[start:end]
assert not re.search(r'call_(?:ext|lifted|local)', leaf)
(out / 'original/src').mkdir(parents=True, exist_ok=True)
(out / 'src').mkdir(exist_ok=True)
(out / 'original/src/lib.rs').write_text('pub use ddnative_base::{mem,x86};\nuse mem::Mem;\nuse x86::{Cpu,Flags};\nuse ddnative_base::emit;\n' + leaf)
(out / 'original/Cargo.toml').write_text(f'''[package]
name="l140-original-adler"
version="0.0.0"
edition="2024"
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
''')
(out / 'Cargo.toml').write_text(f'''[package]
name="l140-adler-bench"
version="0.0.0"
edition="2024"
[workspace]
[lib]
crate-type=["cdylib"]
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
ddruntime={{path="{root.as_posix()}/crates/ddruntime",default-features=false}}
l140-original-adler={{path="original"}}
[profile.release]
lto=false
codegen-units=64
[profile.release.package.l140-original-adler]
opt-level=1
''')
(out / 'src/main.rs').write_text(r'''
use ddnative_base::{mem::Mem,x86::Cpu};
use ddruntime::{memory::{GuestMemory,READ,WRITE},fastmem::write_bulk};
use std::{io::{Cursor,Read},time::Instant};
fn word(c:&mut Cursor<Vec<u8>>)->u32 {let mut b=[0;4];c.read_exact(&mut b).unwrap();u32::from_le_bytes(b)}
fn machine(c:&mut Cursor<Vec<u8>>)->([u32;8],Vec<(u32,Vec<u8>)>) {
 let r=std::array::from_fn(|_|word(c));
 let w=(0..word(c)).map(|_|{let a=word(c);let mut b=vec![0;word(c)as usize];c.read_exact(&mut b).unwrap();(a,b)}).collect();(r,w)
}
fn main(){
 let mut c=Cursor::new(std::fs::read("E:/ddda-re/notes/native-L140/adler.bin").unwrap());c.set_position(8);let count=word(&mut c);
 for index in 0..count {
  let (r,input)=machine(&mut c);let (expected,output)=machine(&mut c);let flags=word(&mut c);
  if ![0,2,20,32,37,38,39,40,41,42,43,44].contains(&index){continue;}
  let mut m=GuestMemory::new();
  for (a,b) in &input {let start=a&!4095;let end=(a+b.len()as u32+4095)&!4095;m.map(start,end-start,READ|WRITE).unwrap();write_bulk(&mut m,*a,b);}
  let initial=Cpu{eax:r[0],ecx:r[1],edx:r[2],ebx:r[3],esp:r[4],ebp:r[5],esi:r[6],edi:r[7],..Cpu::default()};
  let length=m.u32(r[4]+12);
  for round in 0..3 {
   for implementation in if round%2==0 {[0,1]}else{[1,0]} {
    let f=if implementation==0 {l140_original_adler::f_00cf34d0}else{ddruntime::archive_inflate::adler32};
    for (a,b) in &input {write_bulk(&mut m,*a,b);}
    let mut cpu=initial.clone(); f(&mut m,&mut cpu);
    assert_eq!([cpu.eax,cpu.ecx,cpu.edx,cpu.ebx,cpu.esp,cpu.ebp,cpu.esi,cpu.edi],expected);
    assert_eq!(cpu.fl.snapshot().0&0x8c5,flags&0x8c5);
    for(a,b)in &output{let mut actual=vec![0;b.len()];m.read_bytes(*a,&mut actual);assert_eq!(&actual,b);}
    let start=Instant::now();
    for _ in 0..1000 {m.set_u32(r[4]+12,length);let mut cpu=initial.clone();f(&mut m,&mut cpu);std::hint::black_box(cpu.eax);}
    println!("case={index} length={length} round={round} implementation={implementation} ns_per_call={}",start.elapsed().as_nanos()/1000);
   }
  }
 }
}
''')
print(f'Prepared {out}; original opt1, native/runtime opt3, no shard dependencies')

# Bridge for the original-instruction streaming oracle. All game bytes remain
# in Python/Unicorn; only the Adler helper runs natively. The callback exposes
# exactly its read buffer and stack window, so unexpected accesses fail.
(out / 'src/lib.rs').write_text(r'''
use ddnative_base::{mem::Mem,x86::Cpu};
struct Windows<'a>{source:&'a[u8],source_address:u32,stack:&'a mut[u8],stack_address:u32}
impl Mem for Windows<'_>{
 fn u8(&self,a:u32)->u8{
  if a.wrapping_sub(self.stack_address)<self.stack.len()as u32 {self.stack[a.wrapping_sub(self.stack_address)as usize]}
  else {self.source[a.wrapping_sub(self.source_address)as usize]}
 }
 fn set_u8(&mut self,a:u32,v:u8){self.stack[a.wrapping_sub(self.stack_address)as usize]=v;}
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn l140_adler(source:*const u8,length:usize,registers:*mut u32,stack:*mut u8)->u32{
 let r=unsafe{std::slice::from_raw_parts_mut(registers,8)};
 let stack=unsafe{std::slice::from_raw_parts_mut(stack,64)};
 let address=u32::from_le_bytes(stack[40..44].try_into().unwrap());
 let mut m=Windows{source:unsafe{std::slice::from_raw_parts(source,length)},source_address:address,stack,stack_address:r[4]-32};
 let mut c=Cpu{eax:r[0],ecx:r[1],edx:r[2],ebx:r[3],esp:r[4],ebp:r[5],esi:r[6],edi:r[7],..Cpu::default()};
 ddruntime::archive_inflate::adler32(&mut m,&mut c);
 r.copy_from_slice(&[c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi]);
 c.fl.snapshot().0
}
''')
