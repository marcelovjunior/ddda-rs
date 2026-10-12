//! Start DDDA.exe the way its CRT entry does (Session::start: heap, threads
//! data, stdio, arguments, environment, static initializers, then WinMain
//! 0x004079E0) and report where it stops.
//!
//!     cargo run -p ddruntime --example winmain [--frontier] [path\to\DDDA.exe]
//!
//! `--frontier` is a diagnostic probe, not a boot: it looks past CRT gaps
//! to find later stops. Each start-up step that stops is reported and the
//! next one runs anyway; instead of `__cinit` (whose `_FPinit` and
//! `_initterm` stop at the first gap) every static initializer runs on its
//! own, in CRT order, as `examples/initializers` does; then WinMain runs
//! with `__wincmdln`'s command line (or `_acmdln` if that step stopped). The
//! state the later code sees can differ from the original's where an
//! earlier step did not finish, so its stops are leads to check, not
//! verdicts. `__cinit`'s other work (`_FPinit`, the `_RTC_Terminate`
//! atexit entry, the TLS callback) is skipped and listed in the report.
use ddnative_base::mem::Mem;
use ddruntime::session::{Outcome, Session, STACK_TOP};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let frontier = args.iter().position(|a| a == "--frontier").map(|k| args.remove(k)).is_some();
    let path = args.into_iter().next()
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).unwrap_or_else(|e| panic!("{e}"));
    ddruntime::session::install_fault_hook();
    if frontier {
        run_frontier(&mut s);
    } else {
        let started = s.start();
        println!("{}: {}", started.step, started.outcome);
    }
    for (tid, fault) in ddruntime::threads::faults() {
        println!("guest thread {tid:#x} ended with: {fault}");
    }
    // guest threads the game started keep running on host threads; the
    // report is complete, so end the process with them
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

fn report(step: &str, o: &Outcome) {
    match o {
        Outcome::Returned(c) => println!("{step}: returned eax={:#x}", c.eax),
        other => println!("{step}: {other}"),
    }
}

/// The frontier probe (see the module comment).
fn run_frontier(s: &mut Session) {
    // the CRT entry's steps before __cinit (Session::start's list)
    let steps: [(&str, u32); 4] = [
        ("__heap_init", 0x012F_E354), ("__mtinit", 0x012F_982C), ("__RTC_Initialize", 0x012F_99A7), ("__ioinit", 0x012F_7C13),
    ];
    for (name, addr) in steps {
        let o = s.call(addr, 0, &[]);
        report(name, &o);
    }
    // between __ioinit and __setargv (0x012EF956..0x012EF966)
    let token = s.imports.iter().find(|i| i.full_name() == "KERNEL32.dll!GetCommandLineA").expect("GetCommandLineA").token;
    if let Outcome::Returned(c) = s.call(token, 0, &[]) {
        s.mem.set_u32(0x019A_B1C4, c.eax); // _acmdln
    }
    let env = s.call(0x012F_E2BD, 0, &[]);
    report("__crtGetEnvironmentStringsA", &env);
    if let Outcome::Returned(c) = env {
        s.mem.set_u32(0x018D_7A18, c.eax); // _aenvptr
    }
    for (name, addr) in [("__setargv", 0x012F_E202), ("_setenvp", 0x012F_DF8C)] {
        let o = s.call(addr, 0, &[]);
        report(name, &o);
    }
    println!("__cinit: replaced by the initializers one by one (skipped: _FPinit, atexit(_RTC_Terminate), the TLS callback)");
    let (mut done, mut stopped) = (0, 0);
    for (kind, target) in s.initializers() {
        match s.call(target, 0, &[]) {
            Outcome::Returned(c) if c.esp == STACK_TOP - 0x100 => done += 1,
            o => {
                stopped += 1;
                println!("  {kind} initializer {target:#010x}: {}", o.to_string().lines().next().unwrap_or(""));
            }
        }
    }
    println!("  {done} initializers completed, {stopped} stopped");
    let cmdln = match s.call(0x012F_DF2D, 0, &[]) {
        Outcome::Returned(c) => c.eax,
        o => {
            report("__wincmdln", &o);
            s.mem.u32(0x019A_B1C4)
        }
    };
    let o = s.call(0x0040_79E0, 0, &[0x0040_0000, 0, cmdln, 10]);
    report("WinMain", &o);
}
