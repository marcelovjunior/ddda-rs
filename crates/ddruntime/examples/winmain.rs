//! Run the static initializers, then WinMain (0x004079E0), on the loaded
//! image, and report where WinMain stops. A stand-in for the full CRT
//! start-up (examples/boot.rs) until the CRT is lifted (brief L14): the CRT's
//! own start-up state (heap, stdio, environment) is not set up here.
//!
//!     cargo run -p ddruntime --example winmain [path\to\DDDA.exe]
use ddruntime::session::{Outcome, Session};

const WINMAIN: u32 = 0x0040_79E0;
const SW_SHOWNORMAL: u32 = 1;

fn main() {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).unwrap_or_else(|e| panic!("{e}"));
    std::panic::set_hook(Box::new(|_| {}));
    let inits = s.initializers();
    let done = inits.iter().filter(|&&(_, t)| matches!(s.call(t, 0, &[]), Outcome::Returned(_))).count();
    println!("{done} of {} static initializers completed", inits.len());
    // WinMain(hInstance, hPrevInstance, lpCmdLine, nCmdShow); an empty command line
    let cmdline = 0x7F00_0000 - 0x80; // inside the stack page, above the call frame
    ddnative_base::mem::Mem::set_u8(&mut s.mem, cmdline, 0);
    let out = s.call(WINMAIN, 0, &[0x0040_0000, 0, cmdline, SW_SHOWNORMAL]);
    println!("WinMain: {out}");
}
