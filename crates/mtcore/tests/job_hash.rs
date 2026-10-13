//! Private original-instruction vectors; no game files are distributed.
use ddnative_base::{mem::Mem, x86::{self, Cpu, LiftFn}};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Default)]
struct Memory { bytes: BTreeMap<u32,u8>, writes: BTreeMap<u32,u8>,
    accesses: RefCell<Vec<(bool,u32)>>, fail_at: Option<usize> }
impl Memory {
    fn access(&self, write: bool, a: u32) {
        let mut trace = self.accesses.borrow_mut();
        trace.push((write,a));
        assert_ne!(self.fail_at, Some(trace.len()), "synthetic access fault");
    }
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 { self.access(false,a); *self.bytes.get(&a).unwrap_or_else(|| panic!("unrecorded read {a:x}")) }
    fn set_u8(&mut self, a: u32, b: u8) { self.access(true,a); self.bytes.insert(a,b); self.writes.insert(a,b); }
}
fn n(v: &Value) -> u32 { v.as_u64().unwrap() as u32 }
fn pairs(v: &Value) -> BTreeMap<u32,u8> {
    v.as_array().unwrap().iter().map(|p| (n(&p[0]),n(&p[1]) as u8)).collect()
}
fn regs(c: &Cpu) -> [u32;8] { [c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi] }
fn cpu(v: &Value) -> Cpu {
    let mut c = Cpu::default();
    [c.eax,c.ecx,c.edx,c.ebx,c.esp,c.ebp,c.esi,c.edi] = std::array::from_fn(|i| n(&v["regs"][i]));
    c.fl = x86::Flags::new(x86::RAW,0,0,n(&v["flags"]),32);
    c.df = n(&v["flags"]) & 0x400 != 0;
    c.mxcsr = x86::Mxcsr(n(&v["mxcsr"]));
    c.xmm = std::array::from_fn(|i| std::array::from_fn(|j| n(&v["xmm"][i][j])));
    c
}
fn vectors() -> Option<Value> {
    let path = std::env::var("DDNATIVE_JOB_HASH_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L158/job_hash.json".into());
    match std::fs::read_to_string(&path) {
        Ok(s) => Some(serde_json::from_str(&s).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}
fn verify(body: LiftFn) {
    let Some(data) = vectors() else { return; };
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 528, "complete L158 corpus required");
    for case in cases {
        let mut m = Memory { bytes:pairs(&case["initial"]), ..Default::default() };
        let mut c = cpu(&case["before"]);
        let x87 = c.st.state;
        let expected = &case["after"];
        body(&mut m,&mut c);
        assert_eq!(regs(&c), std::array::from_fn(|i| n(&expected["regs"][i])), "seed {}", case["seed"]);
        let mask = n(&expected["flag_mask"]);
        assert_eq!((c.fl.snapshot().0 | if c.df {0x400} else {0}) & mask, n(&expected["flags"]) & mask);
        assert_eq!(c.xmm, std::array::from_fn(|i| std::array::from_fn(|j| n(&expected["xmm"][i][j]))));
        assert_eq!(c.mxcsr.0, n(&expected["mxcsr"]));
        assert_eq!(case["before"]["x87"], expected["x87"]);
        assert_eq!(c.st.state, x87);
        assert_eq!(m.writes, pairs(&expected["memory"]));
    }
    eprintln!("job_hash: {} original-instruction returns match", cases.len());
}

// verifies 0x00E0E5E0
#[test]
fn original_dependency_hash_vectors() { verify(mtcore::job_hash::dependency_hash); }
