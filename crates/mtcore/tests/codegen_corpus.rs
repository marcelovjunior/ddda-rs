//! Included by scripts/worker_corpus.py in a private isolated executable.
//! Runtime transcripts are data inputs, never checked into this repository.
#![cfg(any())]
// The script removes the preceding cfg and supplies baseline/candidate crates.
use ddnative_base::{mem::{Mem,PageTable,READ,WRITE,ATOMIC_ACCESSES},x86::{self,Cpu,LiftFn}};
use serde::Deserialize;
use std::{collections::{BTreeMap,BTreeSet},cell::RefCell,rc::Rc,time::Instant};

#[derive(Clone,Deserialize)]
struct Seam {target:u32,input:Vec<u32>,output:Vec<u32>,memory:Vec<(u32,u8)>,writes:Vec<(u32,Vec<u8>)>}
#[derive(Clone,Deserialize)]
struct Case {input:Vec<u32>,output:Vec<u32>,initial:Vec<(u32,u8)>,#[serde(rename="final")]final_:Vec<(u32,u8)>,seams:Vec<Seam>,write_addresses:Vec<u32>}
#[derive(Deserialize)]
struct Corpus {entry:u32,cases:Vec<Case>}

fn snapshot(c:&Cpu)->Vec<u32> {
    let live=x86::stmxcsr();ddnative_base::x87::park();x86::ldmxcsr(x86::MXCSR_DEFAULT);
    let mut v=vec![c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi];v.extend(c.xmm.iter().flatten());
    v.extend([c.fl.kind as u32,c.fl.a,c.fl.b,c.fl.r,c.fl.bits,c.fl.cf_in as u32,c.df as u32,c.fs_base,c.sys_flags,c.mxcsr.0,live]);
    v.extend([c.seg.es,c.seg.cs,c.seg.ss,c.seg.ds,c.seg.fs,c.seg.gs].map(u32::from));
    v.extend(c.st.clone().state.chunks_exact(4).map(|b|u32::from_le_bytes(b.try_into().unwrap())));
    x86::ldmxcsr(live);v
}
fn restore(v:&[u32],c:&mut Cpu) {
    assert_eq!(v.len(),84);[c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi]=v[..8].try_into().unwrap();
    c.xmm=std::array::from_fn(|i|std::array::from_fn(|j|v[8+i*4+j]));
    c.fl=x86::Flags{kind:v[40]as u8,a:v[41],b:v[42],r:v[43],bits:v[44],cf_in:v[45]!=0};
    c.df=v[46]!=0;c.fs_base=v[47];c.sys_flags=v[48];c.mxcsr=x86::Mxcsr(v[49]);
    c.seg=x86::Segments{es:v[51]as u16,cs:v[52]as u16,ss:v[53]as u16,ds:v[54]as u16,fs:v[55]as u16,gs:v[56]as u16};
    ddnative_base::x87::park();c.st=Default::default();
    for i in 0..27{c.st.state[i*4..i*4+4].copy_from_slice(&v[57+i].to_le_bytes());}
    x86::ldmxcsr(v[50]);
}
fn canonical(mut v:Vec<u32>)->Vec<u32> {
    let fl=x86::Flags{kind:v[40]as u8,a:v[41],b:v[42],r:v[43],bits:v[44],cf_in:v[45]!=0};
    let (status,defined)=fl.snapshot();v[40..46].fill(0);v[40]=status&defined;v[41]=defined;
    // Hardware x87 host FIP/FDP vary with ASLR. Guest FNSAVE overrides
    // these with guest metadata; no other x87 field is excluded.
    v[60]=0;v[62]=0;v
}
fn compare(c:&Cpu,expected:&[u32]){assert_eq!(canonical(snapshot(c)),canonical(expected.to_vec()),"CPU output or call boundary");}

struct Memory {table:Option<Box<PageTable>>,known:BTreeMap<u32,u8>,allowed:BTreeSet<u32>,image:BTreeMap<u32,[u8;4096]>}
impl Memory {
    fn new(case:&Case,pages:bool)->Self {
        let mut image=BTreeMap::<u32,[u8;4096]>::new();
        for &(a,_)in case.initial.iter().chain(&case.final_){image.entry(a/4096).or_insert([0xa5;4096]);}
        for &(a,b)in &case.initial{image.get_mut(&(a/4096)).unwrap()[(a%4096)as usize]=b;}
        let table=pages.then(||Box::new(PageTable::default()));
        if let Some(t)=&table {for(&page,bytes)in &image {
            t.set_perm(page as usize,READ|WRITE);let ptr=t.backed_page(page as usize);
            unsafe{std::ptr::copy_nonoverlapping(bytes.as_ptr(),ptr,4096);}
        }}
        Self{table,known:case.initial.iter().copied().collect(),allowed:case.write_addresses.iter().copied().collect(),image}
    }
    fn reset(&mut self) {let t=self.table.as_ref().unwrap();for(&page,bytes)in &self.image{unsafe{std::ptr::copy_nonoverlapping(bytes.as_ptr(),t.page(page as usize),4096);}}}
    fn verify(&self,case:&Case) {
        if let Some(t)=&self.table {
            let mut expected=self.image.clone();for &(a,b)in &case.final_{expected.get_mut(&(a/4096)).unwrap()[(a%4096)as usize]=b;}
            for(page,bytes)in expected {let actual=unsafe{std::slice::from_raw_parts(t.page(page as usize),4096)};assert_eq!(actual,bytes,"page {page:x}");}
        } else {for &(a,b)in &case.final_{assert_eq!(self.known.get(&a),Some(&b),"final byte {a:x}");}}
    }
}
impl Mem for Memory {
    fn page_table(&self)->Option<std::ptr::NonNull<PageTable>>{self.table.as_deref().map(std::ptr::NonNull::from)}
    fn u8(&self,a:u32)->u8{
        if let Some(t)=&self.table{let ptr=t.page((a/4096)as usize);assert!(!ptr.is_null(),"unmapped read {a:x}");unsafe{*ptr.add((a%4096)as usize)}}
        else{*self.known.get(&a).unwrap_or_else(||panic!("read before captured initialization {a:x}"))}
    }
    fn set_u8(&mut self,a:u32,b:u8){
        if let Some(t)=&self.table{let ptr=t.page((a/4096)as usize);assert!(!ptr.is_null(),"unmapped write {a:x}");unsafe{*ptr.add((a%4096)as usize)=b;}}
        else{assert!(self.allowed.contains(&a),"unrecorded write {a:x}");self.known.insert(a,b);}
    }
}
#[derive(Default)]
struct Calls {next:usize,ns:u128}
fn install(case:&Case,check:bool)->Rc<RefCell<Calls>> {
    let state=Rc::new(RefCell::new(Calls::default()));let shared=state.clone();let seams=case.seams.clone();
    x86::set_ext_calls(Some(Box::new(move|m,c,target|{
        let start=Instant::now();let mut state=shared.borrow_mut();let seam=seams.get(state.next).expect("missing captured callee boundary");
        assert_eq!(target,seam.target,"callee target");if check{compare(c,&seam.input);for&(a,b)in &seam.memory{assert_eq!(m.u8(a),b,"callee memory {a:x}");}}
        for(a,bytes)in &seam.writes{for(i,b)in bytes.iter().enumerate(){m.set_u8(a.wrapping_add(i as u32),*b);}}
        restore(&seam.output,c);state.next+=1;state.ns+=start.elapsed().as_nanos();
    })));state
}
fn correctness(case:&Case,body:LiftFn,pages:bool) {
    let mut m=Memory::new(case,pages);let calls=install(case,true);let mut cpu=Cpu::default();restore(&case.input,&mut cpu);
    body(&mut m,&mut cpu);compare(&cpu,&case.output);m.verify(case);assert_eq!(calls.borrow().next,case.seams.len());x86::set_ext_calls(None);
}
fn timed(case:&Case,body:LiftFn,iterations:usize)->(u128,u128) {
    let mut m=Memory::new(case,true);let calls=install(case,false);let mut cpu=Cpu::default();let mut total=0;let mut external=0;
    for _ in 0..iterations {
        m.reset();*calls.borrow_mut()=Calls::default();restore(&case.input,&mut cpu);
        let start=Instant::now();body(&mut m,&mut cpu);let elapsed=start.elapsed().as_nanos();
        let seam=calls.borrow();assert_eq!(seam.next,case.seams.len());total+=elapsed;external+=seam.ns;
        std::hint::black_box(cpu.eax);
    }
    x86::set_ext_calls(None);assert!(external<=total);(total,total-external)
}
fn synthetic_body(m:&mut dyn Mem,c:&mut Cpu) {
    let p=ddnative_base::mem::Pages::of(m);c.eax=p.u32(m,c.ecx);
    c.eax=c.fl.alu_add(c.eax,5,32);c.esp-=4;p.set_u32(m,c.esp,0x12345678);
    x86::call_ext(m,c,0x405000);p.set_u32(m,c.ecx+2,c.eax);c.esp+=4;
}
fn self_test() {
    let mut cpu=Cpu::default();cpu.ecx=0x1ffc;cpu.esp=0x8004;cpu.xmm[7]=[1,2,3,4];
    let input=snapshot(&cpu);cpu.eax=cpu.fl.alu_add(0x04030201,5,32);cpu.esp-=4;
    let before=snapshot(&cpu);cpu.eax=0xabcdef01;cpu.esp+=4;let after=snapshot(&cpu);
    cpu.esp+=4;let output=snapshot(&cpu);
    let mut final_=vec![(0x9000,77)];
    for(a,v)in [(0x8000,0x12345678u32),(0x1ffe,0xabcdef01)]{final_.extend(v.to_le_bytes().iter().enumerate().map(|(i,b)|(a+i as u32,*b)));}
    let case=Case{input,output,initial:vec![(0x1ffc,1),(0x1ffd,2),(0x1ffe,3),(0x1fff,4)],write_addresses:final_.iter().map(|(a,_)|*a).collect(),final_,
        seams:vec![Seam{target:0x405000,input:before,output:after,memory:vec![(0x1ffc,1),(0x8000,0x78)],writes:vec![(0x9000,vec![77])]}]};
    for pages in [false,true]{correctness(&case,synthetic_body,pages);}
    let old=std::panic::take_hook();std::panic::set_hook(Box::new(|_|{}));
    let mut rejected=0;
    for kind in 0..5 {
        let mut bad=case.clone();
        match kind {0=>bad.output[0]^=1,1=>bad.seams[0].input[0]^=1,2=>bad.final_[0].1^=1,3=>bad.seams.clear(),_=>{bad.initial.remove(0);}}
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(||correctness(&bad,synthetic_body,false))).is_err(){rejected+=1;}
    }
    std::panic::set_hook(old);assert_eq!(rejected,5);x86::set_ext_calls(None);
    let (_,exclusive)=timed(&case,synthetic_body,20);assert!(exclusive>0);
    println!("SELFTEST synthetic only: guarded/page exactness, cross-page alias writes, opaque seam, 5 corruption rejections, timing PASS");
}
fn main() {
    ddnative_host::install();ATOMIC_ACCESSES.store(true,std::sync::atomic::Ordering::Relaxed);
    let args:Vec<String>=std::env::args().collect();if args.get(1).map(String::as_str)==Some("--self-test"){self_test();return;}
    let corpus:Corpus=serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
    let iterations:usize=args.get(2).map(|v|v.parse().unwrap()).unwrap_or(100);
    let baseline=baseline::lookup(corpus.entry).expect("baseline entry");let candidate=candidate::lookup(corpus.entry).expect("candidate entry");
    let saved=x86::stmxcsr();
    for (index,case) in corpus.cases.iter().enumerate() {
        for body in [baseline,candidate]{for pages in [false,true]{correctness(case,body,pages);}}
        for round in 0..7 {for step in 0..2 {let port=(round+step)%2;let body=if port==0{baseline}else{candidate};
            let (inclusive,exclusive)=timed(case,body,iterations);
            println!("CORPUS {} {index} {round} {port} {iterations} {inclusive} {exclusive}",corpus.entry);
        }}
    }
    x86::ldmxcsr(saved);
}
