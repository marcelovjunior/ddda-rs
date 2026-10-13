//! L184 s2: semantic ports against the original's recorded vectors, at the
//! ABI level: EAX, the callee-saved registers, ESP and every memory byte
//! the original leaves at or above its returned stack pointer. ECX, EDX,
//! the flags and the popped stack scratch below the returned ESP are not
//! compared (MSVC callers never read them after a call).
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

/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        0x011c_2810 => 4,
        0x0116_6c00 => 2,
        0x00de_46f0 => 1,
        0x011c_3a30 => 0,
        0x010a_f350 | 0x010b_4710 | 0x010b_4560 | 0x010b_4960 | 0x010f_ea80 | 0x0111_b2b0 => 4,
        0x0111_b570 => 5,
        0x0115_f650 | 0x011c_3280 => 5,
        0x0116_6b10 | 0x00de_f0c0 => 2,
        0x00de_6010 => 1,
        0x007c_6f90 => 0,
        0x00fb_3790 => 0,
        0x0117_87c0 => 6,
        0x0117_3dc0 | 0x0117_40a0 | 0x0117_3800 | 0x0117_3ae0 | 0x0117_31b0 | 0x0117_3520 | 0x0117_4380 | 0x0117_5340 | 0x0117_5620 | 0x0117_4d80 | 0x0117_5060 | 0x0117_4730 | 0x0117_4aa0 | 0x0117_5900 => 6,
        0x00de_d390 | 0x0076_7960 => 1,
        0x0116_6a60 | 0x0116_69b0 => 2,
        0x0116_68a0 | 0x0116_5ef0 | 0x0116_67b0 | 0x0116_5e00 | 0x0116_4500 | 0x0116_5560 => 3,
        _ => panic!("unknown callee {target:x}"),
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
            // the objects the callee is handed, as the original built them
            if let Some(objects) = call.get("objects") {
                for (a, b) in pairs(objects) {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} object byte {a:x}");
                }
            }
            let effects = if paired { pairs(&back["effects"]) } else { pairs(&back["memory"]) };
            for (a, b) in effects {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
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


// verifies 0x00fac6f0 (ABI level)
#[test]
fn semantic_probe_segment_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/probe_segment.json", mtcore::semantic_s2::probe_segment);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none());
    eprintln!("{n} ABI-level probe_segment cases passed");
}

// verifies 0x0079e200 (ABI level; void). The tiny parameter-block methods
// are run as the original code by the recorder and done inline by the
// port; the stack objects are compared at every collision call.
#[test]
fn semantic_sweep_step_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/sweep_step.json", mtcore::semantic_s2::sweep_step, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none());
    eprintln!("{n} ABI-level sweep_step cases passed");
}

// verifies 0x01166c00 / 0x01166b10 / 0x00dea870 (ABI level)
#[test]
fn semantic_collision_query_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/cast_b.json", mtcore::semantic_s2::cast_shape_b_guest_walks);
    eprintln!("{n} ABI-level cast_shape_b cases passed");
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/cast_a.json", mtcore::semantic_s2::cast_shape_a_guest_walks);
    eprintln!("{n} ABI-level cast_shape_a cases passed");
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/segcast.json", mtcore::semantic_s2::segment_cast);
    eprintln!("{n} ABI-level segment_cast cases passed");
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/query.json", mtcore::semantic_s2::world_query);
    eprintln!("{n} ABI-level world_query cases passed");
}

// verifies the six layer walks 0x011669b0 0x01166a60 0x011667b0 0x011668a0 0x01165e00 0x01165ef0 (ABI level)
#[test]
fn semantic_layer_walks_match_original() {
    ddnative_host::install();
    for (name, body) in [
        ("walk_single_forward", mtcore::semantic_s2::walk_single_forward_guest_nodes as LiftFn),
        ("walk_single_backward", mtcore::semantic_s2::walk_single_backward_guest_nodes),
        ("walk_low_forward", mtcore::semantic_s2::walk_low_forward_guest_nodes),
        ("walk_low_backward", mtcore::semantic_s2::walk_low_backward_guest_nodes),
        ("walk_high_forward", mtcore::semantic_s2::walk_high_forward_guest_nodes),
        ("walk_high_backward", mtcore::semantic_s2::walk_high_backward_guest_nodes),
    ] {
        let n = abi_cases_with_calls(&format!("E:/ddda-re/notes/native-L184/{name}.json"), body);
        eprintln!("{n} ABI-level {name} cases passed");
    }
}

// verifies 0x011787c0 (ABI level)
#[test]
fn semantic_node_dispatch_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/node.json", mtcore::semantic_s2::node_dispatch);
    eprintln!("{n} ABI-level node_dispatch cases passed");
}

// The whole cast chain native (dispatcher, the three walks, the node
// dispatch) against the original's own code for all of them: only the leaf
// cast routines, 0x00FB3790, the shape's bounds method, 0x01164500 and
// 0x01165560 are recorded stubs.
#[test]
fn semantic_cast_chain_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/deep_b.json", mtcore::semantic_s2::cast_shape_b);
    eprintln!("{n} ABI-level deep cast_shape_b cases passed");
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/deep_a.json", mtcore::semantic_s2::cast_shape_a);
    eprintln!("{n} ABI-level deep cast_shape_a cases passed");
}

// Backed pages exercise the same fast accessor path as the parallel runtime.
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
        // SAFETY: this table owns the backed page for the lifetime of self.
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: this table owns the backed page and a is inside it.
        unsafe {
            ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32);
        }
    }
}

/// ns/call of a port over every recorded case (collision callees replayed
/// from the recording, so their cost is a table lookup), and the mean
/// number of instructions the original executes for the same cases.
fn bench(path: &str, body: LiftFn, name: &str) {
    use ddnative_base::x86;
    use std::{cell::RefCell, rc::Rc};
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let mut m = Paged::default();
    let mut total = std::time::Duration::ZERO;
    let mut floor = std::time::Duration::ZERO;
    let mut calls = 0u32;
    let mut instructions = 0u64;
    for round in 0..5 {
        for case in cases {
            for (a, b) in pairs(&case["initial"]) {
                m.set_u8(a, b);
            }
            let before = regs(&case["before"]);
            let mut c = Cpu::default();
            [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
            // parsed outside the timed region
            let trace: Rc<Vec<(Vec<(u32, u8)>, u32, u32)>> = Rc::new(
                case["events"].as_array().unwrap().iter().map(|e| (pairs(&e["after"]["memory"]).into_iter().collect(), regs(&e["after"])[0], regs(&e["after"])[4])).collect(),
            );
            let cursor = Rc::new(RefCell::new(0usize));
            let position = cursor.clone();
            x86::set_ext_calls(Some(Box::new(move |m, c, _| {
                let k = *position.borrow();
                let (writes, eax, esp) = &trace[k];
                for &(a, b) in writes {
                    m.set_u8(a, b);
                }
                c.eax = *eax;
                c.esp = *esp;
                *position.borrow_mut() += 1;
            })));
            let start = std::time::Instant::now();
            body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
            let elapsed = start.elapsed();
            // the same replayed callee calls without the port (the floor)
            let events = case["events"].as_array().unwrap().len();
            let mut c2 = Cpu::default();
            [c2.eax, c2.ecx, c2.edx, c2.ebx, c2.esp, c2.ebp, c2.esi, c2.edi] = before;
            *cursor.borrow_mut() = 0;
            let start = std::time::Instant::now();
            for _ in 0..events {
                x86::call_ext(std::hint::black_box(&mut m), std::hint::black_box(&mut c2), 0);
            }
            let floor_time = start.elapsed();
            if round > 0 {
                total += elapsed;
                floor += floor_time;
                calls += 1;
            }
            instructions += case["insns"].as_u64().unwrap_or(0);
        }
    }
    x86::set_ext_calls(None);
    eprintln!(
        "{name}: port {:.0} ns/call ({:.0} of it replaying the recorded callees) over {calls} calls; original executes {:.1} instructions/call (incl. stubbed callee entries)",
        total.as_nanos() as f64 / f64::from(calls),
        floor.as_nanos() as f64 / f64::from(calls),
        instructions as f64 / (5 * cases.len()) as f64
    );
}

#[test]
#[ignore = "microbenchmark"]
fn semantic_s2_cost() {
    ddnative_host::install();
    bench("E:/ddda-re/notes/native-L184/probe_segment.json", mtcore::semantic_s2::probe_segment, "probe_segment 0x00fac6f0");
    bench("E:/ddda-re/notes/native-L184/sweep_step.json", mtcore::semantic_s2::sweep_step, "sweep_step 0x0079e200");
}
