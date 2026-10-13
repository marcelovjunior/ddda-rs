"""Build the current skip source as a tiny private oracle bridge; no shards."""
from pathlib import Path
import os
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = Path('C:/ddda-targets/L165/skip-chain')


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'Cargo.toml').write_text(f'''[package]
name="l165-skip-chain"
version="0.0.0"
edition="2021"
[workspace]
[lib]
name="l165_skip_chain"
path="bridge.rs"
crate-type=["cdylib"]
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
''', encoding='utf-8')
    # Compile the actual production source; never maintain a second port.
    (OUT / 'bridge.rs').write_text(
        f'#[path="{ROOT.as_posix()}/crates/ddruntime/src/archive_skip.rs"] mod stream;\n' + BRIDGE,
        encoding='utf-8')
    env = dict(os.environ, CARGO_TARGET_DIR='C:/ddda-targets/L165', CARGO_INCREMENTAL='0', CARGO_BUILD_JOBS='4')
    subprocess.run(['cargo', 'build', '--release', '--manifest-path', str(OUT / 'Cargo.toml')], env=env, check=True)


BRIDGE = r'''
use ddnative_base::{mem::Mem,x86::{self,Cpu,Flags}};
#[repr(C)] pub struct Window { address:u32,length:u32,bytes:*mut u8 }
struct Memory<'a>(&'a mut [Window]);
impl Mem for Memory<'_> {
 fn u8(&self,a:u32)->u8 {
  for w in self.0.iter() {if a.wrapping_sub(w.address)<w.length{return unsafe{*w.bytes.add(a.wrapping_sub(w.address)as usize)}}}
  panic!("unmapped read {a:#x}")
 }
 fn set_u8(&mut self,a:u32,v:u8) {
  for w in self.0.iter_mut(){if a.wrapping_sub(w.address)<w.length{unsafe{*w.bytes.add(a.wrapping_sub(w.address)as usize)=v};return}}
  panic!("unmapped write {a:#x}")
 }
}
fn save(cpu:&Cpu,r:&mut[u32]) {
 r[..8].copy_from_slice(&[cpu.eax,cpu.ecx,cpu.edx,cpu.ebx,cpu.esp,cpu.ebp,cpu.esi,cpu.edi]);
 r[8]=cpu.fl.snapshot().0;
 for(i,x)in cpu.xmm.iter().enumerate(){r[9+i*4..13+i*4].copy_from_slice(x);}
}
fn restore(r:&[u32],cpu:&mut Cpu) {
 cpu.eax=r[0];cpu.ecx=r[1];cpu.edx=r[2];cpu.ebx=r[3];cpu.esp=r[4];cpu.ebp=r[5];cpu.esi=r[6];cpu.edi=r[7];
 cpu.fl=Flags::new(x86::RAW,0,0,r[8],32);cpu.df=r[8]&0x400!=0;
 for(i,x)in cpu.xmm.iter_mut().enumerate(){x.copy_from_slice(&r[9+i*4..13+i*4]);}
}
type Callee=unsafe extern "C" fn(u32,*mut u32);
#[no_mangle]
pub unsafe extern "C" fn l165_skip(w:*mut Window,n:usize,r:*mut u32,callee:Callee)->u32 {
 let mut memory=Memory(unsafe{std::slice::from_raw_parts_mut(w,n)});
 let r=unsafe{std::slice::from_raw_parts_mut(r,41)};
 let mut cpu=Cpu::default();restore(r,&mut cpu);
 x86::set_ext_calls(Some(Box::new(move|_,cpu,target|{
  let mut r=[0;41];save(cpu,&mut r);unsafe{callee(target,r.as_mut_ptr())};restore(&r,cpu);
 })));
 stream::skip(&mut memory,&mut cpu);
 x86::set_ext_calls(None);save(&cpu,r);cpu.fl.snapshot().1
}
'''

if __name__ == '__main__':
    main()
