//! Exact outer zlib inflater, including the guest ABI and stack spills.
//!
//! Unlike a host zlib object, all resumable state remains in guest memory.
//! `z_stream`: next_in_00, avail_in_04, total_in_08, next_out_0c,
//! avail_out_10, total_out_14, message_18, state_1c, allocator_20,
//! free_24, opaque_28, data_type_2c, checksum_30.
//! `inflate_state`: mode_00, last_04, wrap_08, dictionary_0c, flags_10,
//! checksum_14, total_18, window_bits_1c, window_size_20, window_have_24,
//! window_write_28, window_2c, hold_30, bits_34, length_38, distance_3c,
//! extra_40, literals_44, distances_48, literal_bits_4c, distance_bits_50.
//!
//! Control labels retain instruction addresses so every error path and ABI
//! spill is auditable. This is one native control-flow graph, without the
//! lifted body's seven outline dispatchers. Helpers retain runtime dispatch
//! so allocation, instrumentation and explicit overrides stay observable.
#![allow(unused_parens)]
use ddnative_base::{
    emit,
    mem::Mem,
    x86::{self, Cpu, Flags},
};

#[inline]
fn flags(kind: u8, a: u32, b: u32, r: u32, bits: u32) -> Flags {
    Flags {
        kind,
        a,
        b,
        r,
        bits,
        cf_in: false,
    }
}

#[inline]
fn alu_sub(f: &mut Flags, a: u32, b: u32, bits: u32) -> u32 {
    let mask = u32::MAX >> (32 - bits);
    let (a, b) = (a & mask, b & mask);
    let r = (a.wrapping_sub(b)) & mask;
    *f = flags(x86::SUB, a, b, r, bits);
    r
}

#[inline]
fn alu_and(f: &mut Flags, a: u32, b: u32, bits: u32) -> u32 {
    let mask = u32::MAX >> (32 - bits);
    let (a, b) = (a & mask, b & mask);
    let r = (a & b) & mask;
    *f = flags(x86::LOGIC, a, b, r, bits);
    r
}

#[inline]
fn alu_xor(f: &mut Flags, a: u32, b: u32, bits: u32) -> u32 {
    let mask = u32::MAX >> (32 - bits);
    let (a, b) = (a & mask, b & mask);
    let r = (a ^ b) & mask;
    *f = flags(x86::LOGIC, a, b, r, bits);
    r
}

// All branch-local flags here are integer operations. Keep the base helper
// for a callee's raw/partial flags rather than assuming their representation.
#[inline]
fn zero(f: &Flags) -> bool {
    match f.kind {
        x86::ADD
        | x86::SUB
        | x86::LOGIC
        | x86::INC
        | x86::DEC
        | x86::SHIFT
        | x86::SHL
        | x86::SHR
        | x86::SAR => f.r & (u32::MAX >> (32 - f.bits)) == 0,
        _ => f.zf(),
    }
}

/// Translated from 0x00cf02e0 (DDDA.exe 2017-12-13 Steam).
/// Whole function: zlib/raw/gzip headers, dictionaries, stored/fixed/dynamic
/// blocks, slow/fast decode, checksums, partial output and all return codes.
/// Callees not owned here use the existing runtime dispatcher unchanged.
pub fn inflate(m: &mut dyn Mem, cpu: &mut Cpu) {
    inflate_with(m, cpu, &mut |m, cpu, target, lifted| {
        if lifted {
            x86::call_lifted(m, cpu, target)
        } else {
            x86::call_ext(m, cpu, target)
        }
    });
}

/// Explicit callee boundary for original-instruction differential replay.
#[doc(hidden)]
pub fn inflate_with(
    m: &mut dyn Mem,
    cpu: &mut Cpu,
    call: &mut impl FnMut(&mut dyn Mem, &mut Cpu, u32, bool),
) {
    let pt = m.page_table();
    let mut pc = 0xCF02E0;
    loop {
        match pc {
            0xCF02E0 => {
                emit::push(cpu.ebp, m, pt, cpu);
                cpu.ebp = cpu.esp;
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.esp = cpu.esp.wrapping_sub(0x30u32);
                emit::push(cpu.edi, m, pt, cpu);
                alu_and(&mut cpu.fl, cpu.edx, cpu.edx, 32);
                pc = if zero(&cpu.fl) { 0xCF158E } else { 0xCF02F2 };
                continue;
            }
            0xCF02F2 => {
                cpu.edi = m.fast_u32(pt, cpu.edx.wrapping_add(0x1C));
                alu_and(&mut cpu.fl, cpu.edi, cpu.edi, 32);
                pc = if zero(&cpu.fl) { 0xCF158E } else { 0xCF02FD };
                continue;
            }
            0xCF02FD => {
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.edx.wrapping_add(0xC)), 0x0u32, 32);
                pc = if zero(&cpu.fl) { 0xCF158E } else { 0xCF0307 };
                continue;
            }
            0xCF0307 => {
                let condition = (m.fast_u32(pt, cpu.edx)) != (0x0u32);
                pc = if condition { 0xCF0316 } else { 0xCF030C };
                continue;
            }
            0xCF030C => {
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.edx.wrapping_add(0x4)), 0x0u32, 32);
                pc = if (!zero(&cpu.fl)) { 0xCF158E } else { 0xCF0316 };
                continue;
            }
            0xCF0316 => {
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.edi), 0xBu32, 32);
                pc = if (!zero(&cpu.fl)) { 0xCF0321 } else { 0xCF031B };
                continue;
            }
            0xCF031B => {
                m.fast_set_u32(pt, cpu.edi, 0xCu32);
                pc = 0xCF0321;
                continue;
            }
            0xCF0321 => {
                cpu.ecx = m.fast_u32(pt, cpu.edx.wrapping_add(0x10));
                cpu.eax = m.fast_u32(pt, cpu.edx.wrapping_add(0xC));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0), cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edx.wrapping_add(0x4));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), cpu.ecx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD0), cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                emit::push(cpu.ebx, m, pt, cpu);
                cpu.ebx = m.fast_u32(pt, cpu.edi.wrapping_add(0x30));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4), cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.edx);
                emit::push(cpu.esi, m, pt, cpu);
                cpu.esi = m.fast_u32(pt, cpu.edi.wrapping_add(0x34));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), 0x0u32);
                alu_sub(&mut cpu.fl, cpu.ecx, 0x1Cu32, 32);
                pc = if (cpu.fl.cf() || zero(&cpu.fl)) {
                    0xCF0366
                } else {
                    0xCF0357
                };
                continue;
            }
            0xCF0357 => {
                cpu.eax = 0xFFFFFFFEu32;
                pc = 0xCF035C;
                continue;
            }
            0xCF035C => {
                cpu.esi = emit::pop(m, pt, cpu);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF0363 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                pc = 0xCF0366;
                continue;
            }
            0xCF0366 => {
                let t = m.fast_u32(pt, cpu.ecx.wrapping_mul(4).wrapping_add(0xCF1598));
                if matches!(
                    t,
                    0xCF036D
                        | 0xCF0499
                        | 0xCF0525
                        | 0xCF0598
                        | 0xCF05F6
                        | 0xCF065D
                        | 0xCF06BD
                        | 0xCF0723
                        | 0xCF0789
                        | 0xCF0802
                        | 0xCF0869
                        | 0xCF0893
                        | 0xCF089D
                        | 0xCF08FD
                        | 0xCF090E
                        | 0xCF093B
                        | 0xCF094C
                        | 0xCF0967
                        | 0xCF09C6
                        | 0xCF0A22
                        | 0xCF0AA3
                        | 0xCF0B8F
                        | 0xCF0E6F
                        | 0xCF104A
                        | 0xCF1091
                        | 0xCF11DD
                        | 0xCF1249
                        | 0xCF12FC
                        | 0xCF1320
                        | 0xCF13E3
                        | 0xCF147E
                        | 0xCF1487
                        | 0xCF14DA
                ) {
                    pc = t;
                    continue;
                }
                call(m, cpu, t, false);
                return;
            }
            0xCF036D => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x8))) != (0x0u32);
                pc = if condition { 0xCF037E } else { 0xCF0373 };
                continue;
            }
            0xCF0373 => {
                m.fast_set_u32(pt, cpu.edi, 0xCu32);
                pc = 0xCF1437;
                continue;
            }
            0xCF037E => {
                let condition = (cpu.esi) >= (0x10u32);
                pc = if condition { 0xCF03A5 } else { 0xCF0383 };
                continue;
            }
            0xCF0383 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF038D };
                continue;
            }
            0xCF038D => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x10u32);
                pc = if condition { 0xCF0383 } else { 0xCF03A5 };
                continue;
            }
            0xCF03A5 => {
                cpu.eax = m.fast_u32(pt, cpu.edi.wrapping_add(0x8));
                let condition = (((((cpu.eax >> 0) & 0xFF) & 0xFF) & ((0x2u32) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF03ED } else { 0xCF03AC };
                continue;
            }
            0xCF03AC => {
                alu_sub(&mut cpu.fl, cpu.ebx, 0x8B1Fu32, 32);
                pc = if (!zero(&cpu.fl)) { 0xCF03ED } else { 0xCF03B4 };
                continue;
            }
            0xCF03B4 => {
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0xCF03BF, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                emit::push(0x2u32, m, pt, cpu);
                cpu.eax = cpu.ebp.wrapping_add(0xFFFFFFEC);
                m.fast_set_u16(pt, cpu.ebp.wrapping_add(0xFFFFFFEC), (cpu.ebx & 0xFFFF) as u16);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF03D5, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                cpu.esp = cpu.esp.wrapping_add(0x18u32);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0x1u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF03ED => {
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x10), 0x0u32);
                let condition = (((((cpu.eax >> 0) & 0xFF) & 0xFF) & ((0x1u32) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF0487 } else { 0xCF03FC };
                continue;
            }
            0xCF03FC => {
                cpu.eax = ((cpu.ebx >> 0) & 0xFF);
                cpu.eax = cpu.eax << 0x8u32;
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.eax = cpu.eax.wrapping_add(cpu.edx);
                cpu.edx = cpu.edx ^ cpu.edx;
                cpu.ecx = 0x1Fu32;
                {
                    let d = cpu.ecx;
                    let (q, r) = x86::divide(cpu.eax, cpu.edx, d, 32, false).expect("x86 divide error");
                    cpu.eax = q;
                    cpu.edx = r;
                }
                let condition = (((cpu.edx) & (cpu.edx)) as i32) != 0;
                pc = if condition { 0xCF0487 } else { 0xCF0416 };
                continue;
            }
            0xCF0416 => {
                cpu.edx = cpu.ebx;
                {
                    let r = ((cpu.edx >> 0) & 0xFF) & 0xFu32 & 0xFF;
                    cpu.edx = (cpu.edx & !(0xFF << 0)) | (((r) & 0xFF) << 0);
                }
                let condition = (((cpu.edx >> 0) & 0xFF) & 0xFF) == ((0x8u32) & 0xFF);
                pc = if condition { 0xCF0432 } else { 0xCF0420 };
                continue;
            }
            0xCF0420 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x18), 0x1403FCCu32);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                pc = 0xCF1431;
                continue;
            }
            0xCF0432 => {
                cpu.ebx = cpu.ebx >> 0x4u32;
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0xFu32;
                cpu.ecx = cpu.ecx.wrapping_add(0x8u32);
                cpu.esi = cpu.esi.wrapping_sub(0x4u32);
                alu_sub(
                    &mut cpu.fl,
                    cpu.ecx,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x1C)),
                    32,
                );
                pc = if (cpu.fl.cf() || zero(&cpu.fl)) {
                    0xCF0457
                } else {
                    0xCF0445
                };
                continue;
            }
            0xCF0445 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403FB8u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF0457 => {
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0xCF0462, m, pt, cpu);
                call(m, cpu, 0x00CF34D0, true);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.ebx = cpu.ebx >> 0x8u32;
                cpu.ebx = (!cpu.ebx) & u32::MAX;
                cpu.ebx = cpu.ebx & 0x2u32;
                cpu.ebx = cpu.ebx | 0x9u32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x30), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                m.fast_set_u32(pt, cpu.edi, cpu.ebx);
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                pc = 0xCF1437;
                continue;
            }
            0xCF0487 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403FA0u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF0499 => {
                let condition = (cpu.esi) >= (0x10u32);
                pc = if condition { 0xCF04C2 } else { 0xCF049E };
                continue;
            }
            0xCF049E => {
                pc = 0xCF04A0;
                continue;
            }
            0xCF04A0 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF04AA };
                continue;
            }
            0xCF04AA => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x10u32);
                pc = if condition { 0xCF04A0 } else { 0xCF04C2 };
                continue;
            }
            0xCF04C2 => {
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x10), cpu.ebx);
                let condition = (((cpu.ebx >> 0) & 0xFF) & 0xFF) == ((0x8u32) & 0xFF);
                pc = if condition { 0xCF04D9 } else { 0xCF04CA };
                continue;
            }
            0xCF04CA => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x18), 0x1403FCCu32);
                pc = 0xCF1431;
                continue;
            }
            0xCF04D9 => {
                let condition = (((cpu.ebx) & (0xE000u32)) as i32) == 0;
                pc = if condition { 0xCF04F0 } else { 0xCF04E1 };
                continue;
            }
            0xCF04E1 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403F84u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF04F0 => {
                let condition = (((cpu.ebx) & (0x200u32)) as i32) == 0;
                pc = if condition { 0xCF0519 } else { 0xCF04F8 };
                continue;
            }
            0xCF04F8 => {
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEC),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x8u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.fl = flags(x86::SHR, a, n, r, 32);
                    cpu.ebx = r;
                }
                emit::push(0x2u32, m, pt, cpu);
                cpu.eax = cpu.ebp.wrapping_add(0xFFFFFFEC);
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFED),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF0510, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF0519;
                continue;
            }
            0xCF0519 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0x2u32);
                pc = 0xCF0530;
                continue;
            }
            0xCF0525 => {
                let condition = (cpu.esi) >= (0x20u32);
                pc = if condition { 0xCF0552 } else { 0xCF052A };
                continue;
            }
            0xCF052A => {
                pc = 0xCF0530;
                continue;
            }
            0xCF0530 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF053A };
                continue;
            }
            0xCF053A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x20u32);
                pc = if condition { 0xCF0530 } else { 0xCF0552 };
                continue;
            }
            0xCF0552 => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x200u32)) as i32) == 0;
                pc = if condition { 0xCF058C } else { 0xCF055B };
                continue;
            }
            0xCF055B => {
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEC),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                cpu.eax = cpu.ebx;
                cpu.ecx = cpu.ebx;
                cpu.eax = cpu.eax >> 0x8u32;
                cpu.ecx = cpu.ecx >> 0x10u32;
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x18u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.fl = flags(x86::SHR, a, n, r, 32);
                    cpu.ebx = r;
                }
                emit::push(0x4u32, m, pt, cpu);
                cpu.edx = cpu.ebp.wrapping_add(0xFFFFFFEC);
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFED),
                    ((cpu.eax >> 0) & 0xFF) as u8,
                );
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEE),
                    ((cpu.ecx >> 0) & 0xFF) as u8,
                );
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEF),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                cpu.eax = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(0xCF0583, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF058C;
                continue;
            }
            0xCF058C => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0x3u32);
                pc = 0xCF05A0;
                continue;
            }
            0xCF0598 => {
                let condition = (cpu.esi) >= (0x10u32);
                pc = if condition { 0xCF05C2 } else { 0xCF059D };
                continue;
            }
            0xCF059D => {
                pc = 0xCF05A0;
                continue;
            }
            0xCF05A0 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF05AA };
                continue;
            }
            0xCF05AA => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x10u32);
                pc = if condition { 0xCF05A0 } else { 0xCF05C2 };
                continue;
            }
            0xCF05C2 => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x200u32)) as i32) == 0;
                pc = if condition { 0xCF05EC } else { 0xCF05CB };
                continue;
            }
            0xCF05CB => {
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEC),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x8u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.fl = flags(x86::SHR, a, n, r, 32);
                    cpu.ebx = r;
                }
                emit::push(0x2u32, m, pt, cpu);
                cpu.eax = cpu.ebp.wrapping_add(0xFFFFFFEC);
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFED),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF05E3, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF05EC;
                continue;
            }
            0xCF05EC => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0x4u32);
                pc = 0xCF05F6;
                continue;
            }
            0xCF05F6 => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x400u32)) as i32) == 0;
                pc = if condition { 0xCF0657 } else { 0xCF05FF };
                continue;
            }
            0xCF05FF => {
                let condition = (cpu.esi) >= (0x10u32);
                pc = if condition { 0xCF0626 } else { 0xCF0604 };
                continue;
            }
            0xCF0604 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF060E };
                continue;
            }
            0xCF060E => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x10u32);
                pc = if condition { 0xCF0604 } else { 0xCF0626 };
                continue;
            }
            0xCF0626 => {
                alu_and(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x200u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), cpu.ebx);
                pc = if zero(&cpu.fl) { 0xCF0653 } else { 0xCF0632 };
                continue;
            }
            0xCF0632 => {
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFEC),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x8u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.fl = flags(x86::SHR, a, n, r, 32);
                    cpu.ebx = r;
                }
                emit::push(0x2u32, m, pt, cpu);
                cpu.eax = cpu.ebp.wrapping_add(0xFFFFFFEC);
                m.fast_set_u8(
                    pt,
                    cpu.ebp.wrapping_add(0xFFFFFFED),
                    ((cpu.ebx >> 0) & 0xFF) as u8,
                );
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF064A, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF0653;
                continue;
            }
            0xCF0653 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                pc = 0xCF0657;
                continue;
            }
            0xCF0657 => {
                m.fast_set_u32(pt, cpu.edi, 0x5u32);
                pc = 0xCF065D;
                continue;
            }
            0xCF065D => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x400u32)) as i32) == 0;
                pc = if condition { 0xCF06B7 } else { 0xCF0666 };
                continue;
            }
            0xCF0666 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x38));
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF0678 } else { 0xCF0673 };
                continue;
            }
            0xCF0673 => {
                cpu.ecx = cpu.edx;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                pc = 0xCF0678;
                continue;
            }
            0xCF0678 => {
                let condition = (((cpu.ecx) & (cpu.ecx)) as i32) == 0;
                pc = if condition { 0xCF06AD } else { 0xCF067C };
                continue;
            }
            0xCF067C => {
                alu_and(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x200u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF06A2 } else { 0xCF0685 };
                continue;
            }
            0xCF0685 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF0696, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF06A2;
                continue;
            }
            0xCF06A2 => {
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))
                        .wrapping_sub(cpu.ecx);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.eax = cpu.eax.wrapping_add(cpu.ecx);
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x38)).wrapping_sub(cpu.ecx);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), r);
                }
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                pc = 0xCF06AD;
                continue;
            }
            0xCF06AD => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x38))) != (0x0u32);
                pc = if condition { 0xCF148E } else { 0xCF06B7 };
                continue;
            }
            0xCF06B7 => {
                m.fast_set_u32(pt, cpu.edi, 0x6u32);
                pc = 0xCF06BD;
                continue;
            }
            0xCF06BD => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x800u32)) as i32) == 0;
                pc = if condition { 0xCF071D } else { 0xCF06C6 };
                continue;
            }
            0xCF06C6 => {
                let condition = (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))) == (0x0u32);
                pc = if condition { 0xCF148E } else { 0xCF06D0 };
                continue;
            }
            0xCF06D0 => {
                cpu.ecx = cpu.ecx ^ cpu.ecx;
                pc = 0xCF06D2;
                continue;
            }
            0xCF06D2 => {
                cpu.edx = (m.fast_u8(pt, cpu.ecx.wrapping_add(cpu.eax.wrapping_mul(1))) as u32);
                cpu.ecx = cpu.ecx.wrapping_add(1);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.edx);
                let condition = (((cpu.edx) & (cpu.edx)) as i32) == 0;
                pc = if condition { 0xCF06E3 } else { 0xCF06DE };
                continue;
            }
            0xCF06DE => {
                let condition = (cpu.ecx) < (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)));
                pc = if condition { 0xCF06D2 } else { 0xCF06E3 };
                continue;
            }
            0xCF06E3 => {
                alu_and(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x2000u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                pc = if zero(&cpu.fl) { 0xCF070B } else { 0xCF06EF };
                continue;
            }
            0xCF06EF => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.edx = cpu.ecx;
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF06FF, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF070B;
                continue;
            }
            0xCF070B => {
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))
                        .wrapping_sub(cpu.ecx);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.eax = cpu.eax.wrapping_add(cpu.ecx);
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)),
                    0x0u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                pc = if (!zero(&cpu.fl)) { 0xCF148E } else { 0xCF071D };
                continue;
            }
            0xCF071D => {
                m.fast_set_u32(pt, cpu.edi, 0x7u32);
                pc = 0xCF0723;
                continue;
            }
            0xCF0723 => {
                let condition = (((m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) & (0x1000u32)) as i32) == 0;
                pc = if condition { 0xCF0783 } else { 0xCF072C };
                continue;
            }
            0xCF072C => {
                let condition = (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))) == (0x0u32);
                pc = if condition { 0xCF148E } else { 0xCF0736 };
                continue;
            }
            0xCF0736 => {
                cpu.ecx = cpu.ecx ^ cpu.ecx;
                pc = 0xCF0738;
                continue;
            }
            0xCF0738 => {
                cpu.edx = (m.fast_u8(pt, cpu.ecx.wrapping_add(cpu.eax.wrapping_mul(1))) as u32);
                cpu.ecx = cpu.ecx.wrapping_add(1);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.edx);
                let condition = (((cpu.edx) & (cpu.edx)) as i32) == 0;
                pc = if condition { 0xCF0749 } else { 0xCF0744 };
                continue;
            }
            0xCF0744 => {
                let condition = (cpu.ecx) < (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)));
                pc = if condition { 0xCF0738 } else { 0xCF0749 };
                continue;
            }
            0xCF0749 => {
                alu_and(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x2000u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                pc = if zero(&cpu.fl) { 0xCF0771 } else { 0xCF0755 };
                continue;
            }
            0xCF0755 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.edx = cpu.ecx;
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF0765, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF0771;
                continue;
            }
            0xCF0771 => {
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))
                        .wrapping_sub(cpu.ecx);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.eax = cpu.eax.wrapping_add(cpu.ecx);
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)),
                    0x0u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                pc = if (!zero(&cpu.fl)) { 0xCF148E } else { 0xCF0783 };
                continue;
            }
            0xCF0783 => {
                m.fast_set_u32(pt, cpu.edi, 0x8u32);
                pc = 0xCF0789;
                continue;
            }
            0xCF0789 => {
                alu_and(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x200u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF07DD } else { 0xCF0792 };
                continue;
            }
            0xCF0792 => {
                let condition = (cpu.esi) >= (0x10u32);
                pc = if condition { 0xCF07C2 } else { 0xCF0797 };
                continue;
            }
            0xCF0797 => {
                pc = 0xCF07A0;
                continue;
            }
            0xCF07A0 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF07AA };
                continue;
            }
            0xCF07AA => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x10u32);
                pc = if condition { 0xCF07A0 } else { 0xCF07C2 };
                continue;
            }
            0xCF07C2 => {
                cpu.ecx = (m.fast_u16(pt, cpu.edi.wrapping_add(0x14)) as u32);
                let condition = (cpu.ebx) == (cpu.ecx);
                pc = if condition { 0xCF07D9 } else { 0xCF07CA };
                continue;
            }
            0xCF07CA => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403F70u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF07D9 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = alu_xor(&mut cpu.fl, cpu.esi, cpu.esi, 32);
                pc = 0xCF07DD;
                continue;
            }
            0xCF07DD => {
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0xCF07E8, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x30), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                m.fast_set_u32(pt, cpu.edi, 0xBu32);
                pc = 0xCF1437;
                continue;
            }
            0xCF0802 => {
                let condition = (cpu.esi) >= (0x20u32);
                pc = if condition { 0xCF0832 } else { 0xCF0807 };
                continue;
            }
            0xCF0807 => {
                pc = 0xCF0810;
                continue;
            }
            0xCF0810 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF081A };
                continue;
            }
            0xCF081A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x20u32);
                pc = if condition { 0xCF0810 } else { 0xCF0832 };
                continue;
            }
            0xCF0832 => {
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0xFF00u32;
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx << 0x10u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.ecx = cpu.ecx << 0x8u32;
                cpu.edx = cpu.edx & 0xFF00u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.ebx = cpu.ebx >> 0x18u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.ebx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.ecx);
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x30), cpu.ecx);
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0xAu32);
                pc = 0xCF0869;
                continue;
            }
            0xCF0869 => {
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.edi.wrapping_add(0xC)), 0x0u32, 32);
                pc = if zero(&cpu.fl) { 0xCF144E } else { 0xCF0873 };
                continue;
            }
            0xCF0873 => {
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0xCF087E, m, pt, cpu);
                call(m, cpu, 0x00CF34D0, true);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x30), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                m.fast_set_u32(pt, cpu.edi, 0xBu32);
                pc = 0xCF0893;
                continue;
            }
            0xCF0893 => {
                let condition = (m.fast_u32(pt, cpu.ebp.wrapping_add(0xC))) == (0x5u32);
                pc = if condition { 0xCF148E } else { 0xCF089D };
                continue;
            }
            0xCF089D => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x4))) == (0x0u32);
                pc = if condition { 0xCF08B7 } else { 0xCF08A3 };
                continue;
            }
            0xCF08A3 => {
                cpu.ecx = cpu.esi;
                cpu.ecx = cpu.ecx & 0x7u32;
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                m.fast_set_u32(pt, cpu.edi, 0x18u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF08B7 => {
                let condition = (cpu.esi) >= (0x3u32);
                pc = if condition { 0xCF08E2 } else { 0xCF08BC };
                continue;
            }
            0xCF08BC => {
                pc = 0xCF08C0;
                continue;
            }
            0xCF08C0 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF08CA };
                continue;
            }
            0xCF08CA => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x3u32);
                pc = if condition { 0xCF08C0 } else { 0xCF08E2 };
                continue;
            }
            0xCF08E2 => {
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0x1u32;
                cpu.ebx = cpu.ebx >> 0x1u32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x4), cpu.ecx);
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0x3u32;
                alu_sub(&mut cpu.fl, cpu.ecx, 0x3u32, 32);
                pc = if (!cpu.fl.cf() && !zero(&cpu.fl)) {
                    0xCF095C
                } else {
                    0xCF08F6
                };
                continue;
            }
            0xCF08F6 => {
                let t = m.fast_u32(pt, cpu.ecx.wrapping_mul(4).wrapping_add(0xCF160C));
                if matches!(t, 0xCF08FD | 0xCF090E | 0xCF093B | 0xCF094C) {
                    pc = t;
                    continue;
                }
                call(m, cpu, t, false);
                return;
            }
            0xCF08FD => {
                cpu.ebx = cpu.ebx >> 0x2u32;
                m.fast_set_u32(pt, cpu.edi, 0xDu32);
                cpu.esi = cpu.esi.wrapping_sub(0x3u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF090E => {
                cpu.ebx = cpu.ebx >> 0x2u32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x44), 0x1403580u32);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x4C), 0x9u32);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x48), 0x1403D80u32);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x50), 0x5u32);
                m.fast_set_u32(pt, cpu.edi, 0x12u32);
                cpu.esi = cpu.esi.wrapping_sub(0x3u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF093B => {
                cpu.ebx = cpu.ebx >> 0x2u32;
                m.fast_set_u32(pt, cpu.edi, 0xFu32);
                cpu.esi = cpu.esi.wrapping_sub(0x3u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF094C => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403F5Cu32);
                m.fast_set_u32(pt, cpu.edi, 0x1Bu32);
                pc = 0xCF095C;
                continue;
            }
            0xCF095C => {
                cpu.ebx = cpu.ebx >> 0x2u32;
                cpu.esi = cpu.esi.wrapping_sub(0x3u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF0967 => {
                cpu.ecx = cpu.esi;
                cpu.ecx = cpu.ecx & 0x7u32;
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                let condition = (cpu.esi) >= (0x20u32);
                pc = if condition { 0xCF0997 } else { 0xCF0975 };
                continue;
            }
            0xCF0975 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF097F };
                continue;
            }
            0xCF097F => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x20u32);
                pc = if condition { 0xCF0975 } else { 0xCF0997 };
                continue;
            }
            0xCF0997 => {
                cpu.edx = cpu.ebx;
                cpu.ecx = cpu.ebx;
                cpu.edx = (!cpu.edx) & u32::MAX;
                cpu.ecx = cpu.ecx & 0xFFFFu32;
                cpu.edx = cpu.edx >> 0x10u32;
                let condition = (cpu.ecx) == (cpu.edx);
                pc = if condition { 0xCF09B9 } else { 0xCF09AA };
                continue;
            }
            0xCF09AA => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x18), 0x1403F3Cu32);
                pc = 0xCF1431;
                continue;
            }
            0xCF09B9 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), cpu.ecx);
                cpu.esi = cpu.esi ^ cpu.esi;
                m.fast_set_u32(pt, cpu.edi, 0xEu32);
                pc = 0xCF09C6;
                continue;
            }
            0xCF09C6 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x38));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                let condition = (((cpu.ecx) & (cpu.ecx)) as i32) == 0;
                pc = if condition { 0xCF101C } else { 0xCF09D4 };
                continue;
            }
            0xCF09D4 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC));
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF09E0 } else { 0xCF09DB };
                continue;
            }
            0xCF09DB => {
                cpu.ecx = cpu.edx;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                pc = 0xCF09E0;
                continue;
            }
            0xCF09E0 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF09EC } else { 0xCF09E7 };
                continue;
            }
            0xCF09E7 => {
                cpu.ecx = cpu.edx;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                pc = 0xCF09EC;
                continue;
            }
            0xCF09EC => {
                alu_and(&mut cpu.fl, cpu.ecx, cpu.ecx, 32);
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF09F4 };
                continue;
            }
            0xCF09F4 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF0A05, m, pt, cpu);
                call(m, cpu, 0x012EFEA0, false);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))
                        .wrapping_sub(cpu.eax);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8))
                        .wrapping_add(cpu.eax);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), r);
                }
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0))
                        .wrapping_sub(cpu.eax);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0), r);
                }
                {
                    let r = m
                        .fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8))
                        .wrapping_add(cpu.eax);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8), r);
                }
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x38)).wrapping_sub(cpu.eax);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), r);
                }
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                pc = 0xCF1437;
                continue;
            }
            0xCF0A22 => {
                let condition = (cpu.esi) >= (0xEu32);
                pc = if condition { 0xCF0A52 } else { 0xCF0A27 };
                continue;
            }
            0xCF0A27 => {
                pc = 0xCF0A30;
                continue;
            }
            0xCF0A30 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0A3A };
                continue;
            }
            0xCF0A3A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0xEu32);
                pc = if condition { 0xCF0A30 } else { 0xCF0A52 };
                continue;
            }
            0xCF0A52 => {
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0x1Fu32;
                cpu.ebx = cpu.ebx >> 0x5u32;
                cpu.ecx = cpu.ecx.wrapping_add(0x101u32);
                cpu.edx = cpu.ebx;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x58), cpu.ecx);
                cpu.ebx = cpu.ebx >> 0x5u32;
                cpu.ecx = cpu.ebx;
                cpu.edx = cpu.edx & 0x1Fu32;
                cpu.ecx = cpu.ecx & 0xFu32;
                cpu.edx = cpu.edx.wrapping_add(1);
                cpu.ecx = cpu.ecx.wrapping_add(0x4u32);
                cpu.ebx = cpu.ebx >> 0x4u32;
                cpu.esi = cpu.esi.wrapping_sub(0xEu32);
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x58)),
                    0x11Eu32,
                    32,
                );
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x5C), cpu.edx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x54), cpu.ecx);
                pc = if (!cpu.fl.cf() && !zero(&cpu.fl)) {
                    0xCF0B73
                } else {
                    0xCF0A8D
                };
                continue;
            }
            0xCF0A8D => {
                let condition = (cpu.edx) > (0x1Eu32);
                pc = if condition { 0xCF0B73 } else { 0xCF0A96 };
                continue;
            }
            0xCF0A96 => {
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), 0x0u32);
                m.fast_set_u32(pt, cpu.edi, 0x10u32);
                pc = 0xCF0AA3;
                continue;
            }
            0xCF0AA3 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                let condition = (cpu.ecx) >= (m.fast_u32(pt, cpu.edi.wrapping_add(0x54)));
                pc = if condition { 0xCF0AFD } else { 0xCF0AAB };
                continue;
            }
            0xCF0AAB => {
                pc = 0xCF0AB0;
                continue;
            }
            0xCF0AB0 => {
                let condition = (cpu.esi) >= (0x3u32);
                pc = if condition { 0xCF0AD7 } else { 0xCF0AB5 };
                continue;
            }
            0xCF0AB5 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0ABF };
                continue;
            }
            0xCF0ABF => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x3u32);
                pc = if condition { 0xCF0AB5 } else { 0xCF0AD7 };
                continue;
            }
            0xCF0AD7 => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                cpu.edx = (m.fast_u16(pt, cpu.edx.wrapping_mul(2).wrapping_add(0x1403E00)) as u32);
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0x7u32;
                m.fast_set_u16(
                    pt,
                    cpu.edi.wrapping_add(cpu.edx.wrapping_mul(2)).wrapping_add(0x68),
                    (cpu.ecx & 0xFFFF) as u16,
                );
                {
                    let a = m.fast_u32(pt, cpu.edi.wrapping_add(0x60)) & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), r);
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                cpu.ebx = cpu.ebx >> 0x3u32;
                cpu.esi = cpu.esi.wrapping_sub(0x3u32);
                let condition = (cpu.ecx) < (m.fast_u32(pt, cpu.edi.wrapping_add(0x54)));
                pc = if condition { 0xCF0AB0 } else { 0xCF0AFD };
                continue;
            }
            0xCF0AFD => {
                cpu.eax = 0x13u32;
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x60)),
                    cpu.eax,
                    32,
                );
                pc = if (!cpu.fl.cf()) { 0xCF0B2A } else { 0xCF0B07 };
                continue;
            }
            0xCF0B07 => {
                pc = 0xCF0B10;
                continue;
            }
            0xCF0B10 => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                cpu.ecx = (m.fast_u16(pt, cpu.edx.wrapping_mul(2).wrapping_add(0x1403E00)) as u32);
                cpu.edx = alu_xor(&mut cpu.fl, cpu.edx, cpu.edx, 32);
                m.fast_set_u16(
                    pt,
                    cpu.edi.wrapping_add(cpu.ecx.wrapping_mul(2)).wrapping_add(0x68),
                    (cpu.edx & 0xFFFF) as u16,
                );
                {
                    let a = m.fast_u32(pt, cpu.edi.wrapping_add(0x60)) & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), r);
                }
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x60)),
                    cpu.eax,
                    32,
                );
                pc = if cpu.fl.cf() { 0xCF0B10 } else { 0xCF0B2A };
                continue;
            }
            0xCF0B2A => {
                cpu.eax = cpu.edi.wrapping_add(0x528);
                cpu.ecx = cpu.edi.wrapping_add(0x64);
                m.fast_set_u32(pt, cpu.ecx, cpu.eax);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x44), cpu.eax);
                cpu.edx = cpu.edi.wrapping_add(0x2E8);
                emit::push(cpu.edx, m, pt, cpu);
                cpu.eax = cpu.edi.wrapping_add(0x4C);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                m.fast_set_u32(pt, cpu.eax, 0x7u32);
                emit::push(0x13u32, m, pt, cpu);
                cpu.eax = cpu.edi.wrapping_add(0x68);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(0x0u32, m, pt, cpu);
                emit::push(0xCF0B57, m, pt, cpu);
                call(m, cpu, 0x00CF30C0, true);
                cpu.esp = cpu.esp.wrapping_add(0x18u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), cpu.eax);
                alu_and(&mut cpu.fl, cpu.eax, cpu.eax, 32);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                pc = if zero(&cpu.fl) { 0xCF0B82 } else { 0xCF0B64 };
                continue;
            }
            0xCF0B64 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x18), 0x1403F20u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF0B73 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403EFCu32);
                pc = 0xCF1431;
                continue;
            }
            0xCF0B82 => {
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), 0x0u32);
                m.fast_set_u32(pt, cpu.edi, 0x11u32);
                pc = 0xCF0B8F;
                continue;
            }
            0xCF0B8F => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x5C));
                cpu.edx = cpu.edx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x58)));
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x60)),
                    cpu.edx,
                    32,
                );
                pc = if (!cpu.fl.cf()) { 0xCF0DD1 } else { 0xCF0B9E };
                continue;
            }
            0xCF0B9E => {
                pc = 0xCF0BA0;
                continue;
            }
            0xCF0BA0 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x4C));
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.edx = cpu.ecx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.edx = ((cpu.edx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                let condition = (cpu.edx) <= (cpu.esi);
                pc = if condition { 0xCF0C01 } else { 0xCF0BC2 };
                continue;
            }
            0xCF0BC2 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0BCC };
                continue;
            }
            0xCF0BCC => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x4C));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.edx = cpu.ecx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.edx = ((cpu.edx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                let condition = (cpu.edx) > (cpu.esi);
                pc = if condition { 0xCF0BC2 } else { 0xCF0C01 };
                continue;
            }
            0xCF0C01 => {
                cpu.edx = cpu.ecx;
                cpu.edx = cpu.edx >> 0x10u32;
                let condition = (cpu.edx) >= (0x10u32);
                pc = if condition { 0xCF0C62 } else { 0xCF0C0B };
                continue;
            }
            0xCF0C0B => {
                cpu.edx = cpu.ecx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.edx = ((cpu.edx >> 0) & 0xFF);
                let condition = (cpu.esi) >= (cpu.edx);
                pc = if condition { 0xCF0C47 } else { 0xCF0C17 };
                continue;
            }
            0xCF0C17 => {
                pc = 0xCF0C20;
                continue;
            }
            0xCF0C20 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0C2A };
                continue;
            }
            0xCF0C2A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = ((cpu.ecx >> 8) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                let condition = (cpu.esi) < (cpu.edx);
                pc = if condition { 0xCF0C20 } else { 0xCF0C47 };
                continue;
            }
            0xCF0C47 => {
                cpu.edx = (cpu.edx & !0xFFFF)
                    | ((m.fast_u16(pt, cpu.ebp.wrapping_add(0xFFFFFFF6)) as u32) & 0xFFFF);
                cpu.ecx = ((cpu.ecx >> 8) & 0xFF);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = alu_sub(&mut cpu.fl, cpu.esi, cpu.ecx, 32);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                m.fast_set_u16(
                    pt,
                    cpu.edi.wrapping_add(cpu.ecx.wrapping_mul(2)).wrapping_add(0x68),
                    (cpu.edx & 0xFFFF) as u16,
                );
                {
                    let a = m.fast_u32(pt, cpu.edi.wrapping_add(0x60)) & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), r);
                }
                pc = 0xCF0DA4;
                continue;
            }
            0xCF0C62 => {
                cpu.edx = (cpu.edx & !0xFFFF)
                    | ((m.fast_u16(pt, cpu.ebp.wrapping_add(0xFFFFFFF6)) as u32) & 0xFFFF);
                cpu.ecx = ((cpu.ecx >> 8) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.ecx);
                let condition = ((cpu.edx & 0xFFFF) & 0xFFFF) != ((0x10u32) & 0xFFFF);
                pc = if condition { 0xCF0CD4 } else { 0xCF0C72 };
                continue;
            }
            0xCF0C72 => {
                cpu.edx = cpu.ecx.wrapping_add(0x2);
                let condition = (cpu.esi) >= (cpu.edx);
                pc = if condition { 0xCF0CA7 } else { 0xCF0C79 };
                continue;
            }
            0xCF0C79 => {
                pc = 0xCF0C80;
                continue;
            }
            0xCF0C80 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0C8A };
                continue;
            }
            0xCF0C8A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ecx.wrapping_add(0x2);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                let condition = (cpu.esi) < (cpu.edx);
                pc = if condition { 0xCF0C80 } else { 0xCF0CA7 };
                continue;
            }
            0xCF0CA7 => {
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                alu_and(&mut cpu.fl, cpu.ecx, cpu.ecx, 32);
                pc = if zero(&cpu.fl) { 0xCF0DB5 } else { 0xCF0CB6 };
                continue;
            }
            0xCF0CB6 => {
                cpu.ecx = (m.fast_u16(
                    pt,
                    cpu.edi.wrapping_add(cpu.ecx.wrapping_mul(2)).wrapping_add(0x66),
                ) as u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.ecx);
                cpu.ecx = cpu.ebx;
                cpu.ecx = cpu.ecx & 0x3u32;
                cpu.ecx = cpu.ecx.wrapping_add(0x3u32);
                cpu.ebx = cpu.ebx >> 0x2u32;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.esi = cpu.esi.wrapping_sub(0x2u32);
                pc = 0xCF0D70;
                continue;
            }
            0xCF0CD4 => {
                let condition = ((cpu.edx & 0xFFFF) & 0xFFFF) != ((0x11u32) & 0xFFFF);
                pc = if condition { 0xCF0D1F } else { 0xCF0CDA };
                continue;
            }
            0xCF0CDA => {
                cpu.edx = cpu.ecx.wrapping_add(0x3);
                let condition = (cpu.esi) >= (cpu.edx);
                pc = if condition { 0xCF0D08 } else { 0xCF0CE1 };
                continue;
            }
            0xCF0CE1 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0CEB };
                continue;
            }
            0xCF0CEB => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ecx.wrapping_add(0x3);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                let condition = (cpu.esi) < (cpu.edx);
                pc = if condition { 0xCF0CE1 } else { 0xCF0D08 };
                continue;
            }
            0xCF0D08 => {
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx & 0x7u32;
                cpu.edx = cpu.edx.wrapping_add(0x3u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x3u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.ebx = r;
                }
                cpu.edx = 0xFFFFFFFDu32;
                pc = 0xCF0D62;
                continue;
            }
            0xCF0D1F => {
                cpu.edx = cpu.ecx.wrapping_add(0x7);
                let condition = (cpu.esi) >= (cpu.edx);
                pc = if condition { 0xCF0D4D } else { 0xCF0D26 };
                continue;
            }
            0xCF0D26 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0D30 };
                continue;
            }
            0xCF0D30 => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ecx.wrapping_add(0x7);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                let condition = (cpu.esi) < (cpu.edx);
                pc = if condition { 0xCF0D26 } else { 0xCF0D4D };
                continue;
            }
            0xCF0D4D => {
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx & 0x7Fu32;
                cpu.edx = cpu.edx.wrapping_add(0xBu32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = 0x7u32;
                    let r = (a >> n) & u32::MAX;
                    cpu.ebx = r;
                }
                cpu.edx = 0xFFFFFFF9u32;
                pc = 0xCF0D62;
                continue;
            }
            0xCF0D62 => {
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.esi = cpu.esi.wrapping_add(cpu.edx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), 0x0u32);
                pc = 0xCF0D70;
                continue;
            }
            0xCF0D70 => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x5C));
                cpu.ecx = cpu.ecx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x58)));
                alu_sub(&mut cpu.fl, cpu.edx, cpu.ecx, 32);
                pc = if (!cpu.fl.cf() && !zero(&cpu.fl)) {
                    0xCF0DC1
                } else {
                    0xCF0D7F
                };
                continue;
            }
            0xCF0D7F => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF0DA4 } else { 0xCF0D85 };
                continue;
            }
            0xCF0D85 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0));
                pc = 0xCF0D90;
                continue;
            }
            0xCF0D90 => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x60));
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    cpu.fl.inc_dec(x86::DEC, a, r, 32);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), r);
                }
                m.fast_set_u16(
                    pt,
                    cpu.edi.wrapping_add(cpu.edx.wrapping_mul(2)).wrapping_add(0x68),
                    (cpu.ecx & 0xFFFF) as u16,
                );
                {
                    let a = m.fast_u32(pt, cpu.edi.wrapping_add(0x60)) & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x60), r);
                }
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4)),
                    0x0u32,
                    32,
                );
                pc = if (!zero(&cpu.fl)) { 0xCF0D90 } else { 0xCF0DA4 };
                continue;
            }
            0xCF0DA4 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x5C));
                cpu.ecx = cpu.ecx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x58)));
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x60)),
                    cpu.ecx,
                    32,
                );
                pc = if cpu.fl.cf() { 0xCF0BA0 } else { 0xCF0DB3 };
                continue;
            }
            0xCF0DB3 => {
                pc = 0xCF0DD1;
                continue;
            }
            0xCF0DB5 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403EE0u32);
                pc = 0xCF0DCB;
                continue;
            }
            0xCF0DC1 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x18), 0x1403EE0u32);
                pc = 0xCF0DCB;
                continue;
            }
            0xCF0DCB => {
                m.fast_set_u32(pt, cpu.edi, 0x1Bu32);
                pc = 0xCF0DD1;
                continue;
            }
            0xCF0DD1 => {
                cpu.eax = cpu.edi.wrapping_add(0x528);
                cpu.ecx = cpu.edi.wrapping_add(0x64);
                m.fast_set_u32(pt, cpu.ecx, cpu.eax);
                cpu.edx = cpu.edi.wrapping_add(0x2E8);
                emit::push(cpu.edx, m, pt, cpu);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x44), cpu.eax);
                cpu.eax = cpu.edi.wrapping_add(0x4C);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x58));
                emit::push(cpu.ecx, m, pt, cpu);
                cpu.edx = cpu.edi.wrapping_add(0x68);
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(0x1u32, m, pt, cpu);
                m.fast_set_u32(pt, cpu.eax, 0x9u32);
                emit::push(0xCF0E00, m, pt, cpu);
                call(m, cpu, 0x00CF30C0, true);
                cpu.esp = cpu.esp.wrapping_add(0x18u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), cpu.eax);
                alu_and(&mut cpu.fl, cpu.eax, cpu.eax, 32);
                pc = if zero(&cpu.fl) { 0xCF0E1C } else { 0xCF0E0A };
                continue;
            }
            0xCF0E0A => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x18), 0x1403EC4u32);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                pc = 0xCF1431;
                continue;
            }
            0xCF0E1C => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x64));
                cpu.ecx = cpu.edi.wrapping_add(0x64);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x48), cpu.edx);
                cpu.edx = cpu.edi.wrapping_add(0x2E8);
                emit::push(cpu.edx, m, pt, cpu);
                cpu.eax = cpu.edi.wrapping_add(0x50);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x58));
                m.fast_set_u32(pt, cpu.eax, 0x6u32);
                cpu.eax = m.fast_u32(pt, cpu.edi.wrapping_add(0x5C));
                emit::push(cpu.eax, m, pt, cpu);
                cpu.edx = cpu.edi.wrapping_add(cpu.ecx.wrapping_mul(2)).wrapping_add(0x68);
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(0x2u32, m, pt, cpu);
                emit::push(0xCF0E4A, m, pt, cpu);
                call(m, cpu, 0x00CF30C0, true);
                cpu.esp = cpu.esp.wrapping_add(0x18u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), cpu.eax);
                let condition = (((cpu.eax) & (cpu.eax)) as i32) == 0;
                pc = if condition { 0xCF0E66 } else { 0xCF0E54 };
                continue;
            }
            0xCF0E54 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x18), 0x1403EACu32);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                pc = 0xCF1431;
                continue;
            }
            0xCF0E66 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                m.fast_set_u32(pt, cpu.edi, 0x12u32);
                pc = 0xCF0E6F;
                continue;
            }
            0xCF0E6F => {
                let condition = (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC))) < (0x6u32);
                pc = if condition { 0xCF0ED2 } else { 0xCF0E75 };
                continue;
            }
            0xCF0E75 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)),
                    0x102u32,
                    32,
                );
                pc = if cpu.fl.cf() { 0xCF0ED2 } else { 0xCF0E7E };
                continue;
            }
            0xCF0E7E => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0xC), cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                m.fast_set_u32(pt, cpu.eax, cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x10), cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC));
                emit::push(cpu.ecx, m, pt, cpu);
                m.fast_set_u32(pt, cpu.eax.wrapping_add(0x4), cpu.edx);
                emit::push(cpu.eax, m, pt, cpu);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x30), cpu.ebx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x34), cpu.esi);
                emit::push(0xCF0EA8, m, pt, cpu);
                call(m, cpu, 0x00CF2CB0, true);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.edx = m.fast_u32(pt, cpu.eax.wrapping_add(0xC));
                cpu.ecx = m.fast_u32(pt, cpu.eax.wrapping_add(0x10));
                cpu.ebx = m.fast_u32(pt, cpu.edi.wrapping_add(0x30));
                cpu.esi = m.fast_u32(pt, cpu.edi.wrapping_add(0x34));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8), cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.eax.wrapping_add(0x4));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), cpu.eax);
                cpu.esp = cpu.esp.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0), cpu.ecx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.edx);
                cpu.eax = cpu.edx;
                pc = 0xCF1437;
                continue;
            }
            0xCF0ED2 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x4C));
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.edx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                let condition = (cpu.ecx) <= (cpu.esi);
                pc = if condition { 0xCF0F33 } else { 0xCF0EF4 };
                continue;
            }
            0xCF0EF4 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0EFE };
                continue;
            }
            0xCF0EFE => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x4C));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.edx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                let condition = (cpu.ecx) > (cpu.esi);
                pc = if condition { 0xCF0EF4 } else { 0xCF0F33 };
                continue;
            }
            0xCF0F33 => {
                let condition =
                    (((((cpu.edx >> 0) & 0xFF) & 0xFF) & (((cpu.edx >> 0) & 0xFF) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF0FF1 } else { 0xCF0F3B };
                continue;
            }
            0xCF0F3B => {
                let condition = (((((cpu.edx >> 0) & 0xFF) & 0xFF) & ((0xF0u32) & 0xFF)) as i8) != 0;
                pc = if condition { 0xCF0FF1 } else { 0xCF0F44 };
                continue;
            }
            0xCF0F44 => {
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.ecx);
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.ecx);
                cpu.ecx = ((cpu.edx >> 0) & 0xFF);
                cpu.ecx = cpu
                    .ecx
                    .wrapping_add(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0));
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.ecx = cpu.ecx >> 0x10u32;
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.edx = ((cpu.ecx >> 0) & 0xFF);
                cpu.ecx = (m.fast_u8(pt, cpu.ebp.wrapping_add(0xFFFFFFD4)) as u32);
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                let condition = (cpu.edx) <= (cpu.esi);
                pc = if condition { 0xCF0FE6 } else { 0xCF0F8B };
                continue;
            }
            0xCF0F8B => {
                pc = 0xCF0F90;
                continue;
            }
            0xCF0F90 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF0F9A };
                continue;
            }
            0xCF0F9A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.fl = flags(x86::SHL, a, n, r, 32);
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = ((cpu.ecx >> 8) & 0xFF);
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0));
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = (m.fast_u16(pt, cpu.ebp.wrapping_add(0xFFFFFFDE)) as u32);
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x44));
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.edx = ((cpu.ecx >> 0) & 0xFF);
                cpu.edx = cpu
                    .edx
                    .wrapping_add(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)));
                let condition = (cpu.edx) > (cpu.esi);
                pc = if condition { 0xCF0F90 } else { 0xCF0FE6 };
                continue;
            }
            0xCF0FE6 => {
                cpu.ecx = (m.fast_u8(pt, cpu.ebp.wrapping_add(0xFFFFFFDD)) as u32);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                pc = 0xCF0FF1;
                continue;
            }
            0xCF0FF1 => {
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.ecx);
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x10u32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), cpu.ecx);
                let condition =
                    (((((cpu.edx >> 0) & 0xFF) & 0xFF) & (((cpu.edx >> 0) & 0xFF) & 0xFF)) as i8) != 0;
                pc = if condition { 0xCF1017 } else { 0xCF100C };
                continue;
            }
            0xCF100C => {
                m.fast_set_u32(pt, cpu.edi, 0x17u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF1017 => {
                let condition = (((((cpu.edx >> 0) & 0xFF) & 0xFF) & ((0x20u32) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF1027 } else { 0xCF101C };
                continue;
            }
            0xCF101C => {
                m.fast_set_u32(pt, cpu.edi, 0xBu32);
                pc = 0xCF1437;
                continue;
            }
            0xCF1027 => {
                let condition = (((((cpu.edx >> 0) & 0xFF) & 0xFF) & ((0x40u32) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF103B } else { 0xCF102C };
                continue;
            }
            0xCF102C => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403E90u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF103B => {
                cpu.ecx = ((cpu.edx >> 0) & 0xFF);
                cpu.ecx = cpu.ecx & 0xFu32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x40), cpu.ecx);
                m.fast_set_u32(pt, cpu.edi, 0x13u32);
                pc = 0xCF104A;
                continue;
            }
            0xCF104A => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x40));
                let condition = (((cpu.ecx) & (cpu.ecx)) as i32) == 0;
                pc = if condition { 0xCF108B } else { 0xCF1051 };
                continue;
            }
            0xCF1051 => {
                let condition = (cpu.esi) >= (cpu.ecx);
                pc = if condition { 0xCF1077 } else { 0xCF1055 };
                continue;
            }
            0xCF1055 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF105F };
                continue;
            }
            0xCF105F => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (m.fast_u32(pt, cpu.edi.wrapping_add(0x40)));
                pc = if condition { 0xCF1055 } else { 0xCF1077 };
                continue;
            }
            0xCF1077 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x40));
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x38)).wrapping_add(cpu.edx);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), r);
                }
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                pc = 0xCF108B;
                continue;
            }
            0xCF108B => {
                m.fast_set_u32(pt, cpu.edi, 0x14u32);
                pc = 0xCF1091;
                continue;
            }
            0xCF1091 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x50));
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x48));
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.edx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                let condition = (cpu.ecx) <= (cpu.esi);
                pc = if condition { 0xCF10F2 } else { 0xCF10B3 };
                continue;
            }
            0xCF10B3 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF10BD };
                continue;
            }
            0xCF10BD => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x50));
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x48));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                cpu.edx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.edx);
                let condition = (cpu.ecx) > (cpu.esi);
                pc = if condition { 0xCF10B3 } else { 0xCF10F2 };
                continue;
            }
            0xCF10F2 => {
                let condition = (((((cpu.edx >> 0) & 0xFF) & 0xFF) & ((0xF0u32) & 0xFF)) as i8) != 0;
                pc = if condition { 0xCF11A3 } else { 0xCF10FB };
                continue;
            }
            0xCF10FB => {
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.ecx);
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.ecx);
                cpu.ecx = ((cpu.edx >> 0) & 0xFF);
                cpu.ecx = cpu
                    .ecx
                    .wrapping_add(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0));
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                cpu.ecx = cpu.ecx >> 0x10u32;
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x48));
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.edx = ((cpu.ecx >> 0) & 0xFF);
                cpu.ecx = (m.fast_u8(pt, cpu.ebp.wrapping_add(0xFFFFFFD4)) as u32);
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                let condition = (cpu.edx) <= (cpu.esi);
                pc = if condition { 0xCF1198 } else { 0xCF1142 };
                continue;
            }
            0xCF1142 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF114C };
                continue;
            }
            0xCF114C => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.fl = flags(x86::SHL, a, n, r, 32);
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                cpu.edx = ((cpu.ecx >> 8) & 0xFF);
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.edx);
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0));
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.ecx = (m.fast_u16(pt, cpu.ebp.wrapping_add(0xFFFFFFDE)) as u32);
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x48));
                cpu.ecx = m.fast_u32(pt, cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.edx = ((cpu.ecx >> 0) & 0xFF);
                cpu.edx = cpu
                    .edx
                    .wrapping_add(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0)));
                let condition = (cpu.edx) > (cpu.esi);
                pc = if condition { 0xCF1142 } else { 0xCF1198 };
                continue;
            }
            0xCF1198 => {
                cpu.ecx = (m.fast_u8(pt, cpu.ebp.wrapping_add(0xFFFFFFDD)) as u32);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                pc = 0xCF11A3;
                continue;
            }
            0xCF11A3 => {
                cpu.ecx = cpu.edx;
                cpu.ecx = cpu.ecx >> 0x8u32;
                cpu.ecx = ((cpu.ecx >> 0) & 0xFF);
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.ecx);
                let condition = (((((cpu.edx >> 0) & 0xFF) & 0xFF) & ((0x40u32) & 0xFF)) as i8) == 0;
                pc = if condition { 0xCF11C6 } else { 0xCF11B7 };
                continue;
            }
            0xCF11B7 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403E78u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF11C6 => {
                cpu.ecx = cpu.edx;
                cpu.edx = ((cpu.edx >> 0) & 0xFF);
                cpu.ecx = cpu.ecx >> 0x10u32;
                cpu.edx = cpu.edx & 0xFu32;
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x3C), cpu.ecx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x40), cpu.edx);
                m.fast_set_u32(pt, cpu.edi, 0x15u32);
                pc = 0xCF11DD;
                continue;
            }
            0xCF11DD => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x40));
                let condition = (((cpu.ecx) & (cpu.ecx)) as i32) == 0;
                pc = if condition { 0xCF1226 } else { 0xCF11E4 };
                continue;
            }
            0xCF11E4 => {
                let condition = (cpu.esi) >= (cpu.ecx);
                pc = if condition { 0xCF1212 } else { 0xCF11E8 };
                continue;
            }
            0xCF11E8 => {
                pc = 0xCF11F0;
                continue;
            }
            0xCF11F0 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF11FA };
                continue;
            }
            0xCF11FA => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (m.fast_u32(pt, cpu.edi.wrapping_add(0x40)));
                pc = if condition { 0xCF11F0 } else { 0xCF1212 };
                continue;
            }
            0xCF1212 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x40));
                cpu.edx = 0x1u32;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.edx = cpu.edx.wrapping_sub(1);
                cpu.edx = cpu.edx & cpu.ebx;
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x3C)).wrapping_add(cpu.edx);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x3C), r);
                }
                {
                    let a = cpu.ebx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.ebx = r;
                    }
                }
                cpu.esi = cpu.esi.wrapping_sub(cpu.ecx);
                pc = 0xCF1226;
                continue;
            }
            0xCF1226 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x24));
                cpu.ecx = cpu
                    .ecx
                    .wrapping_sub(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)));
                cpu.ecx = cpu
                    .ecx
                    .wrapping_add(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4)));
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x3C))) <= (cpu.ecx);
                pc = if condition { 0xCF1243 } else { 0xCF1234 };
                continue;
            }
            0xCF1234 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x18), 0x1403E58u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF1243 => {
                m.fast_set_u32(pt, cpu.edi, 0x16u32);
                pc = 0xCF1249;
                continue;
            }
            0xCF1249 => {
                let condition = (m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0))) == (0x0u32);
                pc = if condition { 0xCF148E } else { 0xCF1253 };
                continue;
            }
            0xCF1253 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                cpu.edx = cpu
                    .edx
                    .wrapping_sub(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)));
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x3C));
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF1297 } else { 0xCF1260 };
                continue;
            }
            0xCF1260 => {
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x28));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF127B } else { 0xCF126C };
                continue;
            }
            0xCF126C => {
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x2C));
                cpu.edx = cpu.edx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x20)));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                pc = 0xCF1286;
                continue;
            }
            0xCF127B => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x2C));
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                cpu.edx = cpu.edx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x28)));
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4));
                pc = 0xCF1286;
                continue;
            }
            0xCF1286 => {
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x38));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.edx);
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF12A8 } else { 0xCF1293 };
                continue;
            }
            0xCF1293 => {
                cpu.ecx = cpu.edx;
                pc = 0xCF12A5;
                continue;
            }
            0xCF1297 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x38));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.edx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.ecx);
                pc = 0xCF12A5;
                continue;
            }
            0xCF12A5 => {
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                pc = 0xCF12A8;
                continue;
            }
            0xCF12A8 => {
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                let condition = (cpu.ecx) <= (cpu.edx);
                pc = if condition { 0xCF12B4 } else { 0xCF12AF };
                continue;
            }
            0xCF12AF => {
                cpu.ecx = cpu.edx;
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.ecx);
                pc = 0xCF12B4;
                continue;
            }
            0xCF12B4 => {
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0), cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFDC));
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4));
                cpu.ecx = alu_sub(
                    &mut cpu.fl,
                    cpu.ecx,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8)),
                    32,
                );
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x38), cpu.edx);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.ecx);
                pc = 0xCF12D3;
                continue;
            }
            // 0xCF12D0..0xCF12E5: forward overlapping match copy. Keep
            // guest spill writes in their original order; only the last DEC's
            // flags escape this loop. INC/DEC both preserve the incoming CF.
            0xCF12D3 => {
                let carry = cpu.fl.cf();
                loop {
                    let output_18 = m.fast_u32(pt, cpu.ebp.wrapping_sub(0x18));
                    let byte = m.fast_u8(pt, output_18.wrapping_add(cpu.ecx));
                    cpu.edx = (output_18 & !255) | u32::from(byte);
                    cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_sub(0x18));
                    m.fast_set_u8(pt, cpu.ecx, byte);
                    cpu.ecx = cpu.ecx.wrapping_add(1);
                    let remaining_0c = m.fast_u32(pt, cpu.ebp.wrapping_sub(0x0c)).wrapping_sub(1);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_sub(0x0c), remaining_0c);
                    m.fast_set_u32(pt, cpu.ebp.wrapping_sub(0x18), cpu.ecx);
                    if remaining_0c == 0 {
                        cpu.fl = Flags {
                            cf_in: carry,
                            ..flags(x86::DEC, 1, 1, 0, 32)
                        };
                        break;
                    }
                    cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_sub(0x2c));
                }
                pc = 0xCF12E7;
                continue;
            }
            0xCF12E7 => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x38))) != (0x0u32);
                pc = if condition { 0xCF1437 } else { 0xCF12F1 };
                continue;
            }
            0xCF12F1 => {
                m.fast_set_u32(pt, cpu.edi, 0x12u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF12FC => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF1306 };
                continue;
            }
            0xCF1306 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                cpu.edx = (cpu.edx & !(0xFF << 0))
                    | (((m.fast_u8(pt, cpu.edi.wrapping_add(0x38)) as u32) & 0xFF) << 0);
                m.fast_set_u8(pt, cpu.ecx, ((cpu.edx >> 0) & 0xFF) as u8);
                {
                    let a = cpu.ecx & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    cpu.fl.inc_dec(x86::INC, a, r, 32);
                    cpu.ecx = r;
                }
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0), r);
                }
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8), cpu.ecx);
                m.fast_set_u32(pt, cpu.edi, 0x12u32);
                pc = 0xCF1437;
                continue;
            }
            0xCF1320 => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x8))) == (0x0u32);
                pc = if condition { 0xCF13DD } else { 0xCF132A };
                continue;
            }
            0xCF132A => {
                let condition = (cpu.esi) >= (0x20u32);
                pc = if condition { 0xCF1352 } else { 0xCF132F };
                continue;
            }
            0xCF132F => {
                pc = 0xCF1330;
                continue;
            }
            0xCF1330 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF133A };
                continue;
            }
            0xCF133A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x20u32);
                pc = if condition { 0xCF1330 } else { 0xCF1352 };
                continue;
            }
            0xCF1352 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                cpu.ecx = cpu
                    .ecx
                    .wrapping_sub(m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0)));
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                {
                    let r = m.fast_u32(pt, cpu.edx.wrapping_add(0x14)).wrapping_add(cpu.ecx);
                    m.fast_set_u32(pt, cpu.edx.wrapping_add(0x14), r);
                }
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x18)).wrapping_add(cpu.ecx);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x18), r);
                }
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4), cpu.ecx);
                alu_and(&mut cpu.fl, cpu.ecx, cpu.ecx, 32);
                pc = if zero(&cpu.fl) { 0xCF1396 } else { 0xCF1368 };
                continue;
            }
            0xCF1368 => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                cpu.eax = cpu.ecx;
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                emit::push(cpu.eax, m, pt, cpu);
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.eax);
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x0u32,
                    32,
                );
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(cpu.edx, m, pt, cpu);
                pc = if zero(&cpu.fl) { 0xCF1382 } else { 0xCF137B };
                continue;
            }
            0xCF137B => {
                emit::push(0xCF1380, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                pc = 0xCF1387;
                continue;
            }
            0xCF1382 => {
                emit::push(0xCF1387, m, pt, cpu);
                call(m, cpu, 0x00CF34D0, true);
                pc = 0xCF1387;
                continue;
            }
            0xCF1387 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x30), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF1396;
                continue;
            }
            0xCF1396 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x0u32,
                    32,
                );
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4), cpu.edx);
                cpu.ecx = cpu.ebx;
                pc = if (!zero(&cpu.fl)) { 0xCF13C8 } else { 0xCF13A4 };
                continue;
            }
            0xCF13A4 => {
                cpu.ecx = cpu.ecx & 0xFF00u32;
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx << 0x10u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx >> 0x8u32;
                cpu.edx = cpu.edx & 0xFF00u32;
                cpu.ecx = cpu.ecx << 0x8u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                cpu.edx = cpu.ebx;
                cpu.edx = cpu.edx >> 0x18u32;
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edx);
                pc = 0xCF13C8;
                continue;
            }
            0xCF13C8 => {
                let condition = (cpu.ecx) == (m.fast_u32(pt, cpu.edi.wrapping_add(0x14)));
                pc = if condition { 0xCF13D9 } else { 0xCF13CD };
                continue;
            }
            0xCF13CD => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x18), 0x1403E40u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF13D9 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                pc = 0xCF13DD;
                continue;
            }
            0xCF13DD => {
                m.fast_set_u32(pt, cpu.edi, 0x19u32);
                pc = 0xCF13E3;
                continue;
            }
            0xCF13E3 => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x8))) == (0x0u32);
                pc = if condition { 0xCF1478 } else { 0xCF13ED };
                continue;
            }
            0xCF13ED => {
                let condition = (m.fast_u32(pt, cpu.edi.wrapping_add(0x10))) == (0x0u32);
                pc = if condition { 0xCF1478 } else { 0xCF13F7 };
                continue;
            }
            0xCF13F7 => {
                let condition = (cpu.esi) >= (0x20u32);
                pc = if condition { 0xCF1422 } else { 0xCF13FC };
                continue;
            }
            0xCF13FC => {
                pc = 0xCF1400;
                continue;
            }
            0xCF1400 => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)),
                    0x0u32,
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF148E } else { 0xCF140A };
                continue;
            }
            0xCF140A => {
                cpu.edx = (m.fast_u8(pt, cpu.eax) as u32);
                {
                    let a = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC)) & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC), r);
                }
                cpu.ecx = cpu.esi;
                {
                    let a = cpu.edx & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edx = r;
                    }
                }
                cpu.eax = cpu.eax.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x8u32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ebx = cpu.ebx.wrapping_add(cpu.edx);
                let condition = (cpu.esi) < (0x20u32);
                pc = if condition { 0xCF1400 } else { 0xCF1422 };
                continue;
            }
            0xCF1422 => {
                let condition = (cpu.ebx) == (m.fast_u32(pt, cpu.edi.wrapping_add(0x18)));
                pc = if condition { 0xCF1474 } else { 0xCF1427 };
                continue;
            }
            0xCF1427 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x18), 0x1403E28u32);
                pc = 0xCF1431;
                continue;
            }
            0xCF1431 => {
                m.fast_set_u32(pt, cpu.edi, 0x1Bu32);
                pc = 0xCF1437;
                continue;
            }
            0xCF1437 => {
                cpu.ecx = m.fast_u32(pt, cpu.edi);
                alu_sub(&mut cpu.fl, cpu.ecx, 0x1Cu32, 32);
                pc = if (cpu.fl.cf() || zero(&cpu.fl)) {
                    0xCF0363
                } else {
                    0xCF1442
                };
                continue;
            }
            0xCF1442 => {
                cpu.esi = emit::pop(m, pt, cpu);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.eax = 0xFFFFFFFEu32;
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF144E => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                m.fast_set_u32(pt, cpu.edx, cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0xC), cpu.ecx);
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x4), cpu.eax);
                m.fast_set_u32(pt, cpu.edx.wrapping_add(0x10), cpu.ecx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x34), cpu.esi);
                cpu.esi = emit::pop(m, pt, cpu);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x30), cpu.ebx);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.eax = 0x2u32;
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF1474 => {
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.esi = cpu.esi ^ cpu.esi;
                pc = 0xCF1478;
                continue;
            }
            0xCF1478 => {
                m.fast_set_u32(pt, cpu.edi, 0x1Au32);
                pc = 0xCF147E;
                continue;
            }
            0xCF147E => {
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), 0x1u32);
                pc = 0xCF148E;
                continue;
            }
            0xCF1487 => {
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8), 0xFFFFFFFDu32);
                pc = 0xCF148E;
                continue;
            }
            0xCF148E => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE8));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0xC), cpu.edx);
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFF0));
                m.fast_set_u32(pt, cpu.ecx, cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFFC));
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x10), cpu.edx);
                m.fast_set_u32(pt, cpu.ecx.wrapping_add(0x4), cpu.eax);
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x20)),
                    0x0u32,
                    32,
                );
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x30), cpu.ebx);
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x34), cpu.esi);
                pc = if (!zero(&cpu.fl)) { 0xCF14C1 } else { 0xCF14B1 };
                continue;
            }
            0xCF14B1 => {
                let condition = ((m.fast_u32(pt, cpu.edi)) as i32) >= ((0x18u32) as i32);
                pc = if condition { 0xCF14E6 } else { 0xCF14B6 };
                continue;
            }
            0xCF14B6 => {
                cpu.ecx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                cpu.edx = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                alu_sub(
                    &mut cpu.fl,
                    cpu.ecx,
                    m.fast_u32(pt, cpu.edx.wrapping_add(0x10)),
                    32,
                );
                pc = if zero(&cpu.fl) { 0xCF14E6 } else { 0xCF14C1 };
                continue;
            }
            0xCF14C1 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                emit::push(cpu.eax, m, pt, cpu);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                emit::push(0xCF14CD, m, pt, cpu);
                call(m, cpu, 0xCF01E0u32, false);
                cpu.esp = cpu.esp.wrapping_add(0x4u32);
                alu_and(&mut cpu.fl, cpu.eax, cpu.eax, 32);
                pc = if zero(&cpu.fl) { 0xCF14E6 } else { 0xCF14D4 };
                continue;
            }
            0xCF14D4 => {
                m.fast_set_u32(pt, cpu.edi, 0x1Cu32);
                pc = 0xCF14DA;
                continue;
            }
            0xCF14DA => {
                cpu.esi = emit::pop(m, pt, cpu);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.eax = 0xFFFFFFFCu32;
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF14E6 => {
                cpu.esi = m.fast_u32(pt, cpu.ebp.wrapping_add(0x8));
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD0));
                cpu.eax = cpu.eax.wrapping_sub(m.fast_u32(pt, cpu.esi.wrapping_add(0x4)));
                cpu.ebx = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFE4));
                cpu.ebx = cpu.ebx.wrapping_sub(m.fast_u32(pt, cpu.esi.wrapping_add(0x10)));
                {
                    let r = m.fast_u32(pt, cpu.esi.wrapping_add(0x8)).wrapping_add(cpu.eax);
                    m.fast_set_u32(pt, cpu.esi.wrapping_add(0x8), r);
                }
                {
                    let r = m.fast_u32(pt, cpu.esi.wrapping_add(0x14)).wrapping_add(cpu.ebx);
                    m.fast_set_u32(pt, cpu.esi.wrapping_add(0x14), r);
                }
                {
                    let r = m.fast_u32(pt, cpu.edi.wrapping_add(0x18)).wrapping_add(cpu.ebx);
                    m.fast_set_u32(pt, cpu.edi.wrapping_add(0x18), r);
                }
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.edi.wrapping_add(0x8)), 0x0u32, 32);
                m.fast_set_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD0), cpu.eax);
                pc = if zero(&cpu.fl) { 0xCF153E } else { 0xCF1507 };
                continue;
            }
            0xCF1507 => {
                let condition = (((cpu.ebx) & (cpu.ebx)) as i32) == 0;
                pc = if condition { 0xCF153E } else { 0xCF150B };
                continue;
            }
            0xCF150B => {
                alu_sub(
                    &mut cpu.fl,
                    m.fast_u32(pt, cpu.edi.wrapping_add(0x10)),
                    0x0u32,
                    32,
                );
                emit::push(cpu.ebx, m, pt, cpu);
                pc = if zero(&cpu.fl) { 0xCF1523 } else { 0xCF1512 };
                continue;
            }
            0xCF1512 => {
                cpu.ecx = m.fast_u32(pt, cpu.esi.wrapping_add(0xC));
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                cpu.ecx = alu_sub(&mut cpu.fl, cpu.ecx, cpu.ebx, 32);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(cpu.edx, m, pt, cpu);
                emit::push(0xCF1521, m, pt, cpu);
                call(m, cpu, 0xCF38D0u32, false);
                pc = 0xCF1532;
                continue;
            }
            0xCF1523 => {
                cpu.eax = m.fast_u32(pt, cpu.esi.wrapping_add(0xC));
                cpu.ecx = m.fast_u32(pt, cpu.edi.wrapping_add(0x14));
                cpu.eax = alu_sub(&mut cpu.fl, cpu.eax, cpu.ebx, 32);
                emit::push(cpu.eax, m, pt, cpu);
                emit::push(cpu.ecx, m, pt, cpu);
                emit::push(0xCF1532, m, pt, cpu);
                call(m, cpu, 0x00CF34D0, true);
                pc = 0xCF1532;
                continue;
            }
            0xCF1532 => {
                m.fast_set_u32(pt, cpu.edi.wrapping_add(0x14), cpu.eax);
                m.fast_set_u32(pt, cpu.esi.wrapping_add(0x30), cpu.eax);
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD0));
                cpu.esp = cpu.esp.wrapping_add(0xCu32);
                pc = 0xCF153E;
                continue;
            }
            0xCF153E => {
                cpu.edx = m.fast_u32(pt, cpu.edi.wrapping_add(0x4));
                {
                    let a = cpu.edx & u32::MAX;
                    let r = 0u32.wrapping_sub(a) & u32::MAX;
                    cpu.fl = flags(x86::SUB, 0, a, r, 32);
                    cpu.edx = r;
                }
                {
                    let (a, b) = (cpu.edx, cpu.edx);
                    let r = cpu.fl.alu_carry(a, b, 32, true);
                    cpu.edx = r;
                }
                cpu.edx = cpu.edx & 0x40u32;
                cpu.ecx = cpu.ecx ^ cpu.ecx;
                let condition = (m.fast_u32(pt, cpu.edi)) != (0xBu32);
                cpu.ecx = (cpu.ecx & !(0xFF << 0)) | (((condition as u32) & 0xFF) << 0);
                cpu.ecx = cpu.ecx.wrapping_sub(1);
                cpu.ecx = cpu.ecx & 0x80u32;
                cpu.edx = cpu.edx.wrapping_add(cpu.ecx);
                cpu.edx = cpu.edx.wrapping_add(m.fast_u32(pt, cpu.edi.wrapping_add(0x34)));
                m.fast_set_u32(pt, cpu.esi.wrapping_add(0x2C), cpu.edx);
                let condition = (((cpu.eax) & (cpu.eax)) as i32) != 0;
                pc = if condition { 0xCF1567 } else { 0xCF1563 };
                continue;
            }
            0xCF1563 => {
                let condition = (((cpu.ebx) & (cpu.ebx)) as i32) == 0;
                pc = if condition { 0xCF156D } else { 0xCF1567 };
                continue;
            }
            0xCF1567 => {
                alu_sub(&mut cpu.fl, m.fast_u32(pt, cpu.ebp.wrapping_add(0xC)), 0x4u32, 32);
                pc = if (!zero(&cpu.fl)) { 0xCF1584 } else { 0xCF156D };
                continue;
            }
            0xCF156D => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8));
                alu_and(&mut cpu.fl, cpu.eax, cpu.eax, 32);
                pc = if (!zero(&cpu.fl)) { 0xCF035C } else { 0xCF1578 };
                continue;
            }
            0xCF1578 => {
                cpu.esi = emit::pop(m, pt, cpu);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.eax = 0xFFFFFFFBu32;
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF1584 => {
                cpu.eax = m.fast_u32(pt, cpu.ebp.wrapping_add(0xFFFFFFD8));
                cpu.esi = emit::pop(m, pt, cpu);
                cpu.ebx = emit::pop(m, pt, cpu);
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            0xCF158E => {
                cpu.eax = 0xFFFFFFFEu32;
                cpu.edi = emit::pop(m, pt, cpu);
                cpu.esp = cpu.ebp;
                cpu.ebp = emit::pop(m, pt, cpu);
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                return;
            }
            _ => unreachable!("invalid inflate control label {pc:#x}"),
        }
    }
}
