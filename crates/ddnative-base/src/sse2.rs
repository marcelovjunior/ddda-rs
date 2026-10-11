//! Legacy SSE/SSE2, Intel SDM volume 2. Inline assembly fixes operand order
//! even for commutative operations with distinct NaN payloads. These helpers
//! use the recorder's masked, round-to-nearest MXCSR and restore host MXCSR.
use std::arch::{asm, x86_64::__m128i};

/// Intel SDM SFENCE: order prior stores before subsequent stores.
/// Keep a compiler memory barrier and execute the actual host instruction;
/// guest Mem writes are synchronous, but this is not a concurrency model.
pub fn sfence() {
    unsafe { asm!("sfence", options(nostack, preserves_flags)); }
}

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
            let mut old=0u32;let default=0x1f80u32;
            unsafe {
                asm!("stmxcsr [{old}]", "ldmxcsr [{default}]",
                     concat!(stringify!($op), " {a}, {b}"), "ldmxcsr [{old}]",
                     a=inout(xmm_reg) a,b=in(xmm_reg) b,
                     old=in(reg) &mut old,default=in(reg) &default,
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
    packuswb, paddusb, paddusw, paddsb, paddsw, psubusb, psubusw, psubsb, psubsw, pmaxsw, pminsw
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
            let mut old=0u32;let default=0x1f80u32;
            unsafe {
                asm!("stmxcsr [{old}]","ldmxcsr [{default}]",
                     concat!($op," {a}, {b}, {imm}"),"ldmxcsr [{old}]",
                     a=inout(xmm_reg) a,b=in(xmm_reg) b,imm=const IMM,
                     old=in(reg) &mut old,default=in(reg) &default,
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
            let mut old=0u32;let default=0x1f80u32;let result:u32;
            unsafe {asm!("stmxcsr [{old}]","ldmxcsr [{default}]",
                         concat!(stringify!($op)," {result:e}, {b}"),"ldmxcsr [{old}]",
                         result=out(reg) result,b=in(xmm_reg) b,
                         old=in(reg) &mut old,default=in(reg) &default,
                         options(nostack,preserves_flags));}result
        }
    )*};
}
to_int!(cvtss2si, cvtsd2si, cvttsd2si);

pub fn cvtsi2sd(a: [u32; 4], b: u32) -> [u32; 4] {
    let mut a: __m128i = unsafe { std::mem::transmute(a) };
    unsafe {
        asm!("cvtsi2sd {a}, {b:e}",a=inout(xmm_reg) a,b=in(reg) b,options(nostack,preserves_flags));
        std::mem::transmute(a)
    }
}
