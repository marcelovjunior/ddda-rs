//! L191: paired benchmark of the readable ports against the original's lifted
//! bodies on the recorded vectors (page-table memory, like production).
//! `cargo test --release -p mtcore --test l191_bench -- --ignored --nocapture`
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;

#[path = "l191_bench/lifted.rs"]
mod lifted;

#[derive(Default)]
struct Paged {
    table: ddnative_base::mem::PageTable,
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a >> 12) as usize);
        assert!(!p.is_null(), "unmapped read {a:x}");
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) }
    }
}

fn n(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn pairs(v: &Value) -> Vec<(u32, u8)> {
    v.as_array().unwrap().iter().map(|p| (n(&p[0]), n(&p[1]) as u8)).collect()
}
fn regs(v: &Value) -> Vec<u32> {
    v["regs"].as_array().unwrap().iter().map(n).collect()
}

/// Time each body (min of 20 runs per case, mean over cases) on every case of
/// the vector file; recorded guest calls are replayed (their effects applied).
fn bench(path: &str, name: &str, bodies: &[(&str, LiftFn)]) {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let mut m = Paged::default();
    let mut totals = vec![0f64; bodies.len()];
    let mut count = 0u64;
    for round in 0..3 {
        for case in cases.iter().step_by(3) {
            let initial = pairs(&case["initial"]);
            let before = regs(&case["before"]);
            let events: Vec<(Vec<(u32, u8)>, u32, u32)> = case["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (pairs(&e["after"]["memory"]), regs(&e["after"])[0], regs(&e["after"])[4]))
                .collect();
            for (k, (_, body)) in bodies.iter().enumerate() {
                let mut best = std::time::Duration::MAX;
                for _ in 0..20 {
                    let at = std::cell::Cell::new(0usize);
                    let ev = events.clone();
                    x86::set_ext_calls(Some(Box::new(move |m, c, _| {
                        let (effects, eax, esp) = &ev[at.get()];
                        at.set(at.get() + 1);
                        for &(a, b) in effects {
                            m.set_u8(a, b);
                        }
                        c.eax = *eax;
                        c.esp = *esp;
                    })));
                    for &(a, b) in &initial {
                        m.set_u8(a, b);
                    }
                    let mut c = Cpu::default();
                    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = [before[0], before[1], before[2], before[3], before[4], before[5], before[6], before[7]];
                    let t = std::time::Instant::now();
                    body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                    best = best.min(t.elapsed());
                    std::hint::black_box(c.eax);
                }
                if round > 0 {
                    totals[k] += best.as_nanos() as f64;
                }
            }
            if round > 0 {
                count += 1;
            }
        }
    }
    x86::set_ext_calls(None);
    for (k, (label, _)) in bodies.iter().enumerate() {
        eprintln!("{name}: {label} ns/call = {:.1}", totals[k] / count as f64);
    }
    eprintln!("{name}: speedup = {:.2}x", totals[0] / totals[1]);
}

const DIR: &str = "E:/ddda-re/notes/native-L191/";

#[test]
#[ignore]
fn bench_obj_model_vf11() {
    bench(&format!("{DIR}vf11.json"), "0x00886880", &[("lifted", lifted::f_00886880), ("port", mtcore::semantic_l191::obj_model_vf11)]);
}
