//! The fixed cost of a Direct3D 9 device call from game code (brief L101),
//! a light build that needs the installed game (DDDA_EXE):
//!
//!     cargo run --release -p ddruntime --no-default-features --example d3d_calls
//!
//! A session with a window and a device in parallel mode, as the game's
//! render thread runs: guest code without the guest lock, each call made
//! the way lifted code makes it (`x86::call_ext` to the vtable entry, the
//! arguments and return address pushed, the method's `ret` popping them).
//! Measured: SetRenderState and SetTexture (the hot non-draw methods), and
//! for comparison the same service bodies called directly.
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::session::{Outcome, Session, STACK_TOP};
use std::time::Instant;

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const N: u32 = 2_000_000;

/// A session with a window and a D3D9 device (windowed 1280x720, auto
/// depth D24S8): (session, device).
fn device_session() -> (Session, u32) {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let mut s = Session::new(&path).expect("session (DDDA_EXE)");
    ddruntime::threads::set_parallel(true);
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let (wc, name) = (SCRATCH, SCRATCH + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    eprintln!("window {hwnd:#x}");
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    for (k, v) in [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0].into_iter().enumerate() {
        s.mem.set_u32(PP + 4 * k as u32, v);
    }
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, PP, OUT]) else { panic!() };
    let dev = s.mem.u32(OUT);
    eprintln!("device {dev:#x}");
    (s, dev)
}

/// Method `slot` of `obj`'s vtable.
fn method(s: &Session, obj: u32, slot: u32) -> u32 {
    s.mem.u32(s.mem.u32(obj) + 4 * slot)
}

/// `n` calls of `target` the way lifted code makes them: `args(k)` pushed
/// right to left, a return address, then call_ext. Nanoseconds per call.
fn calls(s: &mut Session, target: u32, n: u32, args: impl Fn(u32) -> [u32; 3]) -> f64 {
    calls_with(s, target, n, args, false)
}

/// [`calls`]; `x87`: each call preceded by x87 instructions (FLD1, FSTP ST(0)), so
/// the guest's x87 state is live in the FPU when the call is made, as in
/// the game's float-heavy code.
fn calls_with(s: &mut Session, target: u32, n: u32, args: impl Fn(u32) -> [u32; 3], x87: bool) -> f64 {
    // pushes as lifted code makes them: the inline fast path
    let mem: &mut dyn Mem = &mut s.mem;
    let pt = mem.page_table();
    let mut cpu = Cpu { esp: STACK_TOP - 0x1000, ..Cpu::default() };
    let esp = cpu.esp;
    let t = Instant::now();
    ddruntime::threads::without_lock(|| {
        for k in 0..n {
            for a in args(k).into_iter().rev() {
                cpu.esp -= 4;
                mem.fast_set_u32(pt, cpu.esp, a);
            }
            cpu.esp -= 4;
            mem.fast_set_u32(pt, cpu.esp, 0x0040_1000);
            if x87 {
                cpu.st.register::<0xE8D9>(); // FLD1
                cpu.st.register::<0xD8DD>(); // FSTP ST(0)
            }
            x86::call_ext(mem, &mut cpu, target);
        }
    });
    let ns = t.elapsed().as_secs_f64() * 1e9 / n as f64;
    assert_eq!(cpu.esp, esp, "the method popped its arguments");
    assert_eq!(cpu.eax, 0, "D3D_OK");
    ns
}

/// The service body alone, called directly `n` times: no dispatch and no
/// call-wide D3D lock (each object access takes the lock itself).
fn bodies(s: &mut Session, f: fn(&mut dyn Mem, &mut Cpu), n: u32, args: impl Fn(u32) -> [u32; 3]) -> f64 {
    let mem: &mut dyn Mem = &mut s.mem;
    let pt = mem.page_table();
    let mut cpu = Cpu { esp: STACK_TOP - 0x1000, ..Cpu::default() };
    let t = Instant::now();
    for k in 0..n {
        for a in args(k).into_iter().rev() {
            cpu.esp -= 4;
            mem.fast_set_u32(pt, cpu.esp, a);
        }
        cpu.esp -= 4;
        mem.fast_set_u32(pt, cpu.esp, 0x0040_1000);
        f(mem, &mut cpu);
    }
    t.elapsed().as_secs_f64() * 1e9 / n as f64
}

fn main() {
    let (mut s, dev) = device_session();
    // two textures to alternate (CreateTexture 64x64, one level, A8R8G8B8, managed)
    let create_texture = method(&s, dev, 23);
    let mut tex = [0u32; 2];
    for t in &mut tex {
        let Outcome::Returned(c) = s.call(create_texture, 0, &[dev, 64, 64, 1, 0, 21, 1, OUT, 0]) else { panic!() };
        assert_eq!(c.eax, 0);
        *t = s.mem.u32(OUT);
    }
    eprintln!("textures {tex:x?}");
    let (set_rs, set_tex) = (method(&s, dev, 57), method(&s, dev, 65));
    // D3DRS_ALPHAREF (24): a state the game sets often; the value changes each call
    let rs = |k: u32| [dev, 24, k & 0xFF];
    // each call changes the stage's texture (AddRef the new, Release the old)
    let st = |k: u32| [dev, k & 7, tex[((k >> 3) & 1) as usize]];
    // profiling mode: SetRenderState in a loop for this many seconds
    if let Some(secs) = std::env::var("D3D_CALLS_PROFILE").ok().and_then(|v| v.parse::<f64>().ok()) {
        let t = Instant::now();
        while t.elapsed().as_secs_f64() < secs {
            calls(&mut s, set_rs, N, rs);
        }
        quit();
    }
    // the minimum of six rounds after a warm-up round (the machine is shared)
    let mut best = [f64::MAX; 5];
    for round in 0..7 {
        let r = [calls(&mut s, set_rs, N, rs), calls(&mut s, set_tex, N, st),
                 bodies(&mut s, ddruntime::d3d9state::set_render_state, N, rs), bodies(&mut s, ddruntime::d3d9state::set_texture, N, st),
                 calls_with(&mut s, set_rs, N, rs, true)];
        if round > 0 {
            for (b, x) in best.iter_mut().zip(r) {
                *b = b.min(x);
            }
        }
    }
    let [a, b, c, d, e] = best;
    println!("SetRenderState: {a:7.1} ns per call (body alone {c:6.1} ns); SetTexture: {b:7.1} ns per call (body alone {d:6.1} ns);               SetRenderState with live x87 state: {e:7.1} ns; best of 6 rounds of {N}");
    quit();
}

/// End the process at once: a session's process otherwise hangs in its exit.
fn quit() -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn TerminateProcess(process: isize, code: u32) -> i32;
    }
    // SAFETY: plain Win32 calls on this process
    unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    std::process::exit(0)
}
