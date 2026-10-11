//! Stage R04 (docs/RUNTIME.md): boot DDDA.exe through its own CRT entry
//! point, 0x012EFA14 (`__tmainCRTStartup` path of the analysis image's
//! entry; the installed exe's wrapper entry 0x019EB310 is not used), and
//! report where it stops: an untranslated function, an import without a
//! service, a guest fault, or ExitProcess.
//!
//!     cargo run -p ddruntime --example boot [path\to\DDDA.exe]
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use ddruntime::dispatch::{Registry, Unbound};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use ddruntime::win32::Exit;
use std::rc::Rc;

const CRT_ENTRY: u32 = 0x012E_FA14;
const STACK_TOP: u32 = 0x7F00_0000;
const STACK_SIZE: u32 = 0x0010_0000;

fn main() {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let exe = ddexe::ExeFile::open(&path).unwrap_or_else(|e| panic!("{e}"));
    let mut m = GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap_or_else(|e| panic!("{e}"));
    m.map_lazy(STACK_TOP - STACK_SIZE, STACK_SIZE, READ | WRITE).unwrap();
    let mut registry = Registry::with_lifted_program();
    ddruntime::services::install(&mut m, &mut registry).unwrap_or_else(|e| panic!("{e:?}"));
    ddruntime::services::set_exe_path(&path);
    ddruntime::files::set_roots(std::path::Path::new(&path).parent().expect("install dir"), &ddruntime::services::sandbox_home());
    let imports = ddruntime::image::bind_imports(&mut m, &exe).unwrap_or_else(|e| panic!("{e}"));
    let apis = ddruntime::services::bind_apis(&mut registry, &imports).unwrap_or_else(|e| panic!("{e:?}"));
    println!("{} bindings ({apis} of {} imports have services)", registry.len(), imports.len());
    let registry = Rc::new(registry);
    registry.install();
    std::panic::set_hook(Box::new(|_| {}));

    let mut cpu = Cpu { esp: STACK_TOP - 0x10, ..Cpu::default() };
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0); // the entry point never returns normally
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| registry.call(&mut m, &mut cpu, CRT_ENTRY)));
    match r {
        Ok(Ok(())) => println!("the CRT entry returned (eax {:#x})", cpu.eax),
        Ok(Err(u)) => println!("stopped: {u}"),
        Err(e) => {
            if let Some(u) = e.downcast_ref::<Unbound>() {
                println!("stopped: {u}");
            } else if let Some(Exit(code)) = e.downcast_ref::<Exit>() {
                println!("ExitProcess({code:#x})");
            } else {
                let msg = e.downcast_ref::<String>().cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                println!("stopped: {msg}");
            }
        }
    }
    println!("{} exit functions registered, {} guest pages backed",
             ddruntime::services::exit_list().len(), m.backed_pages());
}
