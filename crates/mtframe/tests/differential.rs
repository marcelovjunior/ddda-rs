//! Differential tests of the readable frame-system ports against the
//! original (research/native/vectors_frame.py records the vectors; private
//! files that are skipped when absent, required with
//! DDNATIVE_REQUIRE_VECTORS=1).
use ddnative_base::mem::{Mem, Sparse, HEAP_BASE};
use ddnative_base::x86::{self, Cpu, Entry, LiftFn};
use mtframe::abi::PORTS;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

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

fn vectors(name: &str) -> Option<Vec<Value>> {
    read(name)?.as_array().cloned()
}

fn u(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}

fn hex_bytes(s: &str) -> impl Iterator<Item = u8> + '_ {
    (0..s.len() / 2).map(move |k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap())
}

fn port(addr: u32) -> (LiftFn, u8) {
    let p = PORTS
        .iter()
        .find(|p| p.0 == addr)
        .unwrap_or_else(|| panic!("no port for {addr:#x}"));
    (p.1, p.2)
}

/// Ports of other crates the frame ports call (through `call_ext`, as the
/// lifted bodies do): they ran as the original in the recorder.
fn foreign_port(addr: u32) -> Option<LiftFn> {
    mtcore::abi::PORTS
        .iter()
        .chain(mtmath::abi::PORTS)
        .find(|p| p.0 == addr)
        .map(|p| p.1)
}

/// The other lifted bodies the ports call through `x86::call_lifted`. In
/// these tests each one goes to the call handler, which replays the call
/// the recorder made to a fake of it.
macro_rules! forwarded {
    ($($a:literal),* $(,)?) => {
        const FORWARDED: &[u32] = &[$($a),*];
        fn lookup(target: u32) -> Option<LiftFn> {
            match target {
                $($a => Some(|m, cpu| x86::call_ext(m, cpu, $a)),)*
                _ => None,
            }
        }
    };
}
forwarded!(0x00A0_0BF0, 0x00FB_5E90, 0x0105_70F0);

const REGS: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];

fn regs(c: &Value) -> [u32; 8] {
    REGS.map(|k| u(&c[k]))
}

fn cpu_regs(cpu: &Cpu) -> [u32; 8] {
    [
        cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi,
    ]
}

fn entry(r: [u32; 8]) -> Cpu {
    Cpu::from(Entry {
        eax: r[0],
        ecx: r[1],
        edx: r[2],
        ebx: r[3],
        esp: r[4],
        ebp: r[5],
        esi: r[6],
        edi: r[7],
        xmm0: 0,
    })
}

fn message(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Replays the calls to unported code a lifted vector recorded, as the
/// generated lifted test does. A call into another lifted body ran that
/// body in the lifted test; it is not available here, so such a case is
/// left to the integration build's replay of the whole lifted program,
/// where the port is wired in.
fn replay_lift_calls(c: &Value) -> Rc<RefCell<VecDeque<Value>>> {
    let calls = Rc::new(RefCell::new(
        c.get("calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<VecDeque<Value>>(),
    ));
    let q = calls.clone();
    x86::set_ext_calls(Some(Box::new(move |m, cpu: &mut Cpu, target| {
        if let Some(f) = foreign_port(target) {
            return f(m, cpu);
        }
        if FORWARDED.contains(&target) {
            panic!("enters the lifted body {target:#x}");
        }
        let ev = q
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
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
        if let Some(v) = o.get("st_push").and_then(|v| v.as_u64()) {
            cpu.st.push(v as u32);
        }
    })));
    calls
}

/// The lifted program's vectors for each ported address, under the lifted
/// test's contract (registers, xmm0, written bytes, x87 result).
// verifies 0x00A36DF0 0x00E6C260 0x00FB64D0 0x011E48E0
#[test]
fn ports_replay_the_lifted_vectors() {
    let Some(cases) = vectors("frame_lift") else { return };
    x86::set_lifted_lookup(lookup);
    let mut seen = std::collections::BTreeSet::new();
    let (mut failures, mut deferred) = (Vec::new(), 0);
    for c in &cases {
        let addr = u(&c["addr"]);
        let mut m = Sparse::default();
        for run in c["reads"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                m.bytes.insert(u(&run[0]) + k as u32, b);
            }
        }
        let mut cpu = entry(regs(&c["regs_in"]));
        cpu.fs_base = 0x7FFD_E000;
        let calls = replay_lift_calls(c);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port(addr).0(&mut m, &mut cpu)));
        let mut want = BTreeMap::new();
        for run in c["writes"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                want.insert(u(&run[0]) + k as u32, b);
            }
        }
        match made {
            Err(e) => {
                let msg = message(e);
                if msg.starts_with("enters the lifted body") {
                    deferred += 1;
                } else {
                    failures.push(format!("{addr:#x}: {msg}"));
                }
            }
            Ok(()) => {
                let st0 = c["st0_out"].as_u64().map(|v| v as u32);
                let ok = calls.borrow().is_empty()
                    && cpu_regs(&cpu) == regs(&c["regs_out"])
                    && cpu.xmm[0][0] == u(&c["xmm0_out"])
                    && m.written == want
                    && (st0.is_none() || cpu.st.top_f32() == st0);
                if !ok {
                    failures.push(format!(
                        "{addr:#x}: regs {:x?} want {:x?}",
                        cpu_regs(&cpu),
                        regs(&c["regs_out"])
                    ));
                }
            }
        }
        seen.insert(addr);
    }
    eprintln!(
        "{} lifted vectors, {deferred} enter another lifted body (checked by the integration replay)",
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "{} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(seen.len(), PORTS.len(), "every port has lifted vectors");
}

/// Memory made of the recorded windows, plus the platform models' heap
/// (allocated from mem::HEAP_BASE as Sparse and the recorder do).
struct Windows {
    bytes: BTreeMap<u32, u8>,
    heap: u32,
}

impl Mem for Windows {
    fn u8(&self, addr: u32) -> u8 {
        *self
            .bytes
            .get(&addr)
            .unwrap_or_else(|| panic!("read outside the recorded windows: {addr:#x}"))
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        *self
            .bytes
            .get_mut(&addr)
            .unwrap_or_else(|| panic!("write outside the recorded windows: {addr:#x}")) = v;
    }
    fn alloc(&mut self, size: u32) -> u32 {
        if self.heap == 0 {
            self.heap = HEAP_BASE;
        }
        let a = self.heap;
        self.heap = self.heap.wrapping_add(size.max(1)).wrapping_add(15) & !15;
        a
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

fn xmm_of(v: &Value) -> [[u32; 4]; 8] {
    std::array::from_fn(|k| std::array::from_fn(|l| u(&v[k][l])))
}

/// Replays the stub and fake calls an edge case recorded (calls to ported
/// functions run their ports, as the runtime's dispatcher does): at the
/// callee's entry (return address pushed) every general register and all
/// xmm lanes must be as the original had them; the callee's return state
/// (registers, xmm, stack window, a float left in st0) is then restored.
fn replay_edge_calls(c: &Value) -> Rc<RefCell<VecDeque<Value>>> {
    let calls = Rc::new(RefCell::new(
        c["calls"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<VecDeque<Value>>(),
    ));
    let q = calls.clone();
    let stack = c["mem_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| u(&w[0]))
        .find(|&a| (0x0F00_0000..0x1000_0000).contains(&a))
        .expect("stack window");
    x86::set_ext_calls(Some(Box::new(move |m, cpu: &mut Cpu, target| {
        if let Some(p) = PORTS.iter().find(|p| p.0 == target) {
            return (p.1)(m, cpu);
        }
        if let Some(f) = foreign_port(target) {
            return f(m, cpu);
        }
        let ev = q
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
        assert_eq!(target, u(&ev["target"]), "call target");
        assert_eq!(cpu_regs(cpu), regs(&ev["in"]["regs"]), "registers at the call to {target:#x}");
        assert_eq!(cpu.xmm, xmm_of(&ev["in"]["xmm"]), "xmm at the call to {target:#x}");
        let o = &ev["out"];
        [
            cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi,
        ] = regs(&o["regs"]);
        cpu.xmm = xmm_of(&o["xmm"]);
        for (k, b) in hex_bytes(o["stack"].as_str().unwrap()).enumerate() {
            m.set_u8(stack + k as u32, b);
        }
        if let Some(v) = o.get("st_push").and_then(|v| v.as_u64()) {
            cpu.st.push(v as u32);
        }
    })));
    calls
}

/// Sets the x87 control word (`fldcw`).
fn set_fpcw(cpu: &mut Cpu, cw: u16) {
    let mut data = [0u8; 10];
    data[..2].copy_from_slice(&cw.to_le_bytes());
    cpu.st.exec::<0x28d9>(&mut data);
}

/// Edge cases recorded from the original: built objects, every general
/// register and xmm lane, and every byte of the object, stack, heap and
/// global windows.
// verifies 0x00A36DF0 0x00E6C260 0x00FB64D0 0x011E48E0
#[test]
fn ports_match_the_original_on_edge_cases() {
    let Some(cases) = vectors("frame_edge") else { return };
    x86::set_lifted_lookup(lookup);
    let mut seen = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for (n, c) in cases.iter().enumerate() {
        let addr = u(&c["addr"]);
        let mut m = Windows {
            bytes: windows(&c["mem_in"]),
            heap: 0,
        };
        let mut cpu = entry(regs(&c["regs_in"]));
        cpu.xmm = xmm_of(&c["xmm_in"]);
        set_fpcw(&mut cpu, u(&c["fpcw"]) as u16);
        let entry_xmm = cpu.xmm;
        let (f, exact) = port(addr);
        let calls = replay_edge_calls(c);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut m, &mut cpu)));
        if let Err(e) = made {
            failures.push(format!("case {n} at {addr:#x}: {}", message(e)));
            continue;
        }
        let want_xmm: [[u32; 4]; 8] = std::array::from_fn(|k| {
            if exact >> k & 1 != 0 {
                std::array::from_fn(|lane| u(&c["xmm_out"][k][lane]))
            } else {
                entry_xmm[k]
            }
        });
        let want = windows(&c["mem_out"]);
        let ok = cpu_regs(&cpu) == regs(&c["regs_out"])
            && cpu.xmm == want_xmm
            && m.bytes == want
            && cpu.st.is_empty()
            && cpu.st.control() as u32 == u(&c["fpcw_out"])
            && calls.borrow().is_empty();
        if !ok {
            let diff: Vec<String> = want
                .iter()
                .filter(|(a, v)| m.bytes.get(a) != Some(v))
                .take(6)
                .map(|(a, v)| format!("[{a:#x}] {:x?} want {v:#04x}", m.bytes.get(a)))
                .collect();
            let xmm: Vec<String> = (0..8)
                .filter(|&k| cpu.xmm[k] != want_xmm[k])
                .map(|k| format!("xmm{k} {:x?} want {:x?}", cpu.xmm[k], want_xmm[k]))
                .collect();
            failures.push(format!(
                "case {n} at {addr:#x}: regs {:x?} want {:x?}; mem {}; {}; calls left {}",
                cpu_regs(&cpu),
                regs(&c["regs_out"]),
                diff.join(", "),
                xmm.join(", "),
                calls.borrow().len()
            ));
        }
        seen.insert(addr);
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(20)].join("\n")
    );
    assert_eq!(seen.len(), PORTS.len(), "every port has edge vectors");
}
