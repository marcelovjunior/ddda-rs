//! L184 s8: paired benchmarks of existing native ports against their faster
//! versions on the recorded vectors (page-table memory, like production).
//! `cargo test --release -p mtcore --test s8_perf -- --ignored --nocapture`
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;

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

/// Time `body` on every `step`-th case of `path`; `ext` serves the guest calls
/// (target -> body that performs the callee natively).
type Ext = Box<dyn Fn(&mut dyn Mem, &mut Cpu, u32)>;

fn bench(path: &str, name: &str, bodies: &[(&str, LiftFn)], step: usize, ext: &dyn Fn(&Value) -> Ext) {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let mut m = Paged::default();
    let mut totals = vec![0f64; bodies.len()];
    let mut count = 0u64;
    for round in 0..4 {
        for case in cases.iter().step_by(step) {
            if case["events"].as_array().map_or(0, |e| e.len()) < std::env::var("S8_MIN_EVENTS").ok().and_then(|v| v.parse().ok()).unwrap_or(0) {
                continue;
            }
            let initial = pairs(&case["initial"]);
            if initial.len() > std::env::var("S8_MAX_INITIAL").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX) {
                continue;
            }
            let regs: Vec<u32> = case["before"]["regs"].as_array().unwrap().iter().map(n).collect();
            for (k, (_, body)) in bodies.iter().enumerate() {
                let mut best = std::time::Duration::MAX;
                for _ in 0..20 {
                    x86::set_ext_calls(Some(ext(case)));
                    for &(a, b) in &initial {
                        m.set_u8(a, b);
                    }
                    let mut c = Cpu::default();
                    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = [regs[0], regs[1], regs[2], regs[3], regs[4], regs[5], regs[6], regs[7]];
                    let t = std::time::Instant::now();
                    body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                    best = best.min(t.elapsed());
                    std::hint::black_box(c.eax);
                    if c.st.home != 0 {
                        (ddnative_base::hooks::get().x87_release)(&mut c.st);
                    }
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
    if bodies.len() == 2 {
        eprintln!("{name}: speedup = {:.2}x", totals[0] / totals[1]);
    }
}

/// The guest callees of the old intern path, served natively.
fn intern_ext(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    match target {
        0x00e0_e5e0 => mtcore::semantic::hash_key_words(m, c),
        other => panic!("call {other:x}"),
    }
}

#[test]
#[ignore]
fn bench_intern_keys() {
    bench(
        "E:/ddda-re/notes/native-L184/intern_keys.json",
        "intern_dependency_keys",
        &[("existing", mtcore::semantic::intern_dependency_keys_reference), ("new", mtcore::semantic::intern_dependency_keys)],
        3,
        &|_| Box::new(intern_ext),
    );
}

/// Replay of the L174 event format (`effects` bytes, `eax`, `returned_esp` per call).
fn replay_l174(case: &Value) -> Ext {
    let events: Vec<(Vec<(u32, u8)>, u32, u32)> = case["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (pairs(&e["effects"]), n(&e["eax"]), n(&e["returned_esp"])))
        .collect();
    let at = std::cell::Cell::new(0usize);
    Box::new(move |m, c, _| {
        let (effects, eax, esp) = &events[at.get()];
        at.set(at.get() + 1);
        apply_runs(m, effects);
        c.eax = *eax;
        c.esp = *esp;
    })
}

#[test]
#[ignore]
fn bench_palette() {
    bench(
        "E:/ddda-re/notes/native-L174/00ffc9f0.json",
        "prepare_model_palette",
        &[("current", mtcore::semantic_draw_b::prepare_model_palette)],
        1,
        &replay_l174,
    );
}

/// Write `(address, byte)` effects: contiguous runs through the page table.
fn apply_runs(m: &mut dyn Mem, effects: &[(u32, u8)]) {
    let pg = ddnative_base::mem::Pages::of(m);
    let mut i = 0;
    while i < effects.len() {
        let start = effects[i].0;
        let mut j = i + 1;
        while j < effects.len() && effects[j].0 == effects[j - 1].0.wrapping_add(1) && (effects[j].0 >> 12) == (start >> 12) {
            j += 1;
        }
        match pg.bytes(start, j - i, ddnative_base::mem::WRITE) {
            Some(p) => {
                for (k, &(_, b)) in effects[i..j].iter().enumerate() {
                    unsafe { *p.add(k) = b };
                }
            }
            None => {
                for &(a, b) in &effects[i..j] {
                    m.set_u8(a, b);
                }
            }
        }
        i = j;
    }
}

#[test]
#[ignore]
fn bench_palette_loop() {
    use mtcore::semantic_draw_b::{palette_matrices, palette_matrix_reference};
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let pg_of = |m: &Paged| ddnative_base::mem::Pages::of(m);
    let (matrices, transforms, out_new, out_ref) = (0x1000_0000u32, 0x1001_0000u32, 0x1002_0000u32, 0x1003_0000u32);
    let count = 150u32;
    let mut m = Paged::default();
    let mut seed = 0x9e37_79b9u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for i in 0..count * 16 {
        let v = (next() % 2000) as f32 / 1000.0 - 1.0;
        for (k, b) in v.to_bits().to_le_bytes().into_iter().enumerate() {
            m.set_u8(matrices + 4 * i + k as u32, b);
        }
    }
    for i in 0..count * 40 {
        let v = (next() % 2000) as f32 / 1000.0 - 1.0;
        for (k, b) in v.to_bits().to_le_bytes().into_iter().enumerate() {
            m.set_u8(transforms + 4 * i + k as u32, b);
        }
    }
    for base in [out_new, out_ref] {
        for i in 0..count * 12 * 4 {
            m.set_u8(base + i, 0);
        }
    }
    let pg = pg_of(&m);
    let time = |m: &mut Paged, f: &mut dyn FnMut(&mut Paged)| {
        let mut best = f64::MAX;
        for _ in 0..30 {
            let t = std::time::Instant::now();
            for _ in 0..20 {
                f(std::hint::black_box(&mut *m));
            }
            best = best.min(t.elapsed().as_nanos() as f64 / 20.0);
        }
        best
    };
    let reference = time(&mut m, &mut |m| {
        for i in 0..count {
            palette_matrix_reference(m, pg, matrices + 64 * i, transforms + 160 * i, out_ref + 48 * i);
        }
    });
    let fast = time(&mut m, &mut |m| palette_matrices(m, pg, count, matrices, transforms, out_new));
    for i in 0..count * 48 {
        assert_eq!(m.u8(out_new + i), m.u8(out_ref + i), "byte {i}");
    }
    eprintln!("palette loop ({count} joints): existing {reference:.0} ns -> new {fast:.0} ns ({:.2}x)", reference / fast);
}

// the packed palette block equals the scalar code bit for bit, NaN payloads included
#[test]
fn palette_packed_equals_scalar_on_random_bits() {
    use mtcore::semantic_draw_b::{palette_matrices, palette_matrix_reference};
    ddnative_host::install();
    let (matrices, transforms, out_new, out_ref) = (0x1000_0000u32, 0x1001_0000u32, 0x1002_0000u32, 0x1003_0000u32);
    let mut m = Paged::default();
    let mut seed = 0x1234_5679u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut nans = 0;
    for round in 0..300 {
        for i in 0..16 + 40 {
            let base = if i < 16 { matrices + 4 * i } else { transforms + 4 * (i - 16) };
            let mut v = next();
            match next() % 8 {
                0 => v = 0x7f80_0000 | (v & 0x007f_ffff) | (v & 0x8000_0000), // inf / NaN with random payload
                1 => v &= 0x807f_ffff,                                         // denormals and zero
                2 => v = (v & 0x8000_0000) | 0x3f80_0000 | (v & 0x7f_ffff),    // near one
                3 => v = (v & 0x8000_0000) | 0x3000_0000 | (v & 0x0fff_ffff),
                _ => {}
            }
            if (v >> 23) & 0xff == 0xff && v & 0x7f_ffff != 0 {
                nans += 1;
            }
            for (k, b) in v.to_le_bytes().into_iter().enumerate() {
                m.set_u8(base + k as u32, b);
            }
        }
        let pg = ddnative_base::mem::Pages::of(&m);
        palette_matrix_reference(&mut m, pg, matrices, transforms, out_ref);
        palette_matrices(&mut m, pg, 1, matrices, transforms, out_new);
        for i in 0..48 {
            assert_eq!(m.u8(out_new + i), m.u8(out_ref + i), "round {round} byte {i}");
        }
    }
    assert!(nans > 100);
}

/// Replay of the paired call/return event format (`effects` and registers of the return event).
fn replay_paired(case: &Value) -> Ext {
    let events: Vec<(Vec<(u32, u8)>, u32, u32)> = case["events"]
        .as_array()
        .unwrap()
        .chunks(2)
        .map(|e| {
            let regs: Vec<u32> = e[1]["regs"].as_array().unwrap().iter().map(n).collect();
            (pairs(&e[1]["effects"]), regs[0], regs[4])
        })
        .collect();
    let at = std::cell::Cell::new(0usize);
    Box::new(move |m, c, _| {
        let (effects, eax, esp) = &events[at.get()];
        at.set(at.get() + 1);
        apply_runs(m, effects);
        c.eax = *eax;
        c.esp = *esp;
    })
}

#[test]
#[ignore]
fn bench_draw_mesh() {
    bench(
        "E:/ddda-re/notes/native-L171/draw_mesh.json",
        "draw_mesh",
        &[("current", mtcore::semantic_draw::draw_mesh)],
        3,
        &replay_paired,
    );
}

