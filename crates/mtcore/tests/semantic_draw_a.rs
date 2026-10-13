//! Original instruction oracle; exclude only discarded STACK bytes, never heap.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self
            .bytes
            .get(&a)
            .unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, b: u8) {
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
fn regs(v: &Value) -> [u32; 8] {
    std::array::from_fn(|i| n(&v["regs"][i]))
}
fn cpu(v: &Value) -> Cpu {
    let mut c = Cpu::default();
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = regs(v);
    c.df = n(&v["flags"]) & 0x400 != 0;
    c.mxcsr = x86::Mxcsr(n(&v["mxcsr"]));
    c.st.state[..2].copy_from_slice(&(n(&v["x87"][0]) as u16).to_le_bytes());
    c
}
fn vectors(entry: u32) -> Option<Value> {
    let path = format!("E:/ddda-re/notes/native-L173/{entry:08x}.json");
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(serde_json::from_str(&t).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}
fn visible(a: u32, esp: u32) -> bool {
    !(0x0f000000..esp).contains(&a)
}
fn install_calls(case: &Value) -> Rc<RefCell<usize>> {
    let events = case["events"].as_array().unwrap().clone();
    let position = Rc::new(RefCell::new(0));
    let cursor = position.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, target| {
        let k = *cursor.borrow();
        let call = &events[k];
        let back = &events[k + 1];
        assert_eq!(target, n(&call["target"]), "call target");
        // memcpy is cdecl: ECX is not an input. All other seams are thiscall.
        if target != 0x12efea0 {
            assert_eq!(c.ecx, regs(call)[1], "call {target:x} ECX");
        }
        assert_eq!(c.esp, regs(call)[4], "call {target:x} ESP");
        let count = call["args"].as_array().unwrap().len();
        let recorded = pairs(&call["memory"]);
        for (&a, &b) in &recorded {
            if !(0x0f000000..0x0f010000).contains(&a) || (a >= c.esp && a < c.esp + 4 + 4 * count as u32) {
                assert_eq!(m.u8(a), b, "call {target:x} memory {a:x}");
            }
        }
        for (a, b) in pairs(&back["effects"]) {
            m.set_u8(a, b);
        }
        let r = regs(back);
        c.eax = r[0];
        c.ecx = r[1];
        c.edx = r[2];
        c.esp = r[4];
        *cursor.borrow_mut() += 2;
    })));
    position
}
fn check(c: &Cpu, m: &dyn Mem, case: &Value) {
    let after = regs(&case["after"]);
    assert_eq!(
        [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi],
        [after[0], after[3], after[4], after[5], after[6], after[7]],
        "seed {} registers",
        case["seed"]
    );
    assert_eq!(c.st.control(), n(&case["before"]["x87"][0]) as u16);
    assert!(c.st.is_empty());
    assert_eq!(x86::stmxcsr() & !0x3f, n(&case["before"]["mxcsr"]) & !0x3f);
    for (a, b) in pairs(&case["after"]["memory"]) {
        if visible(a, after[4]) {
            assert_eq!(m.u8(a), b, "seed {} memory {a:x}", case["seed"]);
        }
    }
}
fn verify(entry: u32, body: LiftFn) {
    ddnative_host::install();
    let Some(data) = vectors(entry) else { return };
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 96);
    for case in cases {
        let mut m = Memory {
            bytes: pairs(&case["initial"]),
            ..Default::default()
        };
        let mut c = cpu(&case["before"]);
        let cursor = install_calls(case);
        x86::ldmxcsr(c.mxcsr.0);
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), case["events"].as_array().unwrap().len());
        check(&c, &m, case);
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &m.writes {
            if visible(a, c.esp) {
                assert_eq!(
                    expected.get(&a),
                    Some(&b),
                    "seed {} extra write {a:x}",
                    case["seed"]
                );
            }
        }
    }
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    eprintln!("{entry:08x}: {} original ABI cases", cases.len());
}
// verifies 0x00ff1920 (ABI level)
#[test]
fn shadow_draws_original() {
    verify(0xff1920, mtcore::semantic_draw_a::shadow_draws);
}
// verifies 0x00ff14d0 (ABI level)
#[test]
fn replay_draw_passes_original() {
    verify(0xff14d0, mtcore::semantic_draw_a::replay_draw_passes);
}
// verifies 0x00fefb90 (ABI level)
#[test]
fn bind_render_target_original() {
    verify(0xfefb90, mtcore::semantic_draw_a::bind_render_target);
}
