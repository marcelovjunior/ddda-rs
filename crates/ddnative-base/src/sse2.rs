//! Legacy SSE/SSE2, Intel SDM volume 2. Inline assembly fixes operand order
//! even for commutative operations with distinct NaN payloads.
//!
//! L22: lifted code runs with the guest's MXCSR live in the host MXCSR
//! (Cpu::mxcsr holds it outside lifted code), so these helpers execute under
//! it: rounding, FTZ/DAZ and the sticky exception flags are the hardware's.
//! The asm blocks are not `pure`, so a flag-setting operation is never
//! dropped or reordered against STMXCSR/LDMXCSR.
use std::arch::{asm, x86_64::__m128i};

/// Intel SDM SFENCE: order prior stores before subsequent stores.
/// Keep a compiler memory barrier and execute the actual host instruction;
/// guest Mem writes are synchronous, but this is not a concurrency model.
#[inline]
pub fn sfence() {
    unsafe { asm!("sfence", options(nostack, preserves_flags)); }
}

#[inline]
pub fn comisd(a: [u32; 4], b: [u32; 4]) -> super::Flags {
    let x = a[0] as u64 | ((a[1] as u64) << 32);
    let y = b[0] as u64 | ((b[1] as u64) << 32);
    let nan = |v: u64| v & 0x7ff0000000000000 == 0x7ff0000000000000 && v & 0xfffffffffffff != 0;
    // Integer ordering avoids disturbing host MXCSR even on signaling NaNs.
    let r = if nan(x) || nan(y) {
        3
    } else if x == y || (x << 1 == 0 && y << 1 == 0) {
        0
    } else {
        let less = if (x ^ y) >> 63 != 0 {
            x >> 63 != 0
        } else if x >> 63 != 0 {
            x > y
        } else {
            x < y
        };
        if less {
            2
        } else {
            1
        }
    };
    super::Flags::new(super::COMISS, 0, 0, r, 32)
}

macro_rules! binary {
    ($($op:ident),* $(,)?) => {$(
        pub fn $op(a:[u32;4],b:[u32;4])->[u32;4] {
            let mut a:__m128i=unsafe{std::mem::transmute(a)};
            let b:__m128i=unsafe{std::mem::transmute(b)};
            unsafe {
                asm!(concat!(stringify!($op), " {a}, {b}"),
                     a=inout(xmm_reg) a,b=in(xmm_reg) b,
                     options(nostack,preserves_flags));
                std::mem::transmute(a)
            }
        }
    )*};
}
binary!(
    addps, subps, mulps, divps, minps, maxps, sqrtps, addpd, subpd, mulpd, divpd, minpd, maxpd, sqrtpd,
    addsd, subsd, mulsd, divsd, minsd, maxsd, sqrtsd, cvtps2pd, cvtpd2ps, cvtss2sd, cvtsd2ss, cvtdq2ps,
    cvttps2dq, cvtps2dq, cvtdq2pd, cvttpd2dq, cvtpd2dq, unpcklps, unpckhps, unpcklpd, unpckhpd, paddb, paddw,
    paddd, paddq, psubb, psubw, psubd, psubq, pcmpeqb, pcmpeqw, pcmpeqd, pcmpgtb, pcmpgtw, pcmpgtd, pslld,
    psllw, psllq, psrld, psrlw, psrlq, psrad, psraw, pmullw, pmulhw, pmulhuw, pmuludq, pmaddwd, punpcklbw,
    punpcklwd, punpckldq, punpcklqdq, punpckhbw, punpckhwd, punpckhdq, punpckhqdq, packsswb, packssdw,
    packuswb, paddusb, paddusw, paddsb, paddsw, psubusb, psubusw, psubsb, psubsw, pmaxsw, pminsw,
    // L22: hardware approximations; the result is the executing CPU's (the
    // recorder's oracle takes the same host result, cases carry the CPU).
    rcpps, rsqrtps, rcpss, rsqrtss
);

macro_rules! byte_shift {
    ($($op:ident),* $(,)?) => {$(
        /// Whole-register byte shift by an immediate (no MXCSR involvement).
        pub fn $op<const IMM:u8>(a:[u32;4])->[u32;4] {
            let mut a:__m128i=unsafe{std::mem::transmute(a)};
            unsafe {
                asm!(concat!(stringify!($op), " {a}, {imm}"), a=inout(xmm_reg) a, imm=const IMM,
                     options(nostack,preserves_flags,pure,nomem));
                std::mem::transmute(a)
            }
        }
    )*};
}
byte_shift!(pslldq, psrldq);

macro_rules! immediate {
    ($($name:ident => $op:literal),* $(,)?) => {$(
        pub fn $name<const IMM:u8>(a:[u32;4],b:[u32;4])->[u32;4] {
            let mut a:__m128i=unsafe{std::mem::transmute(a)};
            let b:__m128i=unsafe{std::mem::transmute(b)};
            unsafe {
                asm!(concat!($op," {a}, {b}, {imm}"),
                     a=inout(xmm_reg) a,b=in(xmm_reg) b,imm=const IMM,
                     options(nostack,preserves_flags));
                std::mem::transmute(a)
            }
        }
    )*};
}
immediate!(compare_ps=>"cmpps",compare_ss=>"cmpss",compare_pd=>"cmppd",compare_sd=>"cmpsd",
           shuffle_ps=>"shufps",shuffle_pd=>"shufpd",shuffle_d=>"pshufd",
           shuffle_lw=>"pshuflw",shuffle_hw=>"pshufhw");

macro_rules! to_int {
    ($($op:ident),* $(,)?) => {$(
        pub fn $op(b:[u32;4])->u32 {
            let b:__m128i=unsafe{std::mem::transmute(b)};
            let result:u32;
            unsafe {asm!(concat!(stringify!($op)," {result:e}, {b}"),
                         result=out(reg) result,b=in(xmm_reg) b,
                         options(nostack,preserves_flags));}result
        }
    )*};
}
to_int!(cvtss2si, cvtsd2si, cvttsd2si);

#[inline]
pub fn cvtsi2sd(a: [u32; 4], b: u32) -> [u32; 4] {
    let mut a: __m128i = unsafe { std::mem::transmute(a) };
    unsafe {
        asm!("cvtsi2sd {a}, {b:e}",a=inout(xmm_reg) a,b=in(reg) b,options(nostack,preserves_flags));
        std::mem::transmute(a)
    }
}

/// MXCSR of a Windows thread at start (and the recorder's fixture value).
pub const MXCSR_DEFAULT: u32 = 0x1F80;

/// STMXCSR: the live MXCSR (the guest's while lifted code runs).
pub fn stmxcsr() -> u32 {
    let mut v = 0u32;
    unsafe { asm!("stmxcsr [{p}]", p = in(reg) &mut v, options(nostack, preserves_flags)); }
    v
}

/// LDMXCSR. Reserved bits would fault (#GP) and unmasked exceptions would
/// trap the host: both are unsupported, as the recorder rejects them.
pub fn ldmxcsr(v: u32) {
    assert_eq!(v >> 16, 0, "LDMXCSR with reserved bits {v:#x}");
    assert_eq!(v & 0x1F80, 0x1F80, "LDMXCSR with unmasked SSE exceptions {v:#x}");
    unsafe { asm!("ldmxcsr [{p}]", p = in(reg) &v, options(nostack, preserves_flags)); }
}

fn compare_flags(rflags: u64) -> super::Flags {
    // ZF PF CF: 111 unordered, 000 greater, 001 less, 100 equal
    let r = match (rflags & 0x40 != 0, rflags & 4 != 0, rflags & 1 != 0) {
        (true, true, true) => 3,
        (false, false, true) => 2,
        (true, false, false) => 0,
        _ => 1,
    };
    super::Flags::new(super::COMISS, 0, 0, r, 32)
}

/// COMISS (ORDERED) / UCOMISS on the host, under the live MXCSR: the
/// comparison and the exception flags (IE for any NaN or only an SNaN, DE)
/// are the hardware's.
pub fn comiss_hw<const ORDERED: bool>(a: u32, b: u32) -> super::Flags {
    let rflags: u64;
    unsafe {
        if ORDERED {
            asm!("movd {x}, {a:e}", "movd {y}, {b:e}", "comiss {x}, {y}", "pushfq", "pop {f}",
                 a = in(reg) a, b = in(reg) b, x = out(xmm_reg) _, y = out(xmm_reg) _, f = out(reg) rflags);
        } else {
            asm!("movd {x}, {a:e}", "movd {y}, {b:e}", "ucomiss {x}, {y}", "pushfq", "pop {f}",
                 a = in(reg) a, b = in(reg) b, x = out(xmm_reg) _, y = out(xmm_reg) _, f = out(reg) rflags);
        }
    }
    compare_flags(rflags)
}

/// COMISD (ORDERED) / UCOMISD on the host (low 64 bits of each operand).
pub fn comisd_hw<const ORDERED: bool>(a: [u32; 4], b: [u32; 4]) -> super::Flags {
    let (x, y): (__m128i, __m128i) = unsafe { (std::mem::transmute(a), std::mem::transmute(b)) };
    let rflags: u64;
    unsafe {
        if ORDERED {
            asm!("comisd {x}, {y}", "pushfq", "pop {f}", x = in(xmm_reg) x, y = in(xmm_reg) y, f = out(reg) rflags);
        } else {
            asm!("ucomisd {x}, {y}", "pushfq", "pop {f}", x = in(xmm_reg) x, y = in(xmm_reg) y, f = out(reg) rflags);
        }
    }
    compare_flags(rflags)
}
