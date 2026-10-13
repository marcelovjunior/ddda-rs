//! L184 (s4): mechanically translated semantic port (the joint rebase 0x0081FD10). Regenerate with
//! `research/native/semport.py 0x81fd10 0x82144d joint_rebase out.rs doc.txt`; the generated
//! body is below, one commented Rust statement per original instruction.
//! Verified against the original's recorded behaviour
//! (crates/mtcore/tests/semantic_s4.rs).
use super::semantic_s4_rt as rt;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

// Translated from 0x0081fd10 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport.py (operation order, widths and memory accesses
// of the original; registers, XMM registers and flags in locals).
/// 0x0081FD10, `object->rebase_joints(first, second)` (EDI = object, two
/// byte flags on the stack, `ret 8`, void; the names are guesses): for each
/// of the object's two joint ids (u32 at +0x24 and +0x28; 0xFFFF = none) whose flag
/// is set, find the joint record of the model at `object[4]` (index map at
/// model+0x368, 0xA0-byte records from +0x364; the parent record is the one
/// named by the byte at +0x91). The joint's 4x4 matrix (16 floats at
/// +0x10) is inverted by cofactors (the identity constants at 0x016295C0
/// when its determinant equals the constant at 0x01521690), the parent's
/// position (+0x40..+0x48) is carried through the inverse, then through the
/// matrix of the joint 0x00421ED0(8 or 0x14... looked up by index) when
/// that exists, and stored back with w = 1.0 (0x015319B8).

#[allow(unused_assignments, unused_variables, unused_mut, clippy::all)]
pub fn joint_rebase(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));
    let mut fr = [0f32; 43];
    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);
    let mut ld0 = 0u32; let mut ld1 = 0u32; let mut ld2 = 0u32; let mut ld3 = 0u32; let mut ld4 = 0u32; let mut ld5 = 0u32; let mut ld6 = 0u32; let mut ld7 = 0u32; let mut ld8 = 0u32; let mut ld9 = 0u32; let mut ld10 = 0u32; let mut ld11 = 0u32; let mut ld12 = 0u32; let mut ld13 = 0u32; let mut ld14 = 0u32; let mut ld15 = 0u32; let mut ld16 = 0u32; let mut ld17 = 0u32; let mut ld18 = 0u32; let mut ld19 = 0u32; let mut ld20 = 0u32; let mut ld21 = 0u32; let mut ld22 = 0u32; let mut ld23 = 0u32; let mut ld24 = 0u32; let mut ld25 = 0u32; let mut ld26 = 0u32; let mut ld27 = 0u32; let mut ld28 = 0u32; let mut ld29 = 0u32; let mut ld30 = 0u32; let mut ld31 = 0u32; let mut ld32 = 0u32; let mut ld33 = 0u32; let mut ld34 = 0u32; let mut ld35 = 0u32; let mut ld36 = 0u32; let mut ld37 = 0u32; let mut ld38 = 0u32; let mut ld39 = 0u32; let mut ld40 = 0u32; let mut ld41 = 0u32; let mut ld42 = 0u32; let mut ld43 = 0u32; let mut ld44 = 0u32; let mut ld45 = 0u32; let mut ld46 = 0u32; let mut ld47 = 0u32; let mut ld48 = 0u32; let mut ld49 = 0u32; let mut ld50 = 0u32; let mut ld51 = 0u32; let mut ld52 = 0u32; let mut ld53 = 0u32; let mut ld54 = 0u32; let mut ld55 = 0u32; let mut ld56 = 0u32; let mut ld57 = 0u32; let mut ld58 = 0u32; let mut ld59 = 0u32; let mut ld60 = 0u32; let mut ld61 = 0u32; let mut ld62 = 0u32; let mut ld63 = 0u32; let mut ld64 = 0u32; let mut ld65 = 0u32; let mut ld66 = 0u32; let mut ld67 = 0u32; let mut ld68 = 0u32; let mut ld69 = 0u32; let mut ld70 = 0u32; let mut ld71 = 0u32; let mut ld72 = 0u32; let mut ld73 = 0u32; let mut ld74 = 0u32; let mut ld75 = 0u32; let mut ld76 = 0u32; let mut ld77 = 0u32; let mut ld78 = 0u32; let mut ld79 = 0u32; let mut ld80 = 0u32; let mut ld81 = 0u32; let mut ld82 = 0u32; let mut ld83 = 0u32; let mut ld84 = 0u32; let mut ld85 = 0u32; let mut ld86 = 0u32; let mut ld87 = 0u32; let mut ld88 = 0u32; let mut ld89 = 0u32; let mut ld90 = 0u32; let mut ld91 = 0u32; let mut ld92 = 0u32; let mut ld93 = 0u32; let mut ld94 = 0u32; let mut ld95 = 0u32; let mut ld96 = 0u32; let mut ld97 = 0u32; let mut ld98 = 0u32; let mut ld99 = 0u32; let mut ld100 = 0u32; let mut ld101 = 0u32; let mut ld102 = 0u32; let mut ld103 = 0u32; let mut ld104 = 0u32; let mut ld105 = 0u32; let mut ld106 = 0u32; let mut ld107 = 0u32; let mut ld108 = 0u32; let mut ld109 = 0u32; let mut ld110 = 0u32; let mut ld111 = 0u32; let mut ld112 = 0u32; let mut ld113 = 0u32; let mut ld114 = 0u32; let mut ld115 = 0u32; let mut ld116 = 0u32; let mut ld117 = 0u32; let mut ld118 = 0u32; let mut ld119 = 0u32; let mut ld120 = 0u32; let mut ld121 = 0u32; let mut ld122 = 0u32; let mut ld123 = 0u32; let mut ld124 = 0u32; let mut ld125 = 0u32; let mut ld126 = 0u32; let mut ld127 = 0u32; let mut ld128 = 0u32; let mut ld129 = 0u32; let mut ld130 = 0u32; let mut ld131 = 0u32; let mut ld132 = 0u32; let mut ld133 = 0u32; let mut ld134 = 0u32; let mut ld135 = 0u32; let mut ld136 = 0u32; let mut ld137 = 0u32; let mut ld138 = 0u32; let mut ld139 = 0u32; let mut ld140 = 0u32; let mut ld141 = 0u32; let mut ld142 = 0u32; let mut ld143 = 0u32; let mut ld144 = 0u32; let mut ld145 = 0u32; let mut ld146 = 0u32; let mut ld147 = 0u32; let mut ld148 = 0u32; let mut ld149 = 0u32; let mut ld150 = 0u32; let mut ld151 = 0u32; let mut ld152 = 0u32; let mut ld153 = 0u32; let mut ld154 = 0u32; let mut ld155 = 0u32;
    let mut pc: u32 = 0x0081fd10;
    loop {
        match pc {
            0x0081fd10 => {
                // 0081fd10: push ebp
                { let v = ebp; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 0081fd11: mov ebp, esp
                ebp = esp;
                // 0081fd13: and esp, 0xfffffff0
                { let (a, b) = (esp, 0xfffffff0u32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                let r = (a & b) & 0xffffffffu32;
                esp = r;
                }
                // 0081fd16: mov eax, dword ptr [edi + 0x24]
                eax = { ld0 = pg.u32(m, edi.wrapping_add(0x24)); ld0 };
                // 0081fd19: movss xmm4, dword ptr [0x161c280]
                xmm[4] = [f32::from_bits({ ld1 = pg.u32(m, 0x0161c280u32); ld1 }), 0.0, 0.0, 0.0];
                // 0081fd21: sub esp, 0xac
                { let (a, b) = (esp, 0xacu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                let r = a.wrapping_sub(b) & 0xffffffffu32;
                esp = r;
                }
                // 0081fd27: push esi
                { let v = esi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 0081fd28: cmp eax, 0xffff
                { let (a, b) = (eax, 0xffffu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 0081fd2d: je 0x8208b3
                if zf { pc = 0x008208b3; continue; }
                pc = 0x0081fd33;
                continue;
            }
            0x0081fd33 => {
                // 0081fd33: cmp byte ptr [ebp + 8], 0
                { let (a, b) = ({ ld2 = pg.u8(m, ebp.wrapping_add(0x8)) as u32; ld2 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 0081fd37: je 0x8208b3
                if zf { pc = 0x008208b3; continue; }
                pc = 0x0081fd3d;
                continue;
            }
            0x0081fd3d => {
                // 0081fd3d: mov ecx, dword ptr [edi + 4]
                ecx = { ld3 = pg.u32(m, edi.wrapping_add(0x4)); ld3 };
                // 0081fd40: mov edx, dword ptr [ecx + 0x368]
                edx = { ld4 = pg.u32(m, ecx.wrapping_add(0x368)); ld4 };
                // 0081fd46: and eax, 0xff
                { let (a, b) = (eax, 0xffu32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                let r = (a & b) & 0xffffffffu32;
                eax = r;
                }
                // 0081fd4b: movzx eax, byte ptr [eax + edx]
                eax = { ld5 = pg.u8(m, eax.wrapping_add(edx)) as u32; ld5 };
                // 0081fd4f: cmp eax, 0xff
                { let (a, b) = (eax, 0xffu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 0081fd54: je 0x8208b3
                if zf { pc = 0x008208b3; continue; }
                pc = 0x0081fd5a;
                continue;
            }
            0x0081fd5a => {
                // 0081fd5a: lea edx, [eax + eax*4]
                edx = eax.wrapping_add(eax.wrapping_mul(4));
                // 0081fd5d: shl edx, 5
                { let a = edx;
                let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
                (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
                edx = r;
                }
                // 0081fd60: add edx, dword ptr [ecx + 0x364]
                { let (a, b) = (edx, { ld6 = pg.u32(m, ecx.wrapping_add(0x364)); ld6 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 0081fd66: je 0x8208b3
                if zf { pc = 0x008208b3; continue; }
                pc = 0x0081fd6c;
                continue;
            }
            0x0081fd6c => {
                // 0081fd6c: movzx eax, byte ptr [edx + 0x91]
                eax = { ld7 = pg.u8(m, edx.wrapping_add(0x91)) as u32; ld7 };
                // 0081fd73: lea esi, [eax + eax*4]
                esi = eax.wrapping_add(eax.wrapping_mul(4));
                // 0081fd76: shl esi, 5
                { let a = esi;
                let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
                (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
                esi = r;
                }
                // 0081fd79: add esi, dword ptr [ecx + 0x364]
                { let (a, b) = (esi, { ld8 = pg.u32(m, ecx.wrapping_add(0x364)); ld8 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 0081fd7f: je 0x8208b3
                if zf { pc = 0x008208b3; continue; }
                pc = 0x0081fd85;
                continue;
            }
            0x0081fd85 => {
                // 0081fd85: movss xmm2, dword ptr [edx + 0x38]
                xmm[2] = [f32::from_bits({ ld9 = pg.u32(m, edx.wrapping_add(0x38)); ld9 }), 0.0, 0.0, 0.0];
                // 0081fd8a: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits({ ld10 = pg.u32(m, edx.wrapping_add(0x2c)); ld10 }), 0.0, 0.0, 0.0];
                // 0081fd8f: movss xmm0, dword ptr [edx + 0x48]
                xmm[0] = [f32::from_bits({ ld11 = pg.u32(m, edx.wrapping_add(0x48)); ld11 }), 0.0, 0.0, 0.0];
                // 0081fd94: movss xmm6, dword ptr [edx + 0x44]
                xmm[6] = [f32::from_bits({ ld12 = pg.u32(m, edx.wrapping_add(0x44)); ld12 }), 0.0, 0.0, 0.0];
                // 0081fd99: movss xmm5, dword ptr [edx + 0x4c]
                xmm[5] = [f32::from_bits({ ld13 = pg.u32(m, edx.wrapping_add(0x4c)); ld13 }), 0.0, 0.0, 0.0];
                // 0081fd9e: movss xmm1, dword ptr [edx + 0x24]
                xmm[1] = [f32::from_bits({ ld14 = pg.u32(m, edx.wrapping_add(0x24)); ld14 }), 0.0, 0.0, 0.0];
                // 0081fda3: movaps xmm3, xmm7
                xmm[3] = xmm[7];
                // 0081fda6: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits({ ld15 = pg.u32(m, edx.wrapping_add(0x34)); ld15 }));
                // 0081fdab: mulss xmm3, xmm2
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
                // 0081fdaf: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 0081fdb3: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 0081fdb7: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 0081fdbb: movss dword ptr [esp + 0x18], xmm3
                fr[5] = xmm[3][0];
                // 0081fdc1: movaps xmm3, xmm0
                xmm[3] = xmm[0];
                // 0081fdc4: mulss xmm3, dword ptr [edx + 0x3c]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits({ ld16 = pg.u32(m, edx.wrapping_add(0x3c)); ld16 }));
                // 0081fdc9: movss dword ptr [esp + 0x10], xmm3
                fr[3] = xmm[3][0];
                // 0081fdcf: movss xmm3, dword ptr [edx + 0x28]
                xmm[3] = [f32::from_bits({ ld17 = pg.u32(m, edx.wrapping_add(0x28)); ld17 }), 0.0, 0.0, 0.0];
                // 0081fdd4: mulss xmm3, dword ptr [edx + 0x3c]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld16));
                // 0081fdd9: mulss xmm3, xmm6
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[6][0]);
                // 0081fddd: addss xmm3, xmm1
                xmm[3][0] = rt::addss(xmm[3][0], xmm[1][0]);
                // 0081fde1: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 0081fde7: addss xmm3, xmm7
                xmm[3][0] = rt::addss(xmm[3][0], xmm[7][0]);
                // 0081fdeb: movss xmm7, dword ptr [esp + 0x10]
                xmm[7] = [fr[3], 0.0, 0.0, 0.0];
                // 0081fdf1: mulss xmm7, dword ptr [edx + 0x24]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld14));
                // 0081fdf6: mulss xmm1, xmm6
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
                // 0081fdfa: subss xmm3, xmm1
                xmm[3][0] = rt::subss(xmm[3][0], xmm[1][0]);
                // 0081fdfe: movss xmm1, dword ptr [edx + 0x14]
                xmm[1] = [f32::from_bits({ ld18 = pg.u32(m, edx.wrapping_add(0x14)); ld18 }), 0.0, 0.0, 0.0];
                // 0081fe03: subss xmm3, xmm7
                xmm[3][0] = rt::subss(xmm[3][0], xmm[7][0]);
                // 0081fe07: movss xmm7, dword ptr [edx + 0x28]
                xmm[7] = [f32::from_bits(ld17), 0.0, 0.0, 0.0];
                // 0081fe0c: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 0081fe10: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld15));
                // 0081fe15: subss xmm3, xmm7
                xmm[3][0] = rt::subss(xmm[3][0], xmm[7][0]);
                // 0081fe19: movss xmm7, dword ptr [edx + 0x18]
                xmm[7] = [f32::from_bits({ ld19 = pg.u32(m, edx.wrapping_add(0x18)); ld19 }), 0.0, 0.0, 0.0];
                // 0081fe1e: movss dword ptr [esp + 0x20], xmm7
                fr[7] = xmm[7][0];
                // 0081fe24: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld16));
                // 0081fe29: movss dword ptr [esp + 0x28], xmm1
                fr[9] = xmm[1][0];
                // 0081fe2f: movss xmm1, dword ptr [edx + 0x1c]
                xmm[1] = [f32::from_bits({ ld20 = pg.u32(m, edx.wrapping_add(0x1c)); ld20 }), 0.0, 0.0, 0.0];
                // 0081fe34: movss dword ptr [esp + 0x24], xmm1
                fr[8] = xmm[1][0];
                // 0081fe3a: movss xmm1, dword ptr [esp + 0x20]
                xmm[1] = [fr[7], 0.0, 0.0, 0.0];
                // 0081fe40: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 0081fe44: movss dword ptr [esp + 0x30], xmm7
                fr[11] = xmm[7][0];
                // 0081fe4a: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 0081fe4e: movss dword ptr [esp + 0x2c], xmm1
                fr[10] = xmm[1][0];
                // 0081fe54: movss xmm1, dword ptr [esp + 0x28]
                xmm[1] = [fr[9], 0.0, 0.0, 0.0];
                // 0081fe5a: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 0081fe5e: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 0081fe62: movss xmm5, dword ptr [esp + 0x24]
                xmm[5] = [fr[8], 0.0, 0.0, 0.0];
                // 0081fe68: mulss xmm5, dword ptr [edx + 0x24]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld14));
                // 0081fe6d: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 0081fe71: movss xmm7, dword ptr [esp + 0x24]
                xmm[7] = [fr[8], 0.0, 0.0, 0.0];
                // 0081fe77: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld15));
                // 0081fe7c: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 0081fe80: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 0081fe84: movss xmm7, dword ptr [esp + 0x24]
                xmm[7] = [fr[8], 0.0, 0.0, 0.0];
                // 0081fe8a: mulss xmm7, xmm2
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
                // 0081fe8e: movss xmm2, dword ptr [esp + 0x28]
                xmm[2] = [fr[9], 0.0, 0.0, 0.0];
                // 0081fe94: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 0081fe98: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 0081fe9c: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 0081fe9f: mulss xmm7, dword ptr [esp + 0x10]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[3]);
                // 0081fea5: mulss xmm2, dword ptr [edx + 0x28]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld17));
                // 0081feaa: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 0081feae: movss xmm7, dword ptr [esp + 0x2c]
                xmm[7] = [fr[10], 0.0, 0.0, 0.0];
                // 0081feb4: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld15));
                // 0081feb9: movss dword ptr [esp + 0x10], xmm2
                fr[3] = xmm[2][0];
                // 0081febf: movss xmm2, dword ptr [esp + 0x20]
                xmm[2] = [fr[7], 0.0, 0.0, 0.0];
                // 0081fec5: mulss xmm2, dword ptr [edx + 0x2c]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld10));
                // 0081feca: movss dword ptr [esp + 0x20], xmm5
                fr[7] = xmm[5][0];
                // 0081fed0: movss xmm5, dword ptr [esp + 0x24]
                xmm[5] = [fr[8], 0.0, 0.0, 0.0];
                // 0081fed6: mulss xmm5, dword ptr [edx + 0x28]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld17));
                // 0081fedb: movss dword ptr [esp + 0x1c], xmm5
                fr[6] = xmm[5][0];
                // 0081fee1: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 0081fee5: movss xmm7, dword ptr [esp + 0x10]
                xmm[7] = [fr[3], 0.0, 0.0, 0.0];
                // 0081feeb: mulss xmm7, dword ptr [edx + 0x4c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld13));
                // 0081fef0: movaps xmm5, xmm2
                xmm[5] = xmm[2];
                // 0081fef3: mulss xmm5, xmm6
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[6][0]);
                // 0081fef7: movss xmm6, dword ptr [esp + 0x20]
                xmm[6] = [fr[7], 0.0, 0.0, 0.0];
                // 0081fefd: addss xmm5, xmm7
                xmm[5][0] = rt::addss(xmm[5][0], xmm[7][0]);
                // 0081ff01: mulss xmm6, xmm0
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[0][0]);
                // 0081ff05: xorps xmm1, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
                // 0081ff08: addss xmm5, xmm6
                xmm[5][0] = rt::addss(xmm[5][0], xmm[6][0]);
                // 0081ff0c: mulss xmm0, dword ptr [edx + 0x2c]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld10));
                // 0081ff11: movss xmm7, dword ptr [esp + 0x1c]
                xmm[7] = [fr[6], 0.0, 0.0, 0.0];
                // 0081ff17: mulss xmm7, dword ptr [edx + 0x44]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld12));
                // 0081ff1c: movss xmm6, dword ptr [esp + 0x28]
                xmm[6] = [fr[9], 0.0, 0.0, 0.0];
                // 0081ff22: mulss xmm2, dword ptr [edx + 0x34]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld15));
                // 0081ff27: mulss xmm0, xmm6
                xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
                // 0081ff2b: mulss xmm6, dword ptr [esp + 0x18]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[5]);
                // 0081ff31: subss xmm5, xmm7
                xmm[5][0] = rt::subss(xmm[5][0], xmm[7][0]);
                // 0081ff35: subss xmm5, xmm0
                xmm[5][0] = rt::subss(xmm[5][0], xmm[0][0]);
                // 0081ff39: movss xmm0, dword ptr [esp + 0x2c]
                xmm[0] = [fr[10], 0.0, 0.0, 0.0];
                // 0081ff3f: mulss xmm0, dword ptr [edx + 0x24]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld14));
                // 0081ff44: subss xmm5, xmm0
                xmm[5][0] = rt::subss(xmm[5][0], xmm[0][0]);
                // 0081ff48: movss xmm0, dword ptr [esp + 0x10]
                xmm[0] = [fr[3], 0.0, 0.0, 0.0];
                // 0081ff4e: mulss xmm0, dword ptr [edx + 0x3c]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld16));
                // 0081ff53: addss xmm2, xmm0
                xmm[2][0] = rt::addss(xmm[2][0], xmm[0][0]);
                // 0081ff57: movss xmm0, dword ptr [esp + 0x20]
                xmm[0] = [fr[7], 0.0, 0.0, 0.0];
                // 0081ff5d: mulss xmm0, dword ptr [edx + 0x38]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld9));
                // 0081ff62: addss xmm2, xmm0
                xmm[2][0] = rt::addss(xmm[2][0], xmm[0][0]);
                // 0081ff66: movss xmm0, dword ptr [esp + 0x1c]
                xmm[0] = [fr[6], 0.0, 0.0, 0.0];
                // 0081ff6c: mulss xmm0, dword ptr [edx + 0x34]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld15));
                // 0081ff71: subss xmm2, xmm0
                xmm[2][0] = rt::subss(xmm[2][0], xmm[0][0]);
                // 0081ff75: movss xmm0, dword ptr [esp + 0x30]
                xmm[0] = [fr[11], 0.0, 0.0, 0.0];
                // 0081ff7b: mulss xmm0, dword ptr [edx + 0x24]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld14));
                // 0081ff80: subss xmm2, xmm6
                xmm[2][0] = rt::subss(xmm[2][0], xmm[6][0]);
                // 0081ff84: subss xmm2, xmm0
                xmm[2][0] = rt::subss(xmm[2][0], xmm[0][0]);
                // 0081ff88: xorps xmm2, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[2][k] = f32::from_bits(xmm[2][k].to_bits() ^ s[k].to_bits()); } }
                // 0081ff8b: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 0081ff8e: movss xmm2, dword ptr [edx + 0x10]
                xmm[2] = [f32::from_bits({ ld21 = pg.u32(m, edx.wrapping_add(0x10)); ld21 }), 0.0, 0.0, 0.0];
                // 0081ff93: movaps xmm6, xmm2
                xmm[6] = xmm[2];
                // 0081ff96: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 0081ff9a: movaps xmm0, xmm1
                xmm[0] = xmm[1];
                // 0081ff9d: mulss xmm0, dword ptr [edx + 0x20]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld22 = pg.u32(m, edx.wrapping_add(0x20)); ld22 }));
                // 0081ffa2: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 0081ffa6: movaps xmm0, xmm5
                xmm[0] = xmm[5];
                // 0081ffa9: mulss xmm0, dword ptr [edx + 0x30]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld23 = pg.u32(m, edx.wrapping_add(0x30)); ld23 }));
                // 0081ffae: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 0081ffb2: movss xmm0, dword ptr [edx + 0x40]
                xmm[0] = [f32::from_bits({ ld24 = pg.u32(m, edx.wrapping_add(0x40)); ld24 }), 0.0, 0.0, 0.0];
                // 0081ffb7: mulss xmm0, xmm7
                xmm[0][0] = rt::mulss(xmm[0][0], xmm[7][0]);
                // 0081ffbb: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 0081ffbf: ucomiss xmm6, dword ptr [0x1521690]
                { let (c_f, z_f, p_f) = rt::comiss(xmm[6][0], f32::from_bits({ ld25 = pg.u32(m, 0x01521690u32); ld25 })); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
                // 0081ffc6: lahf 
                eax = (eax & 0xffff_00ff) | ((((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32)) << 8);
                // 0081ffc7: test ah, 0x44
                { let (a, b) = (((eax >> 8) & 0xff), 0x44u32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
                }
                // 0081ffca: jp 0x8200c1
                if pf { pc = 0x008200c1; continue; }
                pc = 0x0081ffd0;
                continue;
            }
            0x0081ffd0 => {
                // 0081ffd0: movss xmm0, dword ptr [0x16295c0]
                xmm[0] = [f32::from_bits({ ld26 = pg.u32(m, 0x016295c0u32); ld26 }), 0.0, 0.0, 0.0];
                // 0081ffd8: movss dword ptr [esp + 0x50], xmm0
                fr[19] = xmm[0][0];
                // 0081ffde: movss xmm0, dword ptr [0x16295c4]
                xmm[0] = [f32::from_bits({ ld27 = pg.u32(m, 0x016295c4u32); ld27 }), 0.0, 0.0, 0.0];
                // 0081ffe6: movss dword ptr [esp + 0x54], xmm0
                fr[20] = xmm[0][0];
                // 0081ffec: movss xmm0, dword ptr [0x16295c8]
                xmm[0] = [f32::from_bits({ ld28 = pg.u32(m, 0x016295c8u32); ld28 }), 0.0, 0.0, 0.0];
                // 0081fff4: movss dword ptr [esp + 0x58], xmm0
                fr[21] = xmm[0][0];
                // 0081fffa: movss xmm0, dword ptr [0x16295cc]
                xmm[0] = [f32::from_bits({ ld29 = pg.u32(m, 0x016295ccu32); ld29 }), 0.0, 0.0, 0.0];
                // 00820002: movss dword ptr [esp + 0x5c], xmm0
                fr[22] = xmm[0][0];
                // 00820008: movss xmm0, dword ptr [0x16295d0]
                xmm[0] = [f32::from_bits({ ld30 = pg.u32(m, 0x016295d0u32); ld30 }), 0.0, 0.0, 0.0];
                // 00820010: movss dword ptr [esp + 0x60], xmm0
                fr[23] = xmm[0][0];
                // 00820016: movss xmm0, dword ptr [0x16295d4]
                xmm[0] = [f32::from_bits({ ld31 = pg.u32(m, 0x016295d4u32); ld31 }), 0.0, 0.0, 0.0];
                // 0082001e: movss dword ptr [esp + 0x64], xmm0
                fr[24] = xmm[0][0];
                // 00820024: movss xmm0, dword ptr [0x16295d8]
                xmm[0] = [f32::from_bits({ ld32 = pg.u32(m, 0x016295d8u32); ld32 }), 0.0, 0.0, 0.0];
                // 0082002c: movss dword ptr [esp + 0x68], xmm0
                fr[25] = xmm[0][0];
                // 00820032: movss xmm0, dword ptr [0x16295dc]
                xmm[0] = [f32::from_bits({ ld33 = pg.u32(m, 0x016295dcu32); ld33 }), 0.0, 0.0, 0.0];
                // 0082003a: movss dword ptr [esp + 0x6c], xmm0
                fr[26] = xmm[0][0];
                // 00820040: movss xmm0, dword ptr [0x16295e0]
                xmm[0] = [f32::from_bits({ ld34 = pg.u32(m, 0x016295e0u32); ld34 }), 0.0, 0.0, 0.0];
                // 00820048: movss dword ptr [esp + 0x70], xmm0
                fr[27] = xmm[0][0];
                // 0082004e: movss xmm0, dword ptr [0x16295e4]
                xmm[0] = [f32::from_bits({ ld35 = pg.u32(m, 0x016295e4u32); ld35 }), 0.0, 0.0, 0.0];
                // 00820056: movss dword ptr [esp + 0x74], xmm0
                fr[28] = xmm[0][0];
                // 0082005c: movss xmm0, dword ptr [0x16295e8]
                xmm[0] = [f32::from_bits({ ld36 = pg.u32(m, 0x016295e8u32); ld36 }), 0.0, 0.0, 0.0];
                // 00820064: movss dword ptr [esp + 0x78], xmm0
                fr[29] = xmm[0][0];
                // 0082006a: movss xmm0, dword ptr [0x16295ec]
                xmm[0] = [f32::from_bits({ ld37 = pg.u32(m, 0x016295ecu32); ld37 }), 0.0, 0.0, 0.0];
                // 00820072: movss dword ptr [esp + 0x7c], xmm0
                fr[30] = xmm[0][0];
                // 00820078: movss xmm0, dword ptr [0x16295f0]
                xmm[0] = [f32::from_bits({ ld38 = pg.u32(m, 0x016295f0u32); ld38 }), 0.0, 0.0, 0.0];
                // 00820080: movss dword ptr [esp + 0x80], xmm0
                fr[31] = xmm[0][0];
                // 00820089: movss xmm0, dword ptr [0x16295f4]
                xmm[0] = [f32::from_bits({ ld39 = pg.u32(m, 0x016295f4u32); ld39 }), 0.0, 0.0, 0.0];
                // 00820091: movss dword ptr [esp + 0x84], xmm0
                fr[32] = xmm[0][0];
                // 0082009a: movss xmm0, dword ptr [0x16295f8]
                xmm[0] = [f32::from_bits({ ld40 = pg.u32(m, 0x016295f8u32); ld40 }), 0.0, 0.0, 0.0];
                // 008200a2: movss dword ptr [esp + 0x88], xmm0
                fr[33] = xmm[0][0];
                // 008200ab: movss xmm0, dword ptr [0x16295fc]
                xmm[0] = [f32::from_bits({ ld41 = pg.u32(m, 0x016295fcu32); ld41 }), 0.0, 0.0, 0.0];
                // 008200b3: movss dword ptr [esp + 0x8c], xmm0
                fr[34] = xmm[0][0];
                // 008200bc: jmp 0x8207ab
                pc = 0x008207ab;
                continue;
            }
            0x008200c1 => {
                // 008200c1: movss xmm0, dword ptr [0x15319b8]
                xmm[0] = [f32::from_bits({ ld42 = pg.u32(m, 0x015319b8u32); ld42 }), 0.0, 0.0, 0.0];
                // 008200c9: divss xmm0, xmm6
                xmm[0][0] = rt::divss(xmm[0][0], xmm[6][0]);
                // 008200cd: movaps xmm6, xmm0
                xmm[6] = xmm[0];
                // 008200d0: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 008200d4: movss dword ptr [esp + 0x50], xmm6
                fr[19] = xmm[6][0];
                // 008200da: movss xmm6, dword ptr [edx + 0x20]
                xmm[6] = [f32::from_bits({ ld43 = pg.u32(m, edx.wrapping_add(0x20)); ld43 }), 0.0, 0.0, 0.0];
                // 008200df: mulss xmm6, dword ptr [edx + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits({ ld44 = pg.u32(m, edx.wrapping_add(0x38)); ld44 }));
                // 008200e4: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits({ ld45 = pg.u32(m, edx.wrapping_add(0x4c)); ld45 }));
                // 008200e9: movaps xmm3, xmm0
                xmm[3] = xmm[0];
                // 008200ec: mulss xmm3, xmm1
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
                // 008200f0: movaps xmm1, xmm0
                xmm[1] = xmm[0];
                // 008200f3: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 008200f7: movss xmm5, dword ptr [edx + 0x40]
                xmm[5] = [f32::from_bits({ ld46 = pg.u32(m, edx.wrapping_add(0x40)); ld46 }), 0.0, 0.0, 0.0];
                // 008200fc: movss dword ptr [esp + 0x58], xmm1
                fr[21] = xmm[1][0];
                // 00820102: movaps xmm1, xmm0
                xmm[1] = xmm[0];
                // 00820105: mulss xmm1, xmm7
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[7][0]);
                // 00820109: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits({ ld47 = pg.u32(m, edx.wrapping_add(0x2c)); ld47 }), 0.0, 0.0, 0.0];
                // 0082010e: movss dword ptr [esp + 0x54], xmm3
                fr[20] = xmm[3][0];
                // 00820114: movss xmm3, dword ptr [edx + 0x30]
                xmm[3] = [f32::from_bits({ ld48 = pg.u32(m, edx.wrapping_add(0x30)); ld48 }), 0.0, 0.0, 0.0];
                // 00820119: mulss xmm7, xmm3
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[3][0]);
                // 0082011d: movss dword ptr [esp + 0x28], xmm7
                fr[9] = xmm[7][0];
                // 00820123: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits(ld47), 0.0, 0.0, 0.0];
                // 00820128: mulss xmm7, dword ptr [edx + 0x38]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld44));
                // 0082012d: movss dword ptr [esp + 0x2c], xmm7
                fr[10] = xmm[7][0];
                // 00820133: movss xmm7, dword ptr [edx + 0x48]
                xmm[7] = [f32::from_bits({ ld49 = pg.u32(m, edx.wrapping_add(0x48)); ld49 }), 0.0, 0.0, 0.0];
                // 00820138: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits({ ld50 = pg.u32(m, edx.wrapping_add(0x3c)); ld50 }));
                // 0082013d: movss dword ptr [esp + 0x18], xmm7
                fr[5] = xmm[7][0];
                // 00820143: movss xmm7, dword ptr [edx + 0x28]
                xmm[7] = [f32::from_bits({ ld51 = pg.u32(m, edx.wrapping_add(0x28)); ld51 }), 0.0, 0.0, 0.0];
                // 00820148: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld50));
                // 0082014d: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820151: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820155: movss xmm6, dword ptr [esp + 0x18]
                xmm[6] = [fr[5], 0.0, 0.0, 0.0];
                // 0082015b: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld43));
                // 00820160: movss dword ptr [esp + 0x5c], xmm1
                fr[22] = xmm[1][0];
                // 00820166: movss xmm1, dword ptr [edx + 0x48]
                xmm[1] = [f32::from_bits(ld49), 0.0, 0.0, 0.0];
                // 0082016b: mulss xmm1, dword ptr [esp + 0x28]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[9]);
                // 00820171: addss xmm7, xmm1
                xmm[7][0] = rt::addss(xmm[7][0], xmm[1][0]);
                // 00820175: movss xmm1, dword ptr [esp + 0x2c]
                xmm[1] = [fr[10], 0.0, 0.0, 0.0];
                // 0082017b: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 0082017f: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820183: movss xmm1, dword ptr [edx + 0x4c]
                xmm[1] = [f32::from_bits(ld45), 0.0, 0.0, 0.0];
                // 00820188: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082018c: movss xmm6, dword ptr [edx + 0x28]
                xmm[6] = [f32::from_bits(ld51), 0.0, 0.0, 0.0];
                // 00820191: mulss xmm6, xmm1
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[1][0]);
                // 00820195: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820199: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082019d: movss xmm6, dword ptr [edx + 0x18]
                xmm[6] = [f32::from_bits({ ld52 = pg.u32(m, edx.wrapping_add(0x18)); ld52 }), 0.0, 0.0, 0.0];
                // 008201a2: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008201a6: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 008201a9: movss dword ptr [esp + 0x60], xmm7
                fr[23] = xmm[7][0];
                // 008201af: movss xmm7, dword ptr [edx + 0x1c]
                xmm[7] = [f32::from_bits({ ld53 = pg.u32(m, edx.wrapping_add(0x1c)); ld53 }), 0.0, 0.0, 0.0];
                // 008201b4: mulss xmm7, xmm3
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[3][0]);
                // 008201b8: movss dword ptr [esp + 0x14], xmm7
                fr[4] = xmm[7][0];
                // 008201be: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 008201c4: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld50));
                // 008201c9: movss xmm7, dword ptr [esp + 0x10]
                xmm[7] = [fr[3], 0.0, 0.0, 0.0];
                // 008201cf: mulss xmm7, xmm1
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[1][0]);
                // 008201d3: movss dword ptr [esp + 0x20], xmm7
                fr[7] = xmm[7][0];
                // 008201d9: movss xmm7, dword ptr [edx + 0x38]
                xmm[7] = [f32::from_bits(ld44), 0.0, 0.0, 0.0];
                // 008201de: mulss xmm7, xmm2
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
                // 008201e2: mulss xmm7, xmm1
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[1][0]);
                // 008201e6: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 008201ec: movss dword ptr [esp + 0x38], xmm6
                fr[13] = xmm[6][0];
                // 008201f2: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008201f6: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 008201fa: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00820200: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld49));
                // 00820205: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820209: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld53), 0.0, 0.0, 0.0];
                // 0082020e: mulss xmm6, dword ptr [edx + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld44));
                // 00820213: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 00820217: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 0082021b: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082021f: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820223: movss xmm1, dword ptr [esp + 0x20]
                xmm[1] = [fr[7], 0.0, 0.0, 0.0];
                // 00820229: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 0082022d: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820231: movss xmm1, dword ptr [esp + 0x10]
                xmm[1] = [fr[3], 0.0, 0.0, 0.0];
                // 00820237: mulss xmm1, dword ptr [edx + 0x2c]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld47));
                // 0082023c: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820240: movss dword ptr [esp + 0x64], xmm7
                fr[24] = xmm[7][0];
                // 00820246: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld53), 0.0, 0.0, 0.0];
                // 0082024b: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld43));
                // 00820250: movss dword ptr [esp + 0x24], xmm6
                fr[8] = xmm[6][0];
                // 00820256: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld53), 0.0, 0.0, 0.0];
                // 0082025b: mulss xmm6, dword ptr [edx + 0x28]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld51));
                // 00820260: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 00820266: movss dword ptr [esp + 0x18], xmm1
                fr[5] = xmm[1][0];
                // 0082026c: movss xmm1, dword ptr [edx + 0x28]
                xmm[1] = [f32::from_bits(ld51), 0.0, 0.0, 0.0];
                // 00820271: movss xmm7, dword ptr [esp + 0x18]
                xmm[7] = [fr[5], 0.0, 0.0, 0.0];
                // 00820277: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 0082027b: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 0082027f: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00820282: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld45));
                // 00820287: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 0082028b: movss xmm6, dword ptr [esp + 0x24]
                xmm[6] = [fr[8], 0.0, 0.0, 0.0];
                // 00820291: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld49));
                // 00820296: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 0082029a: movss xmm6, dword ptr [esp + 0x10]
                xmm[6] = [fr[3], 0.0, 0.0, 0.0];
                // 008202a0: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008202a4: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008202a8: movss xmm6, dword ptr [edx + 0x48]
                xmm[6] = [f32::from_bits(ld49), 0.0, 0.0, 0.0];
                // 008202ad: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld47));
                // 008202b2: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 008202b6: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008202ba: movss dword ptr [esp + 0x30], xmm1
                fr[11] = xmm[1][0];
                // 008202c0: movss xmm1, dword ptr [esp + 0x20]
                xmm[1] = [fr[7], 0.0, 0.0, 0.0];
                // 008202c6: mulss xmm1, dword ptr [edx + 0x20]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld43));
                // 008202cb: movss xmm6, dword ptr [esp + 0x30]
                xmm[6] = [fr[11], 0.0, 0.0, 0.0];
                // 008202d1: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld50));
                // 008202d6: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 008202da: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 008202e0: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008202e4: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 008202e7: movss dword ptr [esp + 0x68], xmm7
                fr[25] = xmm[7][0];
                // 008202ed: movss xmm7, dword ptr [esp + 0x24]
                xmm[7] = [fr[8], 0.0, 0.0, 0.0];
                // 008202f3: mulss xmm7, dword ptr [edx + 0x38]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld44));
                // 008202f8: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 008202fc: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 00820300: movss xmm6, dword ptr [esp + 0x10]
                xmm[6] = [fr[3], 0.0, 0.0, 0.0];
                // 00820306: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 0082030a: movss xmm7, dword ptr [edx + 0x44]
                xmm[7] = [f32::from_bits({ ld54 = pg.u32(m, edx.wrapping_add(0x44)); ld54 }), 0.0, 0.0, 0.0];
                // 0082030f: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld50));
                // 00820314: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820318: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 0082031c: movss xmm6, dword ptr [esp + 0x2c]
                xmm[6] = [fr[10], 0.0, 0.0, 0.0];
                // 00820322: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00820326: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 0082032a: movss xmm6, dword ptr [esp + 0x38]
                xmm[6] = [fr[13], 0.0, 0.0, 0.0];
                // 00820330: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld43));
                // 00820335: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00820339: movss xmm6, dword ptr [edx + 0x34]
                xmm[6] = [f32::from_bits({ ld55 = pg.u32(m, edx.wrapping_add(0x34)); ld55 }), 0.0, 0.0, 0.0];
                // 0082033e: movss dword ptr [esp + 0x1c], xmm6
                fr[6] = xmm[6][0];
                // 00820344: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00820348: movss dword ptr [esp + 0x18], xmm7
                fr[5] = xmm[7][0];
                // 0082034e: movss dword ptr [esp + 0x6c], xmm1
                fr[26] = xmm[1][0];
                // 00820354: movaps xmm1, xmm6
                xmm[1] = xmm[6];
                // 00820357: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld47));
                // 0082035c: mulss xmm1, dword ptr [edx + 0x20]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld43));
                // 00820361: movss dword ptr [esp + 0x2c], xmm6
                fr[10] = xmm[6][0];
                // 00820367: movss dword ptr [esp + 0xc], xmm1
                fr[2] = xmm[1][0];
                // 0082036d: movss xmm1, dword ptr [edx + 0x24]
                xmm[1] = [f32::from_bits({ ld56 = pg.u32(m, edx.wrapping_add(0x24)); ld56 }), 0.0, 0.0, 0.0];
                // 00820372: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 00820378: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld45));
                // 0082037d: movaps xmm7, xmm1
                xmm[7] = xmm[1];
                // 00820380: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld50));
                // 00820385: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820389: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld54), 0.0, 0.0, 0.0];
                // 0082038e: mulss xmm5, dword ptr [esp + 0x28]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[9]);
                // 00820394: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820398: movss xmm6, dword ptr [esp + 0x2c]
                xmm[6] = [fr[10], 0.0, 0.0, 0.0];
                // 0082039e: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld46));
                // 008203a3: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 008203a7: movss xmm5, dword ptr [edx + 0x4c]
                xmm[5] = [f32::from_bits(ld45), 0.0, 0.0, 0.0];
                // 008203ac: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008203b0: movss xmm6, dword ptr [esp + 0x18]
                xmm[6] = [fr[5], 0.0, 0.0, 0.0];
                // 008203b6: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld43));
                // 008203bb: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008203bf: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 008203c2: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008203c6: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 008203ca: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008203ce: movss xmm6, dword ptr [edx + 0x14]
                xmm[6] = [f32::from_bits({ ld57 = pg.u32(m, edx.wrapping_add(0x14)); ld57 }), 0.0, 0.0, 0.0];
                // 008203d3: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 008203d9: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld50));
                // 008203de: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008203e2: movss dword ptr [esp + 0x70], xmm7
                fr[27] = xmm[7][0];
                // 008203e8: movss xmm7, dword ptr [esp + 0x10]
                xmm[7] = [fr[3], 0.0, 0.0, 0.0];
                // 008203ee: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 008203f2: movss dword ptr [esp + 0x38], xmm6
                fr[13] = xmm[6][0];
                // 008203f8: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld46));
                // 008203fd: movss dword ptr [esp + 0x20], xmm7
                fr[7] = xmm[7][0];
                // 00820403: movss xmm7, dword ptr [esp + 0x1c]
                xmm[7] = [fr[6], 0.0, 0.0, 0.0];
                // 00820409: mulss xmm7, xmm2
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
                // 0082040d: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820411: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820415: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld54), 0.0, 0.0, 0.0];
                // 0082041a: mulss xmm5, dword ptr [esp + 0x14]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[4]);
                // 00820420: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 00820424: movss xmm6, dword ptr [esp + 0x1c]
                xmm[6] = [fr[6], 0.0, 0.0, 0.0];
                // 0082042a: mulss xmm6, dword ptr [edx + 0x1c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld53));
                // 0082042f: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld46));
                // 00820434: movss xmm5, dword ptr [esp + 0x18]
                xmm[5] = [fr[5], 0.0, 0.0, 0.0];
                // 0082043a: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082043e: mulss xmm5, xmm2
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[2][0]);
                // 00820442: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 00820446: movss xmm5, dword ptr [esp + 0x20]
                xmm[5] = [fr[7], 0.0, 0.0, 0.0];
                // 0082044c: mulss xmm5, xmm3
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
                // 00820450: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 00820454: movss xmm5, dword ptr [esp + 0x10]
                xmm[5] = [fr[3], 0.0, 0.0, 0.0];
                // 0082045a: mulss xmm5, dword ptr [edx + 0x2c]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld47));
                // 0082045f: movss dword ptr [esp + 0x14], xmm5
                fr[4] = xmm[5][0];
                // 00820465: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820469: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 0082046c: movss dword ptr [esp + 0x74], xmm7
                fr[28] = xmm[7][0];
                // 00820472: movss xmm7, dword ptr [esp + 0x14]
                xmm[7] = [fr[4], 0.0, 0.0, 0.0];
                // 00820478: mulss xmm7, dword ptr [edx + 0x40]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld46));
                // 0082047d: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00820480: mulss xmm6, dword ptr [edx + 0x1c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld53));
                // 00820485: movss dword ptr [esp + 0x18], xmm6
                fr[5] = xmm[6][0];
                // 0082048b: movaps xmm5, xmm1
                xmm[5] = xmm[1];
                // 0082048e: mulss xmm5, xmm2
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[2][0]);
                // 00820492: movaps xmm6, xmm5
                xmm[6] = xmm[5];
                // 00820495: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld45));
                // 0082049a: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 0082049e: movss xmm6, dword ptr [esp + 0x18]
                xmm[6] = [fr[5], 0.0, 0.0, 0.0];
                // 008204a4: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld46));
                // 008204a9: movss dword ptr [esp + 0x30], xmm5
                fr[11] = xmm[5][0];
                // 008204af: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld54), 0.0, 0.0, 0.0];
                // 008204b4: mulss xmm5, dword ptr [esp + 0x24]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[8]);
                // 008204ba: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 008204be: movss xmm5, dword ptr [esp + 0x20]
                xmm[5] = [fr[7], 0.0, 0.0, 0.0];
                // 008204c4: mulss xmm5, dword ptr [edx + 0x20]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld43));
                // 008204c9: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008204cd: movss xmm6, dword ptr [edx + 0x44]
                xmm[6] = [f32::from_bits(ld54), 0.0, 0.0, 0.0];
                // 008204d2: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld47));
                // 008204d7: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 008204db: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 008204df: movss xmm6, dword ptr [esp + 0x30]
                xmm[6] = [fr[11], 0.0, 0.0, 0.0];
                // 008204e5: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld50));
                // 008204ea: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 008204ee: movss xmm5, dword ptr [esp + 0x14]
                xmm[5] = [fr[4], 0.0, 0.0, 0.0];
                // 008204f4: mulss xmm5, xmm3
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
                // 008204f8: addss xmm5, xmm6
                xmm[5][0] = rt::addss(xmm[5][0], xmm[6][0]);
                // 008204fc: movss xmm6, dword ptr [esp + 0x1c]
                xmm[6] = [fr[6], 0.0, 0.0, 0.0];
                // 00820502: mulss xmm6, dword ptr [esp + 0x24]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[8]);
                // 00820508: addss xmm5, xmm6
                xmm[5][0] = rt::addss(xmm[5][0], xmm[6][0]);
                // 0082050c: movss xmm6, dword ptr [esp + 0x18]
                xmm[6] = [fr[5], 0.0, 0.0, 0.0];
                // 00820512: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820516: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 0082051a: movss xmm6, dword ptr [esp + 0x2c]
                xmm[6] = [fr[10], 0.0, 0.0, 0.0];
                // 00820520: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00820524: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 00820528: movss xmm6, dword ptr [esp + 0x38]
                xmm[6] = [fr[13], 0.0, 0.0, 0.0];
                // 0082052e: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld43));
                // 00820533: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 00820537: mulss xmm5, xmm0
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[0][0]);
                // 0082053b: xorps xmm5, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[5][k] = f32::from_bits(xmm[5][k].to_bits() ^ s[k].to_bits()); } }
                // 0082053e: movss dword ptr [esp + 0x7c], xmm5
                fr[30] = xmm[5][0];
                // 00820544: movss xmm5, dword ptr [edx + 0x38]
                xmm[5] = [f32::from_bits(ld44), 0.0, 0.0, 0.0];
                // 00820549: mulss xmm5, dword ptr [edx + 0x44]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld54));
                // 0082054e: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820552: movss dword ptr [esp + 0x78], xmm7
                fr[29] = xmm[7][0];
                // 00820558: mulss xmm1, dword ptr [edx + 0x38]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld44));
                // 0082055d: mulss xmm1, dword ptr [edx + 0x40]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld46));
                // 00820562: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 00820568: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld49));
                // 0082056d: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 00820571: mulss xmm3, dword ptr [edx + 0x28]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld51));
                // 00820576: mulss xmm3, dword ptr [edx + 0x44]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld54));
                // 0082057b: movss xmm6, dword ptr [esp + 0x1c]
                xmm[6] = [fr[6], 0.0, 0.0, 0.0];
                // 00820581: mulss xmm6, dword ptr [edx + 0x28]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld51));
                // 00820586: addss xmm1, xmm3
                xmm[1][0] = rt::addss(xmm[1][0], xmm[3][0]);
                // 0082058a: movss xmm3, dword ptr [edx + 0x40]
                xmm[3] = [f32::from_bits(ld46), 0.0, 0.0, 0.0];
                // 0082058f: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820593: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00820597: movss xmm6, dword ptr [edx + 0x20]
                xmm[6] = [f32::from_bits(ld43), 0.0, 0.0, 0.0];
                // 0082059c: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 008205a2: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008205a6: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 008205aa: movss xmm6, dword ptr [edx + 0x24]
                xmm[6] = [f32::from_bits(ld56), 0.0, 0.0, 0.0];
                // 008205af: movss dword ptr [esp + 0xc], xmm6
                fr[2] = xmm[6][0];
                // 008205b5: movaps xmm7, xmm6
                xmm[7] = xmm[6];
                // 008205b8: movss xmm6, dword ptr [edx + 0x48]
                xmm[6] = [f32::from_bits(ld49), 0.0, 0.0, 0.0];
                // 008205bd: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 008205c1: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 008205c7: movss xmm6, dword ptr [edx + 0x30]
                xmm[6] = [f32::from_bits(ld48), 0.0, 0.0, 0.0];
                // 008205cc: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 008205d0: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 008205d4: movss xmm7, dword ptr [edx + 0x38]
                xmm[7] = [f32::from_bits(ld44), 0.0, 0.0, 0.0];
                // 008205d9: movss dword ptr [esp + 0x1c], xmm6
                fr[6] = xmm[6][0];
                // 008205df: movss xmm6, dword ptr [edx + 0x14]
                xmm[6] = [f32::from_bits(ld57), 0.0, 0.0, 0.0];
                // 008205e4: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 008205e8: xorps xmm1, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
                // 008205eb: movss dword ptr [esp + 0x80], xmm1
                fr[31] = xmm[1][0];
                // 008205f4: movaps xmm1, xmm6
                xmm[1] = xmm[6];
                // 008205f7: mulss xmm1, xmm7
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[7][0]);
                // 008205fb: movss dword ptr [esp + 0x30], xmm6
                fr[11] = xmm[6][0];
                // 00820601: mulss xmm6, dword ptr [esp + 0x14]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[4]);
                // 00820607: movss dword ptr [esp + 0x2c], xmm7
                fr[10] = xmm[7][0];
                // 0082060d: movss xmm7, dword ptr [edx + 0x34]
                xmm[7] = [f32::from_bits(ld55), 0.0, 0.0, 0.0];
                // 00820612: movss dword ptr [esp + 0x18], xmm6
                fr[5] = xmm[6][0];
                // 00820618: movss dword ptr [esp + 0x24], xmm7
                fr[8] = xmm[7][0];
                // 0082061e: movss xmm6, dword ptr [esp + 0x24]
                xmm[6] = [fr[8], 0.0, 0.0, 0.0];
                // 00820624: movss xmm7, dword ptr [edx + 0x18]
                xmm[7] = [f32::from_bits(ld52), 0.0, 0.0, 0.0];
                // 00820629: movss dword ptr [esp + 0x34], xmm1
                fr[12] = xmm[1][0];
                // 0082062f: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00820633: mulss xmm6, dword ptr [esp + 0x14]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[4]);
                // 00820639: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 0082063d: addss xmm6, xmm1
                xmm[6][0] = rt::addss(xmm[6][0], xmm[1][0]);
                // 00820641: movss xmm1, dword ptr [edx + 0x44]
                xmm[1] = [f32::from_bits(ld54), 0.0, 0.0, 0.0];
                // 00820646: movss dword ptr [esp + 0x28], xmm7
                fr[9] = xmm[7][0];
                // 0082064c: mulss xmm7, dword ptr [esp + 0x1c]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[6]);
                // 00820652: mulss xmm7, xmm1
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[1][0]);
                // 00820656: addss xmm6, xmm7
                xmm[6][0] = rt::addss(xmm[6][0], xmm[7][0]);
                // 0082065a: movss dword ptr [esp + 0x38], xmm1
                fr[13] = xmm[1][0];
                // 00820660: movss xmm1, dword ptr [esp + 0x28]
                xmm[1] = [fr[9], 0.0, 0.0, 0.0];
                // 00820666: mulss xmm1, dword ptr [esp + 0x24]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[8]);
                // 0082066c: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 00820670: subss xmm6, xmm1
                xmm[6][0] = rt::subss(xmm[6][0], xmm[1][0]);
                // 00820674: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 0082067a: mulss xmm1, dword ptr [esp + 0x1c]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[6]);
                // 00820680: mulss xmm5, xmm2
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[2][0]);
                // 00820684: subss xmm6, xmm5
                xmm[6][0] = rt::subss(xmm[6][0], xmm[5][0]);
                // 00820688: movss xmm5, dword ptr [esp + 0x28]
                xmm[5] = [fr[9], 0.0, 0.0, 0.0];
                // 0082068e: subss xmm6, xmm1
                xmm[6][0] = rt::subss(xmm[6][0], xmm[1][0]);
                // 00820692: movss xmm1, dword ptr [edx + 0x28]
                xmm[1] = [f32::from_bits(ld51), 0.0, 0.0, 0.0];
                // 00820697: mulss xmm6, xmm0
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[0][0]);
                // 0082069b: movaps xmm7, xmm5
                xmm[7] = xmm[5];
                // 0082069e: mulss xmm7, dword ptr [esp + 0x10]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[3]);
                // 008206a4: mulss xmm5, dword ptr [esp + 0xc]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[2]);
                // 008206aa: movss dword ptr [esp + 0x84], xmm6
                fr[32] = xmm[6][0];
                // 008206b3: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 008206b9: movss dword ptr [esp + 0x20], xmm7
                fr[7] = xmm[7][0];
                // 008206bf: movss dword ptr [esp + 0x3c], xmm1
                fr[14] = xmm[1][0];
                // 008206c5: mulss xmm1, dword ptr [esp + 0x30]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[11]);
                // 008206cb: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 008206cf: movss dword ptr [esp + 0xc], xmm5
                fr[2] = xmm[5][0];
                // 008206d5: movaps xmm7, xmm6
                xmm[7] = xmm[6];
                // 008206d8: mulss xmm7, dword ptr [esp + 0x14]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[4]);
                // 008206de: movaps xmm5, xmm1
                xmm[5] = xmm[1];
                // 008206e1: mulss xmm5, xmm3
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
                // 008206e5: movss dword ptr [esp + 0x30], xmm6
                fr[11] = xmm[6][0];
                // 008206eb: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 008206ef: movss xmm6, dword ptr [esp + 0x20]
                xmm[6] = [fr[7], 0.0, 0.0, 0.0];
                // 008206f5: mulss xmm6, dword ptr [esp + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[13]);
                // 008206fb: movss xmm5, dword ptr [esp + 0x18]
                xmm[5] = [fr[5], 0.0, 0.0, 0.0];
                // 00820701: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820705: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 0082070b: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 0082070f: movss xmm3, dword ptr [esp + 0x3c]
                xmm[3] = [fr[14], 0.0, 0.0, 0.0];
                // 00820715: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820719: movaps xmm6, xmm3
                xmm[6] = xmm[3];
                // 0082071c: mulss xmm6, dword ptr [esp + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[13]);
                // 00820722: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00820726: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082072a: movss xmm6, dword ptr [esp + 0x10]
                xmm[6] = [fr[3], 0.0, 0.0, 0.0];
                // 00820730: mulss xmm5, xmm6
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[6][0]);
                // 00820734: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 00820738: movss xmm5, dword ptr [esp + 0x1c]
                xmm[5] = [fr[6], 0.0, 0.0, 0.0];
                // 0082073e: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00820742: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820746: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 00820749: movss xmm4, dword ptr [esp + 0x30]
                xmm[4] = [fr[11], 0.0, 0.0, 0.0];
                // 0082074f: mulss xmm4, dword ptr [esp + 0x2c]
                xmm[4][0] = rt::mulss(xmm[4][0], fr[10]);
                // 00820755: addss xmm1, xmm4
                xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
                // 00820759: movss xmm4, dword ptr [esp + 0x20]
                xmm[4] = [fr[7], 0.0, 0.0, 0.0];
                // 0082075f: movss dword ptr [esp + 0x88], xmm7
                fr[33] = xmm[7][0];
                // 00820768: movss xmm7, dword ptr [esp + 0x24]
                xmm[7] = [fr[8], 0.0, 0.0, 0.0];
                // 0082076e: mulss xmm4, xmm7
                xmm[4][0] = rt::mulss(xmm[4][0], xmm[7][0]);
                // 00820772: addss xmm1, xmm4
                xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
                // 00820776: movss xmm4, dword ptr [esp + 0xc]
                xmm[4] = [fr[2], 0.0, 0.0, 0.0];
                // 0082077c: mulss xmm3, xmm7
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[7][0]);
                // 00820780: mulss xmm3, xmm2
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
                // 00820784: movss xmm2, dword ptr [esp + 0x34]
                xmm[2] = [fr[12], 0.0, 0.0, 0.0];
                // 0082078a: mulss xmm4, xmm5
                xmm[4][0] = rt::mulss(xmm[4][0], xmm[5][0]);
                // 0082078e: subss xmm1, xmm4
                xmm[1][0] = rt::subss(xmm[1][0], xmm[4][0]);
                // 00820792: subss xmm1, xmm3
                xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
                // 00820796: mulss xmm2, xmm6
                xmm[2][0] = rt::mulss(xmm[2][0], xmm[6][0]);
                // 0082079a: subss xmm1, xmm2
                xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
                // 0082079e: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 008207a2: movss dword ptr [esp + 0x8c], xmm1
                fr[34] = xmm[1][0];
                pc = 0x008207ab;
                continue;
            }
            0x008207ab => {
                // 008207ab: movss xmm0, dword ptr [esi + 0x40]
                xmm[0] = [f32::from_bits({ ld58 = pg.u32(m, esi.wrapping_add(0x40)); ld58 }), 0.0, 0.0, 0.0];
                // 008207b0: movaps xmm1, xmmword ptr [esp + 0x50]
                xmm[1] = [fr[19], fr[20], fr[21], fr[22]];
                // 008207b5: movaps xmm2, xmmword ptr [esp + 0x60]
                xmm[2] = [fr[23], fr[24], fr[25], fr[26]];
                // 008207ba: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 008207be: mulps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::mulss(xmm[1][k], s[k]); } }
                // 008207c1: movss xmm0, dword ptr [esi + 0x44]
                xmm[0] = [f32::from_bits({ ld59 = pg.u32(m, esi.wrapping_add(0x44)); ld59 }), 0.0, 0.0, 0.0];
                // 008207c6: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 008207ca: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 008207cd: movss xmm0, dword ptr [esi + 0x48]
                xmm[0] = [f32::from_bits({ ld60 = pg.u32(m, esi.wrapping_add(0x48)); ld60 }), 0.0, 0.0, 0.0];
                // 008207d2: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 008207d5: movaps xmm2, xmmword ptr [esp + 0x70]
                xmm[2] = [fr[27], fr[28], fr[29], fr[30]];
                // 008207da: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 008207de: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 008207e1: movaps xmm0, xmmword ptr [esp + 0x80]
                xmm[0] = [fr[31], fr[32], fr[33], fr[34]];
                // 008207e9: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 008207ec: addps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 008207ef: movaps xmmword ptr [esp + 0x90], xmm1
                { let v = xmm[1]; fr[35] = v[0]; fr[36] = v[1]; fr[37] = v[2]; fr[38] = v[3]; }
                // 008207f7: movss xmm0, dword ptr [esp + 0x90]
                xmm[0] = [fr[35], 0.0, 0.0, 0.0];
                // 00820800: movss dword ptr [esp + 0x40], xmm0
                fr[15] = xmm[0][0];
                // 00820806: movss xmm0, dword ptr [esp + 0x94]
                xmm[0] = [fr[36], 0.0, 0.0, 0.0];
                // 0082080f: movss dword ptr [esp + 0x44], xmm0
                fr[16] = xmm[0][0];
                // 00820815: movss xmm0, dword ptr [esp + 0x98]
                xmm[0] = [fr[37], 0.0, 0.0, 0.0];
                // 0082081e: push 8
                { let v = 0x8u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00820820: movss dword ptr [esp + 0x4c], xmm0
                fr[17] = xmm[0][0];
                // 00820826: call 0x421ed0
                let target: u32 = 0x00421ed0u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0082082b);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 0082082b: test eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 0082082d: je 0x82087d
                if zf { pc = 0x0082087d; continue; }
                pc = 0x0082082f;
                continue;
            }
            0x0082082f => {
                // 0082082f: movss xmm0, dword ptr [esp + 0x94]
                xmm[0] = [fr[36], 0.0, 0.0, 0.0];
                // 00820838: movaps xmm1, xmmword ptr [eax + 0x20]
                xmm[1] = [f32::from_bits({ ld61 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(0)); ld61 }), f32::from_bits({ ld62 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(4)); ld62 }), f32::from_bits({ ld63 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(8)); ld63 }), f32::from_bits({ ld64 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(12)); ld64 })];
                // 0082083c: movaps xmm2, xmmword ptr [eax + 0x10]
                xmm[2] = [f32::from_bits({ ld65 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(0)); ld65 }), f32::from_bits({ ld66 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(4)); ld66 }), f32::from_bits({ ld67 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(8)); ld67 }), f32::from_bits({ ld68 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(12)); ld68 })];
                // 00820840: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 00820844: mulps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::mulss(xmm[1][k], s[k]); } }
                // 00820847: movss xmm0, dword ptr [esp + 0x90]
                xmm[0] = [fr[35], 0.0, 0.0, 0.0];
                // 00820850: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 00820854: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 00820857: movss xmm0, dword ptr [esp + 0x98]
                xmm[0] = [fr[37], 0.0, 0.0, 0.0];
                // 00820860: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00820863: movaps xmm2, xmmword ptr [eax + 0x30]
                xmm[2] = [f32::from_bits({ ld69 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(0)); ld69 }), f32::from_bits({ ld70 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(4)); ld70 }), f32::from_bits({ ld71 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(8)); ld71 }), f32::from_bits({ ld72 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(12)); ld72 })];
                // 00820867: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 0082086b: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 0082086e: movaps xmm0, xmmword ptr [eax + 0x40]
                xmm[0] = [f32::from_bits({ ld73 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(0)); ld73 }), f32::from_bits({ ld74 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(4)); ld74 }), f32::from_bits({ ld75 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(8)); ld75 }), f32::from_bits({ ld76 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(12)); ld76 })];
                // 00820872: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00820875: addps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00820878: movaps xmmword ptr [esp + 0x40], xmm1
                { let v = xmm[1]; fr[15] = v[0]; fr[16] = v[1]; fr[17] = v[2]; fr[18] = v[3]; }
                pc = 0x0082087d;
                continue;
            }
            0x0082087d => {
                // 0082087d: movss xmm0, dword ptr [esp + 0x40]
                xmm[0] = [fr[15], 0.0, 0.0, 0.0];
                // 00820883: movss xmm1, dword ptr [esp + 0x44]
                xmm[1] = [fr[16], 0.0, 0.0, 0.0];
                // 00820889: movss xmm2, dword ptr [esp + 0x48]
                xmm[2] = [fr[17], 0.0, 0.0, 0.0];
                // 0082088f: movss dword ptr [esi + 0x40], xmm0
                pg.set_u32(m, esi.wrapping_add(0x40), xmm[0][0].to_bits());
                // 00820894: movss dword ptr [esi + 0x44], xmm1
                pg.set_u32(m, esi.wrapping_add(0x44), xmm[1][0].to_bits());
                // 00820899: movss dword ptr [esi + 0x48], xmm2
                pg.set_u32(m, esi.wrapping_add(0x48), xmm[2][0].to_bits());
                // 0082089e: movss xmm0, dword ptr [0x15319b8]
                xmm[0] = [f32::from_bits({ ld77 = pg.u32(m, 0x015319b8u32); ld77 }), 0.0, 0.0, 0.0];
                // 008208a6: movss dword ptr [esi + 0x4c], xmm0
                pg.set_u32(m, esi.wrapping_add(0x4c), xmm[0][0].to_bits());
                // 008208ab: movss xmm4, dword ptr [0x161c280]
                xmm[4] = [f32::from_bits({ ld78 = pg.u32(m, 0x0161c280u32); ld78 }), 0.0, 0.0, 0.0];
                pc = 0x008208b3;
                continue;
            }
            0x008208b3 => {
                // 008208b3: mov eax, dword ptr [edi + 0x28]
                eax = { ld79 = pg.u32(m, edi.wrapping_add(0x28)); ld79 };
                // 008208b6: cmp eax, 0xffff
                { let (a, b) = (eax, 0xffffu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 008208bb: je 0x821449
                if zf { pc = 0x00821449; continue; }
                pc = 0x008208c1;
                continue;
            }
            0x008208c1 => {
                // 008208c1: cmp byte ptr [ebp + 0xc], 0
                { let (a, b) = ({ ld80 = pg.u8(m, ebp.wrapping_add(0xc)) as u32; ld80 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 008208c5: je 0x821449
                if zf { pc = 0x00821449; continue; }
                pc = 0x008208cb;
                continue;
            }
            0x008208cb => {
                // 008208cb: mov ecx, dword ptr [edi + 4]
                ecx = { ld81 = pg.u32(m, edi.wrapping_add(0x4)); ld81 };
                // 008208ce: mov edx, dword ptr [ecx + 0x368]
                edx = { ld82 = pg.u32(m, ecx.wrapping_add(0x368)); ld82 };
                // 008208d4: and eax, 0xff
                { let (a, b) = (eax, 0xffu32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                let r = (a & b) & 0xffffffffu32;
                eax = r;
                }
                // 008208d9: movzx eax, byte ptr [edx + eax]
                eax = { ld83 = pg.u8(m, edx.wrapping_add(eax)) as u32; ld83 };
                // 008208dd: cmp eax, 0xff
                { let (a, b) = (eax, 0xffu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 008208e2: je 0x821449
                if zf { pc = 0x00821449; continue; }
                pc = 0x008208e8;
                continue;
            }
            0x008208e8 => {
                // 008208e8: lea edx, [eax + eax*4]
                edx = eax.wrapping_add(eax.wrapping_mul(4));
                // 008208eb: shl edx, 5
                { let a = edx;
                let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
                (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
                edx = r;
                }
                // 008208ee: add edx, dword ptr [ecx + 0x364]
                { let (a, b) = (edx, { ld84 = pg.u32(m, ecx.wrapping_add(0x364)); ld84 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 008208f4: je 0x821449
                if zf { pc = 0x00821449; continue; }
                pc = 0x008208fa;
                continue;
            }
            0x008208fa => {
                // 008208fa: movzx eax, byte ptr [edx + 0x91]
                eax = { ld85 = pg.u8(m, edx.wrapping_add(0x91)) as u32; ld85 };
                // 00820901: lea esi, [eax + eax*4]
                esi = eax.wrapping_add(eax.wrapping_mul(4));
                // 00820904: shl esi, 5
                { let a = esi;
                let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
                (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
                esi = r;
                }
                // 00820907: add esi, dword ptr [ecx + 0x364]
                { let (a, b) = (esi, { ld86 = pg.u32(m, ecx.wrapping_add(0x364)); ld86 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 0082090d: je 0x821449
                if zf { pc = 0x00821449; continue; }
                pc = 0x00820913;
                continue;
            }
            0x00820913 => {
                // 00820913: movss xmm2, dword ptr [edx + 0x38]
                xmm[2] = [f32::from_bits({ ld87 = pg.u32(m, edx.wrapping_add(0x38)); ld87 }), 0.0, 0.0, 0.0];
                // 00820918: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits({ ld88 = pg.u32(m, edx.wrapping_add(0x2c)); ld88 }), 0.0, 0.0, 0.0];
                // 0082091d: movss xmm6, dword ptr [edx + 0x48]
                xmm[6] = [f32::from_bits({ ld89 = pg.u32(m, edx.wrapping_add(0x48)); ld89 }), 0.0, 0.0, 0.0];
                // 00820922: movss xmm0, dword ptr [edx + 0x44]
                xmm[0] = [f32::from_bits({ ld90 = pg.u32(m, edx.wrapping_add(0x44)); ld90 }), 0.0, 0.0, 0.0];
                // 00820927: movss xmm5, dword ptr [edx + 0x4c]
                xmm[5] = [f32::from_bits({ ld91 = pg.u32(m, edx.wrapping_add(0x4c)); ld91 }), 0.0, 0.0, 0.0];
                // 0082092c: movss xmm1, dword ptr [edx + 0x24]
                xmm[1] = [f32::from_bits({ ld92 = pg.u32(m, edx.wrapping_add(0x24)); ld92 }), 0.0, 0.0, 0.0];
                // 00820931: movaps xmm3, xmm7
                xmm[3] = xmm[7];
                // 00820934: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits({ ld93 = pg.u32(m, edx.wrapping_add(0x34)); ld93 }));
                // 00820939: mulss xmm3, xmm2
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
                // 0082093d: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 00820941: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00820945: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 00820949: movss dword ptr [esp + 0x20], xmm3
                fr[7] = xmm[3][0];
                // 0082094f: movaps xmm3, xmm6
                xmm[3] = xmm[6];
                // 00820952: mulss xmm3, dword ptr [edx + 0x3c]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits({ ld94 = pg.u32(m, edx.wrapping_add(0x3c)); ld94 }));
                // 00820957: movss dword ptr [esp + 0xc], xmm3
                fr[2] = xmm[3][0];
                // 0082095d: movss xmm3, dword ptr [edx + 0x28]
                xmm[3] = [f32::from_bits({ ld95 = pg.u32(m, edx.wrapping_add(0x28)); ld95 }), 0.0, 0.0, 0.0];
                // 00820962: mulss xmm3, dword ptr [edx + 0x3c]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld94));
                // 00820967: mulss xmm3, xmm0
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[0][0]);
                // 0082096b: addss xmm3, xmm1
                xmm[3][0] = rt::addss(xmm[3][0], xmm[1][0]);
                // 0082096f: movss xmm1, dword ptr [esp + 0x20]
                xmm[1] = [fr[7], 0.0, 0.0, 0.0];
                // 00820975: addss xmm3, xmm7
                xmm[3][0] = rt::addss(xmm[3][0], xmm[7][0]);
                // 00820979: movss xmm7, dword ptr [esp + 0xc]
                xmm[7] = [fr[2], 0.0, 0.0, 0.0];
                // 0082097f: mulss xmm7, dword ptr [edx + 0x24]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld92));
                // 00820984: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00820988: subss xmm3, xmm1
                xmm[3][0] = rt::subss(xmm[3][0], xmm[1][0]);
                // 0082098c: movss xmm1, dword ptr [edx + 0x14]
                xmm[1] = [f32::from_bits({ ld96 = pg.u32(m, edx.wrapping_add(0x14)); ld96 }), 0.0, 0.0, 0.0];
                // 00820991: subss xmm3, xmm7
                xmm[3][0] = rt::subss(xmm[3][0], xmm[7][0]);
                // 00820995: movss xmm7, dword ptr [edx + 0x28]
                xmm[7] = [f32::from_bits(ld95), 0.0, 0.0, 0.0];
                // 0082099a: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 0082099e: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld93));
                // 008209a3: subss xmm3, xmm7
                xmm[3][0] = rt::subss(xmm[3][0], xmm[7][0]);
                // 008209a7: movss xmm7, dword ptr [edx + 0x18]
                xmm[7] = [f32::from_bits({ ld97 = pg.u32(m, edx.wrapping_add(0x18)); ld97 }), 0.0, 0.0, 0.0];
                // 008209ac: movss dword ptr [esp + 0x14], xmm7
                fr[4] = xmm[7][0];
                // 008209b2: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld94));
                // 008209b7: movss dword ptr [esp + 0x28], xmm1
                fr[9] = xmm[1][0];
                // 008209bd: movss xmm1, dword ptr [edx + 0x1c]
                xmm[1] = [f32::from_bits({ ld98 = pg.u32(m, edx.wrapping_add(0x1c)); ld98 }), 0.0, 0.0, 0.0];
                // 008209c2: movss dword ptr [esp + 0x1c], xmm1
                fr[6] = xmm[1][0];
                // 008209c8: movss xmm1, dword ptr [esp + 0x14]
                xmm[1] = [fr[4], 0.0, 0.0, 0.0];
                // 008209ce: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 008209d2: movss dword ptr [esp + 0x34], xmm7
                fr[12] = xmm[7][0];
                // 008209d8: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008209dc: movss dword ptr [esp + 0x18], xmm1
                fr[5] = xmm[1][0];
                // 008209e2: movss xmm1, dword ptr [esp + 0x28]
                xmm[1] = [fr[9], 0.0, 0.0, 0.0];
                // 008209e8: mulss xmm1, xmm2
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
                // 008209ec: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 008209f0: movss xmm5, dword ptr [esp + 0x1c]
                xmm[5] = [fr[6], 0.0, 0.0, 0.0];
                // 008209f6: mulss xmm5, dword ptr [edx + 0x24]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld92));
                // 008209fb: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 008209ff: movss xmm7, dword ptr [esp + 0x1c]
                xmm[7] = [fr[6], 0.0, 0.0, 0.0];
                // 00820a05: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld93));
                // 00820a0a: mulss xmm7, xmm6
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
                // 00820a0e: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 00820a12: movss xmm7, dword ptr [esp + 0x1c]
                xmm[7] = [fr[6], 0.0, 0.0, 0.0];
                // 00820a18: mulss xmm7, xmm2
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
                // 00820a1c: movss xmm2, dword ptr [esp + 0x28]
                xmm[2] = [fr[9], 0.0, 0.0, 0.0];
                // 00820a22: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820a26: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 00820a2a: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 00820a2d: mulss xmm7, dword ptr [esp + 0xc]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[2]);
                // 00820a33: mulss xmm2, dword ptr [edx + 0x28]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld95));
                // 00820a38: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 00820a3c: movss xmm7, dword ptr [esp + 0x18]
                xmm[7] = [fr[5], 0.0, 0.0, 0.0];
                // 00820a42: mulss xmm7, dword ptr [edx + 0x34]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld93));
                // 00820a47: movss dword ptr [esp + 0xc], xmm2
                fr[2] = xmm[2][0];
                // 00820a4d: movss xmm2, dword ptr [esp + 0x14]
                xmm[2] = [fr[4], 0.0, 0.0, 0.0];
                // 00820a53: mulss xmm2, dword ptr [edx + 0x2c]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld88));
                // 00820a58: movss dword ptr [esp + 0x14], xmm5
                fr[4] = xmm[5][0];
                // 00820a5e: movss xmm5, dword ptr [esp + 0x1c]
                xmm[5] = [fr[6], 0.0, 0.0, 0.0];
                // 00820a64: mulss xmm5, dword ptr [edx + 0x28]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld95));
                // 00820a69: movss dword ptr [esp + 0x10], xmm5
                fr[3] = xmm[5][0];
                // 00820a6f: subss xmm1, xmm7
                xmm[1][0] = rt::subss(xmm[1][0], xmm[7][0]);
                // 00820a73: movss xmm7, dword ptr [esp + 0xc]
                xmm[7] = [fr[2], 0.0, 0.0, 0.0];
                // 00820a79: mulss xmm7, dword ptr [edx + 0x4c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld91));
                // 00820a7e: movaps xmm5, xmm2
                xmm[5] = xmm[2];
                // 00820a81: mulss xmm5, xmm0
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[0][0]);
                // 00820a85: movss xmm0, dword ptr [esp + 0x14]
                xmm[0] = [fr[4], 0.0, 0.0, 0.0];
                // 00820a8b: addss xmm5, xmm7
                xmm[5][0] = rt::addss(xmm[5][0], xmm[7][0]);
                // 00820a8f: mulss xmm0, xmm6
                xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
                // 00820a93: xorps xmm1, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
                // 00820a96: addss xmm5, xmm0
                xmm[5][0] = rt::addss(xmm[5][0], xmm[0][0]);
                // 00820a9a: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld88));
                // 00820a9f: movss xmm7, dword ptr [esp + 0x10]
                xmm[7] = [fr[3], 0.0, 0.0, 0.0];
                // 00820aa5: mulss xmm7, dword ptr [edx + 0x44]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld90));
                // 00820aaa: movss xmm0, dword ptr [esp + 0x28]
                xmm[0] = [fr[9], 0.0, 0.0, 0.0];
                // 00820ab0: mulss xmm2, dword ptr [edx + 0x34]
                xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits(ld93));
                // 00820ab5: mulss xmm6, xmm0
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[0][0]);
                // 00820ab9: mulss xmm0, dword ptr [esp + 0x20]
                xmm[0][0] = rt::mulss(xmm[0][0], fr[7]);
                // 00820abf: subss xmm5, xmm7
                xmm[5][0] = rt::subss(xmm[5][0], xmm[7][0]);
                // 00820ac3: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 00820ac7: movss xmm6, dword ptr [esp + 0x18]
                xmm[6] = [fr[5], 0.0, 0.0, 0.0];
                // 00820acd: mulss xmm6, dword ptr [edx + 0x24]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld92));
                // 00820ad2: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 00820ad6: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 00820adc: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld94));
                // 00820ae1: addss xmm2, xmm6
                xmm[2][0] = rt::addss(xmm[2][0], xmm[6][0]);
                // 00820ae5: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00820aeb: mulss xmm6, dword ptr [edx + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld87));
                // 00820af0: addss xmm2, xmm6
                xmm[2][0] = rt::addss(xmm[2][0], xmm[6][0]);
                // 00820af4: movss xmm6, dword ptr [esp + 0x10]
                xmm[6] = [fr[3], 0.0, 0.0, 0.0];
                // 00820afa: mulss xmm6, dword ptr [edx + 0x34]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld93));
                // 00820aff: subss xmm2, xmm6
                xmm[2][0] = rt::subss(xmm[2][0], xmm[6][0]);
                // 00820b03: subss xmm2, xmm0
                xmm[2][0] = rt::subss(xmm[2][0], xmm[0][0]);
                // 00820b07: movss xmm0, dword ptr [esp + 0x34]
                xmm[0] = [fr[12], 0.0, 0.0, 0.0];
                // 00820b0d: mulss xmm0, dword ptr [edx + 0x24]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits(ld92));
                // 00820b12: subss xmm2, xmm0
                xmm[2][0] = rt::subss(xmm[2][0], xmm[0][0]);
                // 00820b16: xorps xmm2, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[2][k] = f32::from_bits(xmm[2][k].to_bits() ^ s[k].to_bits()); } }
                // 00820b19: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 00820b1c: movss xmm2, dword ptr [edx + 0x10]
                xmm[2] = [f32::from_bits({ ld99 = pg.u32(m, edx.wrapping_add(0x10)); ld99 }), 0.0, 0.0, 0.0];
                // 00820b21: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00820b24: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits({ ld100 = pg.u32(m, edx.wrapping_add(0x20)); ld100 }));
                // 00820b29: movaps xmm0, xmm2
                xmm[0] = xmm[2];
                // 00820b2c: mulss xmm0, xmm3
                xmm[0][0] = rt::mulss(xmm[0][0], xmm[3][0]);
                // 00820b30: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 00820b34: movaps xmm0, xmm5
                xmm[0] = xmm[5];
                // 00820b37: mulss xmm0, dword ptr [edx + 0x30]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld101 = pg.u32(m, edx.wrapping_add(0x30)); ld101 }));
                // 00820b3c: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 00820b40: movaps xmm0, xmm7
                xmm[0] = xmm[7];
                // 00820b43: mulss xmm0, dword ptr [edx + 0x40]
                xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld102 = pg.u32(m, edx.wrapping_add(0x40)); ld102 }));
                // 00820b48: addss xmm6, xmm0
                xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
                // 00820b4c: ucomiss xmm6, dword ptr [0x1521690]
                { let (c_f, z_f, p_f) = rt::comiss(xmm[6][0], f32::from_bits({ ld103 = pg.u32(m, 0x01521690u32); ld103 })); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
                // 00820b53: lahf 
                eax = (eax & 0xffff_00ff) | ((((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32)) << 8);
                // 00820b54: test ah, 0x44
                { let (a, b) = (((eax >> 8) & 0xff), 0x44u32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
                }
                // 00820b57: jp 0x820c4e
                if pf { pc = 0x00820c4e; continue; }
                pc = 0x00820b5d;
                continue;
            }
            0x00820b5d => {
                // 00820b5d: movss xmm0, dword ptr [0x16295c0]
                xmm[0] = [f32::from_bits({ ld104 = pg.u32(m, 0x016295c0u32); ld104 }), 0.0, 0.0, 0.0];
                // 00820b65: movss dword ptr [esp + 0x50], xmm0
                fr[19] = xmm[0][0];
                // 00820b6b: movss xmm0, dword ptr [0x16295c4]
                xmm[0] = [f32::from_bits({ ld105 = pg.u32(m, 0x016295c4u32); ld105 }), 0.0, 0.0, 0.0];
                // 00820b73: movss dword ptr [esp + 0x54], xmm0
                fr[20] = xmm[0][0];
                // 00820b79: movss xmm0, dword ptr [0x16295c8]
                xmm[0] = [f32::from_bits({ ld106 = pg.u32(m, 0x016295c8u32); ld106 }), 0.0, 0.0, 0.0];
                // 00820b81: movss dword ptr [esp + 0x58], xmm0
                fr[21] = xmm[0][0];
                // 00820b87: movss xmm0, dword ptr [0x16295cc]
                xmm[0] = [f32::from_bits({ ld107 = pg.u32(m, 0x016295ccu32); ld107 }), 0.0, 0.0, 0.0];
                // 00820b8f: movss dword ptr [esp + 0x5c], xmm0
                fr[22] = xmm[0][0];
                // 00820b95: movss xmm0, dword ptr [0x16295d0]
                xmm[0] = [f32::from_bits({ ld108 = pg.u32(m, 0x016295d0u32); ld108 }), 0.0, 0.0, 0.0];
                // 00820b9d: movss dword ptr [esp + 0x60], xmm0
                fr[23] = xmm[0][0];
                // 00820ba3: movss xmm0, dword ptr [0x16295d4]
                xmm[0] = [f32::from_bits({ ld109 = pg.u32(m, 0x016295d4u32); ld109 }), 0.0, 0.0, 0.0];
                // 00820bab: movss dword ptr [esp + 0x64], xmm0
                fr[24] = xmm[0][0];
                // 00820bb1: movss xmm0, dword ptr [0x16295d8]
                xmm[0] = [f32::from_bits({ ld110 = pg.u32(m, 0x016295d8u32); ld110 }), 0.0, 0.0, 0.0];
                // 00820bb9: movss dword ptr [esp + 0x68], xmm0
                fr[25] = xmm[0][0];
                // 00820bbf: movss xmm0, dword ptr [0x16295dc]
                xmm[0] = [f32::from_bits({ ld111 = pg.u32(m, 0x016295dcu32); ld111 }), 0.0, 0.0, 0.0];
                // 00820bc7: movss dword ptr [esp + 0x6c], xmm0
                fr[26] = xmm[0][0];
                // 00820bcd: movss xmm0, dword ptr [0x16295e0]
                xmm[0] = [f32::from_bits({ ld112 = pg.u32(m, 0x016295e0u32); ld112 }), 0.0, 0.0, 0.0];
                // 00820bd5: movss dword ptr [esp + 0x70], xmm0
                fr[27] = xmm[0][0];
                // 00820bdb: movss xmm0, dword ptr [0x16295e4]
                xmm[0] = [f32::from_bits({ ld113 = pg.u32(m, 0x016295e4u32); ld113 }), 0.0, 0.0, 0.0];
                // 00820be3: movss dword ptr [esp + 0x74], xmm0
                fr[28] = xmm[0][0];
                // 00820be9: movss xmm0, dword ptr [0x16295e8]
                xmm[0] = [f32::from_bits({ ld114 = pg.u32(m, 0x016295e8u32); ld114 }), 0.0, 0.0, 0.0];
                // 00820bf1: movss dword ptr [esp + 0x78], xmm0
                fr[29] = xmm[0][0];
                // 00820bf7: movss xmm0, dword ptr [0x16295ec]
                xmm[0] = [f32::from_bits({ ld115 = pg.u32(m, 0x016295ecu32); ld115 }), 0.0, 0.0, 0.0];
                // 00820bff: movss dword ptr [esp + 0x7c], xmm0
                fr[30] = xmm[0][0];
                // 00820c05: movss xmm0, dword ptr [0x16295f0]
                xmm[0] = [f32::from_bits({ ld116 = pg.u32(m, 0x016295f0u32); ld116 }), 0.0, 0.0, 0.0];
                // 00820c0d: movss dword ptr [esp + 0x80], xmm0
                fr[31] = xmm[0][0];
                // 00820c16: movss xmm0, dword ptr [0x16295f4]
                xmm[0] = [f32::from_bits({ ld117 = pg.u32(m, 0x016295f4u32); ld117 }), 0.0, 0.0, 0.0];
                // 00820c1e: movss dword ptr [esp + 0x84], xmm0
                fr[32] = xmm[0][0];
                // 00820c27: movss xmm0, dword ptr [0x16295f8]
                xmm[0] = [f32::from_bits({ ld118 = pg.u32(m, 0x016295f8u32); ld118 }), 0.0, 0.0, 0.0];
                // 00820c2f: movss dword ptr [esp + 0x88], xmm0
                fr[33] = xmm[0][0];
                // 00820c38: movss xmm0, dword ptr [0x16295fc]
                xmm[0] = [f32::from_bits({ ld119 = pg.u32(m, 0x016295fcu32); ld119 }), 0.0, 0.0, 0.0];
                // 00820c40: movss dword ptr [esp + 0x8c], xmm0
                fr[34] = xmm[0][0];
                // 00820c49: jmp 0x821349
                pc = 0x00821349;
                continue;
            }
            0x00820c4e => {
                // 00820c4e: movss xmm0, dword ptr [0x15319b8]
                xmm[0] = [f32::from_bits({ ld120 = pg.u32(m, 0x015319b8u32); ld120 }), 0.0, 0.0, 0.0];
                // 00820c56: divss xmm0, xmm6
                xmm[0][0] = rt::divss(xmm[0][0], xmm[6][0]);
                // 00820c5a: movaps xmm6, xmm0
                xmm[6] = xmm[0];
                // 00820c5d: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820c61: movss dword ptr [esp + 0x50], xmm6
                fr[19] = xmm[6][0];
                // 00820c67: movss xmm6, dword ptr [edx + 0x20]
                xmm[6] = [f32::from_bits({ ld121 = pg.u32(m, edx.wrapping_add(0x20)); ld121 }), 0.0, 0.0, 0.0];
                // 00820c6c: mulss xmm6, dword ptr [edx + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits({ ld122 = pg.u32(m, edx.wrapping_add(0x38)); ld122 }));
                // 00820c71: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits({ ld123 = pg.u32(m, edx.wrapping_add(0x4c)); ld123 }));
                // 00820c76: movaps xmm3, xmm0
                xmm[3] = xmm[0];
                // 00820c79: mulss xmm3, xmm1
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
                // 00820c7d: movaps xmm1, xmm0
                xmm[1] = xmm[0];
                // 00820c80: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00820c84: movss xmm5, dword ptr [edx + 0x40]
                xmm[5] = [f32::from_bits({ ld124 = pg.u32(m, edx.wrapping_add(0x40)); ld124 }), 0.0, 0.0, 0.0];
                // 00820c89: movss dword ptr [esp + 0x58], xmm1
                fr[21] = xmm[1][0];
                // 00820c8f: movaps xmm1, xmm0
                xmm[1] = xmm[0];
                // 00820c92: mulss xmm1, xmm7
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[7][0]);
                // 00820c96: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits({ ld125 = pg.u32(m, edx.wrapping_add(0x2c)); ld125 }), 0.0, 0.0, 0.0];
                // 00820c9b: movss dword ptr [esp + 0x54], xmm3
                fr[20] = xmm[3][0];
                // 00820ca1: movss xmm3, dword ptr [edx + 0x30]
                xmm[3] = [f32::from_bits({ ld126 = pg.u32(m, edx.wrapping_add(0x30)); ld126 }), 0.0, 0.0, 0.0];
                // 00820ca6: mulss xmm7, xmm3
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[3][0]);
                // 00820caa: movss dword ptr [esp + 0x2c], xmm7
                fr[10] = xmm[7][0];
                // 00820cb0: movss xmm7, dword ptr [edx + 0x2c]
                xmm[7] = [f32::from_bits(ld125), 0.0, 0.0, 0.0];
                // 00820cb5: mulss xmm7, dword ptr [edx + 0x38]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld122));
                // 00820cba: movss dword ptr [esp + 0x10], xmm7
                fr[3] = xmm[7][0];
                // 00820cc0: movss xmm7, dword ptr [edx + 0x48]
                xmm[7] = [f32::from_bits({ ld127 = pg.u32(m, edx.wrapping_add(0x48)); ld127 }), 0.0, 0.0, 0.0];
                // 00820cc5: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits({ ld128 = pg.u32(m, edx.wrapping_add(0x3c)); ld128 }));
                // 00820cca: movss dword ptr [esp + 0xc], xmm7
                fr[2] = xmm[7][0];
                // 00820cd0: movss xmm7, dword ptr [edx + 0x28]
                xmm[7] = [f32::from_bits({ ld129 = pg.u32(m, edx.wrapping_add(0x28)); ld129 }), 0.0, 0.0, 0.0];
                // 00820cd5: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld128));
                // 00820cda: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820cde: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820ce2: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 00820ce8: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld121));
                // 00820ced: movss dword ptr [esp + 0x5c], xmm1
                fr[22] = xmm[1][0];
                // 00820cf3: movss xmm1, dword ptr [edx + 0x48]
                xmm[1] = [f32::from_bits(ld127), 0.0, 0.0, 0.0];
                // 00820cf8: mulss xmm1, dword ptr [esp + 0x2c]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[10]);
                // 00820cfe: addss xmm7, xmm1
                xmm[7][0] = rt::addss(xmm[7][0], xmm[1][0]);
                // 00820d02: movss xmm1, dword ptr [esp + 0x10]
                xmm[1] = [fr[3], 0.0, 0.0, 0.0];
                // 00820d08: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00820d0c: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820d10: movss xmm1, dword ptr [edx + 0x4c]
                xmm[1] = [f32::from_bits(ld123), 0.0, 0.0, 0.0];
                // 00820d15: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820d19: movss xmm6, dword ptr [edx + 0x28]
                xmm[6] = [f32::from_bits(ld129), 0.0, 0.0, 0.0];
                // 00820d1e: mulss xmm6, xmm1
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[1][0]);
                // 00820d22: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820d26: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820d2a: movss xmm6, dword ptr [edx + 0x18]
                xmm[6] = [f32::from_bits({ ld130 = pg.u32(m, edx.wrapping_add(0x18)); ld130 }), 0.0, 0.0, 0.0];
                // 00820d2f: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 00820d35: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld128));
                // 00820d3a: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820d3e: movss dword ptr [esp + 0x20], xmm6
                fr[7] = xmm[6][0];
                // 00820d44: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits({ ld131 = pg.u32(m, edx.wrapping_add(0x1c)); ld131 }), 0.0, 0.0, 0.0];
                // 00820d49: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 00820d4c: movss dword ptr [esp + 0x60], xmm7
                fr[23] = xmm[7][0];
                // 00820d52: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820d56: movss dword ptr [esp + 0x28], xmm6
                fr[9] = xmm[6][0];
                // 00820d5c: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00820d62: mulss xmm6, xmm1
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[1][0]);
                // 00820d66: movss dword ptr [esp + 0x18], xmm6
                fr[5] = xmm[6][0];
                // 00820d6c: movss xmm6, dword ptr [esp + 0x28]
                xmm[6] = [fr[9], 0.0, 0.0, 0.0];
                // 00820d72: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld127));
                // 00820d77: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 00820d7a: mulss xmm7, dword ptr [edx + 0x38]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld122));
                // 00820d7f: mulss xmm7, xmm1
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[1][0]);
                // 00820d83: movss xmm1, dword ptr [esp + 0x20]
                xmm[1] = [fr[7], 0.0, 0.0, 0.0];
                // 00820d89: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00820d8d: addss xmm7, xmm1
                xmm[7][0] = rt::addss(xmm[7][0], xmm[1][0]);
                // 00820d91: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820d95: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld131), 0.0, 0.0, 0.0];
                // 00820d9a: mulss xmm6, dword ptr [edx + 0x38]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld122));
                // 00820d9f: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 00820da3: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820da7: movaps xmm1, xmm2
                xmm[1] = xmm[2];
                // 00820daa: mulss xmm1, dword ptr [esp + 0xc]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[2]);
                // 00820db0: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820db4: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 00820dba: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 00820dbe: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820dc2: movss xmm1, dword ptr [esp + 0x14]
                xmm[1] = [fr[4], 0.0, 0.0, 0.0];
                // 00820dc8: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820dcc: movss dword ptr [esp + 0x64], xmm7
                fr[24] = xmm[7][0];
                // 00820dd2: mulss xmm1, dword ptr [edx + 0x2c]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld125));
                // 00820dd7: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld131), 0.0, 0.0, 0.0];
                // 00820ddc: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld121));
                // 00820de1: movss dword ptr [esp + 0x1c], xmm6
                fr[6] = xmm[6][0];
                // 00820de7: movss xmm6, dword ptr [edx + 0x1c]
                xmm[6] = [f32::from_bits(ld131), 0.0, 0.0, 0.0];
                // 00820dec: mulss xmm6, dword ptr [edx + 0x28]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld129));
                // 00820df1: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 00820df7: movss dword ptr [esp + 0xc], xmm1
                fr[2] = xmm[1][0];
                // 00820dfd: movss xmm7, dword ptr [esp + 0xc]
                xmm[7] = [fr[2], 0.0, 0.0, 0.0];
                // 00820e03: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820e07: movaps xmm1, xmm2
                xmm[1] = xmm[2];
                // 00820e0a: mulss xmm1, dword ptr [edx + 0x28]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld129));
                // 00820e0f: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00820e12: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld123));
                // 00820e17: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820e1b: movss xmm6, dword ptr [esp + 0x1c]
                xmm[6] = [fr[6], 0.0, 0.0, 0.0];
                // 00820e21: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld127));
                // 00820e26: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820e2a: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00820e30: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 00820e34: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820e38: movss xmm6, dword ptr [edx + 0x48]
                xmm[6] = [f32::from_bits(ld127), 0.0, 0.0, 0.0];
                // 00820e3d: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld125));
                // 00820e42: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00820e46: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820e4a: movss dword ptr [esp + 0x34], xmm1
                fr[12] = xmm[1][0];
                // 00820e50: movss xmm1, dword ptr [esp + 0x18]
                xmm[1] = [fr[5], 0.0, 0.0, 0.0];
                // 00820e56: mulss xmm1, dword ptr [edx + 0x20]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld121));
                // 00820e5b: movss xmm6, dword ptr [esp + 0x34]
                xmm[6] = [fr[12], 0.0, 0.0, 0.0];
                // 00820e61: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld128));
                // 00820e66: subss xmm7, xmm1
                xmm[7][0] = rt::subss(xmm[7][0], xmm[1][0]);
                // 00820e6a: movss xmm1, dword ptr [esp + 0xc]
                xmm[1] = [fr[2], 0.0, 0.0, 0.0];
                // 00820e70: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820e74: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 00820e77: movss dword ptr [esp + 0x68], xmm7
                fr[25] = xmm[7][0];
                // 00820e7d: movss xmm7, dword ptr [esp + 0x1c]
                xmm[7] = [fr[6], 0.0, 0.0, 0.0];
                // 00820e83: mulss xmm7, dword ptr [edx + 0x38]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld122));
                // 00820e88: mulss xmm1, xmm3
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
                // 00820e8c: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 00820e90: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00820e96: addss xmm1, xmm7
                xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
                // 00820e9a: movss xmm7, dword ptr [edx + 0x44]
                xmm[7] = [f32::from_bits({ ld132 = pg.u32(m, edx.wrapping_add(0x44)); ld132 }), 0.0, 0.0, 0.0];
                // 00820e9f: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld128));
                // 00820ea4: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820ea8: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00820eac: movaps xmm6, xmm2
                xmm[6] = xmm[2];
                // 00820eaf: mulss xmm6, dword ptr [esp + 0x10]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[3]);
                // 00820eb5: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00820eb9: movss xmm6, dword ptr [esp + 0x20]
                xmm[6] = [fr[7], 0.0, 0.0, 0.0];
                // 00820ebf: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld121));
                // 00820ec4: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00820ec8: movss xmm6, dword ptr [edx + 0x34]
                xmm[6] = [f32::from_bits({ ld133 = pg.u32(m, edx.wrapping_add(0x34)); ld133 }), 0.0, 0.0, 0.0];
                // 00820ecd: movss dword ptr [esp + 0x24], xmm6
                fr[8] = xmm[6][0];
                // 00820ed3: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00820ed7: movss dword ptr [esp + 0xc], xmm7
                fr[2] = xmm[7][0];
                // 00820edd: movss dword ptr [esp + 0x6c], xmm1
                fr[26] = xmm[1][0];
                // 00820ee3: movaps xmm1, xmm6
                xmm[1] = xmm[6];
                // 00820ee6: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld125));
                // 00820eeb: mulss xmm1, dword ptr [edx + 0x20]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld121));
                // 00820ef0: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 00820ef6: movss dword ptr [esp + 0x3c], xmm1
                fr[14] = xmm[1][0];
                // 00820efc: movss xmm1, dword ptr [edx + 0x24]
                xmm[1] = [f32::from_bits({ ld134 = pg.u32(m, edx.wrapping_add(0x24)); ld134 }), 0.0, 0.0, 0.0];
                // 00820f01: movss xmm6, dword ptr [esp + 0x3c]
                xmm[6] = [fr[14], 0.0, 0.0, 0.0];
                // 00820f07: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld123));
                // 00820f0c: movaps xmm7, xmm1
                xmm[7] = xmm[1];
                // 00820f0f: mulss xmm7, dword ptr [edx + 0x3c]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld128));
                // 00820f14: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820f18: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld132), 0.0, 0.0, 0.0];
                // 00820f1d: mulss xmm5, dword ptr [esp + 0x2c]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[10]);
                // 00820f23: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820f27: movss xmm6, dword ptr [esp + 0x10]
                xmm[6] = [fr[3], 0.0, 0.0, 0.0];
                // 00820f2d: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld124));
                // 00820f32: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 00820f36: movss xmm5, dword ptr [edx + 0x4c]
                xmm[5] = [f32::from_bits(ld123), 0.0, 0.0, 0.0];
                // 00820f3b: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820f3f: movss xmm6, dword ptr [esp + 0xc]
                xmm[6] = [fr[2], 0.0, 0.0, 0.0];
                // 00820f45: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld121));
                // 00820f4a: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820f4e: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00820f51: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 00820f55: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 00820f59: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820f5d: movss xmm6, dword ptr [edx + 0x14]
                xmm[6] = [f32::from_bits({ ld135 = pg.u32(m, edx.wrapping_add(0x14)); ld135 }), 0.0, 0.0, 0.0];
                // 00820f62: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 00820f68: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld128));
                // 00820f6d: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820f71: movss dword ptr [esp + 0x70], xmm7
                fr[27] = xmm[7][0];
                // 00820f77: movss xmm7, dword ptr [esp + 0x14]
                xmm[7] = [fr[4], 0.0, 0.0, 0.0];
                // 00820f7d: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820f81: movss dword ptr [esp + 0x30], xmm6
                fr[11] = xmm[6][0];
                // 00820f87: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld124));
                // 00820f8c: movss dword ptr [esp + 0x18], xmm7
                fr[5] = xmm[7][0];
                // 00820f92: movaps xmm7, xmm2
                xmm[7] = xmm[2];
                // 00820f95: mulss xmm7, dword ptr [esp + 0x24]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[8]);
                // 00820f9b: mulss xmm7, xmm5
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[5][0]);
                // 00820f9f: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 00820fa3: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld132), 0.0, 0.0, 0.0];
                // 00820fa8: mulss xmm5, dword ptr [esp + 0x28]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[9]);
                // 00820fae: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 00820fb2: movss xmm6, dword ptr [esp + 0x24]
                xmm[6] = [fr[8], 0.0, 0.0, 0.0];
                // 00820fb8: mulss xmm6, dword ptr [edx + 0x1c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld131));
                // 00820fbd: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld124));
                // 00820fc2: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 00820fc6: movaps xmm5, xmm2
                xmm[5] = xmm[2];
                // 00820fc9: mulss xmm5, dword ptr [esp + 0xc]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[2]);
                // 00820fcf: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 00820fd3: movss xmm5, dword ptr [esp + 0x18]
                xmm[5] = [fr[5], 0.0, 0.0, 0.0];
                // 00820fd9: mulss xmm5, xmm3
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
                // 00820fdd: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 00820fe1: movss xmm5, dword ptr [esp + 0x14]
                xmm[5] = [fr[4], 0.0, 0.0, 0.0];
                // 00820fe7: mulss xmm5, dword ptr [edx + 0x2c]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld125));
                // 00820fec: movss dword ptr [esp + 0xc], xmm5
                fr[2] = xmm[5][0];
                // 00820ff2: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 00820ff6: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 00820ff9: movss dword ptr [esp + 0x74], xmm7
                fr[28] = xmm[7][0];
                // 00820fff: movss xmm7, dword ptr [esp + 0xc]
                xmm[7] = [fr[2], 0.0, 0.0, 0.0];
                // 00821005: mulss xmm7, dword ptr [edx + 0x40]
                xmm[7][0] = rt::mulss(xmm[7][0], f32::from_bits(ld124));
                // 0082100a: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 0082100d: mulss xmm6, dword ptr [edx + 0x1c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld131));
                // 00821012: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 00821018: movaps xmm5, xmm2
                xmm[5] = xmm[2];
                // 0082101b: mulss xmm5, xmm1
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[1][0]);
                // 0082101f: movaps xmm6, xmm5
                xmm[6] = xmm[5];
                // 00821022: mulss xmm6, dword ptr [edx + 0x4c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld123));
                // 00821027: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 0082102b: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00821031: mulss xmm6, dword ptr [edx + 0x40]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld124));
                // 00821036: movss dword ptr [esp + 0x34], xmm5
                fr[12] = xmm[5][0];
                // 0082103c: movss xmm5, dword ptr [edx + 0x44]
                xmm[5] = [f32::from_bits(ld132), 0.0, 0.0, 0.0];
                // 00821041: mulss xmm5, dword ptr [esp + 0x1c]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[6]);
                // 00821047: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 0082104b: movss xmm5, dword ptr [esp + 0x18]
                xmm[5] = [fr[5], 0.0, 0.0, 0.0];
                // 00821051: mulss xmm5, dword ptr [edx + 0x20]
                xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits(ld121));
                // 00821056: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082105a: movss xmm6, dword ptr [edx + 0x44]
                xmm[6] = [f32::from_bits(ld132), 0.0, 0.0, 0.0];
                // 0082105f: mulss xmm6, dword ptr [edx + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld125));
                // 00821064: mulss xmm6, xmm2
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[2][0]);
                // 00821068: subss xmm7, xmm6
                xmm[7][0] = rt::subss(xmm[7][0], xmm[6][0]);
                // 0082106c: movss xmm6, dword ptr [esp + 0x34]
                xmm[6] = [fr[12], 0.0, 0.0, 0.0];
                // 00821072: mulss xmm6, dword ptr [edx + 0x3c]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld128));
                // 00821077: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 0082107b: movss xmm5, dword ptr [esp + 0xc]
                xmm[5] = [fr[2], 0.0, 0.0, 0.0];
                // 00821081: mulss xmm5, xmm3
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
                // 00821085: addss xmm5, xmm6
                xmm[5][0] = rt::addss(xmm[5][0], xmm[6][0]);
                // 00821089: movss xmm6, dword ptr [esp + 0x24]
                xmm[6] = [fr[8], 0.0, 0.0, 0.0];
                // 0082108f: mulss xmm6, dword ptr [esp + 0x1c]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[6]);
                // 00821095: addss xmm5, xmm6
                xmm[5][0] = rt::addss(xmm[5][0], xmm[6][0]);
                // 00821099: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 0082109f: mulss xmm6, xmm3
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
                // 008210a3: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 008210a7: movaps xmm6, xmm2
                xmm[6] = xmm[2];
                // 008210aa: mulss xmm6, dword ptr [esp + 0x10]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[3]);
                // 008210b0: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 008210b4: movss xmm6, dword ptr [esp + 0x30]
                xmm[6] = [fr[11], 0.0, 0.0, 0.0];
                // 008210ba: mulss xmm6, dword ptr [edx + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld121));
                // 008210bf: subss xmm5, xmm6
                xmm[5][0] = rt::subss(xmm[5][0], xmm[6][0]);
                // 008210c3: mulss xmm5, xmm0
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[0][0]);
                // 008210c7: xorps xmm5, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[5][k] = f32::from_bits(xmm[5][k].to_bits() ^ s[k].to_bits()); } }
                // 008210ca: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008210ce: movss dword ptr [esp + 0x7c], xmm5
                fr[30] = xmm[5][0];
                // 008210d4: movss xmm5, dword ptr [edx + 0x40]
                xmm[5] = [f32::from_bits(ld124), 0.0, 0.0, 0.0];
                // 008210d9: movss dword ptr [esp + 0x78], xmm7
                fr[29] = xmm[7][0];
                // 008210df: movss xmm7, dword ptr [edx + 0x44]
                xmm[7] = [f32::from_bits(ld132), 0.0, 0.0, 0.0];
                // 008210e4: movss xmm6, dword ptr [edx + 0x38]
                xmm[6] = [f32::from_bits(ld122), 0.0, 0.0, 0.0];
                // 008210e9: movss dword ptr [esp + 0x18], xmm6
                fr[5] = xmm[6][0];
                // 008210ef: mulss xmm6, xmm7
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[7][0]);
                // 008210f3: movss dword ptr [esp + 0xc], xmm6
                fr[2] = xmm[6][0];
                // 008210f9: movss xmm6, dword ptr [edx + 0x20]
                xmm[6] = [f32::from_bits(ld121), 0.0, 0.0, 0.0];
                // 008210fe: movss dword ptr [esp + 0x10], xmm6
                fr[3] = xmm[6][0];
                // 00821104: movss xmm6, dword ptr [edx + 0x48]
                xmm[6] = [f32::from_bits(ld127), 0.0, 0.0, 0.0];
                // 00821109: movss dword ptr [esp + 0x14], xmm6
                fr[4] = xmm[6][0];
                // 0082110f: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00821112: mulss xmm1, dword ptr [edx + 0x38]
                xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits(ld122));
                // 00821117: movss dword ptr [esp + 0x2c], xmm6
                fr[10] = xmm[6][0];
                // 0082111d: mulss xmm1, xmm5
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
                // 00821121: movaps xmm6, xmm3
                xmm[6] = xmm[3];
                // 00821124: mulss xmm3, dword ptr [edx + 0x28]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld129));
                // 00821129: movss dword ptr [esp + 0x28], xmm6
                fr[9] = xmm[6][0];
                // 0082112f: movss xmm6, dword ptr [esp + 0x3c]
                xmm[6] = [fr[14], 0.0, 0.0, 0.0];
                // 00821135: mulss xmm6, dword ptr [edx + 0x48]
                xmm[6][0] = rt::mulss(xmm[6][0], f32::from_bits(ld127));
                // 0082113a: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 0082113e: movss xmm6, dword ptr [esp + 0x14]
                xmm[6] = [fr[4], 0.0, 0.0, 0.0];
                // 00821144: mulss xmm3, xmm7
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[7][0]);
                // 00821148: addss xmm1, xmm3
                xmm[1][0] = rt::addss(xmm[1][0], xmm[3][0]);
                // 0082114c: movss xmm3, dword ptr [esp + 0x24]
                xmm[3] = [fr[8], 0.0, 0.0, 0.0];
                // 00821152: mulss xmm3, dword ptr [edx + 0x28]
                xmm[3][0] = rt::mulss(xmm[3][0], f32::from_bits(ld129));
                // 00821157: mulss xmm3, xmm5
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[5][0]);
                // 0082115b: subss xmm1, xmm3
                xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
                // 0082115f: movss xmm3, dword ptr [esp + 0x10]
                xmm[3] = [fr[3], 0.0, 0.0, 0.0];
                // 00821165: mulss xmm3, dword ptr [esp + 0xc]
                xmm[3][0] = rt::mulss(xmm[3][0], fr[2]);
                // 0082116b: subss xmm1, xmm3
                xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
                // 0082116f: movss xmm3, dword ptr [esp + 0x2c]
                xmm[3] = [fr[10], 0.0, 0.0, 0.0];
                // 00821175: mulss xmm3, xmm6
                xmm[3][0] = rt::mulss(xmm[3][0], xmm[6][0]);
                // 00821179: mulss xmm3, dword ptr [esp + 0x28]
                xmm[3][0] = rt::mulss(xmm[3][0], fr[9]);
                // 0082117f: subss xmm1, xmm3
                xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
                // 00821183: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00821187: xorps xmm1, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
                // 0082118a: movss dword ptr [esp + 0x80], xmm1
                fr[31] = xmm[1][0];
                // 00821193: movss xmm1, dword ptr [edx + 0x14]
                xmm[1] = [f32::from_bits(ld135), 0.0, 0.0, 0.0];
                // 00821198: movaps xmm3, xmm1
                xmm[3] = xmm[1];
                // 0082119b: mulss xmm3, dword ptr [esp + 0x18]
                xmm[3][0] = rt::mulss(xmm[3][0], fr[5]);
                // 008211a1: movss dword ptr [esp + 0x34], xmm1
                fr[12] = xmm[1][0];
                // 008211a7: mulss xmm1, xmm6
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
                // 008211ab: movss dword ptr [esp + 0x3c], xmm1
                fr[14] = xmm[1][0];
                // 008211b1: movss dword ptr [esp + 0x20], xmm7
                fr[7] = xmm[7][0];
                // 008211b7: movss xmm7, dword ptr [edx + 0x34]
                xmm[7] = [f32::from_bits(ld133), 0.0, 0.0, 0.0];
                // 008211bc: movss dword ptr [esp + 0x24], xmm7
                fr[8] = xmm[7][0];
                // 008211c2: movss xmm7, dword ptr [edx + 0x18]
                xmm[7] = [f32::from_bits(ld130), 0.0, 0.0, 0.0];
                // 008211c7: movaps xmm1, xmm2
                xmm[1] = xmm[2];
                // 008211ca: mulss xmm1, dword ptr [esp + 0x24]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[8]);
                // 008211d0: mulss xmm1, xmm6
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
                // 008211d4: movaps xmm6, xmm3
                xmm[6] = xmm[3];
                // 008211d7: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008211db: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 008211df: movaps xmm6, xmm7
                xmm[6] = xmm[7];
                // 008211e2: mulss xmm6, dword ptr [esp + 0x28]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[9]);
                // 008211e8: mulss xmm6, dword ptr [esp + 0x20]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[7]);
                // 008211ee: addss xmm1, xmm6
                xmm[1][0] = rt::addss(xmm[1][0], xmm[6][0]);
                // 008211f2: movaps xmm6, xmm7
                xmm[6] = xmm[7];
                // 008211f5: mulss xmm6, dword ptr [esp + 0x24]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[8]);
                // 008211fb: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 008211ff: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00821203: movaps xmm6, xmm2
                xmm[6] = xmm[2];
                // 00821206: mulss xmm6, dword ptr [esp + 0xc]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[2]);
                // 0082120c: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00821210: movss xmm6, dword ptr [esp + 0x3c]
                xmm[6] = [fr[14], 0.0, 0.0, 0.0];
                // 00821216: mulss xmm6, dword ptr [esp + 0x28]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[9]);
                // 0082121c: subss xmm1, xmm6
                xmm[1][0] = rt::subss(xmm[1][0], xmm[6][0]);
                // 00821220: movaps xmm6, xmm2
                xmm[6] = xmm[2];
                // 00821223: mulss xmm6, dword ptr [esp + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[10]);
                // 00821229: movss dword ptr [esp + 0xc], xmm6
                fr[2] = xmm[6][0];
                // 0082122f: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00821233: movss dword ptr [esp + 0x84], xmm1
                fr[32] = xmm[1][0];
                // 0082123c: movss xmm1, dword ptr [edx + 0x28]
                xmm[1] = [f32::from_bits(ld129), 0.0, 0.0, 0.0];
                // 00821241: movaps xmm6, xmm7
                xmm[6] = xmm[7];
                // 00821244: mulss xmm6, dword ptr [esp + 0x10]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[3]);
                // 0082124a: movss dword ptr [esp + 0x38], xmm6
                fr[13] = xmm[6][0];
                // 00821250: movss dword ptr [esp + 0x30], xmm1
                fr[11] = xmm[1][0];
                // 00821256: mulss xmm1, dword ptr [esp + 0x34]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[12]);
                // 0082125c: movaps xmm6, xmm7
                xmm[6] = xmm[7];
                // 0082125f: mulss xmm6, dword ptr [esp + 0x2c]
                xmm[6][0] = rt::mulss(xmm[6][0], fr[10]);
                // 00821265: movss dword ptr [esp + 0xac], xmm5
                fr[42] = xmm[5][0];
                // 0082126e: movss dword ptr [esp + 0x1c], xmm7
                fr[6] = xmm[7][0];
                // 00821274: movss xmm7, dword ptr [esp + 0xc]
                xmm[7] = [fr[2], 0.0, 0.0, 0.0];
                // 0082127a: mulss xmm7, dword ptr [esp + 0x14]
                xmm[7][0] = rt::mulss(xmm[7][0], fr[4]);
                // 00821280: movss dword ptr [esp + 0x2c], xmm6
                fr[10] = xmm[6][0];
                // 00821286: movaps xmm6, xmm1
                xmm[6] = xmm[1];
                // 00821289: mulss xmm1, dword ptr [esp + 0x28]
                xmm[1][0] = rt::mulss(xmm[1][0], fr[9]);
                // 0082128f: mulss xmm6, xmm5
                xmm[6][0] = rt::mulss(xmm[6][0], xmm[5][0]);
                // 00821293: movss xmm5, dword ptr [esp + 0x38]
                xmm[5] = [fr[13], 0.0, 0.0, 0.0];
                // 00821299: mulss xmm5, dword ptr [esp + 0x20]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[7]);
                // 0082129f: mulss xmm3, dword ptr [esp + 0x10]
                xmm[3][0] = rt::mulss(xmm[3][0], fr[3]);
                // 008212a5: addss xmm7, xmm6
                xmm[7][0] = rt::addss(xmm[7][0], xmm[6][0]);
                // 008212a9: addss xmm7, xmm5
                xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
                // 008212ad: movss xmm5, dword ptr [esp + 0x2c]
                xmm[5] = [fr[10], 0.0, 0.0, 0.0];
                // 008212b3: mulss xmm5, dword ptr [esp + 0xac]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[42]);
                // 008212bc: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 008212c0: movss xmm5, dword ptr [esp + 0x30]
                xmm[5] = [fr[11], 0.0, 0.0, 0.0];
                // 008212c6: mulss xmm5, dword ptr [esp + 0x20]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[7]);
                // 008212cc: mulss xmm5, xmm2
                xmm[5][0] = rt::mulss(xmm[5][0], xmm[2][0]);
                // 008212d0: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 008212d4: movss xmm5, dword ptr [esp + 0x3c]
                xmm[5] = [fr[14], 0.0, 0.0, 0.0];
                // 008212da: mulss xmm5, dword ptr [esp + 0x10]
                xmm[5][0] = rt::mulss(xmm[5][0], fr[3]);
                // 008212e0: subss xmm7, xmm5
                xmm[7][0] = rt::subss(xmm[7][0], xmm[5][0]);
                // 008212e4: mulss xmm7, xmm0
                xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
                // 008212e8: xorps xmm7, xmm4
                { let s = xmm[4]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
                // 008212eb: movss xmm4, dword ptr [esp + 0xc]
                xmm[4] = [fr[2], 0.0, 0.0, 0.0];
                // 008212f1: mulss xmm4, dword ptr [esp + 0x18]
                xmm[4][0] = rt::mulss(xmm[4][0], fr[5]);
                // 008212f7: addss xmm1, xmm4
                xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
                // 008212fb: movss xmm4, dword ptr [esp + 0x38]
                xmm[4] = [fr[13], 0.0, 0.0, 0.0];
                // 00821301: mulss xmm4, dword ptr [esp + 0x24]
                xmm[4][0] = rt::mulss(xmm[4][0], fr[8]);
                // 00821307: addss xmm1, xmm4
                xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
                // 0082130b: movss xmm4, dword ptr [esp + 0x2c]
                xmm[4] = [fr[10], 0.0, 0.0, 0.0];
                // 00821311: mulss xmm4, dword ptr [esp + 0x28]
                xmm[4][0] = rt::mulss(xmm[4][0], fr[9]);
                // 00821317: subss xmm1, xmm4
                xmm[1][0] = rt::subss(xmm[1][0], xmm[4][0]);
                // 0082131b: movss xmm4, dword ptr [esp + 0x30]
                xmm[4] = [fr[11], 0.0, 0.0, 0.0];
                // 00821321: mulss xmm4, dword ptr [esp + 0x24]
                xmm[4][0] = rt::mulss(xmm[4][0], fr[8]);
                // 00821327: mulss xmm4, xmm2
                xmm[4][0] = rt::mulss(xmm[4][0], xmm[2][0]);
                // 0082132b: subss xmm1, xmm4
                xmm[1][0] = rt::subss(xmm[1][0], xmm[4][0]);
                // 0082132f: subss xmm1, xmm3
                xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
                // 00821333: mulss xmm1, xmm0
                xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
                // 00821337: movss dword ptr [esp + 0x88], xmm7
                fr[33] = xmm[7][0];
                // 00821340: movss dword ptr [esp + 0x8c], xmm1
                fr[34] = xmm[1][0];
                pc = 0x00821349;
                continue;
            }
            0x00821349 => {
                // 00821349: movss xmm0, dword ptr [esi + 0x44]
                xmm[0] = [f32::from_bits({ ld136 = pg.u32(m, esi.wrapping_add(0x44)); ld136 }), 0.0, 0.0, 0.0];
                // 0082134e: movaps xmm1, xmmword ptr [esp + 0x60]
                xmm[1] = [fr[23], fr[24], fr[25], fr[26]];
                // 00821353: movaps xmm2, xmmword ptr [esp + 0x50]
                xmm[2] = [fr[19], fr[20], fr[21], fr[22]];
                // 00821358: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 0082135c: mulps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::mulss(xmm[1][k], s[k]); } }
                // 0082135f: movss xmm0, dword ptr [esi + 0x40]
                xmm[0] = [f32::from_bits({ ld137 = pg.u32(m, esi.wrapping_add(0x40)); ld137 }), 0.0, 0.0, 0.0];
                // 00821364: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 00821368: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 0082136b: movss xmm0, dword ptr [esi + 0x48]
                xmm[0] = [f32::from_bits({ ld138 = pg.u32(m, esi.wrapping_add(0x48)); ld138 }), 0.0, 0.0, 0.0];
                // 00821370: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00821373: movaps xmm2, xmmword ptr [esp + 0x70]
                xmm[2] = [fr[27], fr[28], fr[29], fr[30]];
                // 00821378: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 0082137c: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 0082137f: movaps xmm0, xmmword ptr [esp + 0x80]
                xmm[0] = [fr[31], fr[32], fr[33], fr[34]];
                // 00821387: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 0082138a: addps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 0082138d: movaps xmmword ptr [esp + 0x90], xmm1
                { let v = xmm[1]; fr[35] = v[0]; fr[36] = v[1]; fr[37] = v[2]; fr[38] = v[3]; }
                // 00821395: movss xmm0, dword ptr [esp + 0x90]
                xmm[0] = [fr[35], 0.0, 0.0, 0.0];
                // 0082139e: movss dword ptr [esp + 0x40], xmm0
                fr[15] = xmm[0][0];
                // 008213a4: movss xmm0, dword ptr [esp + 0x94]
                xmm[0] = [fr[36], 0.0, 0.0, 0.0];
                // 008213ad: movss dword ptr [esp + 0x44], xmm0
                fr[16] = xmm[0][0];
                // 008213b3: movss xmm0, dword ptr [esp + 0x98]
                xmm[0] = [fr[37], 0.0, 0.0, 0.0];
                // 008213bc: push 0xc
                { let v = 0xcu32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 008213be: movss dword ptr [esp + 0x4c], xmm0
                fr[17] = xmm[0][0];
                // 008213c4: call 0x421ed0
                let target: u32 = 0x00421ed0u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x008213c9);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 008213c9: test eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 008213cb: je 0x82141b
                if zf { pc = 0x0082141b; continue; }
                pc = 0x008213cd;
                continue;
            }
            0x008213cd => {
                // 008213cd: movss xmm0, dword ptr [esp + 0x94]
                xmm[0] = [fr[36], 0.0, 0.0, 0.0];
                // 008213d6: movaps xmm1, xmmword ptr [eax + 0x20]
                xmm[1] = [f32::from_bits({ ld139 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(0)); ld139 }), f32::from_bits({ ld140 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(4)); ld140 }), f32::from_bits({ ld141 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(8)); ld141 }), f32::from_bits({ ld142 = pg.u32(m, eax.wrapping_add(0x20).wrapping_add(12)); ld142 })];
                // 008213da: movaps xmm2, xmmword ptr [eax + 0x10]
                xmm[2] = [f32::from_bits({ ld143 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(0)); ld143 }), f32::from_bits({ ld144 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(4)); ld144 }), f32::from_bits({ ld145 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(8)); ld145 }), f32::from_bits({ ld146 = pg.u32(m, eax.wrapping_add(0x10).wrapping_add(12)); ld146 })];
                // 008213de: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 008213e2: mulps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::mulss(xmm[1][k], s[k]); } }
                // 008213e5: movss xmm0, dword ptr [esp + 0x90]
                xmm[0] = [fr[35], 0.0, 0.0, 0.0];
                // 008213ee: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 008213f2: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 008213f5: movss xmm0, dword ptr [esp + 0x98]
                xmm[0] = [fr[37], 0.0, 0.0, 0.0];
                // 008213fe: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00821401: movaps xmm2, xmmword ptr [eax + 0x30]
                xmm[2] = [f32::from_bits({ ld147 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(0)); ld147 }), f32::from_bits({ ld148 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(4)); ld148 }), f32::from_bits({ ld149 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(8)); ld149 }), f32::from_bits({ ld150 = pg.u32(m, eax.wrapping_add(0x30).wrapping_add(12)); ld150 })];
                // 00821405: shufps xmm0, xmm0, 0
                { let s = xmm[0]; xmm[0] = rt::shufps(xmm[0], s, 0); }
                // 00821409: mulps xmm2, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[2][k] = rt::mulss(xmm[2][k], s[k]); } }
                // 0082140c: movaps xmm0, xmmword ptr [eax + 0x40]
                xmm[0] = [f32::from_bits({ ld151 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(0)); ld151 }), f32::from_bits({ ld152 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(4)); ld152 }), f32::from_bits({ ld153 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(8)); ld153 }), f32::from_bits({ ld154 = pg.u32(m, eax.wrapping_add(0x40).wrapping_add(12)); ld154 })];
                // 00821410: addps xmm1, xmm2
                { let s = xmm[2]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00821413: addps xmm1, xmm0
                { let s = xmm[0]; for k in 0..4 { xmm[1][k] = rt::addss(xmm[1][k], s[k]); } }
                // 00821416: movaps xmmword ptr [esp + 0x40], xmm1
                { let v = xmm[1]; fr[15] = v[0]; fr[16] = v[1]; fr[17] = v[2]; fr[18] = v[3]; }
                pc = 0x0082141b;
                continue;
            }
            0x0082141b => {
                // 0082141b: movss xmm0, dword ptr [esp + 0x40]
                xmm[0] = [fr[15], 0.0, 0.0, 0.0];
                // 00821421: movss xmm1, dword ptr [esp + 0x44]
                xmm[1] = [fr[16], 0.0, 0.0, 0.0];
                // 00821427: movss xmm2, dword ptr [esp + 0x48]
                xmm[2] = [fr[17], 0.0, 0.0, 0.0];
                // 0082142d: movss dword ptr [esi + 0x40], xmm0
                pg.set_u32(m, esi.wrapping_add(0x40), xmm[0][0].to_bits());
                // 00821432: movss dword ptr [esi + 0x44], xmm1
                pg.set_u32(m, esi.wrapping_add(0x44), xmm[1][0].to_bits());
                // 00821437: movss dword ptr [esi + 0x48], xmm2
                pg.set_u32(m, esi.wrapping_add(0x48), xmm[2][0].to_bits());
                // 0082143c: movss xmm0, dword ptr [0x15319b8]
                xmm[0] = [f32::from_bits({ ld155 = pg.u32(m, 0x015319b8u32); ld155 }), 0.0, 0.0, 0.0];
                // 00821444: movss dword ptr [esi + 0x4c], xmm0
                pg.set_u32(m, esi.wrapping_add(0x4c), xmm[0][0].to_bits());
                pc = 0x00821449;
                continue;
            }
            0x00821449 => {
                // 00821449: pop esi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); esi = v; }
                // 0082144a: mov esp, ebp
                esp = ebp;
                // 0082144c: pop ebp
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
                // 0082144d: ret 8
                esp = esp.wrapping_add(4 + 8);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                return;
            }
            _ => unreachable!("pc {pc:#x}"),
        }
    }
}
