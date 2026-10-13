//! Exact original-layout Huffman table construction. No copied tables:
//! length/distance base and operation arrays remain in the player's image.
//! The instruction-address blocks preserve all guest stack/register effects.
#![allow(unused_parens)]
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

/// Translated from 0x00cf30c0 (DDDA.exe 2017-12-13 Steam).
/// cdecl(kind, lengths, symbol_count, table_cursor, root_bits, work).
/// A single native graph replaces two lifted outline dispatchers. Pages
/// removes trait dispatch from histogram, workspace and table accesses.
pub fn build(m: &mut dyn Mem, cpu: &mut Cpu) {
    let mut pages = TableMemory::new(m, cpu.esp.wrapping_sub(0x88));
    let mut pc = 0xCF30C0;
    loop {
        match pc {
            0xCF30C0 => {
                cpu.esp = cpu.esp.wrapping_sub(4);
                pages.set_u32(m, cpu.esp, cpu.ebp);
                cpu.ebp = cpu.esp;
                cpu.esp = cpu.esp.wrapping_sub(0x78u32);
                cpu.eax = cpu.eax ^ cpu.eax;
                cpu.ecx = (cpu.eax & 0xFFFF);
                cpu.eax = cpu.ecx;
                cpu.ecx = cpu.ecx << 0x10u32;
                cpu.eax = cpu.eax | cpu.ecx;
                cpu.esp = cpu.esp.wrapping_sub(4);
                pages.set_u32(m, cpu.esp, cpu.ebx);
                cpu.ebx = pages.u32(m, cpu.ebp.wrapping_add(0xC));
                cpu.esp = cpu.esp.wrapping_sub(4);
                pages.set_u32(m, cpu.esp, cpu.esi);
                cpu.esi = pages.u32(m, cpu.ebp.wrapping_add(0x10));
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFA8), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFAC), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFB0), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFB4), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFB8), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFBC), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFC0), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFC4), cpu.eax);
                cpu.eax = cpu.eax ^ cpu.eax;
                cpu.fl.alu_and(cpu.esi, cpu.esi, 32);
                pc = if cpu.fl.zf() { 0xCF3112 } else { 0xCF30F8 };
                continue;
            }
            0xCF30F8 => {
                pc = 0xCF3100;
                continue;
            }
            0xCF3100 => {
                loop {
                    cpu.ecx = (pages.u16(m, cpu.ebx.wrapping_add(cpu.eax.wrapping_mul(2))) as u32);
                    {
                        let a = (pages.u16(
                            m,
                            cpu.ebp
                                .wrapping_add(cpu.ecx.wrapping_mul(2))
                                .wrapping_add(0xFFFFFFA8),
                        ) as u32)
                            & 0xFFFF;
                        let r = a.wrapping_add(1) & 0xFFFF;
                        cpu.fl.inc_dec(x86::INC, a, r, 16);
                        pages.set_u16(
                            m,
                            cpu.ebp
                                .wrapping_add(cpu.ecx.wrapping_mul(2))
                                .wrapping_add(0xFFFFFFA8),
                            (r) as u16,
                        );
                    }
                    cpu.ecx = cpu
                        .ebp
                        .wrapping_add(cpu.ecx.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFA8);
                    cpu.eax = cpu.eax.wrapping_add(1);
                    cpu.fl.alu_sub(cpu.eax, cpu.esi, 32);
                    if !(cpu.fl.cf()) {
                        break;
                    }
                }
                pc = 0xCF3112;
                continue;
            }
            0xCF3112 => {
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x18));
                cpu.ecx = pages.u32(m, cpu.edx);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.ecx);
                cpu.eax = 0xFu32;
                pc = 0xCF3120;
                continue;
            }
            0xCF3120 => {
                let condition = ((pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.eax.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFA8),
                ) as u32)
                    & 0xFFFF)
                    != ((0x0u32) & 0xFFFF);
                pc = if condition { 0xCF312E } else { 0xCF3128 };
                continue;
            }
            0xCF3128 => {
                cpu.eax = cpu.eax.wrapping_sub(1);
                let condition = (cpu.eax) >= (0x1u32);
                pc = if condition { 0xCF3120 } else { 0xCF312E };
                continue;
            }
            0xCF312E => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF4), cpu.eax);
                let condition = (cpu.ecx) <= (cpu.eax);
                pc = if condition { 0xCF313A } else { 0xCF3135 };
                continue;
            }
            0xCF3135 => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.eax);
                cpu.ecx = cpu.eax;
                pc = 0xCF313A;
                continue;
            }
            0xCF313A => {
                cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
                pc = if (!cpu.fl.zf()) { 0xCF3147 } else { 0xCF313E };
                continue;
            }
            0xCF313E => {
                cpu.esi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.eax = cpu.fl.alu_or(cpu.eax, 0xFFFFFFFFu32, 32);
                cpu.ebx = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.ebp;
                cpu.ebp = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                pages.flush(m);
                return;
            }
            0xCF3147 => {
                cpu.esp = cpu.esp.wrapping_sub(4);
                pages.set_u32(m, cpu.esp, cpu.edi);
                cpu.edi = 0x1u32;
                pc = 0xCF3150;
                continue;
            }
            0xCF3150 => {
                let condition = ((pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.edi.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFA8),
                ) as u32)
                    & 0xFFFF)
                    != ((0x0u32) & 0xFFFF);
                pc = if condition { 0xCF3192 } else { 0xCF3158 };
                continue;
            }
            0xCF3158 => {
                cpu.fl.alu_sub(
                    (pages.u16(
                        m,
                        cpu.ebp
                            .wrapping_add(cpu.edi.wrapping_mul(2))
                            .wrapping_add(0xFFFFFFAA),
                    ) as u32),
                    0x0u32,
                    16,
                );
                pc = if (!cpu.fl.zf()) { 0xCF3182 } else { 0xCF3160 };
                continue;
            }
            0xCF3160 => {
                let condition = ((pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.edi.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFAC),
                ) as u32)
                    & 0xFFFF)
                    != ((0x0u32) & 0xFFFF);
                pc = if condition { 0xCF3185 } else { 0xCF3168 };
                continue;
            }
            0xCF3168 => {
                let condition = ((pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.edi.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFAE),
                ) as u32)
                    & 0xFFFF)
                    != ((0x0u32) & 0xFFFF);
                pc = if condition { 0xCF318A } else { 0xCF3170 };
                continue;
            }
            0xCF3170 => {
                let condition = ((pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.edi.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFB0),
                ) as u32)
                    & 0xFFFF)
                    != ((0x0u32) & 0xFFFF);
                pc = if condition { 0xCF318F } else { 0xCF3178 };
                continue;
            }
            0xCF3178 => {
                cpu.edi = cpu.edi.wrapping_add(0x5u32);
                let condition = (cpu.edi) <= (0xFu32);
                pc = if condition { 0xCF3150 } else { 0xCF3180 };
                continue;
            }
            0xCF3180 => {
                pc = 0xCF3192;
                continue;
            }
            0xCF3182 => {
                {
                    let a = cpu.edi & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    cpu.edi = r;
                }
                pc = 0xCF3192;
                continue;
            }
            0xCF3185 => {
                cpu.edi = cpu.edi.wrapping_add(0x2u32);
                pc = 0xCF3192;
                continue;
            }
            0xCF318A => {
                cpu.edi = cpu.edi.wrapping_add(0x3u32);
                pc = 0xCF3192;
                continue;
            }
            0xCF318F => {
                cpu.edi = cpu.edi.wrapping_add(0x4u32);
                pc = 0xCF3192;
                continue;
            }
            0xCF3192 => {
                let condition = (cpu.ecx) >= (cpu.edi);
                pc = if condition { 0xCF3199 } else { 0xCF3196 };
                continue;
            }
            0xCF3196 => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8), cpu.edi);
                pc = 0xCF3199;
                continue;
            }
            0xCF3199 => {
                cpu.edx = 0x1u32;
                cpu.eax = cpu.edx;
                pc = 0xCF31A0;
                continue;
            }
            0xCF31A0 => {
                cpu.ecx = (pages.u16(
                    m,
                    cpu.ebp
                        .wrapping_add(cpu.eax.wrapping_mul(2))
                        .wrapping_add(0xFFFFFFA8),
                ) as u32);
                cpu.edx = cpu.edx.wrapping_add(cpu.edx);
                cpu.edx = cpu.fl.alu_sub(cpu.edx, cpu.ecx, 32);
                pc = if cpu.fl.sf() { 0xCF31C8 } else { 0xCF31AB };
                continue;
            }
            0xCF31AB => {
                cpu.eax = cpu.eax.wrapping_add(1);
                let condition = (cpu.eax) <= (0xFu32);
                pc = if condition { 0xCF31A0 } else { 0xCF31B1 };
                continue;
            }
            0xCF31B1 => {
                let condition = (((cpu.edx) & (cpu.edx)) as i32) <= 0;
                pc = if condition { 0xCF31D2 } else { 0xCF31B5 };
                continue;
            }
            0xCF31B5 => {
                cpu.fl
                    .alu_sub(pages.u32(m, cpu.ebp.wrapping_add(0x8)), 0x0u32, 32);
                pc = if cpu.fl.zf() { 0xCF31C8 } else { 0xCF31BB };
                continue;
            }
            0xCF31BB => {
                cpu.eax = (pages.u16(m, cpu.ebp.wrapping_add(0xFFFFFFA8)) as u32);
                cpu.ecx = cpu.esi;
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.eax);
                cpu.fl.alu_sub(cpu.ecx, 0x1u32, 32);
                pc = if cpu.fl.zf() { 0xCF31D2 } else { 0xCF31C8 };
                continue;
            }
            0xCF31C8 => {
                cpu.edi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.eax = cpu.fl.alu_or(cpu.eax, 0xFFFFFFFFu32, 32);
                cpu.ebx = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.ebp;
                cpu.ebp = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                pages.flush(m);
                return;
            }
            0xCF31D2 => {
                cpu.edx = cpu.edx ^ cpu.edx;
                pages.set_u16(m, cpu.ebp.wrapping_add(0xFFFFFF8A), (cpu.edx & 0xFFFF) as u16);
                cpu.eax = 0x2u32;
                pc = 0xCF31E0;
                continue;
            }
            0xCF31E0 => {
                loop {
                    cpu.ecx = (cpu.ecx & !0xFFFF)
                        | ((pages.u16(
                            m,
                            cpu.ebp
                                .wrapping_add(cpu.eax.wrapping_mul(1))
                                .wrapping_add(0xFFFFFF88),
                        ) as u32)
                            & 0xFFFF);
                    {
                        let r = (cpu.ecx & 0xFFFF).wrapping_add(
                            (pages.u16(
                                m,
                                cpu.ebp
                                    .wrapping_add(cpu.eax.wrapping_mul(1))
                                    .wrapping_add(0xFFFFFFA8),
                            ) as u32),
                        ) & 0xFFFF;
                        cpu.ecx = (cpu.ecx & !0xFFFF) | ((r) & 0xFFFF);
                    }
                    cpu.eax = cpu.eax.wrapping_add(0x2u32);
                    pages.set_u16(
                        m,
                        cpu.ebp
                            .wrapping_add(cpu.eax.wrapping_mul(1))
                            .wrapping_add(0xFFFFFF88),
                        (cpu.ecx & 0xFFFF) as u16,
                    );
                    let condition = (cpu.eax) < (0x1Eu32);
                    if !(condition) {
                        break;
                    }
                }
                pc = 0xCF31F7;
                continue;
            }
            0xCF31F7 => {
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x1C));
                cpu.eax = cpu.eax ^ cpu.eax;
                let condition = (((cpu.esi) & (cpu.esi)) as i32) == 0;
                pc = if condition { 0xCF3229 } else { 0xCF3200 };
                continue;
            }
            0xCF3200 => {
                loop {
                    cpu.fl.alu_sub(
                        pages.u16(m, cpu.ebx.wrapping_add(cpu.eax.wrapping_mul(2))) as u32,
                        0,
                        16,
                    );
                    if !cpu.fl.zf() {
                        cpu.ecx = (pages.u16(m, cpu.ebx.wrapping_add(cpu.eax.wrapping_mul(2))) as u32);
                        cpu.ecx = (pages.u16(
                            m,
                            cpu.ebp
                                .wrapping_add(cpu.ecx.wrapping_mul(2))
                                .wrapping_add(0xFFFFFF88),
                        ) as u32);
                        pages.set_u16(
                            m,
                            cpu.edx.wrapping_add(cpu.ecx.wrapping_mul(2)),
                            (cpu.eax & 0xFFFF) as u16,
                        );
                        cpu.esi = (pages.u16(m, cpu.ebx.wrapping_add(cpu.eax.wrapping_mul(2))) as u32);
                        {
                            let a = (pages.u16(
                                m,
                                cpu.ebp
                                    .wrapping_add(cpu.esi.wrapping_mul(2))
                                    .wrapping_add(0xFFFFFF88),
                            ) as u32)
                                & 0xFFFF;
                            let r = a.wrapping_add(1) & 0xFFFF;
                            pages.set_u16(
                                m,
                                cpu.ebp
                                    .wrapping_add(cpu.esi.wrapping_mul(2))
                                    .wrapping_add(0xFFFFFF88),
                                (r) as u16,
                            );
                        }
                        cpu.esi = cpu
                            .ebp
                            .wrapping_add(cpu.esi.wrapping_mul(2))
                            .wrapping_add(0xFFFFFF88);
                        cpu.esi = pages.u32(m, cpu.ebp.wrapping_add(0x10));
                    }
                    cpu.eax = cpu.eax.wrapping_add(1);
                    if cpu.eax >= cpu.esi {
                        break;
                    }
                }
                pc = 0xCF3229;
                continue;
            }
            0xCF3229 => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0x8));
                cpu.eax = cpu.fl.alu_sub(cpu.eax, 0x0u32, 32);
                cpu.ecx = 0xFFFFFFFFu32;
                pc = if cpu.fl.zf() { 0xCF326F } else { 0xCF3236 };
                continue;
            }
            0xCF3236 => {
                {
                    let a = cpu.eax & u32::MAX;
                    let r = a.wrapping_sub(1) & u32::MAX;
                    cpu.fl.inc_dec(x86::DEC, a, r, 32);
                    cpu.eax = r;
                }
                pc = if cpu.fl.zf() { 0xCF324C } else { 0xCF3239 };
                continue;
            }
            0xCF3239 => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE0), 0x14041F8u32);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE4), 0x1404238u32);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFEC), cpu.ecx);
                pc = 0xCF327C;
                continue;
            }
            0xCF324C => {
                cpu.eax = 0x1404178u32;
                cpu.eax = cpu.eax.wrapping_sub(0x202u32);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.eax);
                cpu.eax = 0x14041B8u32;
                cpu.eax = cpu.eax.wrapping_sub(0x202u32);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE4), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFEC), 0x100u32);
                pc = 0xCF327C;
                continue;
            }
            0xCF326F => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE4), cpu.edx);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE0), cpu.edx);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFEC), 0x13u32);
                pc = 0xCF327C;
                continue;
            }
            0xCF327C => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.eax = pages.u32(m, cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD0), cpu.ecx);
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.eax = 0x1u32;
                {
                    let a = cpu.eax & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.eax = r;
                    }
                }
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC), cpu.edi);
                cpu.esi = cpu.esi ^ cpu.esi;
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                cpu.fl
                    .alu_sub(pages.u32(m, cpu.ebp.wrapping_add(0x8)), 0x1u32, 32);
                cpu.edi = cpu.eax;
                cpu.ecx = cpu.eax.wrapping_add(0xFFFFFFFF);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD8), cpu.edi);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE8), cpu.eax);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFCC), cpu.ecx);
                pc = if (!cpu.fl.zf()) { 0xCF32BD } else { 0xCF32AC };
                continue;
            }
            0xCF32AC => {
                cpu.fl.alu_sub(cpu.eax, 0x506u32, 32);
                pc = if cpu.fl.cf() { 0xCF32BD } else { 0xCF32B3 };
                continue;
            }
            0xCF32B3 => {
                cpu.edi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.eax = cpu.esi.wrapping_add(0x1);
                cpu.esi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.ebx = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.ebp;
                cpu.ebp = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                pages.flush(m);
                return;
            }
            0xCF32BD => {
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.edx);
                pc = 0xCF32C5;
                continue;
            }
            0xCF32C2 => {
                cpu.edi = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD8));
                pc = 0xCF32C5;
                continue;
            }
            0xCF32C5 => {
                cpu.eax = (cpu.eax & !(0xFF << 0))
                    | (((pages.u8(m, cpu.ebp.wrapping_add(0xFFFFFFFC)) as u32) & 0xFF) << 0);
                {
                    let r = ((cpu.eax >> 0) & 0xFF).wrapping_sub(((cpu.ebx >> 0) & 0xFF)) & 0xFF;
                    cpu.eax = (cpu.eax & !(0xFF << 0)) | (((r) & 0xFF) << 0);
                }
                pages.set_u8(m, cpu.ebp.wrapping_add(0x11), ((cpu.eax >> 0) & 0xFF) as u8);
                cpu.eax = (pages.u16(m, cpu.edx) as u32);
                cpu.ecx = cpu.eax;
                cpu.fl
                    .alu_sub(cpu.ecx, pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFEC)), 32);
                pc = if (cpu.fl.sf() == cpu.fl.of()) {
                    0xCF32DD
                } else {
                    0xCF32D7
                };
                continue;
            }
            0xCF32D7 => {
                pages.set_u8(m, cpu.ebp.wrapping_add(0x10), (0x0u32) as u8);
                pc = 0xCF3300;
                continue;
            }
            0xCF32DD => {
                pc = if (cpu.fl.zf() || cpu.fl.sf() != cpu.fl.of()) {
                    0xCF32FA
                } else {
                    0xCF32DF
                };
                continue;
            }
            0xCF32DF => {
                cpu.eax = (pages.u16(m, cpu.edx) as u32);
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFE4));
                cpu.eax = cpu.eax.wrapping_add(cpu.eax);
                cpu.edx = (cpu.edx & !(0xFF << 0))
                    | (((pages.u8(m, cpu.eax.wrapping_add(cpu.ecx.wrapping_mul(1))) as u32) & 0xFF) << 0);
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFE0));
                pages.set_u8(m, cpu.ebp.wrapping_add(0x10), ((cpu.edx >> 0) & 0xFF) as u8);
                cpu.edx = (cpu.edx & !0xFFFF)
                    | ((pages.u16(m, cpu.eax.wrapping_add(cpu.ecx.wrapping_mul(1))) as u32) & 0xFFFF);
                pages.set_u16(m, cpu.ebp.wrapping_add(0x12), (cpu.edx & 0xFFFF) as u16);
                pc = 0xCF3304;
                continue;
            }
            0xCF32FA => {
                pages.set_u8(m, cpu.ebp.wrapping_add(0x10), (0x60u32) as u8);
                cpu.eax = cpu.eax ^ cpu.eax;
                pc = 0xCF3300;
                continue;
            }
            0xCF3300 => {
                pages.set_u16(m, cpu.ebp.wrapping_add(0x12), (cpu.eax & 0xFFFF) as u16);
                pc = 0xCF3304;
                continue;
            }
            0xCF3304 => {
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC));
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.ebx);
                cpu.eax = 0x1u32;
                {
                    let a = cpu.eax & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.eax = r;
                    }
                }
                cpu.edx = cpu.edi;
                cpu.ecx = cpu.ebx;
                cpu.edi = cpu.esi;
                {
                    let a = cpu.edi & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edi = r;
                    }
                }
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0));
                cpu.edi = cpu.edi.wrapping_add(cpu.edx);
                cpu.ecx = cpu.ecx.wrapping_add(cpu.edi.wrapping_mul(4));
                pc = 0xCF3320;
                continue;
            }
            0xCF3320 => {
                loop {
                    cpu.edi = cpu.eax.wrapping_mul(4);
                    cpu.ecx = cpu.ecx.wrapping_sub(cpu.edi);
                    cpu.edx = cpu.fl.alu_sub(cpu.edx, cpu.eax, 32);
                    cpu.edi = pages.u32(m, cpu.ebp.wrapping_add(0x10));
                    pages.set_u32(m, cpu.ecx, cpu.edi);
                    if !(!cpu.fl.zf()) {
                        break;
                    }
                }
                pc = 0xCF3332;
                continue;
            }
            0xCF3332 => {
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC));
                cpu.ecx = cpu.ecx.wrapping_sub(1);
                cpu.eax = 0x1u32;
                {
                    let a = cpu.eax & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.eax = r;
                    }
                }
                let condition = (((cpu.esi) & (cpu.eax)) as i32) == 0;
                pc = if condition { 0xCF3347 } else { 0xCF3341 };
                continue;
            }
            0xCF3341 => {
                loop {
                    cpu.eax = cpu.eax >> 0x1u32;
                    let condition = (((cpu.esi) & (cpu.eax)) as i32) != 0;
                    if !(condition) {
                        break;
                    }
                }
                pc = 0xCF3347;
                continue;
            }
            0xCF3347 => {
                let condition = (((cpu.eax) & (cpu.eax)) as i32) == 0;
                pc = if condition { 0xCF3359 } else { 0xCF334B };
                continue;
            }
            0xCF334B => {
                cpu.edx = cpu.eax.wrapping_add(0xFFFFFFFF);
                cpu.edx = cpu.edx & cpu.esi;
                cpu.edx = cpu.edx.wrapping_add(cpu.eax);
                cpu.esi = cpu.edx;
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.edx);
                pc = 0xCF335E;
                continue;
            }
            0xCF3359 => {
                cpu.esi = cpu.esi ^ cpu.esi;
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFDC), cpu.esi);
                pc = 0xCF335E;
                continue;
            }
            0xCF335E => {
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD4));
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC));
                cpu.edx = cpu.edx.wrapping_add(0x2u32);
                cpu.ecx = 0xFFFFu32;
                {
                    let r = cpu.fl.alu_add(
                        (pages.u16(
                            m,
                            cpu.ebp
                                .wrapping_add(cpu.eax.wrapping_mul(2))
                                .wrapping_add(0xFFFFFFA8),
                        ) as u32),
                        (cpu.ecx & 0xFFFF),
                        16,
                    );
                    pages.set_u16(
                        m,
                        cpu.ebp
                            .wrapping_add(cpu.eax.wrapping_mul(2))
                            .wrapping_add(0xFFFFFFA8),
                        (r) as u16,
                    );
                }
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD4), cpu.edx);
                pc = if (!cpu.fl.zf()) { 0xCF338C } else { 0xCF3376 };
                continue;
            }
            0xCF3376 => {
                let condition = (cpu.eax) == (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF4)));
                pc = if condition { 0xCF343D } else { 0xCF337F };
                continue;
            }
            0xCF337F => {
                cpu.eax = (pages.u16(m, cpu.edx) as u32);
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xC));
                cpu.eax = (pages.u16(m, cpu.ecx.wrapping_add(cpu.eax.wrapping_mul(2))) as u32);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC), cpu.eax);
                pc = 0xCF338C;
                continue;
            }
            0xCF338C => {
                let condition = (cpu.eax) <= (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8)));
                pc = if condition { 0xCF32C2 } else { 0xCF3395 };
                continue;
            }
            0xCF3395 => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFCC));
                cpu.eax = cpu.eax & cpu.esi;
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFC8), cpu.eax);
                let condition = (cpu.eax) == (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD0)));
                pc = if condition { 0xCF32C2 } else { 0xCF33A6 };
                continue;
            }
            0xCF33A6 => {
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0));
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD8));
                cpu.eax = cpu.ecx.wrapping_add(cpu.edx.wrapping_mul(4));
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC));
                let condition = (((cpu.ebx) & (cpu.ebx)) as i32) == 0;
                {
                    let v = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8));
                    if condition {
                        cpu.ebx = v;
                    }
                }
                cpu.ecx = cpu.ecx.wrapping_sub(cpu.ebx);
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0), cpu.eax);
                cpu.eax = 0x1u32;
                cpu.edx = cpu.ebx.wrapping_add(cpu.ecx.wrapping_mul(1));
                {
                    let a = cpu.eax & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.eax = r;
                    }
                }
                let condition = (cpu.edx) >= (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF4)));
                pc = if condition { 0xCF33E8 } else { 0xCF33CC };
                continue;
            }
            0xCF33CC => {
                cpu.esi = cpu
                    .ebp
                    .wrapping_add(cpu.edx.wrapping_mul(2))
                    .wrapping_add(0xFFFFFFA8);
                pc = 0xCF33D0;
                continue;
            }
            0xCF33D0 => {
                cpu.edi = (pages.u16(m, cpu.esi) as u32);
                cpu.eax = cpu.eax.wrapping_sub(cpu.edi);
                cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
                pc = if (cpu.fl.zf() || cpu.fl.sf() != cpu.fl.of()) {
                    0xCF33E5
                } else {
                    0xCF33D9
                };
                continue;
            }
            0xCF33D9 => {
                {
                    let a = cpu.edx & u32::MAX;
                    let r = a.wrapping_add(1) & u32::MAX;
                    cpu.edx = r;
                }
                cpu.ecx = cpu.ecx.wrapping_add(1);
                cpu.esi = cpu.esi.wrapping_add(0x2u32);
                cpu.eax = cpu.eax.wrapping_add(cpu.eax);
                let condition = (cpu.edx) < (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF4)));
                pc = if condition { 0xCF33D0 } else { 0xCF33E5 };
                continue;
            }
            0xCF33E5 => {
                cpu.esi = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFDC));
                pc = 0xCF33E8;
                continue;
            }
            0xCF33E8 => {
                cpu.eax = 0x1u32;
                cpu.edi = cpu.eax;
                {
                    let a = cpu.edi & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.edi = r;
                    }
                }
                {
                    let r = pages
                        .u32(m, cpu.ebp.wrapping_add(0xFFFFFFE8))
                        .wrapping_add(cpu.edi);
                    pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFE8), r);
                }
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD8), cpu.edi);
                let condition = (pages.u32(m, cpu.ebp.wrapping_add(0x8))) != (cpu.eax);
                pc = if condition { 0xCF3409 } else { 0xCF33FC };
                continue;
            }
            0xCF33FC => {
                cpu.fl
                    .alu_sub(pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFE8)), 0x506u32, 32);
                pc = if (!cpu.fl.cf()) { 0xCF34BE } else { 0xCF3409 };
                continue;
            }
            0xCF3409 => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFC8));
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.edx = pages.u32(m, cpu.edx);
                pages.set_u8(
                    m,
                    cpu.edx.wrapping_add(cpu.eax.wrapping_mul(4)),
                    ((cpu.ecx >> 0) & 0xFF) as u8,
                );
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.edx = pages.u32(m, cpu.ecx);
                cpu.ecx = (cpu.ecx & !(0xFF << 0))
                    | (((pages.u8(m, cpu.ebp.wrapping_add(0xFFFFFFF8)) as u32) & 0xFF) << 0);
                pages.set_u8(
                    m,
                    cpu.edx.wrapping_add(cpu.eax.wrapping_mul(4)).wrapping_add(0x1),
                    ((cpu.ecx >> 0) & 0xFF) as u8,
                );
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.ecx = pages.u32(m, cpu.edx);
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0));
                cpu.edx = cpu.edx.wrapping_sub(cpu.ecx);
                {
                    let a = cpu.edx & u32::MAX;
                    let n = 0x2u32;
                    let r = ((((a) as i32) >> n) as u32) & u32::MAX;
                    cpu.edx = r;
                }
                pages.set_u16(
                    m,
                    cpu.ecx.wrapping_add(cpu.eax.wrapping_mul(4)).wrapping_add(0x2),
                    (cpu.edx & 0xFFFF) as u16,
                );
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD4));
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFD0), cpu.eax);
                pc = 0xCF32C5;
                continue;
            }
            0xCF343D => {
                cpu.ecx = (cpu.ecx & !(0xFF << 0)) | ((((cpu.eax >> 0) & 0xFF) & 0xFF) << 0);
                {
                    let r = ((cpu.ecx >> 0) & 0xFF).wrapping_sub(((cpu.ebx >> 0) & 0xFF)) & 0xFF;
                    cpu.ecx = (cpu.ecx & !(0xFF << 0)) | (((r) & 0xFF) << 0);
                }
                cpu.edx = cpu.edx ^ cpu.edx;
                pages.set_u8(m, cpu.ebp.wrapping_add(0x10), (0x40u32) as u8);
                pages.set_u8(m, cpu.ebp.wrapping_add(0x11), ((cpu.ecx >> 0) & 0xFF) as u8);
                pages.set_u16(m, cpu.ebp.wrapping_add(0x12), (cpu.edx & 0xFFFF) as u16);
                let condition = (((cpu.esi) & (cpu.esi)) as i32) == 0;
                pc = if condition { 0xCF34A5 } else { 0xCF3452 };
                continue;
            }
            0xCF3452 => {
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF0));
                pc = 0xCF345A;
                continue;
            }
            0xCF3457 => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC));
                pc = 0xCF345A;
                continue;
            }
            0xCF345A => {
                let condition = (((cpu.ebx) & (cpu.ebx)) as i32) == 0;
                pc = if condition { 0xCF3478 } else { 0xCF345E };
                continue;
            }
            0xCF345E => {
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFCC));
                cpu.ecx = cpu.ecx & cpu.esi;
                let condition = (cpu.ecx) == (pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFD0)));
                pc = if condition { 0xCF3478 } else { 0xCF3468 };
                continue;
            }
            0xCF3468 => {
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.edx = pages.u32(m, cpu.edx);
                cpu.ebx = cpu.ebx ^ cpu.ebx;
                pages.set_u32(m, cpu.ebp.wrapping_add(0xFFFFFFFC), cpu.eax);
                pages.set_u8(m, cpu.ebp.wrapping_add(0x11), ((cpu.eax >> 0) & 0xFF) as u8);
                pc = 0xCF3478;
                continue;
            }
            0xCF3478 => {
                cpu.ecx = cpu.ebx;
                cpu.edi = cpu.esi;
                {
                    let a = cpu.edi & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a >> n) & u32::MAX;
                        cpu.edi = r;
                    }
                }
                cpu.ecx = pages.u32(m, cpu.ebp.wrapping_add(0x10));
                pages.set_u32(m, cpu.edx.wrapping_add(cpu.edi.wrapping_mul(4)), cpu.ecx);
                cpu.ecx = cpu.eax.wrapping_add(0xFFFFFFFF);
                cpu.eax = 0x1u32;
                {
                    let a = cpu.eax & u32::MAX;
                    let n = ((cpu.ecx >> 0) & 0xFF) & 31;
                    if n != 0 {
                        let r = (a << n) & u32::MAX;
                        cpu.eax = r;
                    }
                }
                let condition = (((cpu.esi) & (cpu.eax)) as i32) == 0;
                pc = if condition { 0xCF3498 } else { 0xCF3492 };
                continue;
            }
            0xCF3492 => {
                loop {
                    cpu.eax = cpu.eax >> 0x1u32;
                    let condition = (((cpu.esi) & (cpu.eax)) as i32) != 0;
                    if !(condition) {
                        break;
                    }
                }
                pc = 0xCF3498;
                continue;
            }
            0xCF3498 => {
                let condition = (((cpu.eax) & (cpu.eax)) as i32) == 0;
                pc = if condition { 0xCF34A5 } else { 0xCF349C };
                continue;
            }
            0xCF349C => {
                cpu.ecx = cpu.eax.wrapping_add(0xFFFFFFFF);
                cpu.esi = cpu.esi & cpu.ecx;
                cpu.esi = cpu.fl.alu_add(cpu.esi, cpu.eax, 32);
                pc = if (!cpu.fl.zf()) { 0xCF3457 } else { 0xCF34A5 };
                continue;
            }
            0xCF34A5 => {
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFE8));
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0x14));
                cpu.ecx = cpu.edx.wrapping_mul(4);
                {
                    let r = pages.u32(m, cpu.eax).wrapping_add(cpu.ecx);
                    pages.set_u32(m, cpu.eax, r);
                }
                cpu.eax = pages.u32(m, cpu.ebp.wrapping_add(0xFFFFFFF8));
                cpu.edx = pages.u32(m, cpu.ebp.wrapping_add(0x18));
                pages.set_u32(m, cpu.edx, cpu.eax);
                cpu.eax = cpu.fl.alu_xor(cpu.eax, cpu.eax, 32);
                pc = 0xCF34BE;
                continue;
            }
            0xCF34BE => {
                cpu.edi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esi = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.ebx = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.ebp;
                cpu.ebp = {
                    let value = pages.u32(m, cpu.esp);
                    cpu.esp = cpu.esp.wrapping_add(4);
                    value
                };
                cpu.esp = cpu.esp.wrapping_add(4 + 0);
                pages.flush(m);
                return;
            }
            _ => unreachable!("Huffman builder block {pc:#x}"),
        }
    }
}

// cf30c0 is a callee-free leaf. Its 0x88-byte frame and caller argument
// slots are disjoint from lengths/work/output. Keep local spill traffic in
// host storage and commit only bytes actually written before returning.
// This retains even the overwritten symbol_count argument and untouched
// padding, which the original-instruction regression checks byte for byte.
struct TableMemory {
    pages: Pages,
    base: u32,
    bytes: [u8; 0xa4],
    dirty: [bool; 0xa4],
}
impl TableMemory {
    fn new(m: &dyn Mem, base: u32) -> Self {
        // Only the six cdecl arguments are initially read. Every local used
        // by cf30c0 is written before it is read: saved registers, histogram,
        // prefix counts and scalar spills. Unused frame padding is not read.
        let mut bytes = [0; 0xa4];
        m.read_bytes(base.wrapping_add(0x8c), &mut bytes[0x8c..]);
        Self {
            pages: Pages::of(m),
            base,
            bytes,
            dirty: [false; 0xa4],
        }
    }
    #[inline]
    fn load(&self, m: &dyn Mem, address: u32, width: usize) -> u32 {
        let offset = address.wrapping_sub(self.base) as usize;
        if offset <= self.bytes.len() - width {
            let mut bytes = [0; 4];
            bytes[..width].copy_from_slice(&self.bytes[offset..offset + width]);
            u32::from_le_bytes(bytes)
        } else {
            self.pages.load(m, address, width)
        }
    }
    #[inline]
    fn store(&mut self, m: &mut dyn Mem, address: u32, width: usize, value: u32) {
        let offset = address.wrapping_sub(self.base) as usize;
        if offset <= self.bytes.len() - width {
            self.bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
            self.dirty[offset..offset + width].fill(true);
        } else {
            self.pages.store(m, address, width, value);
        }
    }
    #[inline]
    fn u8(&self, m: &dyn Mem, a: u32) -> u8 {
        self.load(m, a, 1) as u8
    }
    #[inline]
    fn u16(&self, m: &dyn Mem, a: u32) -> u16 {
        self.load(m, a, 2) as u16
    }
    #[inline]
    fn u32(&self, m: &dyn Mem, a: u32) -> u32 {
        self.load(m, a, 4)
    }
    #[inline]
    fn set_u8(&mut self, m: &mut dyn Mem, a: u32, v: u8) {
        self.store(m, a, 1, v as u32);
    }
    #[inline]
    fn set_u16(&mut self, m: &mut dyn Mem, a: u32, v: u16) {
        self.store(m, a, 2, v as u32);
    }
    #[inline]
    fn set_u32(&mut self, m: &mut dyn Mem, a: u32, v: u32) {
        self.store(m, a, 4, v);
    }
    fn flush(&self, m: &mut dyn Mem) {
        for i in 0..self.bytes.len() {
            if self.dirty[i] {
                self.pages
                    .set_u8(m, self.base.wrapping_add(i as u32), self.bytes[i]);
            }
        }
    }
}
