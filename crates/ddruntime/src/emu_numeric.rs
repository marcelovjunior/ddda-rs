//! Floating instructions whose host results/status the recorder preserves.
//! Reuse the same native numeric primitives as ddnative-base, not lifted
//! function bodies. Encodings follow the Intel SDM legacy SSE/x87 tables.

use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};

fn header(code: &[u8]) -> Option<(u8, u8, usize, bool, bool)> {
    let (mut prefix, mut fs, mut address16, mut k) = (0, false, false, 0);
    while let Some(&b) = code.get(k) {
        match b {
            0x66 | 0xf2 | 0xf3 => prefix = b,
            0x64 => fs = true,
            0x67 => address16 = true,
            0x26 | 0x2e | 0x36 | 0x3e | 0x65 | 0x9b => {}
            _ => break,
        }
        k += 1;
    }
    let op = *code.get(k)?;
    if op == 0x0f {
        Some((prefix, *code.get(k + 1)?, k + 2, fs, address16))
    } else if op == 0xd9 {
        Some((0xd9, *code.get(k + 1)?, k + 2, fs, address16))
    } else {
        None
    }
}

pub(super) fn handles(code: &[u8]) -> bool {
    let Some((p, op, _, _, _)) = header(code) else {
        return false;
    };
    if p == 0xd9 {
        return matches!(op, 0xe0 | 0xe1 | 0xe5 | 0xe8 | 0xea | 0xeb | 0xed | 0xee
            | 0xf0..=0xf3 | 0xf5 | 0xf8 | 0xfa | 0xfc..=0xff);
    }
    match op {
        0x51 | 0x58 | 0x59 | 0x5a | 0x5c..=0x5f | 0xc2 => matches!(p, 0 | 0x66 | 0xf2 | 0xf3),
        0x52 | 0x53 => matches!(p, 0 | 0xf3),
        0x2a | 0x2c | 0x2d => matches!(p, 0xf2 | 0xf3),
        0x2e | 0x2f => matches!(p, 0 | 0x66),
        0x5b => matches!(p, 0 | 0x66 | 0xf3),
        0xe6 => matches!(p, 0x66 | 0xf2 | 0xf3),
        _ => false,
    }
}

fn gprs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}
fn set_gpr(c: &mut Cpu, r: usize, v: u32) {
    *[
        &mut c.eax, &mut c.ecx, &mut c.edx, &mut c.ebx, &mut c.esp, &mut c.ebp, &mut c.esi, &mut c.edi,
    ][r] = v;
}

// Decode only the legacy ModRM/SIB operand of an already classified opcode.
fn operand(code: &[u8], k: &mut usize, c: &Cpu, fs: bool, address16: bool) -> (usize, Option<u32>) {
    let modrm = code[*k];
    *k += 1;
    let mode = modrm >> 6;
    let rm = (modrm & 7) as usize;
    if mode == 3 {
        return (rm, None);
    }
    let g = gprs(c);
    let mut disp = |n: usize| {
        let mut b = [0u8; 4];
        b[..n].copy_from_slice(&code[*k..*k + n]);
        *k += n;
        u32::from_le_bytes(b)
    };
    let mut address;
    if address16 {
        address = match rm {
            0 => g[3].wrapping_add(g[6]),
            1 => g[3].wrapping_add(g[7]),
            2 => g[5].wrapping_add(g[6]),
            3 => g[5].wrapping_add(g[7]),
            4 => g[6],
            5 => g[7],
            6 if mode == 0 => disp(2),
            6 => g[5],
            _ => g[3],
        };
        address = address.wrapping_add(match mode {
            1 => disp(1) as u8 as i8 as u32,
            2 => disp(2),
            _ => 0,
        }) & 0xffff;
    } else {
        if rm == 4 {
            let sib = disp(1) as u8;
            let base = (sib & 7) as usize;
            let index = ((sib >> 3) & 7) as usize;
            address = if base == 5 && mode == 0 { disp(4) } else { g[base] };
            if index != 4 {
                address = address.wrapping_add(g[index].wrapping_shl((sib >> 6) as u32));
            }
        } else {
            address = if rm == 5 && mode == 0 { disp(4) } else { g[rm] };
        }
        address = address.wrapping_add(match mode {
            1 => disp(1) as u8 as i8 as u32,
            2 => disp(4),
            _ => 0,
        });
    }
    if fs {
        address = address.wrapping_add(c.fs_base);
    }
    (rm, Some(address))
}

pub(super) fn execute(code: &[u8], m: &mut dyn Mem, c: &mut Cpu) {
    let (p, op, mut k, fs, address16) = header(code).unwrap();
    if p == 0xd9 {
        macro_rules! x87 { ($($b:literal),*) => { match op {
            $($b => c.st.register::<{0xd9 | ($b << 8)}>(),)* _ => unreachable!(),
        }}; }
        x87!(
            0xe0, 0xe1, 0xe5, 0xe8, 0xea, 0xeb, 0xed, 0xee, 0xf0, 0xf1, 0xf2, 0xf3, 0xf5, 0xf8, 0xfa, 0xfc,
            0xfd, 0xfe, 0xff
        );
        return;
    }
    assert_eq!(x86::stmxcsr() & 0x1f80, 0x1f80, "unmasked guest SSE exceptions");
    let dest = ((code[k] >> 3) & 7) as usize;
    let (reg, address) = operand(code, &mut k, c, fs, address16);
    let width = match (p, op) {
        (_, 0x2a) => 4,
        (0, 0x5a) | (0xf3, 0xe6) => 8,
        (0xf3, 0x5b) | (0xf2, 0xe6) => 16,
        (0xf3, _) => 4,
        (0xf2, _) => 8,
        (0, 0x2e | 0x2f) => 4,
        (0x66, 0x2e | 0x2f) => 8,
        _ => 16,
    };
    let b = if let Some(address) = address {
        let mut data = [0u8; 16];
        for (i, byte) in data[..width].iter_mut().enumerate() {
            *byte = m.u8(address.wrapping_add(i as u32));
        }
        std::array::from_fn(|i| u32::from_le_bytes(data[4 * i..4 * i + 4].try_into().unwrap()))
    } else if op == 0x2a {
        [gprs(c)[reg], 0, 0, 0]
    } else {
        c.xmm[reg]
    };
    let a = c.xmm[dest];
    let mut result = a;
    macro_rules! packed {
        ($ps:ident,$pd:ident,$ss:ident,$sd:ident) => {
            match p {
                0 => x86::$ps(a, b),
                0x66 => x86::$pd(a, b),
                0xf3 => {
                    let mut r = a;
                    r[0] = x86::$ss(a[0], b[0]);
                    r
                }
                0xf2 => x86::$sd(a, b),
                _ => unreachable!(),
            }
        };
    }
    match op {
        0x58 => result = packed!(addps, addpd, addss, addsd),
        0x59 => result = packed!(mulps, mulpd, mulss, mulsd),
        0x5c => result = packed!(subps, subpd, subss, subsd),
        0x5d => result = packed!(minps, minpd, minss, minsd),
        0x5e => result = packed!(divps, divpd, divss, divsd),
        0x5f => result = packed!(maxps, maxpd, maxss, maxsd),
        0x51 => {
            result = match p {
                0 => x86::sqrtps(a, b),
                0x66 => x86::sqrtpd(a, b),
                0xf2 => x86::sqrtsd(a, b),
                0xf3 => {
                    result[0] = x86::sqrtss(b[0]);
                    result
                }
                _ => unreachable!(),
            }
        }
        0x52 => {
            result = if p == 0 {
                x86::rsqrtps(a, b)
            } else {
                x86::rsqrtss(a, b)
            }
        }
        0x53 => {
            result = if p == 0 {
                x86::rcpps(a, b)
            } else {
                x86::rcpss(a, b)
            }
        }
        0x2a => {
            result = if p == 0xf2 {
                x86::cvtsi2sd(a, b[0])
            } else {
                result[0] = x86::cvtsi2ss(b[0]);
                result
            }
        }
        0x2c | 0x2d => {
            let v = match (p, op) {
                (0xf3, 0x2c) => x86::cvttss2si(b[0]),
                (0xf3, _) => x86::cvtss2si(b),
                (0xf2, 0x2c) => x86::cvttsd2si(b),
                _ => x86::cvtsd2si(b),
            };
            set_gpr(c, dest, v);
            return;
        }
        0x2e | 0x2f => {
            c.fl = match (p, op) {
                (0, 0x2e) => x86::comiss_hw::<false>(a[0], b[0]),
                (0, _) => x86::comiss_hw::<true>(a[0], b[0]),
                (_, 0x2e) => x86::comisd_hw::<false>(a, b),
                _ => x86::comisd_hw::<true>(a, b),
            };
            return;
        }
        0x5a => {
            result = match p {
                0 => x86::cvtps2pd(a, b),
                0x66 => x86::cvtpd2ps(a, b),
                0xf3 => x86::cvtss2sd(a, b),
                _ => x86::cvtsd2ss(a, b),
            }
        }
        0x5b => {
            result = match p {
                0 => x86::cvtdq2ps(a, b),
                0x66 => x86::cvtps2dq(a, b),
                _ => x86::cvttps2dq(a, b),
            }
        }
        0xe6 => {
            result = match p {
                0x66 => x86::cvttpd2dq(a, b),
                0xf2 => x86::cvtpd2dq(a, b),
                _ => x86::cvtdq2pd(a, b),
            }
        }
        0xc2 => {
            macro_rules! cmp {
                ($f:ident) => {
                    match code[k] & 7 {
                        0 => x86::$f::<0>(a, b),
                        1 => x86::$f::<1>(a, b),
                        2 => x86::$f::<2>(a, b),
                        3 => x86::$f::<3>(a, b),
                        4 => x86::$f::<4>(a, b),
                        5 => x86::$f::<5>(a, b),
                        6 => x86::$f::<6>(a, b),
                        _ => x86::$f::<7>(a, b),
                    }
                };
            }
            result = match p {
                0 => cmp!(compare_ps),
                0x66 => cmp!(compare_pd),
                0xf3 => cmp!(compare_ss),
                _ => cmp!(compare_sd),
            };
        }
        _ => unreachable!(),
    }
    c.xmm[dest] = result;
}
