"""Prepare isolated whole-inflater oracle/benchmark, never build ddlift shards.

The extracted original stays in the private target directory. Both candidates
use the same explicit callee interface; the Python oracle executes unowned
callees as original instructions. No game process is launched.
"""
from pathlib import Path
import re
import os

ROOT = Path(__file__).resolve().parents[1]
OUT = Path(os.environ.get('CARGO_TARGET_DIR', 'C:/ddda-targets/L145')) / 'inflate-bench'


def original(address='00cf02e0'):
    location={'00cf01e0':'lift_25/src/generated/lifted_043.rs',
              '00cf34d0':'lift_hot_00/src/generated/lifted_002.rs'}
    path=ROOT / 'crates/ddlift' / location.get(address,'lift_hot_00/src/generated/lifted_001.rs')
    source=path.read_text()
    start = source.index(f'pub fn f_{address}(')
    end = source.find('// Translated from', start)
    if end == -1:
        end = len(source)
    body = source[start:end]
    body = re.sub(r'\(\|m: &mut dyn Mem, cpu: &mut Cpu\| x86::call_local\(m, cpu, (0x[0-9A-F]+), super::lifted::f_[a-f0-9]+\)\)\(m, cpu\)', r'x86::call_ext(m, cpu, \1)', body)
    body = body.replace('x86::call_local(m, cpu, 0x012EFEA0, crate::platform::memcpy)', 'x86::call_ext(m, cpu, 0x012EFEA0)')
    return body


def main():
    (OUT / 'original/src').mkdir(parents=True, exist_ok=True)
    (OUT / 'src').mkdir(exist_ok=True)
    (OUT / 'original/src/lib.rs').write_text('#![allow(warnings)]\npub use ddnative_base::{mem,x86};\nuse mem::Mem;\nuse x86::{Cpu,Flags};\nuse ddnative_base::emit;\n' + '\n'.join(original(a)for a in ['00cf02e0','00cf01e0','00cf30c0','00cf34d0','00cf2cb0']))
    (OUT / 'original/Cargo.toml').write_text(f'''[package]
name="l145-original-inflate"
version="0.0.0"
edition="2024"
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
''')
    (OUT / 'Cargo.toml').write_text(f'''[package]
name="l145-inflate-bench"
version="0.0.0"
edition="2024"
[workspace]
[lib]
name="l145_inflate_native"
crate-type=["cdylib"]
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
ddruntime={{path="{ROOT.as_posix()}/crates/ddruntime",default-features=false}}
l145-original-inflate={{path="original"}}
[profile.release]
lto=false
codegen-units=16
[profile.release.package.l145-original-inflate]
opt-level=1
''')
    (OUT / 'src/lib.rs').write_text(BRIDGE)
    (OUT / 'src/main.rs').write_text(BENCH)
    print(f'Prepared {OUT}: original opt1, native opt3; no shards')


BRIDGE = r'''
use ddnative_base::{mem::Mem,x86::{self,Cpu,Flags}};
#[repr(C)]
pub struct Window { address:u32, length:u32, bytes:*mut u8 }
struct Memory<'a>(&'a mut [Window]);
impl Mem for Memory<'_> {
 fn u8(&self,a:u32)->u8 {
  for w in self.0.iter() { if a.wrapping_sub(w.address)<w.length {return unsafe{*w.bytes.add(a.wrapping_sub(w.address)as usize)}} }
  panic!("unmapped read {a:#x}")
 }
 fn set_u8(&mut self,a:u32,v:u8) {
  for w in self.0.iter_mut() {if a.wrapping_sub(w.address)<w.length {unsafe{*w.bytes.add(a.wrapping_sub(w.address)as usize)=v};return} }
  panic!("unmapped write {a:#x}")
 }
 fn read_bytes(&self,a:u32,b:&mut[u8]){for(i,v)in b.iter_mut().enumerate(){*v=self.u8(a+i as u32)}}
}
fn save(cpu:&Cpu,r:&mut[u32]){
 r[..8].copy_from_slice(&[cpu.eax,cpu.ecx,cpu.edx,cpu.ebx,cpu.esp,cpu.ebp,cpu.esi,cpu.edi]);
 r[8]=cpu.fl.snapshot().0;
 for (i,x) in cpu.xmm.iter().enumerate(){r[9+i*4..13+i*4].copy_from_slice(x);}
}
fn restore(r:&[u32],cpu:&mut Cpu){
 cpu.eax=r[0];cpu.ecx=r[1];cpu.edx=r[2];cpu.ebx=r[3];cpu.esp=r[4];cpu.ebp=r[5];cpu.esi=r[6];cpu.edi=r[7];
 cpu.fl=Flags::new(x86::RAW,0,0,r[8],32);
 for(i,x)in cpu.xmm.iter_mut().enumerate(){x.copy_from_slice(&r[9+i*4..13+i*4]);}
}
type Callee=unsafe extern "C" fn(u32,*mut u32);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn l145_inflate(w:*mut Window,n:usize,r:*mut u32,callee:Callee,implementation:u32)->u32{
 ddnative_base::mem::ATOMIC_ACCESSES.store(true,std::sync::atomic::Ordering::Relaxed);
 let views=unsafe{std::slice::from_raw_parts(w,n)}.iter().map(|w|(w.address,w.length,w.bytes as usize)).collect::<Vec<_>>();
 let mut m=Memory(unsafe{std::slice::from_raw_parts_mut(w,n)});
 let r=unsafe{std::slice::from_raw_parts_mut(r,41)};
 let mut cpu=Cpu::default();restore(r,&mut cpu);
 x86::set_ext_calls(Some(Box::new(move |m,cpu,target|{
  if implementation & 16 != 0 && target==0xcf30c0 { ddruntime::archive_inflate::table::build(m,cpu); return; }
  if implementation & 2 != 0 && target==0xcf2cb0 { fast_mapped(m,cpu,&views,implementation); return; }
  let mut r=[0u32;41];save(cpu,&mut r);unsafe{callee(target,r.as_mut_ptr())};restore(&r,cpu);
 })));
 if implementation & 1==0 {l145_original_inflate::f_00cf02e0(&mut m,&mut cpu)}
 else {ddruntime::archive_inflate::port::inflate_with(&mut m,&mut cpu,&mut |m,cpu,t,_|x86::call_ext(m,cpu,t))}
 x86::set_ext_calls(None);
 save(&cpu,r);
 cpu.fl.snapshot().1
}

// Exercise the actual page-backed specialization, not its Mem-only fallback.
// Reuse the mapping across calls; refresh all stream windows before each leaf
// and copy them back afterwards. The oracle still compares all original bytes.
fn fast_mapped(source:&mut dyn Mem,cpu:&mut Cpu,views:&[(u32,u32,usize)],implementation:u32){
 use ddruntime::{memory::{GuestMemory,READ,WRITE},fastmem::write_bulk};
 use std::{cell::RefCell,collections::BTreeSet};
 thread_local!{static MEMORY:RefCell<(GuestMemory,BTreeSet<u32>)>=RefCell::new((GuestMemory::new(),BTreeSet::new()));}
 MEMORY.with(|memory|{
  let mut memory=memory.borrow_mut();let(m,pages)=&mut *memory;
  let mut put=|address:u32,bytes:&[u8]|{
   for page in (address&!4095..(address+bytes.len()as u32+4095)&!4095).step_by(4096){
    if pages.insert(page){m.map(page,4096,READ|WRITE).unwrap();}
   }
   write_bulk(m,address,bytes);
  };
  for &(address,length,pointer)in &views[..4]{
   put(address,unsafe{std::slice::from_raw_parts(pointer as *const u8,length as usize)});
  }
  let stream=source.u32(cpu.esp+4);let state=source.u32(stream+28);
  for table in [source.u32(state+0x44),source.u32(state+0x48)]{
   for page in (table&!4095..(table+8192+4095)&!4095).step_by(4096){
    if views[..4].iter().any(|&(a,n,_)|page>=a && page+4096<=a+n){continue;}
    let mut bytes=[0;4096];source.read_bytes(page,&mut bytes);put(page,&bytes);
   }
  }
  if implementation&8!=0 {ddruntime::archive_inflate::inflate_fast_wide(m,cpu)}
  else {ddruntime::archive_inflate::inflate_fast_pages(m,cpu)};
  for &(address,length,pointer)in &views[..4]{
   m.read_bytes(address,unsafe{std::slice::from_raw_parts_mut(pointer as *mut u8,length as usize)});
  }
 });
}
'''

BENCH = r'''
use ddnative_base::{mem::Mem,x86::{self,Cpu}};
use ddruntime::{memory::{GuestMemory,READ,WRITE},fastmem::write_bulk};
use std::{io::{Cursor,Read},time::Instant};
use l145_original_inflate as original;
fn word(c:&mut Cursor<Vec<u8>>)->u32{let mut b=[0;4];c.read_exact(&mut b).unwrap();u32::from_le_bytes(b)}
fn windows(c:&mut Cursor<Vec<u8>>)->Vec<(u32,Vec<u8>)>{(0..word(c)).map(|_|{let a=word(c);let mut b=vec![0;word(c)as usize];c.read_exact(&mut b).unwrap();(a,b)}).collect()}
thread_local! {static COST:std::cell::RefCell<[(u128,u64);5]>=const{std::cell::RefCell::new([(0,0);5])};}
fn helper(m:&mut dyn Mem,c:&mut Cpu,t:u32,implementation:u32){
 static PROFILE:std::sync::OnceLock<bool>=std::sync::OnceLock::new();
 let timer=(*PROFILE.get_or_init(||std::env::var_os("L156_PROFILE").is_some())).then(Instant::now);
 match t{
 0xcf01e0=>original::f_00cf01e0(m,c),
 0xcf30c0=>if implementation&16!=0 {ddruntime::archive_inflate::table::build(m,c)} else {original::f_00cf30c0(m,c)},
 0xcf34d0=>ddruntime::archive_inflate::adler32(m,c),
 0xcf2cb0=>if implementation&8!=0 {ddruntime::archive_inflate::inflate_fast_wide(m,c)}
   else if implementation&4!=0 {original::f_00cf2cb0(m,c)}
   else if implementation&2!=0 {ddruntime::archive_inflate::inflate_fast_pages(m,c)}
   else{ddruntime::archive_inflate::inflate_fast(m,c)},
 0x12efea0=>ddruntime::fastmem::memcpy(m,c),_=>panic!("unexpected helper {t:#x}")
}
 if let Some(start)=timer {
  let slot=match t{0xcf2cb0=>0,0xcf30c0=>1,0xcf01e0=>2,0xcf34d0=>3,_=>4};
  COST.with(|cost|{let mut cost=cost.borrow_mut();cost[slot].0+=start.elapsed().as_nanos();cost[slot].1+=1;});
 }
}
fn main(){
 ddnative_base::mem::ATOMIC_ACCESSES.store(true,std::sync::atomic::Ordering::Relaxed);
 println!("atomic_accesses=true units=decimal_decompressed_MB original_opt=1 native_opt=3");
 let repeats:u128=std::env::var("L156_REPEATS").ok().and_then(|v|v.parse().ok()).unwrap_or(200);
 let path=std::env::args().nth(1).expect("private benchmark image");
 let mut c=Cursor::new(std::fs::read(path).unwrap());
 let common=windows(&mut c);let cases=word(&mut c);
 for index in 0..cases {
  let input=windows(&mut c);let packed=word(&mut c);let size=word(&mut c);
  let mut m=GuestMemory::new();
  for(a,b)in common.iter().chain(input.iter()){m.map(*a,b.len()as u32,READ|WRITE).unwrap();write_bulk(&mut m,*a,b);}
  for (input_chunk,output_chunk) in [(u32::MAX,u32::MAX),(8192,16384),(5,257)] {
   if input_chunk==5 && std::env::var_os("L156_SKIP_TINY").is_some(){continue;}
   let mut reference=None;
   for round in 0..3 {
    for implementation in if round%2==0{[4,2,10,18,26]}else{[26,18,10,2,4]} {
     x86::set_ext_calls(Some(Box::new(move|m,c,t|helper(m,c,t,implementation))));
     COST.with(|cost|*cost.borrow_mut()=[(0,0);5]);
     let mut ns=0;let mut calls=0;
     for _ in 0..repeats {
      for(a,b)in &input{write_bulk(&mut m,*a,b);}
      let mut cpu=Cpu::default();let mut limit=0;
      let start=Instant::now();
      loop {
       let stream=0xe000000;
       if m.u32(stream+4)==0 && limit<packed{let n=input_chunk.min(packed-limit);m.set_u32(stream+4,n);limit+=n;}
       m.set_u32(stream+16,output_chunk.min(size+65536-m.u32(stream+20)));
       cpu.esp=0xf0ff000;m.set_u32(cpu.esp,0xdead000);m.set_u32(cpu.esp+4,stream);m.set_u32(cpu.esp+8,0);
       if implementation&1==0{original::f_00cf02e0(&mut m,&mut cpu)}
       else{ddruntime::archive_inflate::port::inflate_with(&mut m,&mut cpu,&mut|m,c,t,_|helper(m,c,t,implementation))}
       calls+=1;
       if cpu.eax==1 {break}
       assert_eq!(cpu.eax,0);
      }
      ns+=start.elapsed().as_nanos();
      assert_eq!(m.u32(0xe000014),size);
     }
     let mut bytes=vec![0;size as usize];m.read_bytes(0x30000000,&mut bytes);
     let digest=bytes.iter().fold(0xcbf29ce484222325u64,|hash,&byte|(hash^u64::from(byte)).wrapping_mul(0x100000001b3));
     if let Some(expected)=&reference{assert_eq!(&bytes,expected)}else{reference=Some(bytes);}
     if std::env::var_os("L156_PROFILE").is_some(){COST.with(|cost|println!("profile case={index} implementation={implementation} total_ns={ns} fast_table_window_adler_copy={:?}",cost.borrow()));}
     println!("case={index} bytes={size} input_chunk={input_chunk} output_chunk={output_chunk} round={round} implementation={implementation} repeats={repeats} calls={calls} output_fnv64={digest:016x} ns_per_stream={} ns_per_MB={}",ns/repeats,ns*1_000_000/(repeats*size as u128));
    }
   }
  }
 }
}
'''

if __name__ == '__main__':
    main()
