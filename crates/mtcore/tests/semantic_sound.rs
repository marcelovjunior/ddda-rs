//! Sound-engine ports against the original's recorded behaviour
//! (research/native/record_filter_bank.py; vectors in `SOUND_VECTORS_DIR`, default
//! E:/ddda-re/notes/native-sound; skipped when absent unless DDNATIVE_REQUIRE_VECTORS is set).
//! ABI level: EAX/ECX/EDX (these functions are leaves, so their leftovers are checked too),
//! EBX/EBP/ESI/EDI, ESP and every memory byte at or above the returned ESP, and no write the
//! original does not make.
use ddnative_base::{
    mem::Mem,
    x86::Cpu,
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

/// Run `body` on every case of `name`; the number of cases.
fn run_cases(name: &str, body: fn(&mut dyn Mem, &mut Cpu)) -> usize {
    let dir = std::env::var("SOUND_VECTORS_DIR").unwrap_or_else(|_| "E:/ddda-re/notes/native-sound".into());
    let path = format!("{dir}/{name}");
    let text = match std::fs::read_to_string(&path) {
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
        body(&mut m, &mut c);
        let got = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        assert_eq!(got, after, "case {i}: eax ecx edx ebx esp ebp esi edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

// verifies 0x00DCDDE0 (void; no guest calls)
#[test]
fn filter_bank_matches_original() {
    let cases = run_cases("filter_bank.json", mtcore::semantic_sound::filter_bank);
    if cases > 0 {
        assert!(cases >= 1000, "{cases} cases");
    }
    eprintln!("{cases} ABI-level filter_bank cases passed");
}
