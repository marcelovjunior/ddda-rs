//! L172 P2a: the recording-lane semantic ports against the original's
//! recorded vectors at the ABI level (EAX is unused by both functions):
//! callee-saved registers, ESP, and every memory byte at or above the
//! returned stack pointer, plus no write the original does not make.
use ddnative_base::{mem::Mem, x86::{Cpu, LiftFn}};
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

fn abi_cases(path: &str, body: LiftFn) -> usize {
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
        body(&mut m, &mut c);
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

// verifies 0x00fed200 (ABI level; the CRT memset/memcpy are done natively)
#[test]
fn semantic_init_recording_lanes_matches_original() {
    let n = abi_cases("E:/ddda-re/notes/native-L184/lanes_init.json", mtcore::semantic_p2a::init_recording_lanes);
    eprintln!("{n} ABI-level init_recording_lanes cases passed");
}

// verifies 0x00fed3b0 (ABI level)
#[test]
fn semantic_merge_recording_lanes_matches_original() {
    let n = abi_cases("E:/ddda-re/notes/native-L184/lanes_merge.json", mtcore::semantic_p2a::merge_recording_lanes);
    eprintln!("{n} ABI-level merge_recording_lanes cases passed");
}

// Backed pages: the runtime's fast accessor path.
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
        assert!(!p.is_null(), "read of unmapped {a:x}");
        // SAFETY: this table owns the backed page for the lifetime of self.
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: this table owns the backed page and a is inside it.
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) };
    }
}

/// ns per call of the port against the instruction-level port (0x00FED200)
/// whose CRT calls are done by the callback, over the recorded cases.
#[test]
#[ignore = "microbenchmark"]
fn init_recording_lanes_cost() {
    use ddnative_base::x86;
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let text = std::fs::read_to_string("E:/ddda-re/notes/native-L184/lanes_init.json").unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let mut m = Paged::default();
    for (name, body) in [("instruction-level port", mtcore::hot::initialize_draw_recording_lanes as LiftFn), ("semantic port", mtcore::semantic_p2a::init_recording_lanes as LiftFn)] {
        x86::set_ext_calls(Some(Box::new(|m, c, target| {
            let (dst, a, len) = (m.u32(c.esp + 4), m.u32(c.esp + 8), m.u32(c.esp + 12));
            if target == 0x012e_fe20 {
                for i in 0..len {
                    m.set_u8(dst + i, a as u8);
                }
            } else {
                let bytes: Vec<u8> = (0..len).map(|i| m.u8(a + i)).collect();
                for (i, b) in bytes.into_iter().enumerate() {
                    m.set_u8(dst + i as u32, b);
                }
            }
            c.eax = dst;
            c.esp += 4;
        })));
        let mut elapsed = std::time::Duration::ZERO;
        let mut calls = 0u32;
        for case in cases.iter().step_by(15).take(100) {
            let initial = pairs(&case["initial"]);
            let before = regs(&case["before"]);
            for _ in 0..20 {
                for (&a, &b) in &initial {
                    m.set_u8(a, b);
                }
                let mut c = Cpu::default();
                [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
                let start = std::time::Instant::now();
                body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                elapsed += start.elapsed();
                calls += 1;
            }
        }
        x86::set_ext_calls(None);
        eprintln!("{name}: {:.0} ns/call", elapsed.as_nanos() as f64 / calls as f64);
    }
}
