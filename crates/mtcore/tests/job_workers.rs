//! Original machine-state fixtures for opt-in worker ports (private corpus).
use ddnative_base::{mem::Mem, x86::{self, Cpu, LiftFn}};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Memory { bytes: BTreeMap<u32,u8>, writes: BTreeMap<u32,u8>, table: Option<Box<ddnative_base::mem::PageTable>> }
impl Mem for Memory {
    fn page_table(&self)->Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> { self.table.as_deref().map(std::ptr::NonNull::from) }
    fn u8(&self,a:u32)->u8 {
        if let Some(t)=&self.table {let p=t.page((a/4096)as usize);assert!(!p.is_null());unsafe{*p.add((a%4096)as usize)}}
        else {*self.bytes.get(&a).unwrap_or_else(||panic!("unrecorded read {a:x}"))}
    }
    fn set_u8(&mut self,a:u32,b:u8) {
        if let Some(t)=&self.table {let p=t.page((a/4096)as usize);assert!(!p.is_null());unsafe{*p.add((a%4096)as usize)=b}}
        self.bytes.insert(a,b);self.writes.insert(a,b);
    }
}
fn n(v:&Value)->u32 { v.as_u64().unwrap() as u32 }
fn pairs(v:&Value)->BTreeMap<u32,u8> {
    v.as_array().unwrap().iter().map(|p|(n(&p[0]),n(&p[1])as u8)).collect()
}
fn regs(c:&Cpu)->[u32;8] { [c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi] }
fn restore(c:&mut Cpu,v:&Value) {
    [c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi]=std::array::from_fn(|i|n(&v["regs"][i]));
    c.fl=x86::Flags::new(x86::RAW,0,0,n(&v["flags"]),32);c.df=n(&v["flags"])&0x400!=0;
    c.mxcsr=x86::Mxcsr(n(&v["mxcsr"]));
    c.xmm=std::array::from_fn(|i|std::array::from_fn(|j|n(&v["xmm"][i][j])));
}
fn compare(c:&Cpu,v:&Value,mask:u32) {
    assert_eq!(regs(c),std::array::from_fn(|i|n(&v["regs"][i])));
    assert_eq!((c.fl.snapshot().0|if c.df{0x400}else{0})&mask,n(&v["flags"])&mask);
    assert_eq!(c.xmm,std::array::from_fn(|i|std::array::from_fn(|j|n(&v["xmm"][i][j]))));
    assert_eq!(c.mxcsr.0,n(&v["mxcsr"]));
}
fn vectors()->Option<Value> {
    let path=std::env::var("DDNATIVE_JOB_JOINTS_VECTORS").unwrap_or_else(|_|"E:/ddda-re/notes/native-L165/job_joints.json".into());
    match std::fs::read_to_string(&path) {
        Ok(text)=>Some(serde_json::from_str(&text).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some()=>panic!("{path}: {e}"),
        Err(_)=>None,
    }
}
fn verify(body:LiftFn, pages:bool) {
    let Some(data)=vectors() else{return};
    let cases=data["cases"].as_array().unwrap();assert_eq!(cases.len(),840);
    let mut total_calls=0;
    for case in cases {
        let mut m=Memory {bytes:pairs(&case["initial"]),..Default::default()};
        let initial=m.bytes.clone();
        if pages {
            use ddnative_base::mem::{PageTable,READ,WRITE,ATOMIC_ACCESSES};
            ATOMIC_ACCESSES.store(true,std::sync::atomic::Ordering::Relaxed);
            let table=Box::new(PageTable::default());
            for a in initial.keys(){let page=(*a/4096)as usize;table.set_perm(page,READ|WRITE);table.backed_page(page);}
            m.table=Some(table);
            for (&a,&b)in &initial {m.set_u8(a,b);}
            m.writes.clear();
        }
        let mut c=Cpu::default();restore(&mut c,&case["before"]);
        let x87=c.st.state;
        let events=case["events"].as_array().unwrap().clone();
        let calls=Rc::new(RefCell::new(0usize));let index=calls.clone();
        x86::set_ext_calls(Some(Box::new(move|m,c,target|{
            let mut i=index.borrow_mut();let event=&events[*i];
            assert_eq!(target,n(&event["target"]));
            let mask=if matches!(target,0x01051b60|0x01052330|0x010533a0|0x0104ef10){0xcc5}else{0xcd5};
            compare(c,&event["before"],mask);
            for(a,b)in pairs(&event["before"]["memory"]){assert_eq!(m.u8(a),b,"callback memory {a:x}");}
            // Replay explicit synthetic callee results, not game behavior.
            for(a,b)in pairs(&event["after"]["memory"]){m.set_u8(a,b);}
            restore(c,&event["after"]);*i+=1;
        })));
        body(&mut m,&mut c);
        x86::set_ext_calls(None);
        assert_eq!(*calls.borrow(),case["events"].as_array().unwrap().len());
        compare(&c,&case["after"],0xcd5);
        assert_eq!(c.st.state,x87);assert_eq!(c.st.home,0);
        let changed=pairs(&case["after"]["memory"]);
        if pages {
            let mut expected=initial;expected.extend(changed);
            for(a,b)in expected{assert_eq!(m.u8(a),b,"seed {} memory {a:x}",case["seed"]);}
        } else {assert_eq!(m.writes,changed,"seed {}",case["seed"]);}
        total_calls+=*calls.borrow();
    }
    eprintln!("joint dispatcher: {} original returns / {total_calls} callback boundaries match",cases.len());
}

// verifies 0x01054700
#[test]
fn original_joint_dispatch_vectors(){verify(mtcore::job_joints::evaluate_joint_indices,false);}
#[test]
fn original_joint_dispatch_pages(){verify(mtcore::job_joints::evaluate_joint_indices,true);}
