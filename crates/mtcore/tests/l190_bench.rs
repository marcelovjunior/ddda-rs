//! L190: paired benchmark of the readable ports against the original's lifted
//! bodies on the recorded vectors (page-table memory, like production).
//! `cargo test --release -p mtcore --test l190_bench -- --ignored --nocapture`
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;

#[path = "l190_bench/lifted.rs"]
mod lifted;
#[path = "l190_bench/lifted_stack.rs"]
mod lifted_stack;

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
    // the stack window: guest 0x0F000000..0x0F010000 on one contiguous host block
    let layout = std::alloc::Layout::from_size_align(0x10000, 4096).unwrap();
    let block = unsafe { std::alloc::alloc_zeroed(layout) };
    assert!(unsafe { m.table.install_external(0x0F000, block, 16, 3) });
    let mut totals = vec![0f64; bodies.len()];
    let mut count = 0u64;
    for round in 0..3 {
        for case in cases.iter().step_by(3) {
            let initial = pairs(&case["initial"]);
            let before = regs(&case["before"]);
            // the CRT memcpy / memset are native in both bodies: not replayed
            let events: Vec<(Vec<(u32, u8)>, u32, u32)> = case["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| ![0x012e_fea0, 0x012e_fe20].contains(&n(&e["target"])))
                .map(|e| (pairs(&e["after"]["memory"]), regs(&e["after"])[0], regs(&e["after"])[4]))
                .collect();
            for (k, (_, body)) in bodies.iter().enumerate() {
                let mut best = std::time::Duration::MAX;
                for _ in 0..20 {
                    let at = std::cell::Cell::new(0usize);
                    let ev = events.clone();
                    x86::set_ext_calls(Some(Box::new(move |m, c, target| {
                        // helpers that are ports in production: run them (their own
                        // guest calls come from the recorded stream)
                        match target {
                            0x00ea_1ee0 => return mtcore::semantic_s3::classify_overlap(m, c),
                            0x00ea_1f40 => return mtcore::semantic_s3::world_position(m, c),
                            0x00ea_1fd0 => return mtcore::semantic_p2c::select_shadow_state_abi(m, c),
                            _ => {}
                        }
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
                    (c.stack_lo, c.stack_len, c.stack_host) = (0x0F00_0000, 0x10000, block as usize);
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
    for k in 1..bodies.len() { eprintln!("{name}: {} speedup vs {} = {:.2}x", bodies[k].0, bodies[0].0, totals[0] / totals[k]); }
}

const DIR: &str = "E:/ddda-re/notes/native-L190/";

#[test]
#[ignore]
fn bench_swing_model_draw_unit() {
    bench(&format!("{DIR}swing.json"), "0x00FA5860", &[("lifted", lifted::f_00fa5860), ("port", mtcore::semantic_l190::swing_model_draw_unit)]);
}

#[test]
#[ignore]
fn bench_skin_remap() {
    bench(&format!("{DIR}skin_remap.json"), "0x00B8B800", &[("lifted", lifted::f_00b8b800), ("port", mtcore::semantic_l190::rebuild_skin_remap)]);
}

#[test]
#[ignore]
fn bench_player_draw_parts() {
    bench(&format!("{DIR}player_parts.json"), "0x00B89790", &[("lifted", lifted::f_00b89790), ("port", mtcore::semantic_l190::player_draw_parts)]);
}

#[test]
#[ignore]
fn bench_prepare_part_blocks() {
    bench(&format!("{DIR}skin_prepare.json"), "0x00B8A770", &[("lifted", lifted::f_00b8a770), ("port", mtcore::semantic_l190::prepare_part_blocks)]);
}

#[test]
#[ignore]
fn bench_player_draw_part() {
    bench(&format!("{DIR}player_part.json"), "0x00B89FE0", &[("lifted", lifted::f_00b89fe0), ("port", mtcore::semantic_l190::player_draw_part)]);
}

#[test]
#[ignore]
fn bench_stack_window() {
    for (name, file, lifted_fn, stacked, port) in [
        ("0x00B89790", "player_parts.json", lifted::f_00b89790 as LiftFn, lifted_stack::f_00b89790 as LiftFn, mtcore::semantic_l190::player_draw_parts as LiftFn),
        ("0x00B89FE0", "player_part.json", lifted::f_00b89fe0, lifted_stack::f_00b89fe0, mtcore::semantic_l190::player_draw_part),
        ("0x00B8A770", "skin_prepare.json", lifted::f_00b8a770, lifted_stack::f_00b8a770, mtcore::semantic_l190::prepare_part_blocks),
        ("0x00FA5860", "swing.json", lifted::f_00fa5860, lifted_stack::f_00fa5860, mtcore::semantic_l190::swing_model_draw_unit),
    ] {
        eprintln!("-- {name}: lifted, lifted with the stack window, port");
        bench(&format!("{DIR}{file}"), name, &[("lifted", lifted_fn), ("stack window", stacked), ("port", port)]);
    }
}
