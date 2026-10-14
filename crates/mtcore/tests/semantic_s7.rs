#![allow(dead_code)]
//! L171: semantic ports against the original's recorded vectors, at the
//! ABI level: EAX, the callee-saved registers, ESP and every memory byte
//! the original leaves at or above its returned stack pointer. ECX, EDX,
//! the flags and the popped stack scratch below the returned ESP are not
//! compared (MSVC callers never read them after a call).
#[path = "support/callee_reads.rs"]
mod callee_reads;
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


/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        0x3000_0020 => 1, // the model's virtual method at vtable +0x58 (argument -1)
        _ => panic!("unexpected call target {target:x}"),
    }
}

/// As `abi_cases`, for a body that makes guest calls: each recorded call
/// must see the recorded memory (its arguments and the buffers they point
/// at) and ECX; the recorded callee's effects and EAX are then replayed.
fn abi_cases_with_calls(path: &str, body: LiftFn) -> usize {
    abi_cases_with_calls_ret(path, body, true)
}

/// `returns`: false for a void function, whose EAX is left over and not
/// part of its ABI.
fn abi_cases_with_calls_ret(path: &str, body: LiftFn, returns: bool) -> usize {
    abi_cases_full(path, body, returns, &[])
}

/// `native_callees`: recorded calls the semantic port does natively (a
/// CRT memcpy, say): their events are dropped and the final memory check
/// covers their effects.
fn abi_cases_full(path: &str, body: LiftFn, returns: bool, native_callees: &[u32]) -> usize {
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
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
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
            let target_recorded = if paired { n(&trace[k]["target"]) } else { n(&trace[k]["target"]) };
            assert_eq!(target, target_recorded, "case {i}: call target");
            assert_eq!(c.ecx, regs(call)[1], "case {i}: call ECX");
            assert_eq!(c.esp, regs(call)[4], "case {i}: call ESP");
            for &r in callee_reads::saved_reads(target) {
                assert_eq!([c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi][r], regs(call)[r], "call {target:x} register {r} (callee reads it)");
            }
            // What the callee reads: its return address, its three
            // arguments and the buffer the first two describe. The rest of
            // the recorded window is the caller's own frame (saved
            // registers, locals), which a semantic port need not rebuild.
            let recorded = pairs(&call["memory"]);
            let esp = regs(call)[4];
            let word = |a: u32| u32::from_le_bytes([0, 1, 2, 3].map(|k| recorded[&(a + k)]));
            // What the callee reads: its return address and arguments, and
            // for the interning call (0xE0E5E0: buffer, byte length, basis)
            // the buffer. The rest of the recorded window is the caller's
            // own frame (saved registers, locals).
            let (buffer, len) = if target == 0x00e0_e5e0 { (word(esp + 4), word(esp + 8)) } else { (0, 0) };
            // the return address and the callee's own stack arguments
            let words = 1 + stack_args(target);
            for (&a, &b) in &recorded {
                let argument = a >= esp && a < esp + 4 * words;
                let data = a >= buffer && a < buffer.wrapping_add(len);
                if argument || data {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} memory {a:x}");
                }
            }
            let effects = if paired { pairs(&back["effects"]) } else { pairs(&back["memory"]) };
            for (a, b) in effects {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
            c.ecx = regs(back)[1];
            c.edx = regs(back)[2];
            c.esp = regs(back)[4];
            *position.borrow_mut() += if paired { 2 } else { 1 };
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        // an x87 result: ST0 (64-bit mantissa, sign/exponent word) and one value on the stack
        if let Some(st0) = case["after"].get("st0") {
            let want = (st0[0].as_u64().unwrap(), n(&st0[1]) as u16);
            assert_eq!(c.st.top_extended(), Some(want), "case {i}: ST0");
            assert_eq!(c.st.depth(), 1, "case {i}: x87 depth");
        }
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let eax = if returns { [c.eax, after[0]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        if std::env::var_os("S7_DIAG").is_some() {
            // the joint matrix: expected (original) against the port, per float
            let word = |bytes: &BTreeMap<u32, u8>, a: u32| u32::from_le_bytes([0, 1, 2, 3].map(|k| bytes.get(&(a + k)).copied().unwrap_or(0)));
            for k in 0..16u32 {
                let a = 0x2000_1010 + 4 * k;
                let (want, got) = (word(&expected, a), word(&m.bytes, a));
                if want != got {
                    eprintln!("case {i}: joint[{k}] want {:?} ({want:08x}) got {:?} ({got:08x})", f32::from_bits(want), f32::from_bits(got));
                }
            }
        }
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


// verifies 0x00f2b5e0 uCnsJointOffset::vf24 (ABI level; void)
#[test]
fn semantic_joint_offset_update_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S7_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/joint_offset.json".into());
    let n = abi_cases_with_calls_ret(&path, mtcore::semantic_s7::joint_offset_update, false);
    eprintln!("{n} ABI-level joint_offset cases passed");
}

// verifies 0x012f0906 / 0x012f0a55 (the CRT sinf and cosf): the port's float
// results, bit for bit, on recorded inputs of every magnitude class
#[test]
fn semantic_crt_trig_matches_original() {
    let path = std::env::var("S7_TRIG_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/crt_trig.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut m = Memory::default();
    for table in data["tables"].as_array().unwrap() {
        let base = n(&table[0]);
        for (k, b) in table[1].as_array().unwrap().iter().enumerate() {
            m.bytes.insert(base + k as u32, n(b) as u8);
        }
    }
    let pg = ddnative_base::mem::Pages::of(&m);
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let (kind, x, want) = (n(&case[0]), f32::from_bits(n(&case[1])), n(&case[2]));
        let got = if kind == 0 { mtcore::semantic_s7_trig::sinf(&m, pg, x) } else { mtcore::semantic_s7_trig::cosf(&m, pg, x) };
        let got = got.unwrap_or_else(|| panic!("case {i}: kind {kind} x {x:e} left to the guest"));
        assert_eq!(got.to_bits(), want, "case {i}: kind {kind} x {x:e} ({:08x}): got {got:e} want {:e}", x.to_bits(), f32::from_bits(want));
    }
    eprintln!("{} trig cases passed", cases.len());
}

// verifies 0x012f161f (the CRT acosf): bit for bit on recorded inputs of every
// magnitude class, reading the routine's tables from the recorded guest memory
#[test]
fn semantic_crt_acosf_matches_original() {
    let path = std::env::var("S7_ACOS_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/crt_acosf.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut m = Memory::default();
    for table in data["tables"].as_array().unwrap() {
        let base = n(&table[0]);
        for (k, b) in table[1].as_array().unwrap().iter().enumerate() {
            m.bytes.insert(base + k as u32, n(b) as u8);
        }
    }
    let pg = ddnative_base::mem::Pages::of(&m);
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let (x, want) = (f32::from_bits(n(&case[0])), n(&case[1]));
        let got = mtcore::semantic_s7_math::acosf(&m, pg, x);
        assert_eq!(got.to_bits(), want, "case {i}: x {x:e} ({:08x}): got {got:e} want {:e}", x.to_bits(), f32::from_bits(want));
    }
    eprintln!("{} acosf cases passed", cases.len());
}

// verifies 0x012f0d4c (the CRT asinf): bit for bit on recorded inputs of every
// magnitude class, reading the routine's tables from the recorded guest memory
#[test]
fn semantic_crt_asinf_matches_original() {
    let path = std::env::var("S7_ASIN_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/crt_asinf.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut m = Memory::default();
    for table in data["tables"].as_array().unwrap() {
        let base = n(&table[0]);
        for (k, b) in table[1].as_array().unwrap().iter().enumerate() {
            m.bytes.insert(base + k as u32, n(b) as u8);
        }
    }
    let pg = ddnative_base::mem::Pages::of(&m);
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let (x, want) = (f32::from_bits(n(&case[0])), n(&case[1]));
        let got = mtcore::semantic_s7_math::asinf(&m, pg, x);
        assert_eq!(got.to_bits(), want, "case {i}: x {x:e} ({:08x}): got {got:e} want {:e}", x.to_bits(), f32::from_bits(want));
    }
    eprintln!("{} asinf cases passed", cases.len());
}

// verifies 0x00d1b840 (segment versus box; cdecl, bool in AL)
#[test]
fn semantic_segment_vs_box_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S7_BOX_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/segment_vs_box.json".into());
    let n = abi_cases_with_calls_ret(&path, mtcore::semantic_s7_collision::segment_vs_box, true);
    eprintln!("{n} ABI-level segment_vs_box cases passed");
}

// verifies 0x012f4d01 (the CRT expf): bit for bit on recorded inputs of every
// magnitude class, reading the routine's tables from the recorded guest memory
#[test]
fn semantic_crt_expf_matches_original() {
    let path = std::env::var("S7_EXP_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/crt_expf.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut m = Memory::default();
    for table in data["tables"].as_array().unwrap() {
        let base = n(&table[0]);
        for (k, b) in table[1].as_array().unwrap().iter().enumerate() {
            m.bytes.insert(base + k as u32, n(b) as u8);
        }
    }
    let pg = ddnative_base::mem::Pages::of(&m);
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let (x, want) = (f32::from_bits(n(&case[0])), n(&case[1]));
        let got = mtcore::semantic_s7_math::expf(&m, pg, x);
        assert_eq!(got.to_bits(), want, "case {i}: x {x:e} ({:08x}): got {got:e} want {:e}", x.to_bits(), f32::from_bits(want));
    }
    eprintln!("{} expf cases passed", cases.len());
}


// verifies 0x00f2ffc0 (distance falloff integral; thiscall, ST0 result)
#[test]
fn semantic_integrate_falloff_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S7_FALLOFF_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/integrate_falloff.json".into());
    let n = abi_cases_with_calls_ret(&path, mtcore::semantic_s7_falloff::integrate_falloff, false);
    eprintln!("{n} ABI-level integrate_falloff cases passed");
}

// verifies 0x012f05c0 (_ftol2_sse): ST0 loaded with fld m64, called, EAX and the
// x87 stack compared with the original
#[test]
fn semantic_ftol2_sse_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S7_FTOL_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/ftol2_sse.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory::default();
        for (k, b) in 1u32.to_le_bytes().iter().enumerate() {
            m.bytes.insert(0x019a_a078 + k as u32, *b);
        }
        let value = case["value"].as_u64().unwrap();
        for (k, b) in value.to_le_bytes().iter().enumerate() {
            m.bytes.insert(0x3000_0000 + k as u32, *b);
        }
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, _, c.ebp, c.esi, c.edi] = before;
        c.esp = n(&case["entry_esp"]);
        c.st.real_memory::<0x00dd, 8, false>(&mut m, 0x3000_0000); // fld qword
        mtcore::semantic_s7_ftol::ftol2_sse(&mut m, &mut c);
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[0], after[3], n(&case["entry_esp"]) + 4, after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: value {:e}", f64::from_bits(value));
        assert_eq!(c.st.depth(), 0, "case {i}: x87 depth");
    }
    eprintln!("{} ftol2_sse cases passed", cases.len());
}
