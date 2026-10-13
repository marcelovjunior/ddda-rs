// Included in the private isolated baseline replay module by l161_check.py.
// Fixture construction/restoration is outside each timed call. Mocked callees
// remain inside it, so these are fixture-path costs, not gameplay CPU times.
struct PageMemory { table: Box<ddnative_base::mem::PageTable> }
impl Mem for PageMemory {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&*self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a / 4096) as usize);
        assert!(!p.is_null(), "unmapped benchmark read {a:#x}");
        unsafe { ddnative_base::mem::load_at(p.add((a % 4096) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let p = self.table.page((a / 4096) as usize);
        assert!(!p.is_null(), "unmapped benchmark write {a:#x}");
        unsafe { ddnative_base::mem::store_at(p.add((a % 4096) as usize), 1, v as u32) }
    }
}

fn initial_cpu(c: &Value) -> Cpu {
    let ri = &c["regs_in"];
    let entry = Entry { eax: u(&ri["eax"]), ecx: u(&ri["ecx"]), edx: u(&ri["edx"]), ebx: u(&ri["ebx"]),
        esp: u(&ri["esp"]), ebp: u(&ri["ebp"]), esi: u(&ri["esi"]), edi: u(&ri["edi"]), xmm0: 0 };
    let mut cpu = Cpu::from(entry);
    cpu.fs_base = 0x7FFD_E000;
    if let Some(s) = c.get("segments") {
        let k = |n: &str| u(&s[n]) as u16;
        cpu.seg = x86::Segments { es: k("es"), cs: k("cs"), ss: k("ss"), ds: k("ds"), fs: k("fs"), gs: k("gs") };
    }
    for v in c.get("st_in").and_then(|v| v.as_array()).into_iter().flatten().rev() {
        let mut data = [0u8; 10];
        data[..8].copy_from_slice(&v[0].as_u64().unwrap().to_le_bytes());
        data[8..].copy_from_slice(&(u(&v[1]) as u16).to_le_bytes());
        cpu.st.exec::<0x28db>(&mut data);
    }
    cpu
}

#[test]
#[ignore = "bounded paired fixture-path CPU benchmark; no game"]
fn paired_cost() {
    use ddnative_base::mem::{PageTable, READ, WRITE, ATOMIC_ACCESSES};
    use std::{hint::black_box, time::Instant, sync::atomic::Ordering::Relaxed};
    ddnative_host::install();
    ATOMIC_ACCESSES.store(true, Relaxed);
    let baseline = crate::baseline::all();
    let candidate = crate::candidate::all();
    let cases = vectors("lift").unwrap();
    for (case_index, c) in cases.iter().enumerate() {
        let addr = u(&c["addr"]);
        let before = initial_cpu(c);
        let mut initial = std::collections::BTreeMap::new();
        // Zero write-only bytes, then install every recorded initial read.
        for key in ["writes", "reads"] {
            for run in c[key].as_array().unwrap() {
                let (a, hexs) = (u(&run[0]), run[1].as_str().unwrap());
                for k in 0..hexs.len() / 2 {
                    initial.insert(a + k as u32, if key == "reads" {
                        u8::from_str_radix(&hexs[2*k..2*k+2],16).unwrap()
                    } else { 0 });
                }
            }
        }
        let mut m = PageMemory { table: Box::new(PageTable::default()) };
        for page in initial.keys().map(|a| a / 4096).collect::<std::collections::BTreeSet<_>>() {
            m.table.set_perm(page as usize, READ | WRITE);
            m.table.backed_page(page as usize);
        }
        for round in 0..7 {
            for step in 0..2 {
                let cached = (round + step) % 2 == 1;
                let functions = if cached { &candidate } else { &baseline };
                let f = black_box(functions.iter().find(|e| e.0 == addr).unwrap().1);
                let table = Rc::new(if cached { crate::candidate::dispatch_table() } else { crate::baseline::dispatch_table() });
                let mut nanos = 0;
                let iterations = 300;
                for _ in 0..iterations {
                    for (&a, &v) in &initial { m.set_u8(a, v); }
                    let mut cpu = before.clone();
                    let calls = Rc::new(RefCell::new(c.get("calls").and_then(|v| v.as_array()).cloned()
                        .unwrap_or_default().into_iter().collect::<VecDeque<Value>>()));
                    x86::set_ext_calls(Some(replay(calls.clone(), table.clone())));
                    x86::ldmxcsr(x86::MXCSR_DEFAULT);
                    let start = Instant::now();
                    f(black_box(&mut m), black_box(&mut cpu));
                    nanos += start.elapsed().as_nanos();
                    black_box(cpu.eax);
                    assert!(calls.borrow().is_empty());
                }
                eprintln!("l161-cost addr={addr:08x} case={case_index} round={round} cached={} ns={:.3}",
                    u8::from(cached), nanos as f64 / iterations as f64);
            }
        }
    }
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
}

#[test]
#[ignore = "bounded paired long-loop CPU benchmark; no game"]
fn hash_loop_cost() {
    use ddnative_base::mem::{PageTable, READ, WRITE, ATOMIC_ACCESSES};
    use std::{hint::black_box, time::Instant, sync::atomic::Ordering::Relaxed};
    ddnative_host::install();
    ATOMIC_ACCESSES.store(true, Relaxed);
    let mut m = PageMemory { table: Box::new(PageTable::default()) };
    for page in [8, 16, 17, 18] {
        m.table.set_perm(page, READ | WRITE);
        m.table.backed_page(page);
    }
    for a in 0x10000..0x13000 { m.set_u8(a, (a as u8).wrapping_mul(17)); }
    m.set_u32(0x8800, 0xdead000);
    m.set_u32(0x880c, 0x811c9dc5);
    for length in [16u32, 64, 256, 1024, 4096] {
        for offset in [0u32, 1, 4093] {
            m.set_u32(0x8804, 0x10000 + offset);
            m.set_u32(0x8808, length);
            let iterations = (2_000_000 / length).max(500);
            for round in 0..7 {
                let mut results = Vec::new();
                for step in 0..2 {
                    let cached = (round + step) % 2 == 1;
                    let f = black_box(if cached { crate::candidate::f_00e0e5e0 as x86::LiftFn }
                                      else { crate::baseline::f_00e0e5e0 as x86::LiftFn });
                    let mut cpu = Cpu::default();
                    let start = Instant::now();
                    for _ in 0..iterations {
                        cpu.esp = 0x8800;
                        f(&mut m, &mut cpu);
                        black_box(cpu.eax);
                    }
                    let ns = start.elapsed().as_nanos() as f64 / iterations as f64;
                    results.push((cpu.exit(), cpu.fl.snapshot()));
                    eprintln!("l161-hash length={length} offset={offset} round={round} cached={} ns={ns:.3}", u8::from(cached));
                }
                assert_eq!(results[0], results[1]);
            }
        }
    }
}
