//! Differential tests of the readable core-library ports against the
//! original (research/native/vectors_core.py records the vectors; private
//! files that are skipped when absent, required with
//! DDNATIVE_REQUIRE_VECTORS=1).
use ddnative_base::mem::{Mem, Sparse, HEAP_BASE};
use ddnative_base::x86::{self, Cpu, Entry};
use mtcore::abi::PORTS;
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

fn port(addr: u32) -> (ddnative_base::x86::LiftFn, u8) {
    let p = PORTS
        .iter()
        .find(|p| p.0 == addr)
        .unwrap_or_else(|| panic!("no port for {addr:#x}"));
    (p.1, p.2)
}

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

/// The exe's CRC table equals the one computed from the polynomial.
#[test]
fn crc_table_matches_the_exe() {
    let Some(t) = read("core_tables") else { return };
    let bytes: Vec<u8> = hex_bytes(t["crc_table"].as_str().unwrap()).collect();
    for (i, w) in bytes.chunks(4).enumerate() {
        assert_eq!(
            u32::from_le_bytes(w.try_into().unwrap()),
            mtcore::hash::TABLE[i],
            "entry {i}"
        );
    }
}

/// Replays the calls to unported code a lifted vector recorded, as the
/// generated lifted test does.
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
    x86::set_ext_calls(Some(Box::new(move |_m, cpu: &mut Cpu, target| {
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
        assert!(o.get("st_push").is_none(), "no x87 result expected");
    })));
    calls
}

/// The lifted program's vectors for each ported address, under the lifted
/// test's contract.
// verifies 0x00402530 0x004025C0 0x00402620 0x004027A0 0x004038F0 0x00CF5450 0x00CF54A0 0x00CF55B0
// verifies 0x00CF55E0 0x00CF5750 0x00CF5870 0x00CF5C00 0x00CF5E20 0x00CF5E40 0x00CF7340 0x00CF74E0
// verifies 0x00CF76C0 0x00D01B60
#[test]
fn ports_replay_the_lifted_vectors() {
    let Some(cases) = vectors("core_lift") else { return };
    let mut seen = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for c in &cases {
        let addr = u(&c["addr"]);
        let mut m = Sparse::default();
        for run in c["reads"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                m.bytes.insert(u(&run[0]) + k as u32, b);
            }
        }
        let mut cpu = entry(regs(&c["regs_in"]));
        let calls = replay_lift_calls(c);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port(addr).0(&mut m, &mut cpu)));
        let mut want = BTreeMap::new();
        for run in c["writes"].as_array().unwrap() {
            for (k, b) in hex_bytes(run[1].as_str().unwrap()).enumerate() {
                want.insert(u(&run[0]) + k as u32, b);
            }
        }
        let ok = made.is_ok()
            && calls.borrow().is_empty()
            && cpu_regs(&cpu) == regs(&c["regs_out"])
            && cpu.xmm[0][0] == u(&c["xmm0_out"])
            && m.written == want
            && cpu.st.is_empty();
        if !ok {
            failures.push(format!(
                "{addr:#x}: {}",
                match made {
                    Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_default(),
                    Ok(()) => format!("regs {:x?} want {:x?}", cpu_regs(&cpu), regs(&c["regs_out"])),
                }
            ));
        }
        seen.insert(addr);
    }
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

/// Replays the stub calls an edge case recorded (calls to ported functions
/// run their ports, as the runtime's dispatcher does): at the stub's entry
/// (return address pushed) every general register and all xmm lanes must
/// be as the original had them; the stub's return state (registers, xmm,
/// stack window) is then restored.
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
    let base = u(&c["mem_in"][1][0]);
    x86::set_ext_calls(Some(Box::new(move |m, cpu: &mut Cpu, target| {
        // a ported callee ran as the original in the recorder: run its port
        if let Some(p) = PORTS.iter().find(|p| p.0 == target) {
            return (p.1)(m, cpu);
        }
        let ev = q
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| panic!("unrecorded call to {target:#x}"));
        assert_eq!(target, u(&ev["target"]), "call target");
        assert_eq!(cpu_regs(cpu), regs(&ev["in"]["regs"]), "registers at the call");
        let xmm: [[u32; 4]; 8] = std::array::from_fn(|k| std::array::from_fn(|l| u(&ev["in"]["xmm"][k][l])));
        assert_eq!(cpu.xmm, xmm, "xmm at the call");
        let o = &ev["out"];
        [
            cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi,
        ] = regs(&o["regs"]);
        cpu.xmm = std::array::from_fn(|k| std::array::from_fn(|l| u(&o["xmm"][k][l])));
        for (k, b) in hex_bytes(o["stack"].as_str().unwrap()).enumerate() {
            m.set_u8(base + k as u32, b);
        }
    })));
    calls
}

/// Edge cases recorded from the original: built object graphs, every
/// general register and xmm lane, and every byte of the scratch, stack,
/// heap and global windows.
// verifies 0x00402530 0x004025C0 0x00402620 0x004027A0 0x004038F0 0x00CF5450 0x00CF54A0 0x00CF55B0
// verifies 0x00CF55E0 0x00CF5750 0x00CF5870 0x00CF5C00 0x00CF5E20 0x00CF5E40 0x00CF7340 0x00CF74E0
// verifies 0x00CF76C0 0x00D01B60
#[test]
fn ports_match_the_original_on_edge_cases() {
    let Some(cases) = vectors("core_edge") else { return };
    let mut seen = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    for (n, c) in cases.iter().enumerate() {
        let addr = u(&c["addr"]);
        let mut m = Windows {
            bytes: windows(&c["mem_in"]),
            heap: 0,
        };
        let mut cpu = entry(regs(&c["regs_in"]));
        for k in 0..8 {
            for lane in 0..4 {
                cpu.xmm[k][lane] = u(&c["xmm_in"][k][lane]);
            }
        }
        let entry_xmm = cpu.xmm;
        let (f, exact) = port(addr);
        let calls = replay_edge_calls(c);
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut m, &mut cpu)));
        if let Err(e) = made {
            let msg = e.downcast_ref::<String>().cloned().unwrap_or_default();
            failures.push(format!("case {n} at {addr:#x}: {msg}"));
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
            && calls.borrow().is_empty();
        if !ok {
            let diff: Vec<String> = want
                .iter()
                .filter(|(a, v)| m.bytes.get(a) != Some(v))
                .take(6)
                .map(|(a, v)| format!("[{a:#x}] {:x?} want {v:#04x}", m.bytes.get(a)))
                .collect();
            failures.push(format!(
                "case {n} at {addr:#x}: regs {:x?} want {:x?}; mem {}",
                cpu_regs(&cpu),
                regs(&c["regs_out"]),
                diff.join(", ")
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
