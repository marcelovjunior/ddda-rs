//! L67: the ported CRT string services against their lifted originals, on
//! random inputs (lengths, alignments, page crossings into an unmapped
//! page, overlap, case-folding edge bytes): eax, ecx, edx, the callee-saved
//! registers, esp and the memory they write are equal, and a fault happens
//! at the same first address. The flags are not compared (every call site
//! writes them first: research/runtime/call_clobbers.py). memmove keeps
//! L61's contract (eax, esp, callee-saved registers, memory).
//!
//! `cargo test -p ddruntime --no-default-features --features crt-check
//! --test crtfast` builds only the two shards holding the CRT (lift_05,
//! lift_06); with `game` the whole lifted program is used.
#![cfg(any(feature = "game", feature = "crt-check"))]
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use ddruntime::crtfast;
use ddruntime::dispatch::{Binding, Registry};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use std::sync::Arc;

/// Three readable pages, then an unmapped one.
const SCRATCH: u32 = 0x0F00_0000;
const SCRATCH_LEN: u32 = 0x3000;
const STACK: u32 = 0x0F10_0000;
const RETURN: u32 = 0xFFFF_0000;

/// The guest memory (the exe's image: the CRT's globals) and the lifted
/// CRT, installed for this thread's lifted and external calls.
struct Session {
    mem: GuestMemory,
    _registry: Arc<Registry>,
}

#[cfg(feature = "game")]
fn lifted_program() -> Registry {
    ddnative::generated::lifted::init();
    Registry::with_lifted_program()
}

#[cfg(not(feature = "game"))]
fn lifted_program() -> Registry {
    fn lookup(a: u32) -> Option<ddnative_base::x86::LiftFn> {
        ddlift_05::lookup(a).or_else(|| ddlift_06::lookup(a))
    }
    ddnative_base::x86::set_lifted_lookup(lookup);
    let mut r = Registry::new();
    for &(a, f) in ddlift_05::FUNCTIONS.iter().chain(ddlift_06::FUNCTIONS) {
        r.insert(a, Binding::Lifted(f)).unwrap();
    }
    r
}

fn session() -> Option<Session> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let exe = ddexe::ExeFile::open(&path).ok()?;
    ddruntime::runtime::enter(Arc::new(ddruntime::runtime::Runtime::default()));
    let mut mem = GuestMemory::new();
    ddruntime::image::load(&mut mem, &exe).expect("image");
    mem.map(SCRATCH, SCRATCH_LEN, READ | WRITE).unwrap();
    mem.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    let mut r = lifted_program();
    ddruntime::services::install(&mut mem, &mut r).unwrap();
    let r = Arc::new(r);
    r.install();
    Some(Session { mem, _registry: r })
}

/// A small deterministic generator (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    /// A byte from the edge-heavy alphabet: letters of both cases, the
    /// bytes around them ('@', '[', '`', '{'), high bytes, and NUL rarely.
    fn byte(&mut self) -> u8 {
        const EDGE: &[u8] = b"@AZ[`az{aAbBzZ09 \x7f\x80\xc1\xe1\xff";
        match self.below(10) {
            0..=5 => EDGE[self.below(EDGE.len() as u32) as usize],
            6..=8 => (self.next() & 0xFF).max(1) as u8,
            _ => b'a' + self.below(3) as u8,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Returned { regs: [u32; 8], memory: Vec<u8> },
    Fault(String),
}

/// Run `f` on the inputs `setup` writes, with `args` and random registers.
fn run(s: &mut Session, setup: &dyn Fn(&mut dyn Mem), args: &[u32], regs: [u32; 6],
       f: &dyn Fn(&mut dyn Mem, &mut Cpu)) -> Outcome {
    let zero = vec![0u8; SCRATCH_LEN as usize];
    s.mem.poke(SCRATCH, &zero).unwrap();
    setup(&mut s.mem);
    let mut cpu = Cpu {
        esp: STACK - 0x80,
        eax: regs[0], ecx: regs[1], edx: regs[2], ebx: regs[3], esi: regs[4], edi: regs[5], ebp: 0x0F0F_0F0F,
        ..Cpu::default()
    };
    for a in args.iter().rev() {
        cpu.esp -= 4;
        s.mem.set_u32(cpu.esp, *a);
    }
    cpu.esp -= 4;
    s.mem.set_u32(cpu.esp, RETURN);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mem = &mut s.mem;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(mem, &mut cpu)));
    std::panic::set_hook(hook);
    match r {
        Ok(()) => {
            let mut memory = vec![0u8; SCRATCH_LEN as usize];
            s.mem.read_bytes(SCRATCH, &mut memory);
            Outcome::Returned { regs: [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esi, cpu.edi, cpu.ebp, cpu.esp], memory }
        }
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned().unwrap_or_default();
            // the faulting address (the width may differ: a dword read
            // where the service reads a byte at the same address)
            let at = msg.split(" at ").nth(1).map(|t| t[..10].to_string()).unwrap_or(msg);
            Outcome::Fault(at)
        }
    }
}

fn lifted(addr: u32) -> impl Fn(&mut dyn Mem, &mut Cpu) {
    move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::lifted_target(addr)(m, c)
}

/// Compare the service with the lifted original on `cases` random inputs;
/// `mask` selects the compared registers (eax ecx edx ebx esi edi ebp esp).
fn differential(name: &str, addr: u32, service: fn(&mut dyn Mem, &mut Cpu), cases: u32, mask: [bool; 8],
                make: &dyn Fn(&mut Rng) -> (Vec<(u32, Vec<u8>)>, Vec<u32>)) {
    let Some(mut s) = session() else { return };
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ addr as u64);
    let mut faults = 0;
    for case in 0..cases {
        let (writes, args) = make(&mut rng);
        let regs = [rng.next(), rng.next(), rng.next(), rng.next(), rng.next(), rng.next()];
        let setup = |m: &mut dyn Mem| {
            for (at, bytes) in &writes {
                for (k, b) in bytes.iter().enumerate() {
                    m.set_u8(at + k as u32, *b);
                }
            }
        };
        let want = run(&mut s, &setup, &args, regs, &lifted(addr));
        let got = run(&mut s, &setup, &args, regs, &service);
        match (&want, &got) {
            (Outcome::Returned { regs: w, memory: wm }, Outcome::Returned { regs: g, memory: gm }) => {
                for k in 0..8 {
                    if mask[k] {
                        assert_eq!(w[k], g[k], "{name} case {case}: register {k} (eax ecx edx ebx esi edi ebp esp), args {args:x?}");
                    }
                }
                assert!(wm == gm, "{name} case {case}: memory differs, args {args:x?}");
            }
            _ => {
                assert_eq!(want, got, "{name} case {case}: args {args:x?}");
                faults += 1;
            }
        }
    }
    eprintln!("{name}: {cases} cases equal ({faults} faulting)");
}

const ALL: [bool; 8] = [true; 8];

/// A NUL-terminated string at a random place (sometimes running into the
/// unmapped page without a terminator).
fn string(rng: &mut Rng, max: u32) -> (u32, Vec<u8>) {
    let len = rng.below(max);
    let open = rng.below(16) == 0;
    let at = if rng.below(4) == 0 {
        // flush against the unmapped page
        SCRATCH + SCRATCH_LEN - len - if open { 0 } else { 1 }
    } else {
        SCRATCH + rng.below(SCRATCH_LEN - max - 1)
    };
    let mut v: Vec<u8> = (0..len).map(|_| rng.byte()).map(|b| if b == 0 { 1 } else { b }).collect();
    if !open {
        v.push(0);
    }
    (at, v)
}

#[test]
fn strlen_equals_the_original() {
    differential("strlen", crtfast::STRLEN, crtfast::strlen, 3000, ALL, &|rng| {
        let (at, v) = string(rng, 300);
        (vec![(at, v)], vec![at])
    });
}

#[test]
fn wcslen_equals_the_original() {
    differential("wcslen", crtfast::WCSLEN, crtfast::wcslen, 2000, ALL, &|rng| {
        let n = rng.below(200);
        let at = SCRATCH + rng.below(SCRATCH_LEN - 2 * n - 2);
        let mut v = vec![];
        for _ in 0..n {
            let w = (rng.next() as u16).max(1);
            v.extend_from_slice(&w.to_le_bytes());
        }
        v.extend_from_slice(&[0, 0]);
        (vec![(at, v)], vec![at])
    });
}

#[test]
fn memchr_equals_the_original() {
    differential("memchr", crtfast::MEMCHR, crtfast::memchr, 3000, ALL, &|rng| {
        let n = rng.below(400);
        let at = SCRATCH + rng.below(SCRATCH_LEN - n);
        let v: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
        let c = if rng.below(2) == 0 && n > 0 { v[rng.below(n) as usize] as u32 } else { rng.byte() as u32 } | (rng.next() & 0xFFFF_FF00);
        let count = if rng.below(8) == 0 { rng.below(n + 64) } else { rng.below(n + 1) };
        (vec![(at, v)], vec![at, c, count.min(SCRATCH + SCRATCH_LEN - at + 8)])
    });
}

#[test]
fn strncpy_equals_the_original() {
    differential("strncpy", crtfast::STRNCPY, crtfast::strncpy, 3000, ALL, &|rng| {
        let (src, v) = string(rng, 200);
        let n = rng.below(260);
        let dst = SCRATCH + rng.below(SCRATCH_LEN - 300);
        (vec![(src, v)], vec![dst, src, n])
    });
}

#[test]
fn strstr_equals_the_original() {
    differential("strstr", crtfast::STRSTR, crtfast::strstr, 3000, ALL, &|rng| {
        let (hay, mut h) = string(rng, 200);
        // a needle cut from the haystack (often found) or random
        let len = rng.below(6);
        let mut n: Vec<u8> = if h.len() as u32 > len + 1 && rng.below(2) == 0 {
            let k = rng.below(h.len() as u32 - len - 1) as usize;
            h[k..k + len as usize].to_vec()
        } else {
            (0..len).map(|_| rng.byte()).collect()
        };
        n.retain(|&b| b != 0);
        n.push(0);
        if h.last() != Some(&0) {
            h.push(0); // the needle search needs a terminated haystack mostly
            h.pop();
        }
        let at = SCRATCH + rng.below(SCRATCH_LEN - 300) / 2;
        let needle = if at + 16 > hay && at < hay + h.len() as u32 + 16 { SCRATCH + 8 } else { at };
        (vec![(hay, h), (needle, n)], vec![hay, needle])
    });
}

fn pair(rng: &mut Rng, max: u32) -> (Vec<(u32, Vec<u8>)>, u32, u32) {
    let (a, va) = string(rng, max);
    // b: a case-changed copy of a (often), maybe cut or altered
    let mut vb: Vec<u8> = va.iter().map(|&c| match rng.below(3) {
        0 if c.is_ascii_alphabetic() => c ^ 0x20,
        _ => c,
    }).collect();
    if rng.below(3) == 0 && !vb.is_empty() {
        let k = rng.below(vb.len() as u32) as usize;
        vb[k] = rng.byte();
    }
    if vb.last() != Some(&0) {
        vb.push(0);
    }
    let b = SCRATCH + 0x10 + rng.below(0x400);
    let b = if b < a + va.len() as u32 + 8 && a < b + vb.len() as u32 + 8 { SCRATCH + 0x2000 - vb.len() as u32 - 8 } else { b };
    (vec![(a, va), (b, vb)], a, b)
}

#[test]
fn stricmp_equals_the_original() {
    differential("stricmp", crtfast::STRICMP, crtfast::stricmp, 3000, ALL, &|rng| {
        let (w, a, b) = pair(rng, 120);
        (w, vec![a, b])
    });
}

#[test]
fn strnicmp_equals_the_original() {
    differential("strnicmp", crtfast::STRNICMP, crtfast::strnicmp, 3000, ALL, &|rng| {
        let (w, a, b) = pair(rng, 120);
        let n = rng.below(140);
        (w, vec![a, b, n])
    });
}

#[test]
fn memmove_equals_the_original() {
    // L61's contract: eax, the callee-saved registers, esp, memory
    let mask = [true, false, false, true, true, true, true, true];
    differential("memmove", crtfast::MEMMOVE, crtfast::SERVICES.iter().find(|s| s.0 == crtfast::MEMMOVE).unwrap().1, 2000, mask, &|rng| {
        let n = if rng.below(4) == 0 { rng.below(0x1800) } else { rng.below(300) };
        let src = SCRATCH + rng.below(SCRATCH_LEN - n);
        let dst = if rng.below(3) == 0 {
            // overlapping, either way
            (src as i64 + rng.below(64) as i64 - 32).clamp(SCRATCH as i64, (SCRATCH + SCRATCH_LEN - n) as i64) as u32
        } else {
            SCRATCH + rng.below(SCRATCH_LEN - n)
        };
        let v: Vec<u8> = (0..SCRATCH_LEN).map(|k| (k * 7 + 3) as u8).collect();
        (vec![(SCRATCH, v)], vec![dst, src, n])
    });
}

/// L67 numbers (`-- --nocapture`): time per call, lifted original vs the
/// service, on typical inputs (no assertion: timings vary).
#[test]
fn service_speed_against_the_lifted_originals() {
    let Some(mut s) = session() else { return };
    let text: Vec<u8> = br"sound\se\Stage\st100_PC_main_battle_02.sngw".iter().cycle().take(200).copied().chain([0]).collect();
    s.mem.poke(SCRATCH + 0x100, &text).unwrap();
    s.mem.poke(SCRATCH + 0x400, &text.to_ascii_uppercase()).unwrap();
    s.mem.poke(SCRATCH + 0x800, b"battle_02\0").unwrap();
    let cases: [(&str, u32, fn(&mut dyn Mem, &mut Cpu), Vec<u32>); 6] = [
        ("strlen (200)", crtfast::STRLEN, crtfast::strlen, vec![SCRATCH + 0x100]),
        ("strncpy (200 of 256)", crtfast::STRNCPY, crtfast::strncpy, vec![SCRATCH + 0x1000, SCRATCH + 0x100, 256]),
        ("strstr (match at 32)", crtfast::STRSTR, crtfast::strstr, vec![SCRATCH + 0x100, SCRATCH + 0x800]),
        ("stricmp (equal, 200)", crtfast::STRICMP, crtfast::stricmp, vec![SCRATCH + 0x100, SCRATCH + 0x400]),
        ("strnicmp (64)", crtfast::STRNICMP, crtfast::strnicmp, vec![SCRATCH + 0x100, SCRATCH + 0x400, 64]),
        ("memchr (200)", crtfast::MEMCHR, crtfast::memchr, vec![SCRATCH + 0x100, 0, 210]),
    ];
    const N: u32 = 20_000;
    for (name, addr, service, args) in cases {
        let mut time = |f: &dyn Fn(&mut dyn Mem, &mut Cpu)| {
            let t = std::time::Instant::now();
            for _ in 0..N {
                let mut cpu = Cpu { esp: STACK - 0x80, ..Cpu::default() };
                for a in args.iter().rev() {
                    cpu.esp -= 4;
                    s.mem.set_u32(cpu.esp, *a);
                }
                cpu.esp -= 4;
                s.mem.set_u32(cpu.esp, RETURN);
                f(&mut s.mem, &mut cpu);
            }
            t.elapsed().as_secs_f64() * 1e9 / N as f64
        };
        let (l, f) = (time(&lifted(addr)), time(&service));
        eprintln!("{name:24} lifted {l:9.0} ns   service {f:7.0} ns   {:5.1}x", l / f);
    }
}
