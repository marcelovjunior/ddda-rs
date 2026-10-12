//! L22 probe: the machine state a 32-bit Windows thread sees (WOW64, like
//! DDDA.exe). Build and run as a 32-bit program:
//!
//!   rustc -O --edition 2021 --target i686-pc-windows-msvc research/native/probe_machine.rs -o <dir>/probe_machine.exe
//!   <dir>/probe_machine.exe > <private>/machine-probe.json
//!
//! Prints JSON: segment selectors, the PUSHFD image, which EFLAGS bits a
//! POPFD at CPL3 can change, and CPUID leaves (basic and extended ranges,
//! subleaf 0, plus leaf 4/7/0xB/0xD subleaves). No exe or game data used.
use std::arch::asm;

fn selectors() -> [u16; 6] {
    let (mut es, mut cs, mut ss, mut ds, mut fs, mut gs) = (0u16, 0u16, 0u16, 0u16, 0u16, 0u16);
    unsafe {
        asm!("mov {0:x}, es", "mov {1:x}, cs", "mov {2:x}, ss", "mov {3:x}, ds", "mov {4:x}, fs", "mov {5:x}, gs",
             out(reg) es, out(reg) cs, out(reg) ss, out(reg) ds, out(reg) fs, out(reg) gs);
    }
    [es, cs, ss, ds, fs, gs]
}

fn pushfd() -> u32 {
    let v: u32;
    unsafe { asm!("pushfd", "pop {0}", out(reg) v) };
    v
}

/// Flip `bit` with POPFD and read it back; restores the original image.
fn toggles(bit: u32) -> bool {
    let (before, after): (u32, u32);
    unsafe {
        asm!("pushfd", "pop {b}", "mov {a}, {b}", "xor {a}, {bit}", "push {a}", "popfd",
             "pushfd", "pop {a}", "push {b}", "popfd",
             b = out(reg) before, a = out(reg) after, bit = in(reg) bit);
    }
    (before ^ after) & bit != 0
}

fn cpuid(leaf: u32, sub: u32) -> [u32; 4] {
    let r = unsafe { std::arch::x86::__cpuid_count(leaf, sub) };
    [r.eax, r.ebx, r.ecx, r.edx]
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThread() -> isize;
    fn SetThreadAffinityMask(thread: isize, mask: usize) -> usize;
}

fn main() {
    // Per-core fields (initial APIC ids in leaves 1, 0xB, 0x8000001E):
    // processor 0, as the runtime models one processor (GetSystemInfo).
    let pinned = unsafe { SetThreadAffinityMask(GetCurrentThread(), 1) } != 0;
    std::thread::yield_now();
    let s = selectors();
    println!("{{");
    println!("  \"bits\": 32,");
    println!("  \"pinned_to_processor_0\": {pinned},");
    println!("  \"selectors\": {{\"es\": {}, \"cs\": {}, \"ss\": {}, \"ds\": {}, \"fs\": {}, \"gs\": {}}},", s[0], s[1], s[2], s[3], s[4], s[5]);
    println!("  \"pushfd\": {},", pushfd());
    // TF (8) would single-step and IF/IOPL cannot change at CPL3; probe the others.
    let names = [("CF", 0), ("PF", 2), ("AF", 4), ("ZF", 6), ("SF", 7), ("DF", 10), ("OF", 11), ("NT", 14),
                 ("RF", 16), ("AC", 18), ("VIF", 19), ("VIP", 20), ("ID", 21), ("IF", 9), ("IOPL0", 12), ("IOPL1", 13)];
    let t: Vec<String> = names.iter().map(|(n, b)| format!("\"{n}\": {}", toggles(1 << b))).collect();
    println!("  \"popfd_changes\": {{{}}},", t.join(", "));
    let mut leaves = Vec::new();
    for base in [0u32, 0x8000_0000] {
        let max = cpuid(base, 0)[0];
        for leaf in base..=max.min(base + 0x40) {
            // Subleaf-indexed leaves (Intel SDM vol. 2A CPUID; AMD APM vol. 3
            // appendix E): probe subleaves 0..=5, others report subleaf 0.
            let subs: &[u32] = match leaf {
                4 | 7 | 0xb | 0xd | 0xf | 0x10 | 0x8000_001d | 0x8000_0020 => &[0, 1, 2, 3, 4, 5],
                _ => &[0],
            };
            for &sub in subs {
                let r = cpuid(leaf, sub);
                leaves.push(format!("    [{leaf}, {sub}, {}, {}, {}, {}]", r[0], r[1], r[2], r[3]));
            }
        }
    }
    println!("  \"cpuid\": [\n{}\n  ],", leaves.join(",\n"));
    // Leaves above each range's maximum, and a subleaf-independent leaf
    // with a nonzero subleaf (the runtime answers these the same way).
    let beyond: Vec<String> = [cpuid(0, 0)[0] + 1, cpuid(0x8000_0000, 0)[0] + 1, 0x4000_0000]
        .iter().map(|&l| { let r = cpuid(l, 0); format!("[{l}, 0, {}, {}, {}, {}]", r[0], r[1], r[2], r[3]) }).collect();
    println!("  \"beyond\": [{}],", beyond.join(", "));
    let r1 = cpuid(1, 7);
    println!("  \"leaf1_subleaf7\": [{}, {}, {}, {}],", r1[0], r1[1], r1[2], r1[3]);
    let brand: Vec<u8> = (0x8000_0002u32..=0x8000_0004).flat_map(|l| cpuid(l, 0)).flat_map(|w| w.to_le_bytes()).collect();
    println!("  \"brand\": {:?}", String::from_utf8_lossy(&brand).trim_end_matches('\0'));
    println!("}}");
}
