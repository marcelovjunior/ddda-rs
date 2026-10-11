//! Differential tests of the readable math ports against the original
//! (research/native/vectors_math.py records the vectors; private files that
//! are skipped when absent, required with DDNATIVE_REQUIRE_VECTORS=1).
use ddnative_base::mem::{Mem, Sparse};
use ddnative_base::x86::{self, Cpu, Entry};
use mtmath::abi::PORTS;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

fn vectors(name: &str) -> Option<Vec<Value>> {
    let dir = std::env::var("LIFT_DATA").unwrap_or_else(|_| "E:/ddda-re/notes/native".into());
    let path = format!("{dir}/vectors/{name}.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return None,
    };
    serde_json::from_str::<Value>(&text).ok()?.as_array().cloned()
}

fn u(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}

fn hex_bytes(s: &str) -> impl Iterator<Item = u8> + '_ {
    (0..s.len() / 2).map(move |k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
}

fn port(addr: u32) -> (ddnative_base::x86::LiftFn, u8) {
    let p = PORTS.iter().find(|p| p.0 == addr).unwrap_or_else(|| panic!("no port for {addr:#x}"));
    (p.1, p.2)
}

/// Replays the calls to unported code a lifted vector recorded, as the
/// generated lifted test does: checks the target and every register at
/// the callee's entry, then applies what the mock returned.
fn replay_lift_calls(c: &Value) -> Rc<RefCell<VecDeque<Value>>> {
    let calls = Rc::new(RefCell::new(c.get("calls").and_then(|v| v.as_array()).cloned()
        .unwrap_or_default().into_iter().collect::<VecDeque<Value>>()));
    let q = calls.clone();
    x86::set_ext_calls(Some(Box::new(move |_m, cpu: &mut Cpu, target| {
        let ev = q.borrow_mut().pop_front().unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
        assert_eq!(target, u(&ev["target"]), "call target");
        assert_eq!(cpu_regs(cpu), regs(&ev["regs"]), "registers at the call");
        let o = &ev["out"];
        cpu.eax = u(&o["eax"]);
        cpu.ecx = u(&o["ecx"]);
        cpu.edx = u(&o["edx"]);
        for k in 0..4 {
            cpu.xmm[0][k] = u(&o["xmm0"][k]);
        }
        cpu.esp = cpu.esp.wrapping_add(4 + u(&o["pop"]));
        assert!(o.get("st_push").is_none(), "no x87 result expected");
    })));
    calls
}

/// Replays the callouts an edge case recorded (research/native/
/// vectors_math.py CALLOUTS): at the callee's entry (return address
/// pushed) every general register and all xmm lanes must be as the
/// original had them at its call; the callee's return state (registers,
/// xmm, stack window) is then restored.
fn replay_edge_calls(c: &Value) -> Rc<RefCell<VecDeque<Value>>> {
    let calls = Rc::new(RefCell::new(c["calls"].as_array().cloned().unwrap_or_default()
        .into_iter().collect::<VecDeque<Value>>()));
    let q = calls.clone();
    let base = u(&c["mem_in"][1][0]); // the stack window
    x86::set_ext_calls(Some(Box::new(move |m, cpu: &mut Cpu, target| {
        let ev = q.borrow_mut().pop_front().unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
        assert_eq!(target, u(&ev["target"]), "call target");
        let mut at_call = cpu_regs(cpu);
        at_call[4] = at_call[4].wrapping_add(4); // recorded before the push
        assert_eq!(at_call, regs(&ev["in"]["regs"]), "registers at the call");
        let xmm: [[u32; 4]; 8] = std::array::from_fn(|k| std::array::from_fn(|l| u(&ev["in"]["xmm"][k][l])));
        assert_eq!(cpu.xmm, xmm, "xmm at the call");
        let o = &ev["out"];
        let r = regs(&o["regs"]);
        [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi] = r;
        cpu.xmm = std::array::from_fn(|k| std::array::from_fn(|l| u(&o["xmm"][k][l])));
        for (k, b) in hex_bytes(o["stack"].as_str().unwrap()).enumerate() {
            m.set_u8(base + k as u32, b);
        }
    })));
    calls
}

const REGS: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];

fn regs(c: &Value) -> [u32; 8] {
    REGS.map(|k| u(&c[k]))
}

fn cpu_regs(cpu: &Cpu) -> [u32; 8] {
    [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi]
}

/// The lifted program's vectors for each ported address, under the lifted
/// test's contract: the eight general registers, xmm0's low lane, and
/// exactly the bytes the original wrote (reading any byte the original did
/// not read panics).
// verifies 0x007AD7A0 0x007AD870 0x007AD8E0 0x007AD9D0 0x00CFC1D0 0x00CFC350 0x00CFC4B0 0x00CFC620
// verifies 0x00CFC770 0x00CFC810 0x00CFC8B0 0x00CFC940
// verifies 0x00CFC9E0 0x00CFCB00 0x00CFCC20 0x00CFCD40 0x00CFCE60 0x00CFCF80
// verifies 0x00CFD5E0 0x00CFD890 0x00CFDB10 0x00D00690 0x00D006C0
#[test]
fn ports_replay_the_lifted_vectors() {
    let Some(cases) = vectors("math_lift") else { return };
    let mut seen = std::collections::BTreeSet::new();
    for c in &cases {
        let addr = u(&c["addr"]);
        let mut m = Sparse::default();
        for run in c["reads"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                m.bytes.insert(u(&run[0]) + k as u32, b);
            }
        }
        let ri = &c["regs_in"];
        let mut cpu = Cpu::from(Entry {
            eax: u(&ri["eax"]), ecx: u(&ri["ecx"]), edx: u(&ri["edx"]), ebx: u(&ri["ebx"]),
            esp: u(&ri["esp"]), ebp: u(&ri["ebp"]), esi: u(&ri["esi"]), edi: u(&ri["edi"]), xmm0: 0,
        });
        let calls = replay_lift_calls(c);
        port(addr).0(&mut m, &mut cpu);
        assert!(calls.borrow().is_empty(), "{addr:#x}: recorded calls not made");
        assert_eq!(cpu_regs(&cpu), regs(&c["regs_out"]), "{addr:#x}: registers");
        assert_eq!(cpu.xmm[0][0], u(&c["xmm0_out"]), "{addr:#x}: xmm0");
        let mut want = BTreeMap::new();
        for run in c["writes"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                want.insert(u(&run[0]) + k as u32, b);
            }
        }
        assert_eq!(m.written, want, "{addr:#x}: written bytes");
        assert!(cpu.st.is_empty(), "{addr:#x}: x87 stack");
        seen.insert(addr);
    }
    assert_eq!(seen.len(), PORTS.len(), "every port has lifted vectors");
}

/// Memory made of the recorded windows; any other address panics.
struct Windows(BTreeMap<u32, u8>);

impl Mem for Windows {
    fn u8(&self, addr: u32) -> u8 {
        *self.0.get(&addr).unwrap_or_else(|| panic!("read outside the recorded windows: {addr:#x}"))
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        *self.0.get_mut(&addr).unwrap_or_else(|| panic!("write outside the recorded windows: {addr:#x}")) = v;
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

/// Edge cases recorded from the original (NaNs, infinities, signed zeros,
/// denormals, random states): all general registers, every lane of each
/// xmm register the port reproduces (the others must keep their entry
/// values), and every byte of the object and stack windows.
// verifies 0x007AD7A0 0x007AD870 0x007AD8E0 0x007AD9D0 0x00CFC1D0 0x00CFC350 0x00CFC4B0 0x00CFC620
// verifies 0x00CFC770 0x00CFC810 0x00CFC8B0 0x00CFC940
// verifies 0x00CFC9E0 0x00CFCB00 0x00CFCC20 0x00CFCD40 0x00CFCE60 0x00CFCF80
// verifies 0x00CFD5E0 0x00CFD890 0x00CFDB10 0x00D00690 0x00D006C0
#[test]
fn ports_match_the_original_on_edge_cases() {
    let Some(cases) = vectors("math_edge") else { return };
    let mut seen = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for (n, c) in cases.iter().enumerate() {
        let addr = u(&c["addr"]);
        let mut m = Windows(windows(&c["mem_in"]));
        let ri = regs(&c["regs_in"]);
        let mut cpu = Cpu::from(Entry {
            eax: ri[0], ecx: ri[1], edx: ri[2], ebx: ri[3], esp: ri[4], ebp: ri[5], esi: ri[6], edi: ri[7], xmm0: 0,
        });
        for k in 0..8 {
            for lane in 0..4 {
                cpu.xmm[k][lane] = u(&c["xmm_in"][k][lane]);
            }
        }
        let entry = cpu.xmm;
        let (f, exact) = port(addr);
        let calls = replay_edge_calls(c);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut m, &mut cpu)));
        if let Err(e) = made {
            let msg = e.downcast_ref::<String>().cloned().unwrap_or_default();
            failures.push(format!("case {n} at {addr:#x}: {msg}"));
            continue;
        }
        let want: [[u32; 4]; 8] = std::array::from_fn(|k| {
            if exact >> k & 1 != 0 { std::array::from_fn(|lane| u(&c["xmm_out"][k][lane])) } else { entry[k] }
        });
        let ok = cpu_regs(&cpu) == regs(&c["regs_out"]) && cpu.xmm == want
            && m.0 == windows(&c["mem_out"]) && cpu.st.is_empty() && calls.borrow().is_empty();
        if !ok {
            failures.push(format!("case {n} at {addr:#x}: regs {:x?} want {:x?}; xmm {:x?} want {want:x?}",
                                  cpu_regs(&cpu), regs(&c["regs_out"]), cpu.xmm));
        }
        seen.insert(addr);
    }
    assert!(failures.is_empty(), "{} of {} cases differ:\n{}", failures.len(), cases.len(),
            failures[..failures.len().min(20)].join("\n"));
    assert_eq!(seen.len(), PORTS.len(), "every port has edge vectors");
}
