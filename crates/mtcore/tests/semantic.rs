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
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
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

// verifies 0x00e0ec50 (ABI level)
#[test]
fn semantic_expand_dependencies_matches_original() {
    let path = std::env::var("L130_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L130/dependencies.json".into());
    let n = abi_cases(&path, mtcore::semantic::expand_dependencies);
    eprintln!("{n} ABI-level cases passed");
}

/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        0x0105_4690 => 0,
        0x3000_0000 => 0, // find_sorted vectors: the count stub
        0x3000_0010 => 1, // and the entry(i) stub
        0x00f7_3b90 => 1, // owner->find(id) (0x00DD42C0's parent lookup)
        0x0104_ef10 | 0x0105_1b60 | 0x0105_33a0 | 0x0105_2330 => 1,
        0x00e0_e5e0 | 0x00e0_ec50 => 3,
        _ => 2, // virtual calls in these corpora take (joint, model)
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
    abi_cases_impl(path, body, returns, native_callees)
}

fn abi_cases_impl(path: &str, body: LiftFn, returns: bool, native_callees: &[u32]) -> usize {
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
            // the callee-saved registers the callee reads (from its lifted code)
            for &r in callee_reads::saved_reads(target) {
                assert_eq!([c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi][r], regs(call)[r], "case {i}: call {target:x} register {r} (callee reads it)");
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
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let eax = if returns { [c.eax, after[0]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
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

// verifies 0x00e10fc0 (ABI level)
#[test]
fn semantic_intern_dependency_keys_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L130/keys.json", |m, c| {
        mtcore::semantic::intern_dependency_keys_with(m, c, mtcore::semantic::guest_expand)
    });
    eprintln!("{n} ABI-level keys cases passed");
}

// verifies 0x01054700 (ABI level)
#[test]
fn semantic_update_joints_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L165/job_joints.json", mtcore::semantic::update_joints, false);
    eprintln!("{n} ABI-level joints cases passed");
}

// verifies 0x00fedd60 (ABI level; its CRT memcpy calls are done natively)
#[test]
fn semantic_sort_pairs_matches_original() {
    let n = abi_cases_full("E:/ddda-re/notes/native-L165/job_sort.json", mtcore::semantic::sort_pairs, true, &[0x012e_fea0]);
    eprintln!("{n} ABI-level sort cases passed");
}

// verifies 0x00cd8760 (ABI level): the devirtualized walk against the
// instruction-level port, whose two virtual calls run the real accessors
// (0x0040E330 count = this[0x64], 0x00CD7A30 entry(i) = this[0x60] + 0x70 i)
#[test]
fn semantic_find_identifier_matches_port() {
    use ddnative_base::x86;
    ddnative_host::install();
    let mut seed = 0x1234_5678u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let (owner, vtable, entries, stack) = (0x2000_0000u32, 0x2000_1000u32, 0x2001_0000u32, 0x2010_0000u32);
    for case in 0..4000 {
        let mut m = Memory::default();
        let count = next() % 40;
        let store = |m: &mut Memory, a: u32, v: u32| (0..4).for_each(|k| m.set_u8(a + k, (v >> (8 * k)) as u8));
        // a quarter of the owners have other methods: the port runs as is
        let real = case % 4 != 0;
        store(&mut m, owner, vtable);
        store(&mut m, owner + 0x60, entries);
        store(&mut m, owner + 0x64, count);
        store(&mut m, vtable + 0x3c, if real { 0x00cd_7a30 } else { 0x00cd_7a31 });
        store(&mut m, vtable + 0x40, 0x0040_e330);
        for i in 0..count {
            let e = entries + i * 0x70;
            let (kind, id) = (0x60 + next() % 0x1e, next() % 4);
            store(&mut m, e + 0x10, kind | (id << 16));
        }
        for a in (stack - 0x100..stack + 8).step_by(4) {
            store(&mut m, a, 0xcccc_cccc);
        }
        let packed = match next() % 8 {
            0 => 0x8000_0000 | next(),
            1 => next(),
            _ => ((0x60 + next() % 0x1e) << 16) | (next() % 4),
        };
        m.writes.clear();
        let run = |body: LiftFn, m: &mut Memory| {
            let mut c = Cpu::default();
            (c.eax, c.esi, c.esp, c.ebx, c.ebp, c.edi) = (packed, owner, stack, 0xb, 0xbb, 0xd);
            x86::set_ext_calls(Some(Box::new(|m, c, target| {
                let this = c.ecx;
                match target {
                    0x0040_e330 => {
                        c.eax = m.u32(this + 0x64);
                        c.esp += 4;
                    }
                    0x00cd_7a30 | 0x00cd_7a31 => {
                        c.eax = m.u32(c.esp + 4).wrapping_mul(0x70).wrapping_add(m.u32(this + 0x60));
                        c.esp += 8;
                    }
                    _ => panic!("call {target:x}"),
                }
            })));
            body(m, &mut c);
            x86::set_ext_calls(None);
            c
        };
        let mut port_memory = Memory { bytes: m.bytes.clone(), writes: BTreeMap::new() };
        let want = run(mtcore::hot::resolve_identifier, &mut port_memory);
        let got = run(mtcore::semantic::find_identifier, &mut m);
        let abi = |c: &Cpu| [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        assert_eq!(abi(&got), abi(&want), "case {case}: packed {packed:x} count {count}");
        // nothing written at or above the returned stack
        assert!(m.writes.keys().all(|&a| a < got.esp), "case {case}: native writes");
    }
}

// verifies 0x00dd42c0 (ABI level; void)
#[test]
fn semantic_dispatch_event_matches_original() {
    ddnative_host::install();
    let n = abi_cases_impl("E:/ddda-re/notes/native-L179/dispatch.json", mtcore::semantic::dispatch_event, false, &[]);
    eprintln!("{n} ABI-level dispatch cases passed");
}

// verifies 0x005b31a0 (ABI level)
#[test]
fn semantic_find_sorted_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L179/find_sorted.json", mtcore::semantic::find_sorted);
    eprintln!("{n} ABI-level find_sorted cases passed");
}

/// Page-table backed memory, as in semantic_s9.rs: the span read of
/// 0x00DD42C0 takes its page fast path here, and a listener head that
/// crosses a page takes the per-field fallback.
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
        assert!(!p.is_null(), "unbacked read {a:x}");
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

/// One random scene for `dispatch_event`: the writes that build it and its
/// initial registers. The same scene is run four ways below.
struct Scene {
    writes: Vec<(u32, u8)>,
    regs: [u32; 8],
    this: u32,
    event: u32,
}

fn dispatch_scene(seed: &mut u32) -> Scene {
    const THIS: u32 = 0x2000_0000;
    const EVENT: u32 = 0x2100_0000;
    const FOUND: u32 = 0x2300_0000;
    const FLOOR: u32 = 0x0156_1738;
    let mut next = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *seed
    };
    let this = THIS + (next() % 0x800) * 4;
    let event = EVENT + (next() % 0x400) * 4;
    let entry = 0x2200_0fe0 + (next() % 8) * 4;
    let mut writes = Vec::new();
    let put = |w: &mut Vec<(u32, u8)>, at: u32, v: u32| {
        for (j, b) in v.to_le_bytes().into_iter().enumerate() {
            w.push((at + j as u32, b));
        }
    };
    for a in 0..0x40u32 {
        writes.push((event + a, next() as u8));
    }
    // the object's bytes, then the fields the port reads set to small values
    for a in 0..0x1d00u32 {
        writes.push((this + a, next() as u8));
    }
    put(&mut writes, this + 0x4cd48, next() % 4);
    put(&mut writes, this + 0x4cd50, next() % 3);
    put(&mut writes, event + 4, if next() % 4 == 0 { u32::MAX } else { next() % 5 });
    put(&mut writes, event + 8, if next() % 4 == 0 { u32::MAX } else { next() % 4 });
    put(&mut writes, event + 0x30, next() % 3);
    writes.push((event + 0x2c, (next() % 2) as u8));
    put(&mut writes, entry + 4, event);
    for i in 0..4u32 {
        let l = this + 0x30 + i * 0x6e0;
        put(&mut writes, l + 4, next() % 2);
        put(&mut writes, l + 8, next() % 7);
        put(&mut writes, l + 0xc, next() % 3);
        put(&mut writes, l + 0x14, if next() % 4 == 0 { u32::MAX } else { next() % 4 });
        put(&mut writes, l + 0x18, if next() % 4 == 0 { u32::MAX } else { next() % 4 });
        put(&mut writes, l + 0xc8, ((next() % 1500) as f32 / 1000.0 - 0.5).to_bits());
    }
    put(&mut writes, FLOOR, ((next() % 1000) as f32 / 1000.0 - 0.2).to_bits());
    // the found object: its parent id at +0x1c, and the bytes around it
    for a in 0..0x40u32 {
        writes.push((FOUND + a, next() as u8));
    }
    put(&mut writes, FOUND + 0x1c, next() % 4);
    // EAX, EBX, ECX (this), EDX, ESP (entry), EBP, ESI, EDI
    let regs = [0x1234_5678, 0x0bad_0001, this, 0x0bad_0005, entry, 0x0bad_0002, 0x0bad_0003, 0x0bad_0004];
    Scene { writes, regs, this, event }
}

/// Runs `dispatch_event` (span reads) and `dispatch_event_per_field` on the
/// same random scenes in plain and page-table memory: the registers, the
/// guest calls made and every byte the scene touches must all match.
#[test]
fn dispatch_event_span_reads_match_per_field() {
    use ddnative_base::x86;
    use std::{cell::RefCell, rc::Rc};
    const FOUND: u32 = 0x2300_0000;
    let mut seed = 0x2545_f491u32;
    let mut cases = 0;
    for case in 0..400 {
        let scene = dispatch_scene(&mut seed);
        let mut results = Vec::new();
        for paged in [false, true] {
            for per_field in [false, true] {
                let mut plain = Memory::default();
                let mut table = Paged::default();
                let m: &mut dyn Mem = if paged { &mut table } else { &mut plain };
                for &(a, v) in &scene.writes {
                    m.set_u8(a, v);
                }
                let mut c = Cpu::default();
                [c.eax, c.ebx, c.ecx, c.edx, c.esp, c.ebp, c.esi, c.edi] = scene.regs;
                let log = Rc::new(RefCell::new(Vec::new()));
                let l = log.clone();
                x86::set_ext_calls(Some(Box::new(move |_m, c, target| {
                    l.borrow_mut().push((target, c.ecx, c.esp, c.ebx, c.ebp, c.esi, c.edi));
                    c.eax = if target == 0x00f7_3b90 { FOUND } else { 0 };
                })));
                if per_field {
                    mtcore::semantic::dispatch_event_per_field(m, &mut c);
                } else {
                    mtcore::semantic::dispatch_event(m, &mut c);
                }
                x86::set_ext_calls(None);
                let regs = [c.eax, c.ebx, c.ecx, c.edx, c.esp, c.ebp, c.esi, c.edi];
                let bytes: Vec<u8> = (0..0x1d00u32)
                    .map(|a| m.u8(scene.this + a))
                    .chain((0..0x40u32).map(|a| m.u8(scene.event + a)))
                    .chain((0..0x40u32).map(|a| m.u8(FOUND + a)))
                    .collect();
                let calls = log.take();
                results.push((regs, calls, bytes));
            }
        }
        cases += 1;
        for r in &results[1..] {
            assert_eq!(r.0, results[0].0, "case {case}: registers");
            assert_eq!(r.1, results[0].1, "case {case}: guest calls");
            assert_eq!(r.2, results[0].2, "case {case}: memory");
        }
    }
    eprintln!("{cases} span/per-field scenes agree (plain and paged)");
}

/// Before (per-field reads, the pre-change path) and after (one span read
/// per listener) on the same paged scene. Run with --release --ignored.
#[test]
#[ignore]
fn dispatch_event_span_bench() {
    use ddnative_base::x86;
    let mut seed = 0x2545_f491u32;
    let scene = dispatch_scene(&mut seed);
    x86::set_ext_calls(Some(Box::new(|_m, c, target| {
        c.eax = if target == 0x00f7_3b90 { 0x2300_0000 } else { 0 };
    })));
    let iters = 300_000;
    for all_live in [false, true] {
    let mut table = Paged::default();
    for &(a, v) in &scene.writes {
        table.set_u8(a, v);
    }
    if all_live {
        // four listeners, each live and matching the event: a call per listener
        let set = |m: &mut Paged, a: u32, v: u32| {
            for (j, b) in v.to_le_bytes().into_iter().enumerate() {
                m.set_u8(a + j as u32, b);
            }
        };
        set(&mut table, scene.this + 0x4cd48, 4);
        for i in 0..4u32 {
            set(&mut table, scene.this + 0x30 + i * 0x6e0 + 8, 1);
        }
        set(&mut table, scene.event + 4, u32::MAX);
        set(&mut table, scene.event + 8, u32::MAX);
        set(&mut table, scene.event + 0x30, 0);
    }
    let mut time = |per_field: bool| {
        let t = std::time::Instant::now();
        for _ in 0..iters {
            let mut c = Cpu::default();
            [c.eax, c.ebx, c.ecx, c.edx, c.esp, c.ebp, c.esi, c.edi] = scene.regs;
            if per_field {
                mtcore::semantic::dispatch_event_per_field(&mut table, &mut c);
            } else {
                mtcore::semantic::dispatch_event(&mut table, &mut c);
            }
        }
        t.elapsed().as_nanos() as f64 / iters as f64
    };
    let (mut before, mut after) = (f64::MAX, f64::MAX);
    for _ in 0..3 {
        before = before.min(time(true));
        after = after.min(time(false));
    }
    eprintln!("dispatch_event (all_live {all_live}): before {before:.1} ns/call, after {after:.1} ns/call, {:.2}x", before / after);
    }
}
