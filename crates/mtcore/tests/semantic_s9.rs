//! L171: semantic ports against the original's recorded vectors, at the
//! ABI level: EAX, the callee-saved registers, ESP and every memory byte
//! the original leaves at or above its returned stack pointer. ECX, EDX,
//! the flags and the popped stack scratch below the returned ESP are not
//! compared (MSVC callers never read them after a call).
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

/// Run `body` on every case of `path` and compare at the ABI level.
fn abi_cases(path: &str, body: LiftFn) -> usize {
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
        body(&mut m, &mut c);
        // [eax, ecx, edx, ebx, esp, ebp, esi, edi]: all but ECX and EDX
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[0], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if a >= esp {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        // no write the original does not make, above the returned stack
        for (&a, &b) in &m.writes {
            if a >= esp {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        0x00fe_d890 | 0x00ff_1310 | 0x00da_7d70 | 0x00ff_04c0 => 1,
        0x012f_45c4 | 0x012f_439e => 0,
        0x00fe_66a0 => 4,
        0x2500_0000 | 0x0117_87c0 => 6,
        0x2500_0100 | 0x00fe_4f30 => 2,
        0x0105_4690 => 0,
        0x3000_0000 => 0, // find_sorted vectors: the count stub
        0x3000_0010 => 1, // and the entry(i) stub
        0x00f7_3b90 => 1, // owner->find(id) (0x00DD42C0's parent lookup)
        0x0104_ef10 | 0x0105_1b60 | 0x0105_33a0 | 0x0105_2330 => 1,
        0x00e0_e5e0 | 0x00e0_ec50 => 3,
        _ => 2,
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
    abi_cases_options(path, body, returns, native_callees, false)
}

/// As `abi_cases_full`; `nan_insensitive`: a float word that is a NaN in both
/// the original's and the port's memory counts as equal (which NaN
/// propagates, its sign and payload, depends on the operand order the
/// compiler picks for a commutative operation).
fn abi_cases_options(
    path: &str,
    body: LiftFn,
    returns: bool,
    native_callees: &[u32],
    nan_insensitive: bool,
) -> usize {
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
            let target_recorded = if paired {
                n(&trace[k]["target"])
            } else {
                n(&trace[k]["target"])
            };
            assert_eq!(target, target_recorded, "case {i}: call target");
            // the CRT log pair takes its argument in XMM0 and ignores ECX
            let register_abi = target == 0x012f_45c4 || target == 0x012f_439e;
            if !register_abi {
                assert_eq!(c.ecx, regs(call)[1], "case {i}: call ECX");
            }
            if register_abi {
                let lanes = if target == 0x012f_439e { 2 } else { 1 };
                for k in 0..lanes {
                    assert_eq!(
                        c.xmm[0][k],
                        n(&call["xmm0"][k]),
                        "case {i}: call {target:x} XMM0 lane {k}"
                    );
                }
            }
            assert_eq!(c.esp, regs(call)[4], "case {i}: call ESP");
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
            let (buffer, len) = if target == 0x00e0_e5e0 {
                (word(esp + 4), word(esp + 8))
            } else {
                (0, 0)
            };
            // the return address and the callee's own stack arguments
            let words = 1 + stack_args(target);
            for (&a, &b) in &recorded {
                let argument = (a >= esp && a < esp + 4 * words)
                    || target == 0x00fe_66a0
                    || target == 0x2500_0000
                    || target == 0x0117_87c0;
                let data = a >= buffer && a < buffer.wrapping_add(len);
                if argument || data {
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
            if register_abi {
                c.xmm[0] = std::array::from_fn(|k| n(&back["xmm0"][k]));
            }
            *position.borrow_mut() += if paired { 2 } else { 1 };
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let eax = if returns { [c.eax, after[0]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        if nan_insensitive {
            let is_nan = |w: u32| w & 0x7f80_0000 == 0x7f80_0000 && w & 0x007f_ffff != 0;
            let word_of = |f: &dyn Fn(u32) -> Option<u8>, a: u32| -> Option<u32> {
                let mut w = 0u32;
                for k in 0..4 {
                    w |= (f(a + k)? as u32) << (8 * k);
                }
                Some(w)
            };
            let addresses: Vec<u32> = expected.keys().map(|a| a & !3).collect();
            for a in addresses {
                let (e, g) = (
                    word_of(&|x| expected.get(&x).copied(), a),
                    word_of(&|x| m.bytes.get(&x).copied(), a),
                );
                if let (Some(e), Some(g)) = (e, g) {
                    if is_nan(e) && is_nan(g) {
                        for k in 0..4 {
                            let b = (e >> (8 * k)) as u8;
                            m.bytes.insert(a + k, b);
                            if m.writes.contains_key(&(a + k)) {
                                m.writes.insert(a + k, b);
                            }
                        }
                    }
                }
            }
        }
        if std::env::var_os("S9_DEBUG").is_some() {
            // list every differing word (little-endian) to localize a mismatch
            let mut words: Vec<u32> = expected.keys().filter(|&&a| a >= esp).map(|a| a & !3).collect();
            words.dedup();
            for a in words {
                let word = |f: &dyn Fn(u32) -> Option<u8>| {
                    (0..4)
                        .map(|k| f(a + k).map_or(String::from("--"), |b| format!("{b:02x}")))
                        .collect::<Vec<_>>()
                        .join("")
                };
                let (e, g) = (
                    word(&|x| expected.get(&x).copied()),
                    word(&|x| m.bytes.get(&x).copied()),
                );
                if e != g {
                    eprintln!("case {i}: word {a:x}: want {e} got {g}");
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

// verifies 0x00fed5d0 (ABI level; the CRT memcpy is done natively)
#[test]
fn pop_render_state_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full(
        "E:/ddda-re/notes/native-L184/pop_render_state.json",
        mtcore::semantic_s9::pop_render_state,
        true,
        &[0x012e_fea0],
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level pop_render_state cases passed");
}

// verifies 0x00ea5b50 (ABI level; void; the CRT memcpy is done natively)
#[test]
fn model_upload_state_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full(
        "E:/ddda-re/notes/native-L184/model_upload_state.json",
        mtcore::semantic_s9::model_upload_state,
        false,
        &[0x012e_fea0],
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level model_upload_state cases passed");
}

// verifies 0x00ff1310 (ABI level; void; the CRT memcpy is done natively)
#[test]
fn set_fade_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full(
        "E:/ddda-re/notes/native-L184/set_fade.json",
        mtcore::semantic_s9::set_fade,
        false,
        &[0x012e_fea0],
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level set_fade cases passed");
}

/// Page-table backed memory, to run the bulk path of `copy_guest`.
#[derive(Default)]
struct Paged {
    table: ddnative_base::mem::PageTable,
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a >> 12) as usize);
        assert!(!p.is_null());
        // SAFETY: the table owns the backed page.
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: the table owns the backed page.
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) }
    }
}

/// The native copy behaves like memmove for every overlap, across page
/// boundaries, on page-backed memory and on the plain trait path.
#[test]
fn copy_guest_is_overlap_safe() {
    let base = 0x3000_0000u32;
    let mut seed = 0x9e37_79b9u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for case in 0..600 {
        let size = 0x3000usize;
        let data: Vec<u8> = (0..size).map(|_| next() as u8).collect();
        let len = [0, 1, 4, 0x144, 0x1000, 0x1234, 0x2000][next() as usize % 7] as usize;
        let src = (next() as usize % 0x800) + if case % 2 == 0 { 0xff0 } else { 0 };
        let dst = (src as i64 + (next() % 0x2000) as i64 - 0x1000).clamp(0, (size - len) as i64) as usize;
        let src = src.min(size - len);
        let mut want = data.clone();
        want.copy_within(src..src + len, dst);
        let paged_run = case % 2 == 0;
        let mut paged = Paged::default();
        let mut plain = Memory::default();
        let m: &mut dyn Mem = if paged_run { &mut paged } else { &mut plain };
        for (i, &b) in data.iter().enumerate() {
            m.set_u8(base + i as u32, b);
        }
        let pg = ddnative_base::mem::Pages::of(m);
        mtcore::semantic_s9::copy_guest(m, pg, base + dst as u32, base + src as u32, len as u32);
        let got: Vec<u8> = (0..size).map(|i| m.u8(base + i as u32)).collect();
        assert!(
            got == want,
            "case {case}: src {src:#x} dst {dst:#x} len {len:#x} paged {paged_run}"
        );
    }
}

// verifies 0x00feb1f0 (ABI level; void; the sphere test 0x00d13600 is folded in)
#[test]
fn scene_collect_visible_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full(
        "E:/ddda-re/notes/native-L184/scene_collect_visible.json",
        mtcore::semantic_s9::scene_collect_visible,
        false,
        &[],
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level scene_collect_visible cases passed");
}

// verifies 0x00ff2ee0 (ABI level; void; the CRT memcpy is done natively)
#[test]
fn set_camera_matrices_matches_original() {
    ddnative_host::install();
    let n = abi_cases_options(
        "E:/ddda-re/notes/native-L184/set_camera_matrices.json",
        mtcore::semantic_s9::set_camera_matrices,
        false,
        &[0x012e_fea0],
        true,
    );
    assert!(n >= 1000, "{n} cases");
    eprintln!("{n} ABI-level set_camera_matrices cases passed");
}
