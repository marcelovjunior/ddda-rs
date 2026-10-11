//! Start DDDA.exe the way its CRT entry does (Session::start: heap, threads
//! data, stdio, arguments, environment, static initializers, then WinMain
//! 0x004079E0) and report where it stops.
//!
//!     cargo run -p ddruntime --example winmain [path\to\DDDA.exe]
use ddruntime::session::Session;

fn main() {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).unwrap_or_else(|e| panic!("{e}"));
    ddruntime::session::install_fault_hook();
    let started = s.start();
    println!("{}: {}", started.step, started.outcome);
    for (tid, fault) in ddruntime::threads::faults() {
        println!("guest thread {tid:#x} ended with: {fault}");
    }
}
