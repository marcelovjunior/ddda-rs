//! Original-instruction ABI vectors. Only dead stack scratch is excluded;
//! heap writes below ESP remain checked, as do all recorded call inputs.
use ddnative_base::{
    mem::{Mem, PageTable, READ, WRITE},
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
    paged: Option<Paged>,
}
impl Mem for Memory {
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        self.paged.as_ref().and_then(Mem::page_table)
    }
    fn u8(&self, a: u32) -> u8 {
        if let Some(p) = &self.paged {
            return p.u8(a);
        }
        *self
            .bytes
            .get(&a)
            .unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, b: u8) {
        if let Some(p) = &mut self.paged {
            p.put(a, b)
        }
        self.bytes.insert(a, b);
        self.writes.insert(a, b);
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
fn cpu(v: &Value) -> Cpu {
    let mut c = Cpu::default();
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = std::array::from_fn(|i| n(&v["regs"][i]));
    c
}
fn read(path: &str) -> Option<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) => Some(serde_json::from_str(&s).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}
fn verify(path: &str, body: LiftFn) -> usize {
    verify_mode(path, body, false)
}
fn verify_mode(path: &str, body: LiftFn, paged: bool) -> usize {
    let Some(data) = read(path) else { return 0 };
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory {
            bytes: pairs(&case["initial"]),
            ..Default::default()
        };
        if paged {
            let mut memory = Paged::new();
            for (&a, &b) in &m.bytes {
                memory.put(a, b)
            }
            m.paged = Some(memory);
        }
        let mut c = cpu(&case["before"]);
        let events = Rc::new(case["events"].as_array().cloned().unwrap_or_default());
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, pos) = (events.clone(), cursor.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *pos.borrow();
            let event = &trace[k];
            assert_eq!(target, n(&event["target"]), "case {i}: target");
            assert_eq!(c.ecx, n(&event["this"]), "case {i}: this");
            assert_eq!(c.esp, n(&event["esp"]), "case {i}: call esp");
            for (a, b) in pairs(&event["inputs"]) {
                assert_eq!(m.u8(a), b, "case {i}: call memory {a:x}");
            }
            for (a, b) in pairs(&event["effects"]) {
                m.set_u8(a, b);
            }
            c.eax = n(&event["eax"]);
            c.ecx = 0xcccccccc;
            c.edx = 0xdddddddd;
            c.esp = n(&event["returned_esp"]);
            *pos.borrow_mut() += 1;
        })));
        let host_mxcsr = x86::stmxcsr();
        x86::ldmxcsr(n(&case["before"]["mxcsr"]));
        body(&mut m, &mut c);
        x86::ldmxcsr(host_mxcsr);
        x86::set_ext_calls(None);
        if let Some(p) = &m.paged {
            // Scan every backed fixture page, not just expected writes:
            // this detects additional fast writes through Pages too.
            let pages: std::collections::BTreeSet<_> = m.bytes.keys().map(|a| a & !4095).collect();
            for base in pages {
                for a in base..base + 4096 {
                    let actual = p.u8(a);
                    if actual != m.bytes.get(&a).copied().unwrap_or(0) {
                        m.bytes.insert(a, actual);
                        m.writes.insert(a, actual);
                    }
                }
            }
        }
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: call count");
        let want = cpu(&case["after"]);
        assert_eq!(
            [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi],
            [want.eax, want.ebx, want.esp, want.ebp, want.esi, want.edi],
            "case {i}: ABI regs"
        );
        let expected = pairs(&case["after"]["memory"]);
        // These oracle stacks are 0x0f000000..0x0f010000. Never exclude
        // heap memory merely because its numeric address is below ESP.
        let live = |a: u32| !(0x0f00_0000..want.esp).contains(&a);
        for (&a, &b) in &expected {
            if live(a) {
                assert_eq!(m.bytes.get(&a), Some(&b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if live(a) {
                assert_eq!(expected.get(&a), Some(&b), "case {i}: extra write {a:x}");
            }
        }
    }
    eprintln!("{} ABI cases (paged={paged}): {path}", cases.len());
    cases.len()
}

// verifies 0x00E0E5E0
#[test]
fn hash_original() {
    let count = verify(
        "E:/ddda-re/notes/native-L158/job_hash.json",
        mtcore::semantic_draw_b::dependency_hash,
    );
    assert!(count == 0 || count >= 528);
}

// verifies 0x010655A0
#[test]
fn apply_material_original() {
    ddnative_host::install();
    verify(
        "E:/ddda-re/notes/native-L174/010655a0.json",
        mtcore::semantic_draw_b::apply_material,
    );
}

// verifies 0x00FFC9F0
#[test]
fn model_palette_original() {
    ddnative_host::install();
    verify(
        "E:/ddda-re/notes/native-L174/00ffc9f0.json",
        mtcore::semantic_draw_b::prepare_model_palette,
    );
}

// verifies 0x01140180
#[test]
fn culled_instances_original() {
    ddnative_host::install();
    verify(
        "E:/ddda-re/notes/native-L174/01140180.json",
        mtcore::semantic_draw_b::draw_culled_instances,
    );
}

// verifies 0x01066C60
#[test]
fn select_material_pass_original() {
    ddnative_host::install();
    verify(
        "E:/ddda-re/notes/native-L174/01066c60.json",
        mtcore::semantic_draw_b::select_material_pass,
    );
}

#[test]
fn abi_oracle_rejects_corruption() {
    let path = "E:/ddda-re/notes/native-L174/010655a0.json";
    if read(path).is_none() {
        return;
    }
    ddnative_host::install();
    let corruptions: [LiftFn; 5] = [
        |m, c| {
            mtcore::semantic_draw_b::apply_material(m, c);
            c.eax ^= 1;
        },
        |m, c| {
            mtcore::semantic_draw_b::apply_material(m, c);
            c.ebx ^= 1;
        },
        |m, c| {
            mtcore::semantic_draw_b::apply_material(m, c);
            c.esp = c.esp.wrapping_add(4);
        },
        |m, c| {
            mtcore::semantic_draw_b::apply_material(m, c);
            let v = m.u8(0x0e001154);
            m.set_u8(0x0e001154, v ^ 1);
        },
        |m, c| {
            mtcore::semantic_draw_b::apply_material(m, c);
            m.set_u8(0x0e003800, 1);
        },
    ];
    for (index, body) in corruptions.into_iter().enumerate() {
        assert!(
            std::panic::catch_unwind(|| verify(path, body)).is_err(),
            "accepted mutation {index}"
        );
    }
}

#[test]
fn page_backed_originals() {
    ddnative_host::install();
    // This test alone changes the global access mode. The sparse tests
    // never supply a page table and are independent of that mode.
    for atomic in [false, true] {
        ddnative_base::mem::ATOMIC_ACCESSES.store(atomic, std::sync::atomic::Ordering::Relaxed);
        for (path, body) in [
            (
                "E:/ddda-re/notes/native-L174/01140180.json",
                mtcore::semantic_draw_b::draw_culled_instances as LiftFn,
            ),
            (
                "E:/ddda-re/notes/native-L174/00ffc9f0.json",
                mtcore::semantic_draw_b::prepare_model_palette as LiftFn,
            ),
            (
                "E:/ddda-re/notes/native-L158/job_hash.json",
                mtcore::semantic_draw_b::dependency_hash as LiftFn,
            ),
            (
                "E:/ddda-re/notes/native-L174/010655a0.json",
                mtcore::semantic_draw_b::apply_material,
            ),
            (
                "E:/ddda-re/notes/native-L174/01066c60.json",
                mtcore::semantic_draw_b::select_material_pass,
            ),
        ] {
            verify_mode(path, body, true);
        }
    }
    ddnative_base::mem::ATOMIC_ACCESSES.store(false, std::sync::atomic::Ordering::Relaxed);
}

struct Paged {
    table: Box<PageTable>,
}
impl Paged {
    fn new() -> Self {
        Self {
            table: Box::new(PageTable::default()),
        }
    }
    fn put(&mut self, a: u32, b: u8) {
        self.table.set_perm((a >> 12) as usize, READ | WRITE);
        unsafe {
            *self
                .table
                .backed_page((a >> 12) as usize)
                .add((a & 4095) as usize) = b;
        }
    }
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        Some(std::ptr::NonNull::from(self.table.as_ref()))
    }
    fn u8(&self, a: u32) -> u8 {
        unsafe { *self.table.page((a >> 12) as usize).add((a & 4095) as usize) }
    }
    fn set_u8(&mut self, a: u32, b: u8) {
        self.put(a, b)
    }
}

fn baseline(entry: u32) -> Option<LiftFn> {
    match entry {
        // L174_BASELINES
        _ => None,
    }
}

#[test]
#[ignore = "isolated opt2 baselines supplied by research/native/l174_check.py"]
fn draw_benchmark() {
    ddnative_host::install();
    // Production job memory uses atomic accesses. Run serial too, without
    // concurrent tests changing this process-global mode.
    for atomic in [false, true] {
        ddnative_base::mem::ATOMIC_ACCESSES.store(atomic, std::sync::atomic::Ordering::Relaxed);
        for (entry, port) in [
            (
                0x1140180,
                mtcore::semantic_draw_b::draw_culled_instances as LiftFn,
            ),
            (0xffc9f0, mtcore::semantic_draw_b::prepare_model_palette as LiftFn),
            (0x10655a0, mtcore::semantic_draw_b::apply_material as LiftFn),
            (0x1066c60, mtcore::semantic_draw_b::select_material_pass),
        ] {
            if let Ok(filter) = std::env::var("L174_BENCH_ENTRY") {
                if filter != format!("{entry:x}") {
                    continue;
                }
            }
            let Some(lifted) = baseline(entry) else { continue };
            let data = read(&format!("E:/ddda-re/notes/native-L174/{entry:08x}.json")).unwrap();
            let cases = data["cases"].as_array().unwrap();
            for index in [0, 1, 17, 53, 95, cases.len() - 1] {
                let case = &cases[index];
                let initial = pairs(&case["initial"]);
                let expected = pairs(&case["after"]["memory"]);
                let before = cpu(&case["before"]);
                let host_mxcsr = x86::stmxcsr();
                x86::ldmxcsr(n(&case["before"]["mxcsr"]));
                let want = cpu(&case["after"]);
                let events: Vec<_> = case["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        (
                            n(&e["target"]),
                            n(&e["eax"]),
                            n(&e["returned_esp"]),
                            pairs(&e["effects"]),
                        )
                    })
                    .collect();
                let cursor = Rc::new(RefCell::new(0usize));
                let pos = cursor.clone();
                x86::set_ext_calls(Some(Box::new(move |m, c, target| {
                    let mut k = pos.borrow_mut();
                    let (expected, eax, sp, effects) = &events[*k];
                    assert_eq!(target, *expected);
                    for (&a, &b) in effects {
                        m.set_u8(a, b)
                    }
                    c.eax = *eax;
                    c.ecx = 0xcccccccc;
                    c.edx = 0xdddddddd;
                    c.esp = *sp;
                    *k += 1;
                })));
                let mut memory = Paged::new();
                for (&a, &b) in &initial {
                    memory.put(a, b)
                }
                let mut ratios = Vec::new();
                let mut medians = [Vec::new(), Vec::new()];
                for round in 0..7 {
                    let mut times = [0.0; 2];
                    for offset in 0..2 {
                        let variant = (round + offset) % 2;
                        let body = if variant == 0 { lifted } else { port };
                        let mut elapsed = std::time::Duration::ZERO;
                        for iteration in 0..520 {
                            for (&a, &b) in &initial {
                                memory.set_u8(a, b)
                            }
                            *cursor.borrow_mut() = 0;
                            let mut c = before.clone();
                            let start = std::time::Instant::now();
                            body(std::hint::black_box(&mut memory), &mut c);
                            let duration = start.elapsed();
                            if iteration >= 20 {
                                elapsed += duration;
                            }
                            if iteration == 0 {
                                assert_eq!(
                                    [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi],
                                    [want.eax, want.ebx, want.esp, want.ebp, want.esi, want.edi]
                                );
                                for (&a, &b) in &expected {
                                    if !(0x0f000000..want.esp).contains(&a) {
                                        assert_eq!(memory.u8(a), b, "paged {entry:x} case {index} byte {a:x}")
                                    }
                                }
                            }
                        }
                        times[variant] = elapsed.as_secs_f64() * 1e9 / 500.0;
                        medians[variant].push(times[variant]);
                    }
                    ratios.push(times[0] / times[1]);
                }
                ratios.sort_by(f64::total_cmp);
                for values in &mut medians {
                    values.sort_by(f64::total_cmp)
                }
                eprintln!(
                    "draw entry={entry:x} seed={} atomic={atomic} lifted_ns={:.1} semantic_ns={:.1} median={:.3}x range={:.3}..{:.3}",
                    case["seed"], medians[0][3], medians[1][3], ratios[3], ratios[0], ratios[6]
                );
                x86::set_ext_calls(None);
                x86::ldmxcsr(host_mxcsr);
            }
        }
    }
    ddnative_base::mem::ATOMIC_ACCESSES.store(false, std::sync::atomic::Ordering::Relaxed);
}

#[test]
#[ignore = "bounded CPU benchmark; run --ignored --nocapture"]
fn hash_benchmark() {
    let Some(data) = read("E:/ddda-re/notes/native-L158/job_hash.json") else {
        return;
    };
    for atomic in [false, true] {
        ddnative_base::mem::ATOMIC_ACCESSES.store(atomic, std::sync::atomic::Ordering::Relaxed);
        for length in [0, 4, 16, 64, 256, 1024, 4096] {
            let case = data["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| n(&v["length"]) == length && n(&v["pointer"]) == 0x0e000000)
                .unwrap();
            let mut m = Paged::new();
            for (a, b) in pairs(&case["initial"]) {
                m.put(a, b)
            }
            let before = cpu(&case["before"]);
            let mut ratios = Vec::new();
            for round in 0..7 {
                let mut times = [0.0; 2];
                for k in 0..2 {
                    let variant = (k + round) % 2;
                    let body: LiftFn = if variant == 0 {
                        mtcore::job_hash::dependency_hash
                    } else {
                        mtcore::semantic_draw_b::dependency_hash
                    };
                    let start = std::time::Instant::now();
                    for _ in 0..20000 {
                        let mut c = before.clone();
                        body(std::hint::black_box(&mut m), &mut c);
                        std::hint::black_box(c.eax);
                    }
                    times[variant] = start.elapsed().as_secs_f64();
                }
                ratios.push(times[0] / times[1]);
            }
            ratios.sort_by(f64::total_cmp);
            eprintln!(
                "hash bytes={length} atomic={atomic} median={:.3}x range={:.3}..{:.3}",
                ratios[3], ratios[0], ratios[6]
            );
        }
    }
    ddnative_base::mem::ATOMIC_ACCESSES.store(false, std::sync::atomic::Ordering::Relaxed);
}
