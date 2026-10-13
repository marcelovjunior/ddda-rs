//! L169 original instructions with explicit synthetic callee boundaries.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
    accesses: RefCell<Vec<(bool, u32)>>,
    fail_at: Option<usize>,
}
impl Memory {
    fn access(&self, write: bool, a: u32) {
        let mut accesses = self.accesses.borrow_mut();
        accesses.push((write, a));
        assert_ne!(Some(accesses.len()), self.fail_at, "injected fault");
    }
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        self.access(false, a);
        *self
            .bytes
            .get(&a)
            .unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.access(true, a);
        self.bytes.insert(a, v);
        self.writes.insert(a, v);
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
fn regs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}
fn x87_image(v: &Value) -> [u8; 108] {
    let mut image = ddnative_base::x87::INIT;
    for (i, offset) in [0, 4, 8].into_iter().enumerate() {
        image[offset..offset + 2].copy_from_slice(&(n(&v[i]) as u16).to_le_bytes());
    }
    for i in 0..8 {
        let offset = 28 + 10 * i;
        // Unicorn FPn is physical; FSAVE's payload is in logical ST order.
        let physical = (i + ((n(&v[1]) >> 11) & 7) as usize) % 8;
        image[offset..offset + 8].copy_from_slice(&v[3 + physical][0].as_u64().unwrap().to_le_bytes());
        image[offset + 8..offset + 10].copy_from_slice(&(n(&v[3 + physical][1]) as u16).to_le_bytes());
    }
    image
}
fn cpu(v: &Value) -> Cpu {
    let r: Vec<_> = v["regs"].as_array().unwrap().iter().map(n).collect();
    let mut c = Cpu::default();
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = r.try_into().unwrap();
    c.fl = x86::Flags::new(x86::RAW, 0, 0, n(&v["flags"]), 32);
    c.df = n(&v["flags"]) & 0x400 != 0;
    c.mxcsr = x86::Mxcsr(n(&v["mxcsr"]));
    c.st.state = x87_image(&v["x87"]);
    if v["x87_executed"] == true {
        for offset in [2, 6, 10] {
            c.st.state[offset..offset + 2].fill(0xff);
        }
    }
    for i in 0..8 {
        for j in 0..4 {
            c.xmm[i][j] = n(&v["xmm"][i][j]);
        }
    }
    c
}
fn check(m: &dyn Mem, c: &Cpu, v: &Value, where_: &str) {
    assert_eq!(
        regs(c).to_vec(),
        v["regs"].as_array().unwrap().iter().map(n).collect::<Vec<_>>(),
        "{where_} regs"
    );
    let flags = c.fl.snapshot().0 | if c.df { 0x400 } else { 0 };
    let mask = v.get("flag_mask").map(n).unwrap_or(0xcd5);
    assert_eq!(flags & mask, n(&v["flags"]) & mask, "{where_} flags");
    for i in 0..8 {
        for j in 0..4 {
            assert_eq!(c.xmm[i][j], n(&v["xmm"][i][j]), "{where_} xmm{i}.{j}");
        }
    }
    assert_eq!(c.mxcsr.0, n(&v["mxcsr"]), "{where_} mxcsr");
    let mut expected_x87 = x87_image(v.get("hardware_x87").unwrap_or(&v["x87"]));
    if v["x87_executed"] == true {
        // Native FNSAVE reserved words are FFFF (x87.rs's hardware
        // contract); Unicorn exposes only CW/SW/tags/register payloads.
        for offset in [2, 6, 10] {
            expected_x87[offset..offset + 2].fill(0xff);
        }
    }
    assert_eq!(c.st.clone().state, expected_x87, "{where_} x87");
    for (a, b) in pairs(&v["memory"]) {
        assert_eq!(m.u8(a), b, "{where_} memory {a:x}");
    }
}

fn vectors() -> Option<Value> {
    let path = std::env::var("DDNATIVE_SHADOW_TRAVERSAL_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L169/shadow_pass.json".into());
    match std::fs::read_to_string(&path) {
        Ok(s) => Some(serde_json::from_str(&s).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}
fn install() {
    ddnative_host::install();
    fn saved_state(m: &mut dyn Mem, c: &mut Cpu) {
        x86::call_ext(m, c, 0xfed8f0);
    }
    fn lookup(a: u32) -> Option<LiftFn> {
        if a == 0xfed8f0 {
            Some(saved_state)
        } else {
            None
        }
    }
    x86::set_lifted_lookup(lookup);
}
fn release(c: &mut Cpu) {
    if c.st.home != 0 {
        (ddnative_base::hooks::get().x87_release)(&mut c.st);
    }
}
fn verify(body: LiftFn) {
    install();
    let Some(data) = vectors() else { return };
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 96, "complete L169 corpus required");
    for case in cases {
        let mut m = Memory {
            bytes: pairs(&case["initial"]),
            ..Default::default()
        };
        let mut c = cpu(&case["before"]);
        let events = Rc::new(case["events"].as_array().unwrap().clone());
        let position = Rc::new(RefCell::new(0usize));
        let (trace, cursor) = (events.clone(), position.clone());
        let seed = case["seed"].clone();
        let old = x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let i = *cursor.borrow();
            assert_eq!(target, n(&trace[i]["target"]), "seed {seed} call {i}");
            check(
                m,
                c,
                &trace[i],
                &format!("seed {seed} call {i} target {target:x}"),
            );
            for (a, b) in pairs(&trace[i + 1]["effects"]) {
                m.set_u8(a, b);
            }
            release(c);
            *c = cpu(&trace[i + 1]);
            *cursor.borrow_mut() += 2;
        })));
        assert!(old.is_none());
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*position.borrow(), events.len());
        check(&m, &c, &case["after"], &format!("seed {} return", case["seed"]));
        assert_eq!(m.writes, pairs(&case["after"]["memory"]));
        release(&mut c);
    }
    eprintln!("shadow pass: {} original returns match", cases.len());
}
// verifies 0x00EFD240
#[test]
fn original_shadow_pass() {
    verify(mtcore::shadow_pass::setup);
}
