//! L176: the material parameter lookup's semantic ports against the
//! original's recorded vectors (research/native/record_material.py), at
//! the ABI level: EAX, the callee-saved registers, ESP and every memory
//! byte the original leaves outside its popped stack scratch. ECX, EDX,
//! the flags and the scratch below the returned ESP are not compared (MSVC
//! callers never read them after a call).
#[path = "../../mtcore/tests/support/callee_reads.rs"]
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

/// Whether the byte at `a` is compared: everything but the popped stack
/// scratch, the 4 KiB below the returned ESP (the recorders put their data
/// structures below the stack, so "at or above the returned ESP" alone
/// would leave them unchecked).
fn compared(a: u32, esp: u32) -> bool {
    !(esp.wrapping_sub(0x1000)..esp).contains(&a)
}

fn vectors() -> Option<Value> {
    let path = std::env::var("L176_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/material_lookup.json".into());
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(serde_json::from_str(&t).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

/// Run `body` on every case of `entry` and compare at the ABI level.
fn abi_cases(data: &Value, entry: u32, body: LiftFn) -> usize {
    let cases: Vec<&Value> = data["cases"].as_array().unwrap().iter().filter(|c| n(&c["entry"]) == entry).collect();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        body(&mut m, &mut c);
        // [eax, ecx, edx, ebx, esp, ebp, esi, edi]: all but ECX and EDX
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[0], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "{entry:#x} case {i} (seed {}): eax/ebx/esp/ebp/esi/edi", case["seed"]);
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if compared(a, esp) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "{entry:#x} case {i}: memory {a:x}");
            }
        }
        // no write the original does not make, above the returned stack
        for (&a, &b) in &m.writes {
            if compared(a, esp) {
                assert_eq!(expected.get(&a).copied(), Some(b), "{entry:#x} case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

// verifies 0x01066590 (ABI level)
#[test]
fn find_parameter_matches_original() {
    let Some(data) = vectors() else { return };
    let n = abi_cases(&data, 0x0106_6590, mtdraw::material::find_parameter);
    assert!(n > 500, "{n} cases");
    eprintln!("0x01066590: {n} ABI-level cases passed");
}

// verifies 0x01066B10 (ABI level)
#[test]
fn bind_parameter_matches_original() {
    let Some(data) = vectors() else { return };
    let n = abi_cases(&data, 0x0106_6B10, mtdraw::material::bind_parameter);
    assert!(n > 500, "{n} cases");
    eprintln!("0x01066B10: {n} ABI-level cases passed");
}

/// As `abi_cases`, for a body that makes one guest call: at the call, ECX,
/// ESP, the two words it pushed (argument, return address) and every byte
/// the original has written by then at or above its returned stack must
/// match; the recorded callee's registers are then replayed.
fn abi_cases_with_call(data: &Value, entry: u32, body: LiftFn) -> usize {
    use ddnative_base::x86;
    use std::{cell::RefCell, rc::Rc};
    let cases: Vec<&Value> = data["cases"].as_array().unwrap().iter().filter(|c| n(&c["entry"]) == entry).collect();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let esp = after[4];
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events = Rc::new(case["events"].as_array().unwrap().clone());
        let calls = Rc::new(RefCell::new(0usize));
        let (trace, made) = (events.clone(), calls.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = 2 * *made.borrow();
            *made.borrow_mut() += 1;
            let call = &trace[k];
            let back = &trace[k + 1];
            let at = regs(call);
            assert_eq!(target, n(&call["target"]), "case {i}: call target");
            assert_eq!((c.ecx, c.esp), (at[1], at[4]), "case {i}: call ECX/ESP");
            for &r in callee_reads::saved_reads(target) {
                assert_eq!([c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi][r], at[r], "case {i}: call {target:x} register {r} (callee reads it)");
            }
            for (&a, &b) in &pairs(&call["memory"]) {
                if compared(a, esp) || (at[4]..at[4].wrapping_add(8)).contains(&a) {
                    assert_eq!(m.u8(a), b, "case {i}: memory {a:x} at the call");
                }
            }
            // the callee's recorded memory writes, then its registers
            if let Some(effects) = back.get("effects") {
                for (a, b) in pairs(effects) {
                    m.set_u8(a, b);
                }
            }
            let r = regs(back);
            [c.eax, c.ecx, c.edx] = [r[0], r[1], r[2]];
            c.esp = r[4];
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*calls.borrow(), events.len() / 2, "case {i}: guest calls");
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[0], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "{entry:#x} case {i} (seed {}): eax/ebx/esp/ebp/esi/edi", case["seed"]);
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if compared(a, esp) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "{entry:#x} case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if compared(a, esp) {
                assert_eq!(expected.get(&a).copied(), Some(b), "{entry:#x} case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

// verifies 0x010655A0 (ABI level)
#[test]
fn apply_material_state_matches_original() {
    let path = std::env::var("L176_STATE_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/material_state.json".into());
    let data: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let n = abi_cases_with_call(&data, 0x0106_55A0, mtdraw::material::apply_material_state);
    assert!(n > 500, "{n} cases");
    eprintln!("0x010655A0: {n} ABI-level cases passed");
}

// verifies 0x00FEFB90 (ABI level; its arena allocation 0x00DA7D70 is a
// checked guest call, its memcpy the platform model)
#[test]
fn set_render_target_matches_original() {
    let path = std::env::var("L176_TARGET_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/set_target.json".into());
    let data: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let n = abi_cases_with_call(&data, 0x00FE_FB90, mtdraw::target::set_render_target);
    assert!(n > 1000, "{n} cases");
    eprintln!("0x00FEFB90: {n} ABI-level cases passed");
}

// verifies 0x00FEDD60 (ABI level; its tail copies are the platform memcpy)
#[test]
fn sort_records_matches_original() {
    let path = std::env::var("L176_SORT_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/record_sort.json".into());
    let data: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let n = abi_cases(&data, 0x00FE_DD60, mtdraw::sort::sort_records);
    assert!(n > 250, "{n} cases");
    eprintln!("0x00FEDD60: {n} ABI-level cases passed");
}

fn cull_vectors() -> Option<Value> {
    let path = std::env::var("L176_CULL_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/cull.json".into());
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(serde_json::from_str(&t).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

// verifies 0x00FF0780, 0x00FF0AD0, 0x00FF0D20 and 0x01067790 (ABI level)
#[test]
fn frustum_tests_and_colour_match_original() {
    let Some(data) = cull_vectors() else { return };
    for (entry, body) in [
        (0x00FF_0780u32, mtdraw::cull::cull_sphere as LiftFn),
        (0x00FF_0AD0, mtdraw::cull::cull_oriented),
        (0x00FF_0D20, mtdraw::cull::cull_box),
        (0x00FF_0E60, mtdraw::cull::cull_box_all),
        (0x0106_7790, mtdraw::material::material_colour),
    ] {
        let n = abi_cases(&data, entry, body);
        assert!(n >= 500, "{entry:#x}: {n} cases");
        eprintln!("{entry:#010x}: {n} ABI-level cases passed");
    }
}

// verifies 0x01062F20 (ABI level; its arena refill 0x00FED890 is a checked
// guest call whose recorded writes are replayed, its memcpy the platform model)
#[test]
fn update_material_matches_original() {
    let path = std::env::var("L176_UPDATE_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/material_update.json".into());
    let data: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let n = abi_cases_with_call(&data, 0x0106_2F20, mtdraw::material::update_material);
    assert!(n > 1000, "{n} cases");
    eprintln!("0x01062F20: {n} ABI-level cases passed");
}

// verifies 0x0049B1A0 (ABI level; its lights' virtual calls and the arena
// refill are checked guest calls with replayed effects)
#[test]
fn scene_lights_matches_original() {
    let path = std::env::var("L176_LIGHTS_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L176/scene_lights.json".into());
    let data: Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let n = abi_cases_with_call(&data, 0x0049_B1A0, mtdraw::lights::scene_lights);
    assert!(n > 1000, "{n} cases");
    eprintln!("0x0049B1A0: {n} ABI-level cases passed");
}
