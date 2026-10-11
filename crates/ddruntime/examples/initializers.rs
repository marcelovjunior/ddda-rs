//! Stage R04 diagnostic (docs/RUNTIME.md): run DDDA.exe's static
//! initializers, in the CRT's order, on the loaded image, and report how
//! far they get and which untranslated targets stop them.
//!
//!     cargo run --release -p ddruntime --example initializers [path\to\DDDA.exe]
//!
//! The C initializer table is 0x014034B4..0x014034D0 and the C++ table
//! 0x0139D490..0x014034B0 (operands of the CRT's initializer driver at
//! 0x012EDA8A/8F/94 and 0x012EDAAB/B0, call at 0x012EDAC2; see
//! scripts/discover_functions.py). Each non-null slot is called like the
//! driver calls it: no arguments, return address pushed. A failed
//! initializer leaves whatever it wrote; the run continues with the next one
//! so the report covers the whole table (this is a frontier census, not a
//! claim that the later ones would see the same state in the original).
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use ddruntime::dispatch::{Registry, Unbound};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use std::collections::BTreeMap;
use std::rc::Rc;

const TABLES: [(&str, u32, u32); 2] = [("C", 0x0140_34B4, 0x0140_34D0), ("C++", 0x0139_D490, 0x0140_34B0)];
const STACK_TOP: u32 = 0x7F00_0000;
const STACK_SIZE: u32 = 0x0010_0000;
const RETURN_SENTINEL: u32 = 0xFFFF_FFF0;

enum Outcome {
    Done,
    Unbound(Unbound),
    Fault(String),
}

fn main() {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let exe = ddexe::ExeFile::open(&path).unwrap_or_else(|e| panic!("{e}"));
    let mut m = GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap_or_else(|e| panic!("{e}"));
    m.map(STACK_TOP - STACK_SIZE, STACK_SIZE, READ | WRITE).unwrap();
    let mut registry = Registry::with_lifted_program();
    let lifted = registry.len();
    ddruntime::services::install(&mut m, &mut registry).unwrap_or_else(|e| panic!("{e:?}"));
    ddruntime::services::set_exe_path(&path);
    ddruntime::files::set_roots(std::path::Path::new(&path).parent().expect("install dir"), &ddruntime::services::sandbox_home());
    let imports = ddruntime::image::bind_imports(&mut m, &exe).unwrap_or_else(|e| panic!("{e}"));
    let apis = ddruntime::services::bind_apis(&mut registry, &imports).unwrap_or_else(|e| panic!("{e:?}"));
    println!("{} imports bound to tokens, {apis} with services", imports.len());
    let registry = Rc::new(registry);
    registry.install();
    println!("{lifted} lifted functions and {} services bound", registry.len() - lifted);
    std::panic::set_hook(Box::new(|_| {})); // outcomes are reported below

    let mut done = 0usize;
    let mut first_stop: Option<(u32, String)> = None;
    let mut unbound: BTreeMap<u32, usize> = BTreeMap::new();
    let mut faults: BTreeMap<String, usize> = BTreeMap::new();
    let mut not_lifted = 0usize;
    let mut total = 0usize;
    for (kind, lo, hi) in TABLES {
        for slot in (lo..hi).step_by(4) {
            let target = m.u32(slot);
            if target == 0 {
                continue;
            }
            total += 1;
            if registry.get(target).is_none() {
                not_lifted += 1;
                *unbound.entry(target).or_default() += 1;
                first_stop.get_or_insert((target, format!("{kind} initializer {target:#010x} is not translated")));
                continue;
            }
            match run(&mut m, &registry, target) {
                Outcome::Done => done += 1,
                Outcome::Unbound(u) => {
                    *unbound.entry(u.target).or_default() += 1;
                    first_stop.get_or_insert((target, format!("{kind} initializer {target:#010x}: {u}")));
                }
                Outcome::Fault(f) => {
                    *faults.entry(f.clone()).or_default() += 1;
                    first_stop.get_or_insert((target, format!("{kind} initializer {target:#010x}: {f}")));
                }
            }
        }
    }
    println!("{total} initializers: {done} ran to completion, {not_lifted} not translated, {} stopped",
             total - done - not_lifted);
    println!("{} exit functions registered; {} guest pages backed", ddruntime::services::exit_list().len(), m.backed_pages());
    if let Some((_, why)) = &first_stop {
        println!("first stop in CRT order: {why}");
    }
    let mut top: Vec<_> = unbound.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("untranslated targets reached (most frequent first):");
    let names: std::collections::HashMap<u32, String> = imports.iter().map(|i| (i.token, i.full_name())).collect();
    for (t, n) in top.iter().take(25) {
        println!("  {n:5} x {t:#010x} {}", names.get(t).map(String::as_str).unwrap_or(""));
    }
    let mut fl: Vec<_> = faults.into_iter().collect();
    fl.sort_by(|a, b| b.1.cmp(&a.1));
    println!("faults (most frequent first):");
    for (f, n) in fl.iter().take(10) {
        println!("  {n:5} x {f}");
    }
}

fn run(m: &mut GuestMemory, registry: &Registry, target: u32) -> Outcome {
    let mut cpu = Cpu { esp: STACK_TOP - 0x100, ..Cpu::default() };
    cpu.esp -= 4;
    m.set_u32(cpu.esp, RETURN_SENTINEL);
    let start_esp = cpu.esp;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| registry.call(m, &mut cpu, target)));
    match r {
        Ok(Ok(())) if cpu.esp == start_esp + 4 => Outcome::Done,
        Ok(Ok(())) => Outcome::Fault(format!("returned with esp {:#x}, expected {:#x}", cpu.esp, start_esp + 4)),
        Ok(Err(u)) => Outcome::Unbound(u),
        Err(e) => {
            if let Some(u) = e.downcast_ref::<Unbound>() {
                Outcome::Unbound(u.clone())
            } else {
                let msg = e.downcast_ref::<String>().cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                Outcome::Fault(msg)
            }
        }
    }
}
