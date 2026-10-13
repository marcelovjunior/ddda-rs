//! L184 s6: the semantic port of 0x00872420 (uCnsMotFilter::vf24) against
//! the original's recorded vectors (research/native/record_motfilter.py),
//! at the ABI level: the callee-saved registers, ESP and every memory byte
//! the original leaves at or above its returned stack pointer; the
//! original's guest calls (pose builders, CRT acosf/sinf, the model's
//! root-matrix method) with their registers, stack frames, arguments and
//! XMM0. The function returns nothing: EAX is not compared.
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

/// [eax, ecx, edx, ebx, esp, ebp, esi, edi]
/// The joint record the vectors use (research/native/record_motfilter.py).
const JOINT: u32 = 0x2100_0000;

fn word(bytes: &BTreeMap<u32, u8>, a: u32) -> Option<u32> {
    let mut w = 0u32;
    for k in 0..4 {
        w |= (*bytes.get(&(a + k))? as u32) << (8 * k);
    }
    Some(w)
}

const EAX: usize = 0;
const ECX: usize = 1;
const EBX: usize = 3;
const ESP: usize = 4;
const EBP: usize = 5;
const ESI: usize = 6;
const EDI: usize = 7;

/// Stack arguments of the guest callees (the stack words after the return
/// address): the 0x00829EC0 builder takes the pose source, the model's
/// root-matrix method (stub 0x30000058) takes -1.
fn stack_args(target: u32) -> u32 {
    match target {
        0x0082_9ec0 | 0x3000_0058 => 1,
        _ => 0,
    }
}

/// Run `body` on every case of `path`.
fn motion_filter_cases(path: &str, body: fn(&mut dyn Mem, &mut Cpu), matrix_at: u32, returns: bool) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    // tables the original's run need not have touched (see the recorder)
    let table: Vec<(u32, u8)> = match data.get("table") {
        Some(t) => {
            let base = n(&data["table_base"]);
            t.as_array().unwrap().iter().enumerate().map(|(i, b)| (base + i as u32, n(b) as u8)).collect()
        }
        None => Vec::new(),
    };
    let (mut nan_total, mut nan_differing) = (0usize, 0usize);
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        for &(a, b) in &table {
            m.bytes.entry(a).or_insert(b);
        }
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
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
            assert_eq!(c.esp, want[ESP], "case {i}: call {target:x} ESP");
            // EBX and EBP as the original holds them (the pose routines'
            // frames set EBP)
            assert_eq!([c.ebx, c.ebp], [want[EBX], want[EBP]], "case {i}: call {target:x} EBX/EBP");
            let q = |lo: u32, hi: u32| lo as u64 | ((hi as u64) << 32);
            match target {
                // double atan2(y = XMM0, x = XMM1) -> XMM0
                0x012f_1109 => {
                    assert_eq!(q(c.xmm[0][0], c.xmm[0][1]), call["xmm0q"].as_u64().unwrap(), "case {i}: atan2 y");
                    assert_eq!(q(c.xmm[1][0], c.xmm[1][1]), call["xmm1q"].as_u64().unwrap(), "case {i}: atan2 x");
                }
                0x3000_0058 => assert_eq!(c.ecx, want[ECX], "case {i}: root_matrix ECX"),
                _ => assert_eq!(c.xmm[0][0], n(&call["xmm0"]), "case {i}: call {target:x} XMM0"),
            }
            // the callee reads its return address and stack arguments
            let recorded = pairs(&call["memory"]);
            let esp = want[ESP];
            for (&a, &b) in &recorded {
                if a >= esp && a < esp + 4 * (1 + stack_args(target)) {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} memory {a:x}");
                }
            }
            for (a, b) in pairs(&back["memory"]) {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[EAX];
            c.esp = regs(back)[ESP];
            let r = back.get("xmm0q").map_or(n(&back["xmm0"]) as u64, |v| v.as_u64().unwrap());
            (c.xmm[0][0], c.xmm[0][1]) = (r as u32, (r >> 32) as u32);
            *position.borrow_mut() += 1;
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        if returns {
            assert_eq!(c.eax, after[EAX], "case {i}: eax");
        }
        let got = [c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[EBX], after[ESP], after[EBP], after[ESI], after[EDI]];
        assert_eq!(got, want, "case {i}: ebx/esp/ebp/esi/edi");
        let esp = after[ESP];
        let expected = pairs(&case["after"]["memory"]);
        // The joint's matrix: a NaN where the original has a NaN is equal
        // (which NaN differs only when two NaNs of different sign meet in
        // an operation: the operand order of a commutative operation
        // picks the one that survives, and the compiler's is not the
        // original's).
        let mut nan_words = Vec::new();
        for k in 0..16u32 {
            let at = matrix_at + 4 * k;
            if let (Some(want), Some(got)) = (word(&expected, at), word(&m.bytes, at)) {
                let is_nan = |w: u32| w & 0x7fff_ffff > 0x7f80_0000;
                if is_nan(want) && is_nan(got) {
                    nan_total += 1;
                    nan_differing += (want != got) as usize;
                    nan_words.push(at);
                }
            }
        }
        let nan_byte = |a: u32| nan_words.iter().any(|&w| a >= w && a < w + 4);
        for (&a, &b) in &expected {
            if a >= esp && !nan_byte(a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if a >= esp && !nan_byte(a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    eprintln!("NaN result words: {nan_total}, of which with different sign/payload: {nan_differing}");
    cases.len()
}

// verifies 0x00872420 (ABI level; void)
#[test]
fn semantic_motion_filter_matches_original() {
    ddnative_host::install();
    let path = std::env::var("L184_S6_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/motfilter.json".into());
    let cases = motion_filter_cases(&path, mtcore::semantic_s6::motion_filter_apply, JOINT + 0x10, false);
    eprintln!("{cases} ABI-level motion filter cases passed");
}

// verifies 0x00829ec0 (ABI level): the aim and twist pose builder
#[test]
fn semantic_aim_pose_matches_original() {
    ddnative_host::install();
    let path = std::env::var("L184_S6_AIM_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/aimpose.json".into());
    let cases = motion_filter_cases(&path, mtcore::semantic_s6_aim::aim_pose, 0x2600_0000, true);
    eprintln!("{cases} ABI-level aim pose cases passed");
}
