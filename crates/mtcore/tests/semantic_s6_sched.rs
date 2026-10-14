//! L186 s6: the job scheduler's semantic ports (crates/mtcore/src/
//! semantic_s6_sched.rs) against the original's code run in Unicorn
//! (research/native/record_sched.py): EBX/EBP/ESI/EDI, ESP, every memory
//! byte at or above the returned ESP, and each guest call the original
//! makes (target, ESP, ECX for the thiscalls, the return address and the
//! arguments it pushes, WaitForMultipleObjects' handle array).
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self.bytes.get(&a).unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.bytes.insert(a, v);
        self.writes.insert(a, v);
    }
}

fn n(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn pairs(v: &Value) -> BTreeMap<u32, u8> {
    v.as_array().unwrap().iter().map(|p| (n(&p[0]), n(&p[1]) as u8)).collect()
}
fn regs(v: &Value) -> [u32; 8] {
    let r: Vec<u32> = v["regs"].as_array().unwrap().iter().map(n).collect();
    r.try_into().unwrap()
}
const EAX: usize = 0;
const ECX: usize = 1;
const EBX: usize = 3;
const ESP: usize = 4;
const EBP: usize = 5;
const ESI: usize = 6;
const EDI: usize = 7;

/// The imports (SetEvent, ResetEvent, WaitFor*) take no ECX.
fn takes_ecx(target: u32) -> bool {
    !matches!(target, 0x3000_0010 | 0x3000_0020 | 0x3000_0030 | 0x3000_0040 | 0x00db_d8d0)
}

fn run(path: &str, body: fn(&mut dyn Mem, &mut Cpu), returns: bool) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let (before, after) = (regs(&case["before"]), regs(&case["after"]));
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events: Rc<Vec<Value>> = Rc::new(case["events"].as_array().unwrap().clone());
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            let event = &trace[k];
            let (call, back) = (&event["before"], &event["after"]);
            assert_eq!(target, n(&event["target"]), "case {i}: call {k} target");
            let want = regs(call);
            assert_eq!(c.esp, want[ESP], "case {i}: call {k} ({target:x}) ESP");
            if takes_ecx(target) {
                assert_eq!(c.ecx, want[ECX], "case {i}: call {k} ({target:x}) ECX");
            }
            // the return address, the arguments and (WaitForMultipleObjects) the handles
            for (a, b) in pairs(&call["memory"]) {
                assert_eq!(m.u8(a), b, "case {i}: call {k} ({target:x}) memory {a:x}");
            }
            for (a, b) in pairs(&back["memory"]) {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[EAX];
            c.esp = regs(back)[ESP];
            *position.borrow_mut() += 1;
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        if returns {
            assert_eq!(c.eax, after[EAX], "case {i}: eax");
        }
        assert_eq!([c.ebx, c.esp, c.ebp, c.esi, c.edi], [after[EBX], after[ESP], after[EBP], after[ESI], after[EDI]], "case {i}: ebx/esp/ebp/esi/edi");
        let esp = after[ESP];
        let expected = pairs(&case["after"]["memory"]);
        // everything but the popped stack scratch below the returned ESP
        let observable = |a: u32| !(0x0f00_0000..esp).contains(&a);
        for (&a, &b) in &expected {
            if observable(a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if observable(a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

fn path(name: &str) -> String {
    std::env::var(format!("L186_{}", name.to_uppercase())).unwrap_or_else(|_| format!("E:/ddda-re/notes/native-L184/{name}.json"))
}

// verifies 0x00dbd8d0 (ABI level; void)
#[test]
fn run_jobs_matches_original() {
    let cases = run(&path("sched_run"), mtcore::semantic_s6_sched::run_jobs, false);
    eprintln!("{cases} run_jobs cases passed");
}

// verifies 0x00dbdc90 (ABI level)
#[test]
fn worker_thread_matches_original() {
    let cases = run(&path("sched_worker"), mtcore::semantic_s6_sched::worker_thread, true);
    eprintln!("{cases} worker_thread cases passed");
}

// verifies 0x00dbe560 (ABI level; void)
#[test]
fn schedule_matches_original() {
    let cases = run(&path("sched_join"), mtcore::semantic_s6_sched::schedule, false);
    eprintln!("{cases} schedule cases passed");
}
