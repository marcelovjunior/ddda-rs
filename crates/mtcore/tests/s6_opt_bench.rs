//! L184 s6: paired microbenchmarks of the shadow job's native ports on the
//! recorded vectors of their tests (page-table memory, the original's guest
//! calls replayed): `primitive_material` (0x00EA23F0), `commit_primitive_state`
//! (0x00FEE320) and `apply_pass_state` (0x010658B0). Ignored by default; run
//! in release with `-- --ignored --nocapture --test-threads=1`.
//! `S6_BENCH_LIMIT` caps the recorded cases used (default 400).
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
use serde_json::Value;
use std::{cell::Cell, rc::Rc};

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
        assert!(!p.is_null(), "read of unmapped {a:x}");
        // SAFETY: this table owns the backed page for the lifetime of self.
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: this table owns the backed page and a is inside it.
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) }
    }
}

fn n(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn pairs(v: &Value) -> Vec<(u32, u8)> {
    v.as_array().unwrap().iter().map(|p| (n(&p[0]), n(&p[1]) as u8)).collect()
}

/// The effect bytes as runs of consecutive addresses (the replay of a guest
/// memcpy without a per-byte cost in the timed region).
fn runs(bytes: &[(u32, u8)]) -> Vec<(u32, Vec<u8>)> {
    let mut runs: Vec<(u32, Vec<u8>)> = Vec::new();
    for &(a, b) in bytes {
        match runs.last_mut() {
            Some((start, data)) if start.wrapping_add(data.len() as u32) == a => data.push(b),
            _ => runs.push((a, vec![b])),
        }
    }
    runs
}

/// Time `body` over the recorded cases of `entry` (calls replayed, as the
/// test does); ns per call.
fn bench_recorded(path: &str, entry: u32, body: LiftFn, label: &str) {
    ddnative_host::install();
    let limit: usize = std::env::var("S6_BENCH_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases: Vec<&Value> = data["cases"].as_array().unwrap().iter().filter(|v| n(&v["entry"]) == entry).take(limit).collect();
    let mut m = Paged::default();
    let mut elapsed = std::time::Duration::ZERO;
    let mut count = 0u64;
    let mut best = f64::MAX;
    for round in 0..12 {
        let (before, before_count) = (elapsed, count);
        for case in &cases {
            let initial = pairs(&case["initial"]);
            let calls: Vec<(Vec<(u32, Vec<u8>)>, u32, u32)> = case["calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter(|call| n(&call["target"]) != 0x012e_fea0)
                .map(|call| (runs(&pairs(&call["effects"])), n(&call["result"]), n(&call["return_esp"])))
                .collect();
            let cursor = Rc::new(Cell::new(0usize));
            let pos = cursor.clone();
            x86::set_ext_calls(Some(Box::new(move |m, c, target| {
                if target == 0x01008200 {
                    mtcore::semantic_draw_c::single_key(m, c);
                    return;
                }
                if target == 0x012e_fea0 {
                    // the platform model of the CRT memcpy (the baseline port calls it)
                    ddnative_base::platform::memcpy(m, c);
                    return;
                }
                let (effects, result, return_esp) = &calls[pos.get()];
                pos.set(pos.get() + 1);
                let pages = ddnative_base::mem::Pages::of(m);
                for (a, data) in effects {
                    // page by page: a run may cross pages
                    let mut done = 0usize;
                    while done < data.len() {
                        let at = a.wrapping_add(done as u32);
                        let chunk = (data.len() - done).min(4096 - (at & 4095) as usize);
                        match pages.bytes(at, chunk, 2) {
                            // SAFETY: bytes() returned `chunk` writable bytes inside one backed page
                            Some(p) => unsafe { std::ptr::copy_nonoverlapping(data[done..].as_ptr(), p, chunk) },
                            None => data[done..done + chunk].iter().enumerate().for_each(|(k, &b)| m.set_u8(at + k as u32, b)),
                        }
                        done += chunk;
                    }
                }
                c.eax = *result;
                c.esp = *return_esp;
                c.ecx = 0xc1a0;
                c.edx = 0xd1a0;
            })));
            for &(a, b) in &initial {
                m.set_u8(a, b);
            }
            let mut c = Cpu::default();
            [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = std::array::from_fn(|i| n(&case["before"]["regs"][i]));
            let old = x86::stmxcsr();
            x86::ldmxcsr(n(&case["before"]["mxcsr"]));
            let mut control = [0; 10];
            control[..2].copy_from_slice(&(n(&case["before"]["x87"][0]) as u16).to_le_bytes());
            c.st.exec::<0x28d9>(&mut control);
            let start = std::time::Instant::now();
            body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
            let time = start.elapsed();
            x86::ldmxcsr(old);
            x86::set_ext_calls(None);
            if round > 0 {
                elapsed += time;
                count += 1;
            }
            std::hint::black_box(c.eax);
        }
        if round > 0 {
            best = best.min((elapsed - before).as_nanos() as f64 / (count - before_count) as f64);
        }
    }
    eprintln!("{label}: {} cases, {:.1} ns/call (best round {best:.1})", cases.len(), elapsed.as_nanos() as f64 / count as f64);
}

#[test]
#[ignore = "paired microbenchmark"]
fn bench_primitive_material() {
    bench_recorded("E:/ddda-re/notes/native-L175/00ea23f0.json", 0xea23f0, mtcore::semantic_draw_c::primitive_material, "primitive_material");
}

#[test]
#[ignore = "paired microbenchmark"]
fn bench_commit_primitive_state() {
    bench_recorded("E:/ddda-re/notes/native-L177/00fee320.json", 0xfee320, mtcore::semantic_draw_c::commit_primitive_state, "commit_primitive_state");
}

#[test]
#[ignore = "paired microbenchmark"]
fn bench_x87_alpha() {
    ddnative_host::install();
    // the cost of the x87 sequence of the alpha threshold (FILD, FMUL, FST, FLD1, FSUB, FDIVR, FSTP, FLD, FUCOMIP, FSTP)
    let mut c = Cpu::default();
    let start = std::time::Instant::now();
    let rounds = 200_000u32;
    for i in 0..rounds {
        let mut operand = [0u8; 10];
        operand[..4].copy_from_slice(&(i & 255).to_le_bytes());
        c.st.exec::<0x00db>(&mut operand);
        operand[..4].copy_from_slice(&0x3b808081u32.to_le_bytes());
        c.st.exec::<0x08d8>(&mut operand);
        c.st.exec::<0x10d9>(&mut operand);
        c.st.register::<0xe8d9>();
        c.st.register::<0xe1d8>();
        operand[..4].copy_from_slice(&0x3f800000u32.to_le_bytes());
        c.st.exec::<0x38d8>(&mut operand);
        c.st.exec::<0x18d9>(&mut operand);
        operand[..4].copy_from_slice(&0x3f000000u32.to_le_bytes());
        c.st.exec::<0x00d9>(&mut operand);
        let f = c.st.exec::<0xe9df>(&mut operand);
        c.st.register::<0xd8dd>();
        std::hint::black_box(f);
    }
    eprintln!("x87 alpha sequence: {:.1} ns", start.elapsed().as_nanos() as f64 / rounds as f64);
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

fn put(m: &mut Paged, a: u32, v: u32) {
    for (k, b) in v.to_le_bytes().into_iter().enumerate() {
        m.set_u8(a + k as u32, b);
    }
}

/// 0x01062F20 as the test stubs it (a result and its `ret 4`).
fn pass_stub(m: &mut dyn Mem, c: &mut Cpu, _target: u32) {
    let argument = m.u32(c.esp.wrapping_add(4));
    c.eax = argument.rotate_left(9) ^ 0x5A5A_0000;
    c.ecx = 0x1111;
    c.edx = 0x2222;
    c.esp = c.esp.wrapping_add(8);
}

/// `apply_pass_state` on the states of its random test (kinds 0xB, 0xC, 0x10
/// most of the time), the one guest call stubbed.
#[test]
#[ignore = "paired microbenchmark"]
fn bench_apply_pass_state() {
    ddnative_host::install();
    let (this, state, object, table, entries, stack) = (0x0010_0000u32, 0x0020_0000u32, 0x0030_0000u32, 0x0034_0000u32, 0x0038_0000u32, 0x0080_0000u32);
    let mut m = Paged::default();
    let mut elapsed = std::time::Duration::ZERO;
    let mut count = 0u64;
    x86::set_ext_calls(Some(Box::new(pass_stub)));
    for seed in 0..4000u64 {
        let mut rng = Rng(0x0106_58B0_1234_5678 ^ seed.wrapping_mul(0x9E37_79B9));
        put(&mut m, 0x018D_2CE8, rng.next());
        put(&mut m, 0x0162_113C, rng.below(3));
        let off = rng.below(4);
        put(&mut m, state + 0x1CC, if off == 1 { rng.next() } else { 0x17E1_5A66 });
        put(&mut m, state + 0x1D0, if off == 2 { rng.next() } else { 0x53E9_BA56 });
        m.set_u8(state + 0x169, if off == 3 { rng.below(3) as u8 } else { rng.below(2) as u8 });
        for at in [0x154, 0x158, 0x110, 0x114, 0x118, 0x11C, 0x120] {
            put(&mut m, state + at, if rng.below(2) == 0 { rng.below(3) } else { rng.next() });
        }
        put(&mut m, state + 0xEC8, rng.below(3));
        put(&mut m, state + 0xD28, rng.below(3));
        put(&mut m, this + 0x24, if rng.below(4) == 0 { 0 } else { object });
        put(&mut m, object + 0x10, rng.next());
        put(&mut m, state + 0x1418, if rng.below(3) == 0 { rng.below(16) } else { table | rng.below(16) });
        for k in 0..3 {
            put(&mut m, table + 4 * k, rng.below(3));
        }
        put(&mut m, this + 0x14, rng.next());
        put(&mut m, this + 0x18, rng.next() & !0x1FF | rng.below(8));
        put(&mut m, this + 0x30, entries);
        for k in 0..8 {
            put(&mut m, entries + 12 * k, rng.below(64) << 20 | rng.below(1 << 20));
            put(&mut m, entries + 12 * k + 4, rng.next());
            put(&mut m, entries + 12 * k + 8, rng.next());
        }
        let kind = [0xB, 0xC, 0x10, 0xB, 0xC, rng.next()][rng.below(6) as usize];
        put(&mut m, stack, 0xFFFF_0000);
        put(&mut m, stack + 4, state);
        put(&mut m, stack + 8, rng.next());
        put(&mut m, stack + 12, kind);
        let mut c = Cpu { eax: rng.next(), ecx: this, edx: rng.next(), ebx: rng.next(), esp: stack, ebp: rng.next(), esi: rng.next(), edi: rng.next(), ..Cpu::default() };
        let start = std::time::Instant::now();
        mtcore::semantic_c2::apply_pass_state(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
        let time = start.elapsed();
        if seed >= 100 {
            elapsed += time;
            count += 1;
        }
        std::hint::black_box(c.eax);
    }
    x86::set_ext_calls(None);
    eprintln!("apply_pass_state: {count} synthetic cases, {:.1} ns/call", elapsed.as_nanos() as f64 / count as f64);
}

#[test]
#[ignore = "paired microbenchmark"]
fn bench_compare_forms() {
    ddnative_host::install();
    let data: Vec<u32> = (0..1024u32).map(|i| (i.wrapping_mul(2654435761) >> 9) | 0x3f00_0000).collect();
    let rounds = 2000;
    let mut best = [f64::MAX; 2];
    for _ in 0..20 {
        let start = std::time::Instant::now();
        let mut hits = 0u32;
        for _ in 0..rounds {
            for w in data.windows(2) {
                hits += (ddnative_base::x86::comiss_hw::<false>(w[0], w[1]).r == 0) as u32;
            }
        }
        best[0] = best[0].min(start.elapsed().as_nanos() as f64 / (rounds * 1023) as f64);
        std::hint::black_box(hits);
        let start = std::time::Instant::now();
        let mut hits = 0u32;
        for _ in 0..rounds {
            for w in data.windows(2) {
                // SAFETY: SSE is part of the x86_64 baseline
                hits += unsafe {
                    use std::arch::x86_64::{_mm_set_ss, _mm_ucomieq_ss};
                    (_mm_ucomieq_ss(_mm_set_ss(f32::from_bits(w[0])), _mm_set_ss(f32::from_bits(w[1]))) != 0) as u32
                };
            }
        }
        best[1] = best[1].min(start.elapsed().as_nanos() as f64 / (rounds * 1023) as f64);
        std::hint::black_box(hits);
    }
    eprintln!("ucomiss compare: comiss_hw {:.2} ns, intrinsic {:.2} ns", best[0], best[1]);
}
