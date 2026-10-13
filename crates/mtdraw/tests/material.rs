//! L176: the material parameter lookup's semantic ports against the
//! original's recorded vectors (research/native/record_material.py), at
//! the ABI level: EAX, the callee-saved registers, ESP and every memory
//! byte the original leaves at or above its returned stack pointer. ECX,
//! EDX, the flags and the popped stack scratch below the returned ESP are
//! not compared (MSVC callers never read them after a call).
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
            if a >= esp {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "{entry:#x} case {i}: memory {a:x}");
            }
        }
        // no write the original does not make, above the returned stack
        for (&a, &b) in &m.writes {
            if a >= esp {
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
