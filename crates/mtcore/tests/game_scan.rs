//! L183: the game-scan semantic ports against the original's recorded
//! vectors (research/native/record_l183.py), at the ABI level: EAX, the
//! callee-saved registers, ESP and every memory byte the original leaves
//! outside its popped stack scratch. At each guest call every general
//! register, the words the call pushed and the memory written so far must
//! match; the callee's recorded writes and registers are then replayed.
//! ECX, EDX, the flags and the scratch below the returned ESP are not
//! compared after the return (MSVC callers never read them).
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::{cell::RefCell, rc::Rc};

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

/// Everything but the popped stack scratch (the 8 KiB below the returned
/// ESP: 0x0109DAC0's frame alone is 0x1474 bytes).
fn compared(a: u32, esp: u32) -> bool {
    !(esp.wrapping_sub(0x2000)..esp).contains(&a)
}

fn vectors(name: &str) -> Option<Value> {
    let dir = std::env::var("L183_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L183".into());
    let path = format!("{dir}/{name}");
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(serde_json::from_str(&t).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

fn abi_cases(data: &Value, body: LiftFn) -> usize {
    abi_cases_checking(data, body, true)
}

/// `all_registers`: every register at a guest call must match; otherwise
/// ECX and ESP (with the pushed words and memory), what an MSVC callee
/// can depend on (it saves EBX, EBP, ESI, EDI before using them, and
/// reads EAX and EDX only as arguments where its convention says so).
fn abi_cases_checking(data: &Value, body: LiftFn, all_registers: bool) -> usize {
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let entry = n(&case["entry"]);
        let seed = case["seed"].to_string();
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let esp = after[4];
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events = Rc::new(case["events"].as_array().unwrap().clone());
        let made = Rc::new(RefCell::new(0usize));
        let (trace, count, seed_at) = (events.clone(), made.clone(), seed.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = 2 * *count.borrow();
            *count.borrow_mut() += 1;
            let call = trace.get(k).unwrap_or_else(|| panic!("{entry:#x} case {i} (seed {seed_at}): an extra call to {target:#x}"));
            let back = &trace[k + 1];
            let at = regs(call);
            assert_eq!(target, n(&call["target"]), "{entry:#x} case {i} (seed {seed_at}): call {k} target");
            if all_registers {
                assert_eq!([c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi], at,
                           "{entry:#x} case {i} (seed {seed_at}): registers at call {}", k / 2);
            } else {
                assert_eq!([c.ecx, c.esp], [at[1], at[4]], "{entry:#x} case {i} (seed {seed_at}): ECX/ESP at call {}", k / 2);
            }
            let pushed = n(&call["pushed"]);
            for (&a, &b) in &pairs(&call["memory"]) {
                if compared(a, esp) || (at[4]..at[4].wrapping_add(pushed)).contains(&a) {
                    assert_eq!(m.u8(a), b, "{entry:#x} case {i} (seed {seed_at}): memory {a:x} at call {}", k / 2);
                }
            }
            for (a, b) in pairs(&back["effects"]) {
                m.set_u8(a, b);
            }
            let r = regs(back);
            [c.eax, c.ecx, c.edx] = [r[0], r[1], r[2]];
            c.esp = r[4];
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*made.borrow(), events.len() / 2, "{entry:#x} case {i} (seed {seed}): guest calls");
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[0], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "{entry:#x} case {i} (seed {seed}): eax/ebx/esp/ebp/esi/edi");
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if compared(a, esp) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "{entry:#x} case {i} (seed {seed}): memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if compared(a, esp) {
                assert_eq!(expected.get(&a).copied(), Some(b), "{entry:#x} case {i} (seed {seed}): extra write {a:x}");
            }
        }
    }
    cases.len()
}

// verifies 0x00D1B840 (ABI level)
#[test]
fn segment_hits_box_matches_original() {
    let Some(data) = vectors("segment.json") else { return };
    let n = abi_cases(&data, mtcore::game_scan::segment_hits_box);
    assert!(n >= 1400, "{n} cases");
    eprintln!("0x00D1B840: {n} ABI-level cases passed");
}

// verifies 0x004172C0 (ABI level; its unlock 0x005B4800 is a checked guest
// call whose recorded writes are replayed)
#[test]
fn scan_unlocks_matches_original() {
    let Some(data) = vectors("scan.json") else { return };
    let n = abi_cases(&data, mtcore::game_scan::scan_unlocks);
    assert!(n >= 1400, "{n} cases");
    eprintln!("0x004172C0: {n} ABI-level cases passed");
}

// verifies 0x0049E920 (ABI level; its type getters are read when they are
// `mov eax, imm32; ret`, else checked guest calls)
#[test]
fn nearest_object_matches_original() {
    let Some(data) = vectors("nearest.json") else { return };
    let n = abi_cases(&data, mtcore::game_scan::nearest_object);
    assert!(n >= 1300, "{n} cases");
    eprintln!("0x0049E920: {n} ABI-level cases passed");
}

// verifies 0x0109D480 (ABI level; its leaf callback and its fallback for
// slots from 0x19 are checked guest calls with replayed effects)
#[test]
fn query_boxes_matches_original() {
    let Some(data) = vectors("boxes.json") else { return };
    let n = abi_cases_checking(&data, mtcore::tree_query::query_boxes, false);
    assert!(n >= 1400, "{n} cases");
    eprintln!("0x0109D480: {n} ABI-level cases passed");
}

// verifies 0x0109DAC0 (ABI level, as 0x0109D480)
#[test]
fn query_box_matches_original() {
    let Some(data) = vectors("sat.json") else { return };
    let n = abi_cases_checking(&data, mtcore::tree_query::query_box, false);
    assert!(n >= 1400, "{n} cases");
    eprintln!("0x0109DAC0: {n} ABI-level cases passed");
}
