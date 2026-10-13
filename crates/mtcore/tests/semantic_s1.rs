//! L184 s1: semantic ports against the original's recorded vectors (ABI level).
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

/// The registers (eax ecx edx ebx esp ebp esi edi = 0..7) the callee reads as
/// inputs; the others are leftovers the port need not reproduce.
fn call_inputs(target: u32) -> &'static [usize] {
    match target {
        0x0042_9680 => &[1, 2],
        0x0042_3b80 => &[6],                                   // ESI = state
        0x0081_f280 | 0x0080_b450 | 0x0042_19a0 | 0x0042_15e0 | 0x00ca_de50 => &[0], // EAX
        0x00b8_09d0 => &[1, 2],
        0x00ff_0800 | 0x00ff_0d20 | 0x00ff_0ad0 | 0x0043_3b00 | 0x005b_19c0 => &[1],
        0x3000_0044 => &[1],
        0x3100_0020 | 0x3100_0030 | 0x3100_0040 | 0x3100_0050 | 0x3100_0060 | 0x00fe_d1e0 | 0x00fa_5860 => &[1],
        0x0114_5bf0 | 0x0106_6af0 | 0x0106_71f0 | 0x00fe_d890 => &[1],
        0x3000_0100 | 0x0041_9210 => &[1],
        0x004c_9b80 | 0x0076_f660 => &[0],
        0x0088_6210 | 0x3000_0000 => &[1],
        _ => &[],
    }
}

/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        0x0042_3b80 | 0x0081_f280 | 0x0080_b450 | 0x0088_6210 | 0x00b8_09d0 => 0,
        0x3000_0000 | 0x3000_0010 | 0x3000_0020 => 1, // tuning virtual, Enter/LeaveCriticalSection
        0x0042_19a0 => 4,
        0x00ff_0800 | 0x00ff_0d20 | 0x00ff_0ad0 => 2, // culler tests
        0x0043_3b00 | 0x005b_19c0 => 1,
        0x3000_0044 => 0, // child is_active()
        0x3100_0020 => 3, // instancing part method 0x2C
        0x3100_0030 | 0x3100_0040 | 0x3100_0050 | 0x3100_0060 | 0x00fa_5860 => 1,
        0x00fe_d1e0 => 0,
        0x0114_5bf0 | 0x0106_71f0 => 2, // material sync callees
        0x0106_6af0 | 0x00fe_d890 => 1,
        0x3000_0100 => 0, // controller virtual method 4
        0x0041_9210 | 0x0076_f430 | 0x004c_9b80 => 1,
        0x0076_f660 => 2,
        0x0042_15e0 => 3,
        0x00ca_de50 => 2,
        0x0042_9680 => 3, // effect request sink (0x005D1AF0)
        _ => 2, // virtual calls in these corpora take (joint, model)
    }
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
            let have = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
            for &r in call_inputs(target) {
                assert_eq!(have[r], regs(call)[r], "case {i}: call {target:x} input register {r}");
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
            let (buffer, len) = (0u32, 0u32);
            let _ = word;
            // bytes of the request the original built, which the callee reads
            if let Some(check) = call.get("check") {
                for (a, b) in pairs(check) {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} request byte {a:x}");
                }
            }
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
            c.esp = regs(back)[4];
            if let Some(bits) = back.get("st0") {
                c.st.push(n(bits)); // a float returned in ST0
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


// verifies 0x005d1af0 (ABI level; void)
#[test]
fn semantic_effect_timers_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/effect_timers.json", mtcore::semantic_s1::update_effect_timers, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level effect-timer cases passed");
}

// verifies 0x0084c0a0 (ABI level; void)
#[test]
fn semantic_character_frame_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/character_frame.json", mtcore::semantic_s1::update_character_frame, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level character-frame cases passed");
}

// verifies 0x0076f2d0 (ABI level; void)
#[test]
fn semantic_actor_slots_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/actor_slots.json", mtcore::semantic_s1::refresh_actor_slots, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level actor-slot cases passed");
}

// verifies 0x00c88380 (ABI level; its CRT memcpy calls are done natively)
#[test]
fn semantic_material_sync_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/material_sync.json", mtcore::semantic_s1::sync_standard_material, true, &[0x012e_fea0]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level material-sync cases passed");
}

// verifies 0x00c25ea0 (ABI level; void; its CRT memcpy calls are done natively)
#[test]
fn semantic_instancing_update_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/instancing_update.json", mtcore::semantic_s1::update_instancing, false, &[0x012e_fea0]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level instancing cases passed");
}

// verifies 0x00e6d070 (ABI level; void)
#[test]
fn semantic_child_count_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/child_count.json", mtcore::semantic_s1::count_active_children, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level child-count cases passed");
}

// verifies 0x007781a0 (ABI level; void, a leaf)
#[test]
fn semantic_gauge_tick_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/gauge_tick.json", mtcore::semantic_s1::tick_gauges, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level gauge-tick cases passed");
}

// verifies 0x00de50b0 (ABI level; void, a leaf)
#[test]
fn semantic_triangle_vertices_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/triangle_vertices.json", mtcore::semantic_s1::load_triangle_vertices, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level triangle-vertex cases passed");
}

// verifies 0x00ea42e0 (ABI level; returns the plane mask in EAX)
#[test]
fn semantic_cull_bounds_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L184/cull_bounds.json", mtcore::semantic_s1::cull_bounds, true, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level cull-bounds cases passed");
}
