//! The observable-equivalent models (not wired) against recordings of the
//! original (research/native/vectors_frame.py `models`; private, skipped
//! when absent, required with DDNATIVE_REQUIRE_VECTORS=1).
//!
//! The contract is weaker than the ports': the model must leave every
//! byte of memory outside the original's own stack frame as the original
//! does, make the same calls in the same order with the same arguments
//! (the `this` or register argument the callee's convention takes, and
//! its stack arguments), and use their results the same way. Register
//! values the original leaves for nobody (scratch registers at calls and
//! at the return, its spills) are not compared.
use ddnative_base::mem::Mem;
use mtframe::recognition;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};

fn read(name: &str) -> Option<Value> {
    let dir = std::env::var("LIFT_DATA").unwrap_or_else(|_| "E:/ddda-re/notes/native".into());
    let path = format!("{dir}/vectors/{name}.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return None,
    };
    serde_json::from_str::<Value>(&text).ok()
}

fn u(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}

fn hex_bytes(s: &str) -> impl Iterator<Item = u8> + '_ {
    (0..s.len() / 2).map(move |k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
}

struct Windows(BTreeMap<u32, u8>);

impl Mem for Windows {
    fn u8(&self, addr: u32) -> u8 {
        *self
            .0
            .get(&addr)
            .unwrap_or_else(|| panic!("read outside the recorded windows: {addr:#x}"))
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        *self
            .0
            .get_mut(&addr)
            .unwrap_or_else(|| panic!("write outside the recorded windows: {addr:#x}")) = v;
    }
}

fn windows(v: &Value) -> BTreeMap<u32, u8> {
    let mut m = BTreeMap::new();
    for w in v.as_array().unwrap() {
        for (k, b) in hex_bytes(w[1].as_str().unwrap()).enumerate() {
            m.insert(u(&w[0]) + k as u32, b);
        }
    }
    m
}

/// A recorded call: the callee's entry state (registers, the stack
/// window) and its return state.
struct Call {
    target: u32,
    regs: BTreeMap<String, u32>,
    stack_base: u32,
    stack: Vec<u8>,
    eax: u32,
}

impl Call {
    fn reg(&self, r: &str) -> u32 {
        self.regs[r]
    }
    /// Stack argument `k` (0 = the first, above the return address).
    fn arg(&self, k: u32) -> u32 {
        let at = (self.reg("esp") + 4 + 4 * k - self.stack_base) as usize;
        u32::from_le_bytes(self.stack[at..at + 4].try_into().unwrap())
    }
}

/// Replays the recorded calls in order and checks each one's target and
/// arguments against the model's.
struct Replay {
    calls: VecDeque<Call>,
}

impl Replay {
    fn next(&mut self, what: &str, target: u32) -> Call {
        let c = self
            .calls
            .pop_front()
            .unwrap_or_else(|| panic!("{what}: the original made no more calls"));
        assert_eq!(c.target, target, "{what}: call target");
        c
    }
}

fn vcall(m: &dyn Mem, object: u32, slot: u32) -> u32 {
    m.u32(m.u32(object).wrapping_add(slot))
}

impl recognition::Calls for Replay {
    fn class_of(&mut self, m: &mut dyn Mem, unit: u32) -> u32 {
        let c = self.next("class_of", vcall(m, unit, 0x10));
        assert_eq!(c.reg("ecx"), unit, "class_of: this");
        c.eax
    }
    fn allocate(&mut self, m: &mut dyn Mem, allocator: u32, size: u32, align: u32) -> u32 {
        let c = self.next("allocate", vcall(m, allocator, 0x20));
        assert_eq!((c.reg("ecx"), c.arg(0), c.arg(1)), (allocator, size, align), "allocate: this, size, align");
        c.eax
    }
    fn allocate_tagged(&mut self, m: &mut dyn Mem, allocator: u32, size: u32, align: u32, tag: u32) -> u32 {
        let c = self.next("allocate_tagged", vcall(m, allocator, 0x1C));
        assert_eq!(
            (c.reg("ecx"), c.arg(0), c.arg(1), c.arg(2)),
            (allocator, size, align, tag),
            "allocate_tagged: this, size, align, tag"
        );
        c.eax
    }
    fn free(&mut self, m: &mut dyn Mem, allocator: u32, block: u32) {
        let c = self.next("free", vcall(m, allocator, 0x24));
        assert_eq!((c.reg("ecx"), c.arg(0)), (allocator, block), "free: this, block");
    }
    fn delete(&mut self, m: &mut dyn Mem, object: u32) {
        let c = self.next("delete", vcall(m, object, 0));
        assert_eq!((c.reg("ecx"), c.arg(0)), (object, 1), "delete: this, flags");
    }
    fn alive(&mut self, _m: &mut dyn Mem, unit: u32) -> bool {
        let c = self.next("alive", 0x005A_02F0);
        assert_eq!(c.reg("eax"), unit, "alive: unit (eax)");
        c.eax & 0xFF != 0
    }
    fn species(&mut self, _m: &mut dyn Mem, unit: u32, a: u8, b: u8) -> u32 {
        let c = self.next("species", 0x008B_C6E0);
        assert_eq!(
            (c.reg("eax"), c.reg("esi"), c.reg("edi")),
            (unit, a as u32, b as u32),
            "species: unit (eax), bytes (esi, edi)"
        );
        c.eax
    }
    fn visible(&mut self, _m: &mut dyn Mem, info: u32) -> bool {
        let c = self.next("visible", 0x0048_8440);
        assert_eq!(c.reg("esi"), info, "visible: record (esi)");
        c.eax & 0xFF != 0
    }
}

fn calls_of(c: &Value, stack_base: u32) -> VecDeque<Call> {
    c["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ev| Call {
            target: u(&ev["target"]),
            regs: ev["in"]["regs"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), u(v)))
                .collect(),
            stack_base,
            stack: hex_bytes(ev["in"]["stack"].as_str().unwrap()).collect(),
            eax: u(&ev["out"]["regs"]["eax"]),
        })
        .collect()
}

/// sRecognition::vf06's model against the original.
// verifies 0x00487B00
#[test]
fn recognition_model_matches_the_original() {
    let Some(cases) = read("frame_models").and_then(|v| v.as_array().cloned()) else { return };
    let mut failures = Vec::new();
    let mut n = 0;
    for (k, c) in cases.iter().enumerate() {
        if u(&c["addr"]) != 0x0048_7B00 {
            continue;
        }
        n += 1;
        let esp = u(&c["regs_in"]["esp"]);
        let stack_base = c["mem_in"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| u(&w[0]))
            .find(|&a| (0x0F00_0000..0x1000_0000).contains(&a))
            .expect("stack window");
        // the original's local MtArray: [esp+0x1C] after its prologue
        let frame = (esp.wrapping_sub(4) & !7).wrapping_sub(0x20).wrapping_sub(0x10);
        let found = frame + 0x1C;
        let mut m = Windows(windows(&c["mem_in"]));
        let mut replay = Replay { calls: calls_of(c, stack_base) };
        let this = u(&c["regs_in"]["ecx"]);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            recognition::update(&mut m, this, found, &mut replay)
        }));
        let msg = match made {
            Err(e) => Some(e.downcast_ref::<String>().cloned().unwrap_or_default()),
            Ok(()) if !replay.calls.is_empty() => Some(format!("{} calls not made", replay.calls.len())),
            Ok(()) => None,
        };
        // memory: everything but the original's own frame (below its
        // caller's stack pointer at the return)
        // and the stubs' own counters (the stubs stand for the callees)
        let cells: Vec<u32> = c["stub_cells"].as_array().unwrap().iter().map(u).collect();
        let live = |a: &u32| !(stack_base..esp.wrapping_add(4)).contains(a) && !cells.iter().any(|&c| (c..c + 4).contains(a));
        let want = windows(&c["mem_out"]);
        let diff: Vec<String> = want
            .iter()
            .filter(|(a, _)| live(a))
            .filter(|(a, v)| m.0.get(a) != Some(v))
            .take(6)
            .map(|(a, v)| format!("[{a:#x}] {:x?} want {v:#04x}", m.0.get(a)))
            .collect();
        if msg.is_some() || !diff.is_empty() {
            failures.push(format!("case {k}: {}; {}", msg.unwrap_or_default(), diff.join(", ")));
        }
    }
    assert!(n > 0, "recordings of 0x00487B00");
    assert!(
        failures.is_empty(),
        "{} of {n} cases differ:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
}

/// The cloth step schedule against the original's instructions
/// 0x00F20847..0x00F208FC: the fields written and the step count (eax).
// verifies 0x00F20847
#[test]
fn cloth_schedule_model_matches_the_original() {
    let Some(cases) = read("frame_models").and_then(|v| v.as_array().cloned()) else { return };
    let mut failures = Vec::new();
    let mut n = 0;
    for (k, c) in cases.iter().enumerate() {
        if u(&c["addr"]) != 0x00F2_0847 {
            continue;
        }
        n += 1;
        let mut m = Windows(windows(&c["mem_in"]));
        let body = u(&c["regs_in"]["esi"]);
        let steps = mtframe::cloth::schedule(&mut m, body);
        let stack = |a: &u32| (0x0F00_0000..0x1000_0000).contains(a);
        let want = windows(&c["mem_out"]);
        let diff: Vec<String> = want
            .iter()
            .filter(|(a, _)| !stack(a))
            .filter(|(a, v)| m.0.get(a) != Some(v))
            .take(6)
            .map(|(a, v)| format!("[{a:#x}] {:x?} want {v:#04x}", m.0.get(a)))
            .collect();
        let eax = u(&c["regs_out"]["eax"]);
        if steps != eax || !diff.is_empty() {
            failures.push(format!("case {k}: steps {steps} want {eax}; {}", diff.join(", ")));
        }
    }
    assert!(n > 0, "recordings of 0x00F20847");
    assert!(
        failures.is_empty(),
        "{} of {n} cases differ:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
}
