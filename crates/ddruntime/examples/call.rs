//! Call one guest function on a fresh session and print how it ended.
//!
//!     cargo run -p ddruntime --example call -- 0xADDR [--after-initializers] [ecx] [args...]
use ddruntime::session::{Outcome, Session};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex number")
}

fn main() {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let after_inits = a.iter().any(|x| x == "--after-initializers");
    a.retain(|x| x != "--after-initializers");
    let target = hex(&a[0]);
    let ecx = a.get(1).map(|s| hex(s)).unwrap_or(0);
    let args: Vec<u32> = a.iter().skip(2).map(|s| hex(s)).collect();
    let mut s = Session::new(r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe").unwrap_or_else(|e| panic!("{e}"));
    ddruntime::session::install_fault_hook();
    if after_inits {
        let inits = s.initializers();
        let done = inits.iter().filter(|&&(_, t)| t != target && matches!(s.call(t, 0, &[]), Outcome::Returned(_))).count();
        println!("{done} of {} initializers completed first", inits.len());
    }
    println!("{target:#010x}: {}", s.call(target, ecx, &args));
}
