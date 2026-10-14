//! L191 trial: the port of 0x00EA5B50 (`mtcore::semantic_trial`) against the
//! original's recorded vectors, at the ABI level: EAX (void, not compared),
//! the callee-saved registers, ESP and every memory byte the original leaves
//! at or above its returned stack pointer. Each recorded guest call must see
//! the recorded ESP, ECX, return address, arguments and buffers; its effects
//! are then replayed. The memcpy (0x012EFEA0) is native and not recorded.
#![allow(dead_code)]
use ddnative_base::{
    mem::Mem,
    x86::{Cpu, LiftFn},
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self
            .bytes
            .get(&a)
            .unwrap_or_else(|| panic!("unrecorded read {a:x}"))
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
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| (n(&p[0]), n(&p[1]) as u8))
        .collect()
}

fn regs(v: &Value) -> [u32; 8] {
    let r: Vec<u32> = v["regs"].as_array().unwrap().iter().map(n).collect();
    r.try_into().unwrap()
}

/// Stack arguments of the guest calls the vectors record (ECX is checked
/// separately; an indirect call's target is not known here, so its window
/// is the default).
fn stack_args(target: u32) -> u32 {
    match target {
        0x00fe_d890 | 0x00ff_1310 => 1,
        0x00fe_66a0 => 4,
        _ => 2,
    }
}

/// Runs `body` on every case of `path` and compares at the ABI level.
/// `native_callees`: recorded calls the port does natively (their events
/// are dropped; the final memory check covers their effects).
fn abi_cases(path: &str, body: LiftFn, native_callees: &[u32]) -> usize {
    use ddnative_base::x86;
    use std::{cell::RefCell, rc::Rc};
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory {
            bytes: pairs(&case["initial"]),
            writes: BTreeMap::new(),
        };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events: Vec<Value> = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e.get("target").map_or(true, |t| !native_callees.contains(&n(t))))
            .cloned()
            .collect();
        let events = Rc::new(events);
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        // Two recorded formats: a "call" event followed by its "return"
        // event (keys.json), or one event per call with "before" and
        // "after" states (the L165 corpora).
        let paired = events.first().is_some_and(|e| e.get("kind").is_some());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            let (call, back) = if paired {
                (&trace[k], &trace[k + 1])
            } else {
                (&trace[k]["before"], &trace[k]["after"])
            };
            assert_eq!(target, n(&trace[k]["target"]), "case {i}: call target");
            assert_eq!(c.ecx, regs(call)[1], "case {i}: call ECX");
            assert_eq!(c.esp, regs(call)[4], "case {i}: call ESP");
            // What the callee reads: its return address and its stack
            // arguments (the recorded window beyond those is the caller's).
            let recorded = pairs(&call["memory"]);
            let esp = regs(call)[4];
            let words = 1 + stack_args(target);
            for (&a, &b) in &recorded {
                let argument = (a >= esp && a < esp + 4 * words) || target == 0x00fe_66a0;
                if argument {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} memory {a:x}");
                }
            }
            let effects = if paired {
                pairs(&back["effects"])
            } else {
                pairs(&back["memory"])
            };
            for (a, b) in effects {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
            c.esp = regs(back)[4];
            *position.borrow_mut() += if paired { 2 } else { 1 };
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let got = [c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: ebx/esp/ebp/esi/edi");
        let esp = after[4];
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
    cases.len()
}

// verifies 0x00ea5b50 (ABI level; void; the CRT memcpy is done natively)
#[test]
fn model_upload_state_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/model_upload_state.json",
        mtcore::semantic_trial::model_upload_state,
        &[0x012e_fea0],
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level model_upload_state cases passed");
}
