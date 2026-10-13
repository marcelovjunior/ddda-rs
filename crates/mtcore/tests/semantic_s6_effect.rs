//! L187 s6: the effect update routines (crates/mtcore/src/semantic_s6_effect.rs)
//! against the original's code run in Unicorn (research/native/record_effect.py):
//! the callee-saved registers, ESP, every memory byte but the popped stack
//! scratch, and each guest call (target, ESP, ECX, return address and
//! arguments). draw_children does its small callees in place (the recorder
//! runs their real code, `_ftol` and the x87 hand-off included).
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
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
const EAX: usize = 0;
const ECX: usize = 1;
const EBX: usize = 3;
const ESP: usize = 4;
const EBP: usize = 5;
const ESI: usize = 6;
const EDI: usize = 7;
const SCRATCH: u32 = 0x7ff0_0000;

/// The guest calls of the case being run: the recorded events, replayed in
/// order by the virtual-call handler and by the lifted-function lookup.
struct Replay {
    events: Rc<Vec<Value>>,
    position: usize,
    case: usize,
}
thread_local! {
    static REPLAY: RefCell<Option<Replay>> = const { RefCell::new(None) };
}

fn replay(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        let r = r.as_mut().expect("no case running");
        let (i, k) = (r.case, r.position);
        assert!(k < r.events.len(), "case {i}: unexpected call {k} to {target:x}");
        let event = &r.events[k];
        let (call, back) = (&event["before"], &event["after"]);
        assert_eq!(target, n(&event["target"]), "case {i}: call {k} target");
        let want = regs(call);
        assert_eq!(c.esp, want[ESP], "case {i}: call {k} ({target:x}) ESP");
        assert_eq!(c.ecx, want[ECX], "case {i}: call {k} ({target:x}) ECX");
        for (a, b) in pairs(&call["memory"]) {
            assert_eq!(m.u8(a), b, "case {i}: call {k} ({target:x}) memory {a:x}");
        }
        for (a, b) in pairs(&back["memory"]) {
            m.set_u8(a, b);
        }
        c.eax = regs(back)[EAX];
        c.esp = regs(back)[ESP];
        r.position += 1;
    });
}

macro_rules! lifted_callees {
    ($($name:ident = $address:literal),* $(,)?) => {
        $(fn $name(m: &mut dyn Mem, c: &mut Cpu) { replay(m, c, $address); })*
        /// The lifted functions the routines call directly (replayed).
        fn lookup(target: u32) -> Option<x86::LiftFn> {
            match target { $($address => Some($name),)* _ => None }
        }
    };
}
lifted_callees!(r_fdf6a0 = 0x00fd_f6a0, r_e75b20 = 0x00e7_5b20, r_1077240 = 0x0107_7240, r_10784b0 = 0x0107_84b0);

fn run(path: &str, body: fn(&mut dyn Mem, &mut Cpu)) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let (before, after) = (regs(&case["before"]), regs(&case["after"]));
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        // guest x87 control word: round to nearest, extended precision
        let mut control = [0x7f, 0x03, 0, 0, 0, 0, 0, 0, 0, 0];
        c.st.exec::<0x28d9>(&mut control);
        let events: Rc<Vec<Value>> = Rc::new(case["events"].as_array().unwrap().clone());
        REPLAY.with(|r| *r.borrow_mut() = Some(Replay { events: events.clone(), position: 0, case: i }));
        x86::set_ext_calls(Some(Box::new(move |m, c, target| replay(m, c, target))));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        let made = REPLAY.with(|r| r.borrow().as_ref().unwrap().position);
        assert_eq!(made, events.len(), "case {i}: calls made");
        assert_eq!([c.ebx, c.esp, c.ebp, c.esi, c.edi], [after[EBX], after[ESP], after[EBP], after[ESI], after[EDI]], "case {i}: ebx/esp/ebp/esi/edi");
        let esp = after[ESP];
        let expected = pairs(&case["after"]["memory"]);
        let observable = |a: u32| !(0x0f00_0000..esp).contains(&a) && !(SCRATCH..SCRATCH + 4).contains(&a);
        for (&a, &b) in &expected {
            if observable(a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if observable(a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

fn path(name: &str) -> String {
    std::env::var(format!("L187_{}", name.to_uppercase())).unwrap_or_else(|_| format!("E:/ddda-re/notes/native-L184/{name}.json"))
}

// verifies 0x00e0bed0 (ABI level; void)
#[test]
fn effect_update_matches_original() {
    let cases = run(&path("effect_update"), mtcore::semantic_s6_effect::effect_update);
    eprintln!("{cases} effect_update cases passed");
}

// verifies 0x00e75ea0 (ABI level; void)
#[test]
fn effect_draw_children_matches_original() {
    let cases = run(&path("effect_draw"), mtcore::semantic_s6_effect::effect_draw_children);
    eprintln!("{cases} effect_draw_children cases passed");
}
