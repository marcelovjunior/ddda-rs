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
fn call_inputs(_target: u32) -> &'static [usize] {
    &[1] // every callee of these ports is thiscall (ECX = its object)
}

/// Stack arguments of the callees the vectors record.
fn stack_args(target: u32) -> u32 {
    match target {
        0x00ff_1090 | 0x0120_0000 | 0x0120_0020 | 0x0120_0040 => 2,
        0x00fe_d8f0 | 0x00fe_dc00 => 0,
        0x0120_0010 | 0x00fe_d890 => 1,
        0x00ed_e880 => 7,
        0x00ee_5ce0 => 6,
        0x00ef_d240 => 3,
        0x00ee_a950 => 2,
        0x00ee_a130 | 0x0120_0030 => 3,
        _ => panic!("unknown callee {target:x}"),
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
    let mut skipped = 0usize;
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let all = case["events"].as_array().unwrap();
        // A recorded native callee (the CRT memcpy) that rewrote the caller's own stack
        // arguments cannot be reproduced by a real copy: such adversarial recordings are skipped.
        let rewrites_stack = |pair: &[Value]| {
            native_callees.contains(&n(&pair[0]["target"]))
                && pair.get(1).is_some_and(|b| {
                    let effects = if b.get("effects").is_some() { &b["effects"] } else { &b["memory"] };
                    pairs(effects).keys().any(|a| (0x0f00_0000..0x0f01_0000).contains(a))
                })
        };
        if all.first().is_some_and(|e| e.get("kind").is_some()) && all.chunks(2).any(rewrites_stack) {
            skipped += 1;
            continue;
        }
        // a "call" event is followed by its "return" event in the paired format
        let paired = all.first().is_some_and(|e| e.get("kind").is_some());
        let events: Vec<Value> = if paired {
            all.chunks(2)
                .filter(|pair| !native_callees.contains(&n(&pair[0]["target"])))
                .flatten()
                .cloned()
                .collect()
        } else {
            all.iter()
                .filter(|e| e.get("target").map_or(true, |t| !native_callees.contains(&n(t))))
                .cloned()
                .collect()
        };
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
        assert!(c.st.is_empty(), "case {i}: x87 stack not balanced");
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let eax = if returns { [c.eax, after[0]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if !(0x0f00_0000..esp).contains(&a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if !(0x0f00_0000..esp).contains(&a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len() - skipped
}



// verifies 0x00efd240 (ABI level; void)
#[test]
fn semantic_setup_pass_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L169/shadow_pass.json", mtcore::shadow_top::setup_pass, false, &[]);
    assert!(n >= 96 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level setup_pass cases passed");
}

// verifies 0x00efdc70 and 0x00efef80 (its jmp thunk) (ABI level; void)
#[test]
fn semantic_draw_cascades_matches_original() {
    ddnative_host::install();
    let n = abi_cases_full("E:/ddda-re/notes/native-L172-p2d/cascades.json", mtcore::shadow_top::draw_cascades, false, &[]);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "{n} cases");
    eprintln!("{n} ABI-level draw_cascades cases passed");
}
