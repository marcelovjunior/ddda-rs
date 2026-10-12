//! Stage R04 diagnostic (docs/RUNTIME.md): run DDDA.exe's static
//! initializers, in the CRT's order, on the loaded image, and report how
//! far they get and which untranslated targets stop them.
//!
//!     cargo run -p ddruntime --example initializers [path\to\DDDA.exe]
//!
//! The C initializer table is 0x014034B4..0x014034D0 and the C++ table
//! 0x0139D490..0x014034B0 (operands of the CRT's initializer driver at
//! 0x012EDA8A/8F/94 and 0x012EDAAB/B0, call at 0x012EDAC2; see
//! scripts/discover_functions.py). Each non-null slot is called like the
//! driver calls it: no arguments, return address pushed, on a full
//! runtime session. A failed initializer leaves whatever it wrote; the run
//! continues with the next one so the report covers the whole table (this
//! is a frontier census, not a claim that the later ones would see the same
//! state in the original).
use ddruntime::session::{Outcome, Session, STACK_TOP};
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).unwrap_or_else(|e| panic!("{e}"));
    println!("{} bindings ({} imports)", s.registry.len(), s.imports.len());
    ddruntime::session::install_fault_hook(); // outcomes are reported below

    let (mut done, mut not_lifted, mut total) = (0usize, 0usize, 0usize);
    let mut first_stop: Option<String> = None;
    let mut unbound: BTreeMap<u32, usize> = BTreeMap::new();
    let mut faults: BTreeMap<String, usize> = BTreeMap::new();
    for (kind, target) in s.initializers() {
        total += 1;
        if s.registry.get(target).is_none() {
            not_lifted += 1;
            *unbound.entry(target).or_default() += 1;
            first_stop.get_or_insert(format!("{kind} initializer {target:#010x} is not translated"));
            continue;
        }
        match s.call(target, 0, &[]) {
            Outcome::Returned(cpu) if cpu.esp == STACK_TOP - 0x100 => done += 1,
            Outcome::Returned(cpu) => {
                let f = format!("returned with esp {:#x}, expected {:#x}", cpu.esp, STACK_TOP - 0x100);
                first_stop.get_or_insert(format!("{kind} initializer {target:#010x}: {f}"));
                *faults.entry(f).or_default() += 1;
            }
            Outcome::Unbound(u) => {
                *unbound.entry(u.target).or_default() += 1;
                first_stop.get_or_insert(format!("{kind} initializer {target:#010x}: {u}"));
            }
            other => {
                let f = other.to_string();
                first_stop.get_or_insert(format!("{kind} initializer {target:#010x}: {f}"));
                *faults.entry(f.lines().next().unwrap_or("").to_string()).or_default() += 1;
            }
        }
    }
    println!("{total} initializers: {done} ran to completion, {not_lifted} not translated, {} stopped",
             total - done - not_lifted);
    println!("{} exit functions registered; {} guest pages backed", ddruntime::services::exit_list().len(), s.mem.backed_pages());
    if let Some(why) = &first_stop {
        println!("first stop in CRT order: {why}");
    }
    let mut top: Vec<_> = unbound.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("untranslated targets reached (most frequent first):");
    for (t, n) in top.iter().take(25) {
        println!("  {n:5} x {t:#010x} {}", s.import_name(*t).unwrap_or_default());
    }
    let mut fl: Vec<_> = faults.into_iter().collect();
    fl.sort_by(|a, b| b.1.cmp(&a.1));
    println!("faults (most frequent first):");
    for (f, n) in fl.iter().take(10) {
        println!("  {n:5} x {f}");
    }
    // initializers may have started guest threads; the report is complete
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}
