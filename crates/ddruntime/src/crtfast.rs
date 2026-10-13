//! The game's statically linked CRT string routines as services (L67).
//!
//! Translated from DDDA.exe (2017-12-13 Steam): each routine below is the
//! original's own algorithm, ported register by register, so the services
//! leave exactly what the originals leave: the return value, ecx and edx
//! (callers' partial-register writes such as `xor dl, dl` keep their upper
//! bits, research/runtime/call_clobbers.py), the callee-saved registers,
//! the memory written and the order of the memory accesses, so a fault on
//! an unmapped page happens at the same first address. The flags are not
//! kept: every call site writes them before reading (call_clobbers.py).
//! The gain over the lifted bodies: no per-instruction flag bookkeeping,
//! and `_strlen` finds the terminator page by page.
//!
//! `__stricmp`/`__strnicmp` take their ASCII paths only where the
//! original does (the CRT's locale never changed, both strings given, the
//! count in range); otherwise the lifted original runs (its locale and
//! errno paths). `_strstr` with a one-character needle ends in the CRT's
//! strchr body (0x012EF426), which runs lifted from the state the
//! original's `jmp` leaves.
//!
//! All of them touch only guest memory with integer code:
//! dispatch::Binding::Integer (L170; session.rs).
use ddnative_base::mem::{Mem, PAGE};
use ddnative_base::x86::Cpu;

/// 0x01301D90 `_strlen`.
pub const STRLEN: u32 = 0x0130_1D90;
/// 0x0130583C `_wcslen`.
pub const WCSLEN: u32 = 0x0130_583C;
/// 0x012F4800 `_memchr`.
pub const MEMCHR: u32 = 0x012F_4800;
/// 0x012EE1D0 `_strncpy`.
pub const STRNCPY: u32 = 0x012E_E1D0;
/// 0x012EE560 `_strstr`.
pub const STRSTR: u32 = 0x012E_E560;
/// 0x012EF5D7 `__stricmp` (with ___ascii_stricmp, 0x012EF4DE).
pub const STRICMP: u32 = 0x012E_F5D7;
/// 0x0130875A `__strnicmp` (with ___ascii_strnicmp, 0x01309AE0).
pub const STRNICMP: u32 = 0x0130_875A;
/// 0x012EDE40: the CRT's second memcpy (memmove semantics, like
/// 0x012EFEA0; 708 bytes differing only in relocated addresses).
pub const MEMMOVE: u32 = 0x012E_DE40;
/// The services by address (session.rs binds them).
pub const SERVICES: &[(u32, fn(&mut dyn Mem, &mut Cpu))] = &[
    (STRLEN, strlen),
    (WCSLEN, wcslen),
    (MEMCHR, memchr),
    (STRNCPY, strncpy),
    (STRSTR, strstr),
    (STRICMP, stricmp),
    (STRNICMP, strnicmp),
    (MEMMOVE, crate::fastmem::memcpy),
];

/// The strchr body `_strstr` ends in (FUN_012EF426).
const STRCHR_TAIL: u32 = 0x012E_F426;
/// `__locale_changed` (the CRT's flag, 0x018D81FC): nonzero once setlocale
/// changed the locale; the ASCII compare paths apply only while it is 0.
const LOCALE_CHANGED: u32 = 0x018D_81FC;

fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// cdecl `ret`.
fn ret(m: &dyn Mem, cpu: &mut Cpu) {
    let _ = m.u32(cpu.esp); // the return address
    cpu.esp = cpu.esp.wrapping_add(4);
}

/// Run the lifted original at `addr` (its return address and arguments on
/// the stack as the service got them).
fn original(m: &mut dyn Mem, cpu: &mut Cpu, addr: u32) {
    // the lifted program's lookup (idempotent; its first call sets it too)
    #[cfg(feature = "game")]
    ddnative::generated::lifted::init();
    ddnative_host::lifted_target(addr)(m, cpu);
}

fn set_low8(r: u32, v: u32) -> u32 {
    (r & !0xFF) | (v & 0xFF)
}
fn set_high8(r: u32, v: u32) -> u32 {
    (r & !0xFF00) | ((v & 0xFF) << 8)
}

/// The index of the first NUL at `s`, read page by page in growing chunks
/// (a page the original's reads would fault on faults at the same first
/// address: `s`, or the start of a later page).
fn nul_index(m: &dyn Mem, s: u32) -> u32 {
    let mut buf = [0u8; PAGE as usize];
    let mut at = s;
    let mut chunk = 64u32;
    loop {
        let n = chunk.min(PAGE - at % PAGE);
        m.read_bytes(at, &mut buf[..n as usize]);
        if let Some(i) = buf[..n as usize].iter().position(|&b| b == 0) {
            return at.wrapping_add(i as u32).wrapping_sub(s);
        }
        at = at.wrapping_add(n);
        chunk = (chunk * 4).min(PAGE);
    }
}

/// `_strlen(s)` (0x01301D90): bytes to the first aligned address, then
/// aligned dwords with the 0x7EFEFEFF test. eax = the length, ecx = s,
/// edx = 0x7EFEFEFF + the dword holding the terminator (the loop's last
/// iteration; the test has no false negatives), unchanged when the
/// terminator is among the leading bytes.
pub fn strlen(m: &mut dyn Mem, cpu: &mut Cpu) {
    let s = arg(m, cpu, 0);
    let len = nul_index(m, s);
    let end = s.wrapping_add(len);
    let first_aligned = if s & 3 == 0 { s } else { (s | 3).wrapping_add(1) };
    if end.wrapping_sub(s) >= first_aligned.wrapping_sub(s) {
        cpu.edx = 0x7EFE_FEFFu32.wrapping_add(m.u32(end & !3));
    }
    cpu.eax = len;
    cpu.ecx = s;
    ret(m, cpu);
}

/// `_wcslen(s)` (0x0130583C): words until 0 (`mov cx, word [eax]`).
pub fn wcslen(m: &mut dyn Mem, cpu: &mut Cpu) {
    let s = arg(m, cpu, 0);
    let mut p = s;
    while m.u16(p) != 0 {
        p = p.wrapping_add(2);
    }
    // eax = (p + 2 - s) / 2 - 1 (sar), cx = 0
    cpu.eax = ((p.wrapping_add(2).wrapping_sub(s) as i32) >> 1).wrapping_sub(1) as u32;
    cpu.ecx &= 0xFFFF_0000;
    ret(m, cpu);
}

/// `_memchr(p, c, n)` (0x012F4800), ported instruction by instruction.
pub fn memchr(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, c, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let (mut eax, mut ecx, mut edx) = (n, cpu.ecx, cpu.edx);
    let bl = c & 0xFF;
    #[derive(Clone, Copy)]
    enum L {
        Done,
        Found(u32),
    }
    let out = 'run: {
        if eax == 0 {
            break 'run L::Done;
        }
        edx = p;
        // leading bytes to an aligned address
        while edx & 3 != 0 {
            ecx = set_low8(ecx, m.u8(edx) as u32 ^ bl);
            edx = edx.wrapping_add(1);
            if ecx & 0xFF == 0 {
                break 'run L::Found(edx.wrapping_sub(1));
            }
            eax -= 1;
            if eax == 0 {
                break 'run L::Done;
            }
        }
        let tail = |m: &mut dyn Mem, mut eax: u32, ecx: &mut u32, edx: &mut u32| -> L {
            // 0x012F4848: eax += 4; then bytes
            eax = eax.wrapping_add(4);
            while eax != 0 {
                *ecx = set_low8(*ecx, m.u8(*edx) as u32 ^ bl);
                *edx = edx.wrapping_add(1);
                if *ecx & 0xFF == 0 {
                    return L::Found(edx.wrapping_sub(1));
                }
                eax -= 1;
            }
            L::Done
        };
        // 0x012F4831: sub eax, 4; jb tail
        let (r, borrow) = eax.overflowing_sub(4);
        eax = r;
        if borrow {
            break 'run tail(m, eax, &mut ecx, &mut edx);
        }
        let pattern = bl | bl << 8 | bl << 16 | bl << 24;
        loop {
            // 0x012F4862
            ecx = m.u32(edx) ^ pattern;
            let edi = 0x7EFE_FEFFu32.wrapping_add(ecx);
            ecx = (ecx ^ 0xFFFF_FFFF) ^ edi;
            edx = edx.wrapping_add(4);
            ecx &= 0x8101_0100;
            if ecx != 0 {
                // 0x012F487D: which byte
                ecx = m.u32(edx.wrapping_sub(4));
                ecx = set_low8(ecx, ecx ^ bl);
                if ecx & 0xFF == 0 {
                    break 'run L::Found(edx.wrapping_sub(4));
                }
                ecx = set_high8(ecx, (ecx >> 8) ^ bl);
                if ecx & 0xFF00 == 0 {
                    break 'run L::Found(edx.wrapping_sub(3));
                }
                ecx >>= 16;
                ecx = set_low8(ecx, ecx ^ bl);
                if ecx & 0xFF == 0 {
                    break 'run L::Found(edx.wrapping_sub(2));
                }
                ecx = set_high8(ecx, (ecx >> 8) ^ bl);
                if ecx & 0xFF00 == 0 {
                    break 'run L::Found(edx.wrapping_sub(1));
                }
            }
            // 0x012F485D: sub eax, 4; jb (pop edi) tail
            let (r, borrow) = eax.overflowing_sub(4);
            eax = r;
            if borrow {
                break 'run tail(m, eax, &mut ecx, &mut edx);
            }
        }
    };
    cpu.eax = match out {
        L::Done => 0,
        L::Found(a) => a,
    };
    cpu.ecx = ecx;
    cpu.edx = edx;
    ret(m, cpu);
}

/// `_strncpy(dst, src, n)` (0x012EE1D0), ported block by block (the
/// labels are the original's addresses).
pub fn strncpy(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dst, src, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let (mut eax, mut ecx, mut edx) = (cpu.eax, n, cpu.edx);
    let (mut ebx, mut esi, mut edi) = (0u32, src, dst);
    let al = |eax: u32| eax & 0xFF;
    let mut pc: u32 = 0x1D0;
    loop {
        pc = match pc {
            0x1D0 => {
                if ecx == 0 {
                    break;
                }
                ebx = ecx;
                if esi & 3 != 0 {
                    0x1FC
                } else {
                    ecx >>= 2;
                    if ecx != 0 { 0x27F } else { 0x223 }
                }
            }
            0x1FC => {
                eax = set_low8(eax, m.u8(esi) as u32);
                esi = esi.wrapping_add(1);
                m.set_u8(edi, al(eax) as u8);
                edi = edi.wrapping_add(1);
                ecx = ecx.wrapping_sub(1);
                if ecx == 0 {
                    break;
                }
                if al(eax) == 0 {
                    0x23E
                } else if esi & 3 != 0 {
                    0x1FC
                } else {
                    ebx = ecx;
                    ecx >>= 2;
                    if ecx != 0 { 0x27F } else { 0x21E }
                }
            }
            0x21E => {
                ebx &= 3;
                if ebx == 0 {
                    break;
                }
                0x223
            }
            0x223 => {
                eax = set_low8(eax, m.u8(esi) as u32);
                esi = esi.wrapping_add(1);
                m.set_u8(edi, al(eax) as u8);
                edi = edi.wrapping_add(1);
                if al(eax) == 0 {
                    0x268
                } else {
                    ebx = ebx.wrapping_sub(1);
                    if ebx != 0 { 0x223 } else { break }
                }
            }
            0x23E => {
                if edi & 3 == 0 {
                    0x25C
                } else {
                    0x246
                }
            }
            0x246 => {
                m.set_u8(edi, al(eax) as u8);
                edi = edi.wrapping_add(1);
                ecx = ecx.wrapping_sub(1);
                if ecx == 0 {
                    break;
                }
                if edi & 3 != 0 { 0x246 } else { 0x25C }
            }
            0x25C => {
                ebx = ecx;
                ecx >>= 2;
                if ecx != 0 { 0x2D7 } else { 0x263 }
            }
            0x263 => {
                m.set_u8(edi, al(eax) as u8);
                edi = edi.wrapping_add(1);
                0x268
            }
            0x268 => {
                ebx = ebx.wrapping_sub(1);
                if ebx != 0 { 0x263 } else { break }
            }
            0x275 => {
                m.set_u32(edi, edx);
                edi = edi.wrapping_add(4);
                ecx = ecx.wrapping_sub(1);
                if ecx == 0 { 0x21E } else { 0x27F }
            }
            0x27F => {
                edx = 0x7EFE_FEFF;
                eax = m.u32(esi);
                edx = edx.wrapping_add(eax);
                eax ^= 0xFFFF_FFFF;
                eax ^= edx;
                edx = m.u32(esi);
                esi = esi.wrapping_add(4);
                if eax & 0x8101_0100 == 0 {
                    0x275
                } else if edx & 0xFF == 0 {
                    edx = 0;
                    m.set_u32(edi, edx);
                    0x2CD
                } else if edx & 0xFF00 == 0 {
                    edx &= 0xFF;
                    m.set_u32(edi, edx);
                    0x2CD
                } else if edx & 0xFF_0000 == 0 {
                    edx &= 0xFFFF;
                    m.set_u32(edi, edx);
                    0x2CD
                } else if edx & 0xFF00_0000 != 0 {
                    0x275
                } else {
                    m.set_u32(edi, edx);
                    0x2CD
                }
            }
            0x2CD => {
                edi = edi.wrapping_add(4);
                eax = 0;
                ecx = ecx.wrapping_sub(1);
                if ecx == 0 { 0x2E3 } else { 0x2D7 }
            }
            0x2D7 => {
                eax = 0;
                m.set_u32(edi, eax);
                edi = edi.wrapping_add(4);
                ecx = ecx.wrapping_sub(1);
                if ecx != 0 { 0x2D7 } else { 0x2E3 }
            }
            0x2E3 => {
                ebx &= 3;
                if ebx != 0 { 0x263 } else { break }
            }
            _ => unreachable!("strncpy label {pc:#x}"),
        };
    }
    let _ = (ebx, esi, edi); // callee-saved: restored by the original's pops
    cpu.eax = dst;
    cpu.ecx = ecx;
    cpu.edx = edx;
    ret(m, cpu);
}

/// `_strstr(haystack, needle)` (0x012EE560), ported block by block.
pub fn strstr(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hay, needle) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let (mut eax, mut ecx, mut edx) = (cpu.eax, needle, cpu.edx);
    let (mut esi, mut edi) = (0u32, hay);
    let b0 = |r: u32| r & 0xFF;
    let result = 'run: {
        edx = set_low8(edx, m.u8(ecx) as u32);
        if edx & 0xFF == 0 {
            break 'run Some(edi); // 0x5E0: eax = haystack
        }
        edx = set_high8(edx, m.u8(ecx.wrapping_add(1)) as u32);
        if edx & 0xFF00 == 0 {
            // 0x5CD: eax = dl, then the strchr body (tail call)
            cpu.eax = edx & 0xFF;
            cpu.ecx = ecx;
            cpu.edx = edx;
            original(m, cpu, STRCHR_TAIL);
            return;
        }
        let (dl, dh) = (edx & 0xFF, (edx >> 8) & 0xFF);
        let mut pc = 0x578u32;
        loop {
            pc = match pc {
                0x578 => {
                    esi = edi;
                    ecx = needle;
                    eax = set_low8(eax, m.u8(edi) as u32);
                    esi = esi.wrapping_add(1);
                    if b0(eax) == dl {
                        0x59E
                    } else if b0(eax) == 0 {
                        break 'run None;
                    } else {
                        0x58B
                    }
                }
                0x58B => {
                    eax = set_low8(eax, m.u8(esi) as u32);
                    esi = esi.wrapping_add(1);
                    0x590
                }
                0x590 => {
                    if b0(eax) == dl {
                        0x59E
                    } else if b0(eax) != 0 {
                        0x58B
                    } else {
                        break 'run None;
                    }
                }
                0x59E => {
                    eax = set_low8(eax, m.u8(esi) as u32);
                    esi = esi.wrapping_add(1);
                    if b0(eax) != dh {
                        0x590
                    } else {
                        edi = esi.wrapping_sub(1);
                        0x5AA
                    }
                }
                0x5AA => {
                    eax = set_high8(eax, m.u8(ecx.wrapping_add(2)) as u32);
                    if eax & 0xFF00 == 0 {
                        break 'run Some(edi.wrapping_sub(1));
                    }
                    eax = set_low8(eax, m.u8(esi) as u32);
                    esi = esi.wrapping_add(2);
                    if b0(eax) != (eax >> 8) & 0xFF {
                        0x578
                    } else {
                        eax = set_low8(eax, m.u8(ecx.wrapping_add(3)) as u32);
                        if b0(eax) == 0 {
                            break 'run Some(edi.wrapping_sub(1));
                        }
                        eax = set_high8(eax, m.u8(esi.wrapping_sub(1)) as u32);
                        ecx = ecx.wrapping_add(2);
                        if b0(eax) == (eax >> 8) & 0xFF { 0x5AA } else { 0x578 }
                    }
                }
                _ => unreachable!("strstr label {pc:#x}"),
            };
        }
    };
    let _ = esi;
    cpu.eax = result.unwrap_or(0);
    cpu.ecx = ecx;
    cpu.edx = edx;
    ret(m, cpu);
}

/// `__stricmp(a, b)` (0x012EF5D7): with the locale unchanged and both
/// strings given, ___ascii_stricmp (0x012EF4DE): eax = the difference of
/// the lowered bytes, ecx = the last lowered byte of `b`, edx = past it.
pub fn stricmp(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (a, b) = (arg(m, cpu, 0), arg(m, cpu, 1));
    if m.u32(LOCALE_CHANGED) != 0 || a == 0 || b == 0 {
        return original(m, cpu, STRICMP);
    }
    let lower = |c: u32| if c.wrapping_sub(0x41) <= 0x19 { c + 0x20 } else { c };
    let (mut esi, mut edx) = (a, b);
    let (eax, ecx) = loop {
        let x = lower(m.u8(esi) as u32);
        esi = esi.wrapping_add(1);
        let y = lower(m.u8(edx) as u32);
        edx = edx.wrapping_add(1);
        if x == 0 || x != y {
            break (x, y);
        }
    };
    cpu.eax = eax.wrapping_sub(ecx);
    cpu.ecx = ecx;
    cpu.edx = edx;
    ret(m, cpu);
}

/// `__strnicmp(a, b, n)` (0x0130875A): with the locale unchanged, both
/// strings given and n <= 0x7FFFFFFF, ___ascii_strnicmp (0x01309AE0):
/// eax = ecx = -1, 0 or 1; dh = 0x20 when n != 0.
pub fn strnicmp(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (a, b, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    if m.u32(LOCALE_CHANGED) != 0 || a == 0 || b == 0 || n > 0x7FFF_FFFF {
        return original(m, cpu, STRNICMP);
    }
    let mut ecx = n;
    if ecx != 0 {
        cpu.edx = set_high8(cpu.edx, 0x20);
        let lower = |c: u32| if (0x41..=0x5A).contains(&c) { c + 0x20 } else { c };
        let (mut esi, mut edi) = (a, b);
        loop {
            let ah = m.u8(esi) as u32;
            let al = m.u8(edi) as u32;
            if ah == 0 || al == 0 {
                // 0x2B: equal only if both end here
                ecx = if ah == al { 0 } else if ah < al { 0xFFFF_FFFF } else { 1 };
                break;
            }
            esi = esi.wrapping_add(1);
            edi = edi.wrapping_add(1);
            let (x, y) = (lower(ah), lower(al));
            if x != y {
                ecx = if x < y { 0xFFFF_FFFF } else { 1 };
                break;
            }
            ecx -= 1;
            if ecx == 0 {
                break;
            }
        }
    }
    cpu.eax = ecx;
    cpu.ecx = ecx;
    ret(m, cpu);
}
