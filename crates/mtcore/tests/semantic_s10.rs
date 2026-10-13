//! L184 s10: semantic ports against the original's recorded behaviour
//! (research/native/record_s10_*.py, vectors in E:/ddda-re/notes/native-L184),
//! at the ABI level: EAX (when the function returns), EBX/EBP/ESI/EDI, ESP
//! and every memory byte at or above the returned ESP, and no write the
//! original does not make. Each recorded guest call must be made with the
//! recorded target, stack, return address and the registers its convention
//! reads; its recorded effects are then replayed.
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

/// Register order of the recorded files.
const EAX: usize = 0;
const ECX: usize = 1;
const EBX: usize = 3;
const ESP: usize = 4;
const EBP: usize = 5;
const ESI: usize = 6;
const EDI: usize = 7;

/// A recorded callee: the registers it reads (checked at the call) and its
/// stack argument count.
struct Callee {
    target: u32,
    reads: &'static [usize],
    stack_args: u32,
}

type Body = fn(&mut dyn Mem, &mut Cpu);

/// Run `body` on every case of `path`; `returns`: EAX is part of the ABI.
/// Returns (cases, calls made per target).
fn run_cases(path: &str, body: Body, returns: bool, callees: &[Callee]) -> (usize, BTreeMap<u32, usize>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return (0, BTreeMap::new()),
    };
    ddnative_host::install();
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let mut made: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events = Rc::new(case["events"].as_array().unwrap().clone());
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        let log = Rc::new(RefCell::new(Vec::<u32>::new()));
        let log2 = log.clone();
        let table: Vec<(u32, &'static [usize], u32)> = callees.iter().map(|c| (c.target, c.reads, c.stack_args)).collect();
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            assert!(k < trace.len(), "case {i}: unexpected call to {target:x}");
            let (call, back) = (&trace[k]["before"], &trace[k]["after"]);
            assert_eq!(target, n(&trace[k]["target"]), "case {i}: call {k} target");
            let &(_, reads, stack_args) = table
                .iter()
                .find(|t| t.0 == target)
                .unwrap_or_else(|| panic!("case {i}: no convention for {target:x}"));
            let at_call = regs(call);
            let now = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
            for &r in reads.iter().chain(&[ESP]) {
                assert_eq!(now[r], at_call[r], "case {i}: call {k} ({target:x}) register {r}");
            }
            let recorded = pairs(&call["memory"]);
            let esp = at_call[ESP];
            let words = 1 + stack_args;
            for (&a, &b) in &recorded {
                if a >= esp && a < esp + 4 * words {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} memory {a:x}");
                }
            }
            for (a, b) in pairs(&back["memory"]) {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[EAX];
            c.esp = regs(back)[ESP];
            log2.borrow_mut().push(target);
            *position.borrow_mut() += 1;
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        for t in log.borrow().iter() {
            *made.entry(*t).or_default() += 1;
        }
        let eax = if returns { [c.eax, after[EAX]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[EBX], after[ESP], after[EBP], after[ESI], after[EDI]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[ESP];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if a >= esp {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if a >= esp {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    (cases.len(), made)
}

fn vectors(name: &str) -> String {
    std::env::var("S10_VECTORS_DIR").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184".into()) + "/" + name
}

// verifies 0x00c85a00 (ABI level; void)
#[test]
fn semantic_apply_param_block_matches_original() {
    const BLOCK_ARG_EAX_ECX: &[usize] = &[EAX, ECX, EBX, EBP, ESI, EDI];
    let c = |target, reads, stack_args| Callee { target, reads, stack_args };
    let callees = [
        c(0x00c8_6390, BLOCK_ARG_EAX_ECX, 0),
        c(0x00c8_6420, BLOCK_ARG_EAX_ECX, 1),
        c(0x00c8_64d0, BLOCK_ARG_EAX_ECX, 0),
        c(0x00c8_79d0, BLOCK_ARG_EAX_ECX, 0),
        c(0x00c8_7a70, &[EBX, EBP, ESI, EDI], 0),
        c(0x00c8_7b70, &[EBX, EBP, ESI, EDI], 0),
        c(0x00c8_7c30, &[EBX, EBP, ESI, EDI], 0),
        c(0x00c8_7ca0, BLOCK_ARG_EAX_ECX, 0),
        c(0x3000_0000, &[ECX, EBX, EBP, ESI, EDI], 0),
        c(0x3000_0010, &[ECX, EBX, EBP, ESI, EDI], 0),
        c(0x3000_0020, &[ECX, EBX, EBP, ESI, EDI], 0),
        c(0x3000_0030, &[ECX, EBX, EBP, ESI, EDI], 0),
    ];
    let (cases, made) = run_cases(&vectors("param_block.json"), mtcore::semantic_s10::apply_param_block, false, &callees);
    eprintln!("{cases} ABI-level param_block cases passed; calls {made:x?}");
}

// verifies 0x007ffcc0 (ABI level; EAX = the final virtual call's result)
#[test]
fn semantic_submit_pass_records_matches_original() {
    let c = |target, stack_args| Callee { target, reads: &[ECX, ESI, EDI], stack_args };
    let callees = [
        c(0x00ff_14d0, 0),
        c(0x00fe_d190, 2),
        c(0x00ea_23f0, 2),
        c(0x3000_0014, 4),
        c(0x3000_0018, 1),
        c(0x3000_001c, 4),
        c(0x3000_0020, 3),
    ];
    let (cases, made) = run_cases(&vectors("pass_records.json"), mtcore::semantic_s10::submit_pass_records, true, &callees);
    eprintln!("{cases} ABI-level pass_records cases passed; calls {made:x?}");
}
