//! Isolated synthetic microbenchmark; enabled by research/native/check_apply.py.
//! No runtime/GPU work. Restore input state outside each measured interval.
#![cfg(any())] // Enabled only by the isolated runner, which supplies references.
// L125_COST_REFERENCE
use ddnative_base::{
    mem::{Mem, PageTable},
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::Cell, io::BufRead, ptr::NonNull, time::Instant};

struct Pages(Box<PageTable>);
impl Pages {
    fn pointer(&self, a: u32) -> *mut u8 {
        let p = self.0.page((a >> 12) as usize);
        assert!(!p.is_null(), "unmapped benchmark address {a:x}");
        unsafe { p.add((a & 4095) as usize) }
    }
}
impl Mem for Pages {
    fn u8(&self, a: u32) -> u8 {
        unsafe { self.pointer(a).read() }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe {
            self.pointer(a).write(v);
        }
    }
    fn u32(&self, a: u32) -> u32 {
        if a & 4095 <= 4092 {
            unsafe { self.pointer(a).cast::<u32>().read_unaligned() }
        } else {
            u32::from_le_bytes(std::array::from_fn(|i| self.u8(a + i as u32)))
        }
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        if a & 4095 <= 4092 {
            unsafe {
                self.pointer(a).cast::<u32>().write_unaligned(v);
            }
        } else {
            for (i, b) in v.to_le_bytes().into_iter().enumerate() {
                self.set_u8(a + i as u32, b);
            }
        }
    }
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(&*self.0))
    }
}
thread_local! {static INDEX:Cell<u32>=const {Cell::new(0)};}
fn mock(_: &mut dyn Mem, c: &mut Cpu, t: u32) {
    let n = match t {
        0xda7510 | 0xda77d0 | 0xda7b30 => 3,
        0xda7250 => 4,
        0xda68a0 => 0,
        t if t >= 0x0d000000 => match (t - 0x0d000000) / 16 {
            57 | 37 | 65 | 86 => 3,
            69 | 94 | 96 | 98 | 109 | 111 | 113 => 4,
            100 => 5,
            39 | 47 | 75 | 104 | 87 | 92 | 107 => 2,
            _ => panic!("COM {t:x}"),
        },
        _ => panic!("target {t:x}"),
    };
    let i = INDEX.with(|v| {
        let i = v.get() + 1;
        v.set(i);
        i
    });
    c.eax = if t >= 0x0d000000 { 0 } else { 0x51500000 | i };
    c.ecx = 0xc1c10000 ^ i;
    c.edx = 0xd2d20000 ^ i;
    c.esp += 4 + n * 4;
}
fn color(m: &mut dyn Mem, c: &mut Cpu) {
    mock(m, c, 0xda68a0);
}
fn number(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}

#[test]
#[ignore = "run the isolated --bench harness in release mode"]
fn apply_cost() {
    ddnative_host::install();
    x86::set_lifted_lookup(|t| if t == 0xda68a0 { Some(color) } else { None });
    x86::set_ext_calls(Some(Box::new(mock)));
    let mut cases: std::collections::BTreeMap<(String, u8), Value> = std::collections::BTreeMap::new();
    let file = std::fs::File::open("E:/ddda-re/notes/native-L125/apply.jsonl").unwrap();
    for line in std::io::BufReader::new(file).lines() {
        let case: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if let Some(kind) = case["kind"].as_str() {
            let count = case["calls"].as_array().unwrap().len();
            let calls = u8::from(count != 0);
            cases
                .entry((kind.to_owned(), calls))
                .or_insert_with(|| case.clone());
            let key = (kind.to_owned(), 2);
            if cases
                .get(&key)
                .is_none_or(|c| c["calls"].as_array().unwrap().len() < count)
            {
                cases.insert(key, case);
            }
        }
    }
    let mut m = Pages(Box::default());
    for kind in ["targets", "fixed", "streams", "shader"] {
        // First identity skip, first transition, then maximal recorded calls.
        // These are bounded synthetic costs, not game profiles.
        for calls in [0, 1, 2] {
            let case = &cases[&(kind.to_owned(), calls)];
            let initial: Vec<_> = case["initial"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (number(&p[0]), number(&p[1]) as u8))
                .collect();
            for &(a, v) in &initial {
                m.0.backed_page((a >> 12) as usize);
                m.0.set_perm((a >> 12) as usize, 3);
                m.set_u8(a, v);
            }
            let r: Vec<_> = case["regs"].as_array().unwrap().iter().map(number).collect();
            let base = Cpu {
                eax: r[0],
                ecx: r[1],
                edx: r[2],
                ebx: r[3],
                esp: r[4],
                ebp: r[5],
                esi: r[6],
                edi: r[7],
                xmm: std::array::from_fn(|i| std::array::from_fn(|j| number(&case["xmm"][i][j]))),
                ..Default::default()
            };
            // L125_COST_BODIES
            for round in 0..3 {
                let mut ns = [0u128; 2];
                for index in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    for _ in 0..5000 {
                        for &(a, v) in &initial {
                            m.set_u8(a, v);
                        }
                        INDEX.with(|v| v.set(0));
                        let mut c = base.clone();
                        let start = Instant::now();
                        bodies[index](&mut m, &mut c);
                        std::hint::black_box(&c);
                        ns[index] += start.elapsed().as_nanos();
                    }
                }
                eprintln!(
                    "{kind} {} calls={} round{round}: lifted {:.3} us, readable {:.3} us",
                    ["identity", "transition", "most-calls"][calls as usize],
                    case["calls"].as_array().unwrap().len(),
                    ns[0] as f64 / 5_000_000.0,
                    ns[1] as f64 / 5_000_000.0
                );
            }
        }
    }
}
