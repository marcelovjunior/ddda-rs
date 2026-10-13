//! L184 (s4): mechanically translated semantic port (the ground probe 0x00B72360). Regenerate with
//! `research/native/semport.py 0xb72360 0xb72d28 probe_ground out.rs doc.txt`; the generated
//! body is below, one commented Rust statement per original instruction.
//! Verified against the original's recorded behaviour
//! (crates/mtcore/tests/semantic_s4.rs).
use super::semantic_s4_rt as rt;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

// Translated from 0x00b72360 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport.py (operation order, widths and memory accesses
// of the original; registers, XMM registers and flags in locals; the frame
// stays in guest memory because its locals are passed to the callees).
/// 0x00B72360, `player->probe_ground() -> found` (ESI = player, returns
/// AL; the names are guesses): up to four candidate points around the
/// stored position (+0x49F0..) are checked against the world with the
/// capsule query 0x00DEA870 (sweeps built with 0x00DE45C0 / 0x00DE4710
/// whose size scale comes from 0x00B85F70 and the state byte at +0x206D),
/// the first hit becomes the new anchor at +0x4A30..+0x4A3C; otherwise the
/// position is probed down and sideways (three more sweeps) and the
/// resulting slide vector is range-checked. Returns whether a hit was found.

#[allow(unused_assignments, unused_variables, unused_mut, unused_labels, unreachable_code, clippy::all)]
pub fn probe_ground(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));
    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);
    let mut ld0 = 0u32; let mut ld1 = 0u32; let mut ld2 = 0u32; let mut ld3 = 0u32; let mut ld4 = 0u32; let mut ld5 = 0u32; let mut ld6 = 0u32; let mut ld7 = 0u32; let mut ld8 = 0u32; let mut ld9 = 0u32; let mut ld10 = 0u32; let mut ld11 = 0u32; let mut ld12 = 0u32; let mut ld13 = 0u32; let mut ld14 = 0u32; let mut ld15 = 0u32; let mut ld16 = 0u32; let mut ld17 = 0u32; let mut ld18 = 0u32; let mut ld19 = 0u32; let mut ld20 = 0u32; let mut ld21 = 0u32; let mut ld22 = 0u32; let mut ld23 = 0u32; let mut ld24 = 0u32; let mut ld25 = 0u32; let mut ld26 = 0u32; let mut ld27 = 0u32; let mut ld28 = 0u32; let mut ld29 = 0u32; let mut ld30 = 0u32; let mut ld31 = 0u32; let mut ld32 = 0u32; let mut ld33 = 0u32; let mut ld34 = 0u32; let mut ld35 = 0u32; let mut ld36 = 0u32; let mut ld37 = 0u32; let mut ld38 = 0u32; let mut ld39 = 0u32; let mut ld40 = 0u32; let mut ld41 = 0u32; let mut ld42 = 0u32; let mut ld43 = 0u32; let mut ld44 = 0u32; let mut ld45 = 0u32; let mut ld46 = 0u32; let mut ld47 = 0u32; let mut ld48 = 0u32; let mut ld49 = 0u32; let mut ld50 = 0u32; let mut ld51 = 0u32; let mut ld52 = 0u32; let mut ld53 = 0u32; let mut ld54 = 0u32; let mut ld55 = 0u32; let mut ld56 = 0u32; let mut ld57 = 0u32; let mut ld58 = 0u32; let mut ld59 = 0u32; let mut ld60 = 0u32; let mut ld61 = 0u32; let mut ld62 = 0u32; let mut ld63 = 0u32; let mut ld64 = 0u32; let mut ld65 = 0u32; let mut ld66 = 0u32; let mut ld67 = 0u32; let mut ld68 = 0u32; let mut ld69 = 0u32; let mut ld70 = 0u32; let mut ld71 = 0u32; let mut ld72 = 0u32; let mut ld73 = 0u32; let mut ld74 = 0u32; let mut ld75 = 0u32; let mut ld76 = 0u32; let mut ld77 = 0u32; let mut ld78 = 0u32; let mut ld79 = 0u32; let mut ld80 = 0u32; let mut ld81 = 0u32; let mut ld82 = 0u32; let mut ld83 = 0u32; let mut ld84 = 0u32; let mut ld85 = 0u32; let mut ld86 = 0u32; let mut ld87 = 0u32; let mut ld88 = 0u32; let mut ld89 = 0u32; let mut ld90 = 0u32; let mut ld91 = 0u32; let mut ld92 = 0u32; let mut ld93 = 0u32; let mut ld94 = 0u32; let mut ld95 = 0u32; let mut ld96 = 0u32; let mut ld97 = 0u32; let mut ld98 = 0u32; let mut ld99 = 0u32; let mut ld100 = 0u32; let mut ld101 = 0u32; let mut ld102 = 0u32; let mut ld103 = 0u32; let mut ld104 = 0u32; let mut ld105 = 0u32; let mut ld106 = 0u32; let mut ld107 = 0u32; let mut ld108 = 0u32; let mut ld109 = 0u32; let mut ld110 = 0u32; let mut ld111 = 0u32; let mut ld112 = 0u32; let mut ld113 = 0u32; let mut ld114 = 0u32; let mut ld115 = 0u32; let mut ld116 = 0u32; let mut ld117 = 0u32; let mut ld118 = 0u32; let mut ld119 = 0u32; let mut ld120 = 0u32; let mut ld121 = 0u32; let mut ld122 = 0u32; let mut ld123 = 0u32; let mut ld124 = 0u32; let mut ld125 = 0u32; let mut ld126 = 0u32; let mut ld127 = 0u32;
    'b00b72d0c: {
    'b00b72cd3: {
    'b00b72cb4: {
    'b00b72c58: {
    'b00b72c4e: {
    'b00b72c3d: {
    'b00b72c31: {
    'b00b72c25: {
    'b00b72b70: {
    'b00b72b6d: {
    'b00b72a33: {
    'b00b72a29: {
    'b00b72a18: {
    'b00b72a0c: {
    'b00b72a00: {
    'b00b7287a: {
    'b00b7286e: {
    'b00b72711: {
    'b00b7267c: {
    'b00b72672: {
    'b00b72661: {
    'b00b72655: {
    'b00b72649: {
    'b00b72594: {
        // 00b72360: push ebp
        { let v = ebp; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72361: mov ebp, esp
        ebp = esp;
        // 00b72363: and esp, 0xfffffff0
        { let (a, b) = (esp, 0xfffffff0u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        let r = (a & b) & 0xffffffffu32;
        esp = r;
        }
        // 00b72366: sub esp, 0x1c8
        { let (a, b) = (esp, 0x1c8u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
        let r = a.wrapping_sub(b) & 0xffffffffu32;
        esp = r;
        }
        // 00b7236c: xorps xmm0, xmm0
        { let s = xmm[0]; for k in 0..4 { xmm[0][k] = f32::from_bits(xmm[0][k].to_bits() ^ s[k].to_bits()); } }
        // 00b7236f: push ebx
        { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72370: push edi
        { let v = edi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72371: lea ecx, [esp + 0x70]
        ecx = esp.wrapping_add(0x70);
        // 00b72375: mov byte ptr [esp + 0x3f], 0
        pg.set_u8(m, esp.wrapping_add(0x3f), (0x0u32) as u8);
        // 00b7237a: movss dword ptr [esp + 0x1c], xmm0
        pg.set_u32(m, esp.wrapping_add(0x1c), xmm[0][0].to_bits());
        // 00b72380: movss dword ptr [esp + 0x2c], xmm0
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[0][0].to_bits());
        // 00b72386: call 0xde5cc0
        let target: u32 = 0x00de5cc0u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b7238b);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b7238b: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7238d: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7238f: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72391: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72393: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72395: push 0xa
        { let v = 0xau32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72397: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72399: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7239b: push 0x3fffffff
        { let v = 0x3fffffffu32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b723a0: push 0x7fffffff
        { let v = 0x7fffffffu32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b723a5: lea ecx, [esp + 0x138]
        ecx = esp.wrapping_add(0x138);
        // 00b723ac: call 0xde45c0
        let target: u32 = 0x00de45c0u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b723b1);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b723b1: movss xmm6, dword ptr [0x1531ae8]
        xmm[6] = [f32::from_bits({ ld0 = pg.u32(m, 0x01531ae8u32); ld0 }), 0.0, 0.0, 0.0];
        // 00b723b9: xor ebx, ebx
        { let (a, b) = (ebx, ebx);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
        let r = (a ^ b) & 0xffffffffu32;
        ebx = r;
        }
        // 00b723bb: lea edi, [esi + 0x49f8]
        edi = esi.wrapping_add(0x49f8);
    'l00b723c1: loop {
    'b00b7257e: {
    'b00b72576: {
    'b00b7251a: {
    'b00b724fb: {
        // 00b723c1: movss xmm3, dword ptr [edi - 8]
        xmm[3] = [f32::from_bits({ ld1 = pg.u32(m, edi.wrapping_add(0xfffffff8)); ld1 }), 0.0, 0.0, 0.0];
        // 00b723c6: movss xmm4, dword ptr [edi - 4]
        xmm[4] = [f32::from_bits({ ld2 = pg.u32(m, edi.wrapping_add(0xfffffffc)); ld2 }), 0.0, 0.0, 0.0];
        // 00b723cb: movss xmm5, dword ptr [edi]
        xmm[5] = [f32::from_bits({ ld3 = pg.u32(m, edi); ld3 }), 0.0, 0.0, 0.0];
        // 00b723cf: movaps xmm0, xmm3
        xmm[0] = xmm[3];
        // 00b723d2: subss xmm0, dword ptr [esi + 0x40]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld4 = pg.u32(m, esi.wrapping_add(0x40)); ld4 }));
        // 00b723d7: movaps xmm1, xmm4
        xmm[1] = xmm[4];
        // 00b723da: subss xmm1, dword ptr [esi + 0x44]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld5 = pg.u32(m, esi.wrapping_add(0x44)); ld5 }));
        // 00b723df: movaps xmm2, xmm5
        xmm[2] = xmm[5];
        // 00b723e2: subss xmm2, dword ptr [esi + 0x48]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld6 = pg.u32(m, esi.wrapping_add(0x48)); ld6 }));
        // 00b723e7: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b723eb: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b723ef: addss xmm1, xmm0
        xmm[1][0] = rt::addss(xmm[1][0], xmm[0][0]);
        // 00b723f3: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b723f7: addss xmm1, xmm2
        xmm[1][0] = rt::addss(xmm[1][0], xmm[2][0]);
        // 00b723fb: sqrtss xmm0, xmm1
        xmm[0][0] = xmm[1][0].sqrt();
        // 00b723ff: comiss xmm6, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[6][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72402: jbe 0xb7257e
        if (cf || zf) { break 'b00b7257e; }
        // 00b72408: movss xmm1, dword ptr [0x1629544]
        xmm[1] = [f32::from_bits({ ld7 = pg.u32(m, 0x01629544u32); ld7 }), 0.0, 0.0, 0.0];
        // 00b72410: movss xmm0, dword ptr [0x1629540]
        xmm[0] = [f32::from_bits({ ld8 = pg.u32(m, 0x01629540u32); ld8 }), 0.0, 0.0, 0.0];
        // 00b72418: movss xmm2, dword ptr [0x1629548]
        xmm[2] = [f32::from_bits({ ld9 = pg.u32(m, 0x01629548u32); ld9 }), 0.0, 0.0, 0.0];
        // 00b72420: mulss xmm1, xmm6
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
        // 00b72424: mulss xmm2, xmm6
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[6][0]);
        // 00b72428: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 00b7242c: movaps xmm6, xmm3
        xmm[6] = xmm[3];
        // 00b7242f: addss xmm6, xmm0
        xmm[6][0] = rt::addss(xmm[6][0], xmm[0][0]);
        // 00b72433: movaps xmm7, xmm1
        xmm[7] = xmm[1];
        // 00b72436: addss xmm7, xmm4
        xmm[7][0] = rt::addss(xmm[7][0], xmm[4][0]);
        // 00b7243a: movss dword ptr [esp + 0x64], xmm7
        pg.set_u32(m, esp.wrapping_add(0x64), xmm[7][0].to_bits());
        // 00b72440: movss dword ptr [esp + 0x10], xmm6
        pg.set_u32(m, esp.wrapping_add(0x10), xmm[6][0].to_bits());
        // 00b72446: movss xmm6, dword ptr [esp + 0x64]
        xmm[6] = [f32::from_bits({ ld10 = pg.u32(m, esp.wrapping_add(0x64)); ld10 }), 0.0, 0.0, 0.0];
        // 00b7244c: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7244e: movaps xmm7, xmm2
        xmm[7] = xmm[2];
        // 00b72451: addss xmm7, xmm5
        xmm[7][0] = rt::addss(xmm[7][0], xmm[5][0]);
        // 00b72455: movss dword ptr [esp + 0x18], xmm6
        pg.set_u32(m, esp.wrapping_add(0x18), xmm[6][0].to_bits());
        // 00b7245b: xorps xmm6, xmm6
        { let s = xmm[6]; for k in 0..4 { xmm[6][k] = f32::from_bits(xmm[6][k].to_bits() ^ s[k].to_bits()); } }
        // 00b7245e: subss xmm3, xmm0
        xmm[3][0] = rt::subss(xmm[3][0], xmm[0][0]);
        // 00b72462: subss xmm4, xmm1
        xmm[4][0] = rt::subss(xmm[4][0], xmm[1][0]);
        // 00b72466: subss xmm5, xmm2
        xmm[5][0] = rt::subss(xmm[5][0], xmm[2][0]);
        // 00b7246a: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7246c: mov ecx, esi
        ecx = esi;
        // 00b7246e: movss dword ptr [esp + 0x4c], xmm1
        pg.set_u32(m, esp.wrapping_add(0x4c), xmm[1][0].to_bits());
        // 00b72474: movss dword ptr [esp + 0x50], xmm2
        pg.set_u32(m, esp.wrapping_add(0x50), xmm[2][0].to_bits());
        // 00b7247a: movss dword ptr [esp + 0x20], xmm7
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[7][0].to_bits());
        // 00b72480: movss dword ptr [esp + 0x24], xmm6
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[6][0].to_bits());
        // 00b72486: movss dword ptr [esp + 0x28], xmm3
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[3][0].to_bits());
        // 00b7248c: movss dword ptr [esp + 0x2c], xmm4
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[4][0].to_bits());
        // 00b72492: movss dword ptr [esp + 0x30], xmm5
        pg.set_u32(m, esp.wrapping_add(0x30), xmm[5][0].to_bits());
        // 00b72498: movss dword ptr [esp + 0x34], xmm6
        pg.set_u32(m, esp.wrapping_add(0x34), xmm[6][0].to_bits());
        // 00b7249e: call 0xb85f70
        let target: u32 = 0x00b85f70u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b724a3);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b724a3: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b724a4: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b724a6: lea ecx, [esp + 0x120]
        ecx = esp.wrapping_add(0x120);
        // 00b724ad: call 0xde4710
        let target: u32 = 0x00de4710u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b724b2);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b724b2: movss xmm0, dword ptr [esp + 0x10]
        xmm[0] = [f32::from_bits({ ld11 = pg.u32(m, esp.wrapping_add(0x10)); ld11 }), 0.0, 0.0, 0.0];
        // 00b724b8: subss xmm0, dword ptr [esp + 0x20]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld12 = pg.u32(m, esp.wrapping_add(0x20)); ld12 }));
        // 00b724be: movss xmm1, dword ptr [esp + 0x14]
        xmm[1] = [f32::from_bits({ ld13 = pg.u32(m, esp.wrapping_add(0x14)); ld13 }), 0.0, 0.0, 0.0];
        // 00b724c4: subss xmm1, dword ptr [esp + 0x24]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld14 = pg.u32(m, esp.wrapping_add(0x24)); ld14 }));
        // 00b724ca: movss xmm2, dword ptr [esp + 0x18]
        xmm[2] = [f32::from_bits({ ld15 = pg.u32(m, esp.wrapping_add(0x18)); ld15 }), 0.0, 0.0, 0.0];
        // 00b724d0: subss xmm2, dword ptr [esp + 0x28]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld16 = pg.u32(m, esp.wrapping_add(0x28)); ld16 }));
        // 00b724d6: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b724da: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b724de: addss xmm1, xmm0
        xmm[1][0] = rt::addss(xmm[1][0], xmm[0][0]);
        // 00b724e2: movss xmm0, dword ptr [0x152cf28]
        xmm[0] = [f32::from_bits({ ld17 = pg.u32(m, 0x0152cf28u32); ld17 }), 0.0, 0.0, 0.0];
        // 00b724ea: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b724ee: addss xmm1, xmm2
        xmm[1][0] = rt::addss(xmm[1][0], xmm[2][0]);
        // 00b724f2: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b724f5: jbe 0xb724fb
        if (cf || zf) { break 'b00b724fb; }
        // 00b724f7: xor eax, eax
        { let (a, b) = (eax, eax);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
        let r = (a ^ b) & 0xffffffffu32;
        eax = r;
        }
        // 00b724f9: jmp 0xb7251a
        break 'b00b7251a;
    } // end of the jumps to block 00b724fb
        // 00b724fb: lea eax, [esp + 0x110]
        eax = esp.wrapping_add(0x110);
        // 00b72502: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72503: lea ecx, [esp + 0x74]
        ecx = esp.wrapping_add(0x74);
        // 00b72507: push ecx
        { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72508: mov ecx, dword ptr [0x18d0dd0]
        ecx = { ld18 = pg.u32(m, 0x018d0dd0u32); ld18 };
        // 00b7250e: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72510: lea edx, [esp + 0x1c]
        edx = esp.wrapping_add(0x1c);
        // 00b72514: push edx
        { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72515: call 0xdea870
        let target: u32 = 0x00dea870u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b7251a);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
    } // end of the jumps to block 00b7251a
        // 00b7251a: test al, 1
        { let (a, b) = (((eax >> 0) & 0xff), 0x1u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b7251c: je 0xb72576
        if zf { break 'b00b72576; }
        // 00b7251e: movss xmm0, dword ptr [esp + 0xe0]
        xmm[0] = [f32::from_bits({ ld19 = pg.u32(m, esp.wrapping_add(0xe0)); ld19 }), 0.0, 0.0, 0.0];
        // 00b72527: movss dword ptr [esi + 0x4a30], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a30), xmm[0][0].to_bits());
        // 00b7252f: movss xmm0, dword ptr [esp + 0xe4]
        xmm[0] = [f32::from_bits({ ld20 = pg.u32(m, esp.wrapping_add(0xe4)); ld20 }), 0.0, 0.0, 0.0];
        // 00b72538: movss dword ptr [esi + 0x4a34], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a34), xmm[0][0].to_bits());
        // 00b72540: movss xmm0, dword ptr [esp + 0xe8]
        xmm[0] = [f32::from_bits({ ld21 = pg.u32(m, esp.wrapping_add(0xe8)); ld21 }), 0.0, 0.0, 0.0];
        // 00b72549: movss dword ptr [esi + 0x4a38], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a38), xmm[0][0].to_bits());
        // 00b72551: xorps xmm7, xmm7
        { let s = xmm[7]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
        // 00b72554: movss dword ptr [esi + 0x4a3c], xmm7
        pg.set_u32(m, esi.wrapping_add(0x4a3c), xmm[7][0].to_bits());
        // 00b7255c: movss xmm0, dword ptr [esi + 0x44]
        xmm[0] = [f32::from_bits({ ld22 = pg.u32(m, esi.wrapping_add(0x44)); ld22 }), 0.0, 0.0, 0.0];
        // 00b72561: subss xmm0, dword ptr [0x150a088]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld23 = pg.u32(m, 0x0150a088u32); ld23 }));
        // 00b72569: movss xmm1, dword ptr [esi + 0x4a34]
        xmm[1] = [f32::from_bits({ ld24 = pg.u32(m, esi.wrapping_add(0x4a34)); ld24 }), 0.0, 0.0, 0.0];
        // 00b72571: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72574: jae 0xb72594
        if !cf { break 'b00b72594; }
    } // end of the jumps to block 00b72576
        // 00b72576: movss xmm6, dword ptr [0x1531ae8]
        xmm[6] = [f32::from_bits({ ld25 = pg.u32(m, 0x01531ae8u32); ld25 }), 0.0, 0.0, 0.0];
    } // end of the jumps to block 00b7257e
        // 00b7257e: inc ebx
        { let a = ebx; let old_cf = cf;
        (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
        let r = a.wrapping_add(1) & 0xffffffffu32;
        cf = old_cf;
        ebx = r;
        }
        // 00b7257f: add edi, 0x10
        { let (a, b) = (edi, 0x10u32);
        (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
        let r = a.wrapping_add(b) & 0xffffffffu32;
        edi = r;
        }
        // 00b72582: cmp ebx, 4
        { let (a, b) = (ebx, 0x4u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
        }
        // 00b72585: jb 0xb723c1
        if cf { continue 'l00b723c1; }
        break 'l00b723c1;
    }
        // 00b7258b: mov bl, byte ptr [esp + 0x3f]
        ebx = (ebx & !(0xffu32 << 0)) | ((({ ld26 = pg.u8(m, esp.wrapping_add(0x3f)) as u32; ld26 }) & 0xff) << 0);
        // 00b7258f: jmp 0xb72d0c
        break 'b00b72d0c;
    } // end of the jumps to block 00b72594
        // 00b72594: cmp byte ptr [esi + 0x2f84], 0
        { let (a, b) = ({ ld27 = pg.u8(m, esi.wrapping_add(0x2f84)) as u32; ld27 }, 0x0u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 00b7259b: movss xmm0, dword ptr [0x18cfe80]
        xmm[0] = [f32::from_bits({ ld28 = pg.u32(m, 0x018cfe80u32); ld28 }), 0.0, 0.0, 0.0];
        // 00b725a3: movss xmm6, dword ptr [0x1520bac]
        xmm[6] = [f32::from_bits({ ld29 = pg.u32(m, 0x01520bacu32); ld29 }), 0.0, 0.0, 0.0];
        // 00b725ab: movss xmm1, dword ptr [esi + 0x44]
        xmm[1] = [f32::from_bits({ ld30 = pg.u32(m, esi.wrapping_add(0x44)); ld30 }), 0.0, 0.0, 0.0];
        // 00b725b0: movss xmm2, dword ptr [esi + 0x48]
        xmm[2] = [f32::from_bits({ ld31 = pg.u32(m, esi.wrapping_add(0x48)); ld31 }), 0.0, 0.0, 0.0];
        // 00b725b5: movss xmm3, dword ptr [0x1629540]
        xmm[3] = [f32::from_bits({ ld32 = pg.u32(m, 0x01629540u32); ld32 }), 0.0, 0.0, 0.0];
        // 00b725bd: movss xmm4, dword ptr [0x1629544]
        xmm[4] = [f32::from_bits({ ld33 = pg.u32(m, 0x01629544u32); ld33 }), 0.0, 0.0, 0.0];
        // 00b725c5: movss xmm5, dword ptr [0x1629548]
        xmm[5] = [f32::from_bits({ ld34 = pg.u32(m, 0x01629548u32); ld34 }), 0.0, 0.0, 0.0];
        // 00b725cd: mov cl, byte ptr [esi + 0x206d]
        ecx = (ecx & !(0xffu32 << 0)) | ((({ ld35 = pg.u8(m, esi.wrapping_add(0x206d)) as u32; ld35 }) & 0xff) << 0);
        // 00b725d3: movss dword ptr [esp + 0x40], xmm0
        pg.set_u32(m, esp.wrapping_add(0x40), xmm[0][0].to_bits());
        // 00b725d9: movss xmm0, dword ptr [0x18cfe88]
        xmm[0] = [f32::from_bits({ ld36 = pg.u32(m, 0x018cfe88u32); ld36 }), 0.0, 0.0, 0.0];
        // 00b725e1: movss dword ptr [esp + 0x48], xmm0
        pg.set_u32(m, esp.wrapping_add(0x48), xmm[0][0].to_bits());
        // 00b725e7: movss xmm0, dword ptr [esi + 0x40]
        xmm[0] = [f32::from_bits({ ld37 = pg.u32(m, esi.wrapping_add(0x40)); ld37 }), 0.0, 0.0, 0.0];
        // 00b725ec: movss dword ptr [esp + 0x10], xmm0
        pg.set_u32(m, esp.wrapping_add(0x10), xmm[0][0].to_bits());
        // 00b725f2: movss dword ptr [esp + 0x14], xmm1
        pg.set_u32(m, esp.wrapping_add(0x14), xmm[1][0].to_bits());
        // 00b725f8: movss dword ptr [esp + 0x18], xmm2
        pg.set_u32(m, esp.wrapping_add(0x18), xmm[2][0].to_bits());
        // 00b725fe: mulss xmm3, xmm6
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[6][0]);
        // 00b72602: mulss xmm4, xmm6
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[6][0]);
        // 00b72606: mulss xmm5, xmm6
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[6][0]);
        // 00b7260a: subss xmm0, xmm3
        xmm[0][0] = rt::subss(xmm[0][0], xmm[3][0]);
        // 00b7260e: subss xmm1, xmm4
        xmm[1][0] = rt::subss(xmm[1][0], xmm[4][0]);
        // 00b72612: subss xmm2, xmm5
        xmm[2][0] = rt::subss(xmm[2][0], xmm[5][0]);
        // 00b72616: mov bl, 1
        ebx = (ebx & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
        // 00b72618: movss dword ptr [esp + 0x1c], xmm7
        pg.set_u32(m, esp.wrapping_add(0x1c), xmm[7][0].to_bits());
        // 00b7261e: movss dword ptr [esp + 0x20], xmm0
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[0][0].to_bits());
        // 00b72624: movss dword ptr [esp + 0x24], xmm1
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[1][0].to_bits());
        // 00b7262a: movss dword ptr [esp + 0x28], xmm2
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[2][0].to_bits());
        // 00b72630: movss dword ptr [esp + 0x2c], xmm7
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[7][0].to_bits());
        // 00b72636: mov eax, 0x3f74e9c7
        eax = 0x3f74e9c7u32;
        // 00b7263b: je 0xb72661
        if zf { break 'b00b72661; }
        // 00b7263d: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72640: je 0xb72649
        if zf { break 'b00b72649; }
        // 00b72642: mov eax, 0x3f54e9c7
        eax = 0x3f54e9c7u32;
        // 00b72647: jmp 0xb7267c
        break 'b00b7267c;
    } // end of the jumps to block 00b72649
        // 00b72649: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b7264c: je 0xb72655
        if zf { break 'b00b72655; }
        // 00b7264e: mov eax, 0x3f5469c7
        eax = 0x3f5469c7u32;
        // 00b72653: jmp 0xb7267c
        break 'b00b7267c;
    } // end of the jumps to block 00b72655
        // 00b72655: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72658: je 0xb7267c
        if zf { break 'b00b7267c; }
        // 00b7265a: mov eax, 0x3f5429c7
        eax = 0x3f5429c7u32;
        // 00b7265f: jmp 0xb7267c
        break 'b00b7267c;
    } // end of the jumps to block 00b72661
        // 00b72661: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72664: jne 0xb7267c
        if !zf { break 'b00b7267c; }
        // 00b72666: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72669: je 0xb72672
        if zf { break 'b00b72672; }
        // 00b7266b: mov eax, 0x3f7469c7
        eax = 0x3f7469c7u32;
        // 00b72670: jmp 0xb7267c
        break 'b00b7267c;
    } // end of the jumps to block 00b72672
        // 00b72672: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72675: je 0xb7267c
        if zf { break 'b00b7267c; }
        // 00b72677: mov eax, 0x3f7429c7
        eax = 0x3f7429c7u32;
    } // end of the jumps to block 00b7267c
        // 00b7267c: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7267e: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72680: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72681: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72683: lea ecx, [esp + 0x120]
        ecx = esp.wrapping_add(0x120);
        // 00b7268a: call 0xde4710
        let target: u32 = 0x00de4710u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b7268f);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b7268f: movss xmm0, dword ptr [esp + 0x10]
        xmm[0] = [f32::from_bits({ ld38 = pg.u32(m, esp.wrapping_add(0x10)); ld38 }), 0.0, 0.0, 0.0];
        // 00b72695: subss xmm0, dword ptr [esp + 0x20]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld39 = pg.u32(m, esp.wrapping_add(0x20)); ld39 }));
        // 00b7269b: movss xmm1, dword ptr [esp + 0x14]
        xmm[1] = [f32::from_bits({ ld40 = pg.u32(m, esp.wrapping_add(0x14)); ld40 }), 0.0, 0.0, 0.0];
        // 00b726a1: subss xmm1, dword ptr [esp + 0x24]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld41 = pg.u32(m, esp.wrapping_add(0x24)); ld41 }));
        // 00b726a7: movss xmm2, dword ptr [esp + 0x18]
        xmm[2] = [f32::from_bits({ ld42 = pg.u32(m, esp.wrapping_add(0x18)); ld42 }), 0.0, 0.0, 0.0];
        // 00b726ad: subss xmm2, dword ptr [esp + 0x28]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld43 = pg.u32(m, esp.wrapping_add(0x28)); ld43 }));
        // 00b726b3: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b726b7: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b726bb: addss xmm0, xmm1
        xmm[0][0] = rt::addss(xmm[0][0], xmm[1][0]);
        // 00b726bf: movss xmm1, dword ptr [0x152cf28]
        xmm[1] = [f32::from_bits({ ld44 = pg.u32(m, 0x0152cf28u32); ld44 }), 0.0, 0.0, 0.0];
        // 00b726c7: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b726cb: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 00b726cf: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b726d2: ja 0xb72711
        if (!cf && !zf) { break 'b00b72711; }
        // 00b726d4: lea eax, [esp + 0x110]
        eax = esp.wrapping_add(0x110);
        // 00b726db: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b726dc: lea ecx, [esp + 0x74]
        ecx = esp.wrapping_add(0x74);
        // 00b726e0: push ecx
        { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b726e1: mov ecx, dword ptr [0x18d0dd0]
        ecx = { ld45 = pg.u32(m, 0x018d0dd0u32); ld45 };
        // 00b726e7: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b726e9: lea edx, [esp + 0x1c]
        edx = esp.wrapping_add(0x1c);
        // 00b726ed: push edx
        { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b726ee: call 0xdea870
        let target: u32 = 0x00dea870u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b726f3);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b726f3: test eax, eax
        { let (a, b) = (eax, eax);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        }
        // 00b726f5: je 0xb72711
        if zf { break 'b00b72711; }
        // 00b726f7: movss xmm2, dword ptr [esp + 0xd0]
        xmm[2] = [f32::from_bits({ ld46 = pg.u32(m, esp.wrapping_add(0xd0)); ld46 }), 0.0, 0.0, 0.0];
        // 00b72700: movss xmm3, dword ptr [esp + 0xd8]
        xmm[3] = [f32::from_bits({ ld47 = pg.u32(m, esp.wrapping_add(0xd8)); ld47 }), 0.0, 0.0, 0.0];
        // 00b72709: xorps xmm4, xmm4
        { let s = xmm[4]; for k in 0..4 { xmm[4][k] = f32::from_bits(xmm[4][k].to_bits() ^ s[k].to_bits()); } }
        // 00b7270c: jmp 0xb7287a
        break 'b00b7287a;
    } // end of the jumps to block 00b72711
        // 00b72711: movss xmm3, dword ptr [esp + 0x28]
        xmm[3] = [f32::from_bits({ ld48 = pg.u32(m, esp.wrapping_add(0x28)); ld48 }), 0.0, 0.0, 0.0];
        // 00b72717: movss xmm1, dword ptr [esp + 0x20]
        xmm[1] = [f32::from_bits({ ld49 = pg.u32(m, esp.wrapping_add(0x20)); ld49 }), 0.0, 0.0, 0.0];
        // 00b7271d: movss xmm2, dword ptr [esp + 0x24]
        xmm[2] = [f32::from_bits({ ld50 = pg.u32(m, esp.wrapping_add(0x24)); ld50 }), 0.0, 0.0, 0.0];
        // 00b72723: movss xmm0, dword ptr [esi + 0x4a38]
        xmm[0] = [f32::from_bits({ ld51 = pg.u32(m, esi.wrapping_add(0x4a38)); ld51 }), 0.0, 0.0, 0.0];
        // 00b7272b: movss xmm5, dword ptr [esi + 0x4a30]
        xmm[5] = [f32::from_bits({ ld52 = pg.u32(m, esi.wrapping_add(0x4a30)); ld52 }), 0.0, 0.0, 0.0];
        // 00b72733: movss xmm6, dword ptr [esi + 0x4a34]
        xmm[6] = [f32::from_bits({ ld53 = pg.u32(m, esi.wrapping_add(0x4a34)); ld53 }), 0.0, 0.0, 0.0];
        // 00b7273b: xorps xmm4, xmm4
        { let s = xmm[4]; for k in 0..4 { xmm[4][k] = f32::from_bits(xmm[4][k].to_bits() ^ s[k].to_bits()); } }
        // 00b7273e: movss dword ptr [esp + 0x28], xmm0
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[0][0].to_bits());
        // 00b72744: subss xmm0, xmm3
        xmm[0][0] = rt::subss(xmm[0][0], xmm[3][0]);
        // 00b72748: movss dword ptr [esp + 0x68], xmm0
        pg.set_u32(m, esp.wrapping_add(0x68), xmm[0][0].to_bits());
        // 00b7274e: movss dword ptr [esp + 0x20], xmm5
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[5][0].to_bits());
        // 00b72754: movss dword ptr [esp + 0x24], xmm6
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[6][0].to_bits());
        // 00b7275a: subss xmm5, xmm1
        xmm[5][0] = rt::subss(xmm[5][0], xmm[1][0]);
        // 00b7275e: subss xmm6, xmm2
        xmm[6][0] = rt::subss(xmm[6][0], xmm[2][0]);
        // 00b72762: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 00b72765: mulss xmm0, xmm5
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[5][0]);
        // 00b72769: movaps xmm7, xmm6
        xmm[7] = xmm[6];
        // 00b7276c: mulss xmm7, xmm6
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[6][0]);
        // 00b72770: addss xmm0, xmm7
        xmm[0][0] = rt::addss(xmm[0][0], xmm[7][0]);
        // 00b72774: movss dword ptr [esp + 0x5c], xmm0
        pg.set_u32(m, esp.wrapping_add(0x5c), xmm[0][0].to_bits());
        // 00b7277a: movss xmm0, dword ptr [esp + 0x68]
        xmm[0] = [f32::from_bits({ ld54 = pg.u32(m, esp.wrapping_add(0x68)); ld54 }), 0.0, 0.0, 0.0];
        // 00b72780: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b72784: movss dword ptr [esp + 0x58], xmm0
        pg.set_u32(m, esp.wrapping_add(0x58), xmm[0][0].to_bits());
        // 00b7278a: movss xmm0, dword ptr [esp + 0x5c]
        xmm[0] = [f32::from_bits({ ld55 = pg.u32(m, esp.wrapping_add(0x5c)); ld55 }), 0.0, 0.0, 0.0];
        // 00b72790: addss xmm0, dword ptr [esp + 0x58]
        xmm[0][0] = rt::addss(xmm[0][0], f32::from_bits({ ld56 = pg.u32(m, esp.wrapping_add(0x58)); ld56 }));
        // 00b72796: comiss xmm0, xmm4
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[4][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72799: movss dword ptr [esp + 0x10], xmm1
        pg.set_u32(m, esp.wrapping_add(0x10), xmm[1][0].to_bits());
        // 00b7279f: movss dword ptr [esp + 0x14], xmm2
        pg.set_u32(m, esp.wrapping_add(0x14), xmm[2][0].to_bits());
        // 00b727a5: movss dword ptr [esp + 0x18], xmm3
        pg.set_u32(m, esp.wrapping_add(0x18), xmm[3][0].to_bits());
        // 00b727ab: movss dword ptr [esp + 0x1c], xmm4
        pg.set_u32(m, esp.wrapping_add(0x1c), xmm[4][0].to_bits());
        // 00b727b1: movss dword ptr [esp + 0x2c], xmm4
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[4][0].to_bits());
        // 00b727b7: jbe 0xb7286e
        if (cf || zf) { break 'b00b7286e; }
        // 00b727bd: sqrtss xmm7, xmm0
        xmm[7][0] = xmm[0][0].sqrt();
        // 00b727c1: movss xmm0, dword ptr [0x15319b8]
        xmm[0] = [f32::from_bits({ ld57 = pg.u32(m, 0x015319b8u32); ld57 }), 0.0, 0.0, 0.0];
        // 00b727c9: divss xmm0, xmm7
        xmm[0][0] = rt::divss(xmm[0][0], xmm[7][0]);
        // 00b727cd: mulss xmm0, dword ptr [0x150a088]
        xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld58 = pg.u32(m, 0x0150a088u32); ld58 }));
        // 00b727d5: mulss xmm5, xmm0
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[0][0]);
        // 00b727d9: mulss xmm6, xmm0
        xmm[6][0] = rt::mulss(xmm[6][0], xmm[0][0]);
        // 00b727dd: mulss xmm0, dword ptr [esp + 0x68]
        xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld59 = pg.u32(m, esp.wrapping_add(0x68)); ld59 }));
        // 00b727e3: addss xmm5, xmm1
        xmm[5][0] = rt::addss(xmm[5][0], xmm[1][0]);
        // 00b727e7: addss xmm6, xmm2
        xmm[6][0] = rt::addss(xmm[6][0], xmm[2][0]);
        // 00b727eb: addss xmm0, xmm3
        xmm[0][0] = rt::addss(xmm[0][0], xmm[3][0]);
        // 00b727ef: subss xmm1, xmm5
        xmm[1][0] = rt::subss(xmm[1][0], xmm[5][0]);
        // 00b727f3: subss xmm2, xmm6
        xmm[2][0] = rt::subss(xmm[2][0], xmm[6][0]);
        // 00b727f7: subss xmm3, xmm0
        xmm[3][0] = rt::subss(xmm[3][0], xmm[0][0]);
        // 00b727fb: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b727ff: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b72803: movss dword ptr [esp + 0x28], xmm0
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[0][0].to_bits());
        // 00b72809: movss xmm0, dword ptr [0x152cf28]
        xmm[0] = [f32::from_bits({ ld60 = pg.u32(m, 0x0152cf28u32); ld60 }), 0.0, 0.0, 0.0];
        // 00b72811: addss xmm1, xmm2
        xmm[1][0] = rt::addss(xmm[1][0], xmm[2][0]);
        // 00b72815: mulss xmm3, xmm3
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[3][0]);
        // 00b72819: addss xmm1, xmm3
        xmm[1][0] = rt::addss(xmm[1][0], xmm[3][0]);
        // 00b7281d: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72820: movss dword ptr [esp + 0x20], xmm5
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[5][0].to_bits());
        // 00b72826: movss dword ptr [esp + 0x24], xmm6
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[6][0].to_bits());
        // 00b7282c: movss dword ptr [esp + 0x2c], xmm4
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[4][0].to_bits());
        // 00b72832: ja 0xb7286e
        if (!cf && !zf) { break 'b00b7286e; }
        // 00b72834: lea eax, [esp + 0x110]
        eax = esp.wrapping_add(0x110);
        // 00b7283b: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7283c: lea ecx, [esp + 0x74]
        ecx = esp.wrapping_add(0x74);
        // 00b72840: push ecx
        { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72841: mov ecx, dword ptr [0x18d0dd0]
        ecx = { ld61 = pg.u32(m, 0x018d0dd0u32); ld61 };
        // 00b72847: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72849: lea edx, [esp + 0x1c]
        edx = esp.wrapping_add(0x1c);
        // 00b7284d: push edx
        { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b7284e: call 0xdea870
        let target: u32 = 0x00dea870u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72853);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72853: xorps xmm4, xmm4
        { let s = xmm[4]; for k in 0..4 { xmm[4][k] = f32::from_bits(xmm[4][k].to_bits() ^ s[k].to_bits()); } }
        // 00b72856: test eax, eax
        { let (a, b) = (eax, eax);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        }
        // 00b72858: je 0xb7286e
        if zf { break 'b00b7286e; }
        // 00b7285a: movss xmm2, dword ptr [esp + 0xd0]
        xmm[2] = [f32::from_bits({ ld62 = pg.u32(m, esp.wrapping_add(0xd0)); ld62 }), 0.0, 0.0, 0.0];
        // 00b72863: movss xmm3, dword ptr [esp + 0xd8]
        xmm[3] = [f32::from_bits({ ld63 = pg.u32(m, esp.wrapping_add(0xd8)); ld63 }), 0.0, 0.0, 0.0];
        // 00b7286c: jmp 0xb7287a
        break 'b00b7287a;
    } // end of the jumps to block 00b7286e
        // 00b7286e: movss xmm2, dword ptr [esp + 0x40]
        xmm[2] = [f32::from_bits({ ld64 = pg.u32(m, esp.wrapping_add(0x40)); ld64 }), 0.0, 0.0, 0.0];
        // 00b72874: movss xmm3, dword ptr [esp + 0x48]
        xmm[3] = [f32::from_bits({ ld65 = pg.u32(m, esp.wrapping_add(0x48)); ld65 }), 0.0, 0.0, 0.0];
    } // end of the jumps to block 00b7287a
        // 00b7287a: movaps xmm0, xmm2
        xmm[0] = xmm[2];
        // 00b7287d: movaps xmm1, xmm3
        xmm[1] = xmm[3];
        // 00b72880: mulss xmm0, xmm2
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[2][0]);
        // 00b72884: mulss xmm1, xmm3
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
        // 00b72888: addss xmm0, xmm1
        xmm[0][0] = rt::addss(xmm[0][0], xmm[1][0]);
        // 00b7288c: comiss xmm0, dword ptr [0x150a350]
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], f32::from_bits({ ld66 = pg.u32(m, 0x0150a350u32); ld66 })); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72893: jbe 0xb72d0c
        if (cf || zf) { break 'b00b72d0c; }
        // 00b72899: cmp byte ptr [esi + 0x2f84], 0
        { let (a, b) = ({ ld67 = pg.u8(m, esi.wrapping_add(0x2f84)) as u32; ld67 }, 0x0u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 00b728a0: movss xmm5, dword ptr [esi + 0x44]
        xmm[5] = [f32::from_bits({ ld68 = pg.u32(m, esi.wrapping_add(0x44)); ld68 }), 0.0, 0.0, 0.0];
        // 00b728a5: subss xmm5, dword ptr [esi + 0x4a34]
        xmm[5][0] = rt::subss(xmm[5][0], f32::from_bits({ ld69 = pg.u32(m, esi.wrapping_add(0x4a34)); ld69 }));
        // 00b728ad: movss xmm6, dword ptr [esi + 0x48]
        xmm[6] = [f32::from_bits({ ld70 = pg.u32(m, esi.wrapping_add(0x48)); ld70 }), 0.0, 0.0, 0.0];
        // 00b728b2: subss xmm6, dword ptr [esi + 0x4a38]
        xmm[6][0] = rt::subss(xmm[6][0], f32::from_bits({ ld71 = pg.u32(m, esi.wrapping_add(0x4a38)); ld71 }));
        // 00b728ba: sqrtss xmm1, xmm0
        xmm[1][0] = xmm[0][0].sqrt();
        // 00b728be: movss xmm0, dword ptr [0x15319b8]
        xmm[0] = [f32::from_bits({ ld72 = pg.u32(m, 0x015319b8u32); ld72 }), 0.0, 0.0, 0.0];
        // 00b728c6: divss xmm0, xmm1
        xmm[0][0] = rt::divss(xmm[0][0], xmm[1][0]);
        // 00b728ca: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 00b728cd: mulss xmm1, xmm2
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
        // 00b728d1: mulss xmm3, xmm0
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[0][0]);
        // 00b728d5: mov cl, byte ptr [esi + 0x206d]
        ecx = (ecx & !(0xffu32 << 0)) | ((({ ld73 = pg.u8(m, esi.wrapping_add(0x206d)) as u32; ld73 }) & 0xff) << 0);
        // 00b728db: movaps xmm2, xmm0
        xmm[2] = xmm[0];
        // 00b728de: mulss xmm2, xmm4
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[4][0]);
        // 00b728e2: movss xmm4, dword ptr [esi + 0x40]
        xmm[4] = [f32::from_bits({ ld74 = pg.u32(m, esi.wrapping_add(0x40)); ld74 }), 0.0, 0.0, 0.0];
        // 00b728e7: subss xmm4, dword ptr [esi + 0x4a30]
        xmm[4][0] = rt::subss(xmm[4][0], f32::from_bits({ ld75 = pg.u32(m, esi.wrapping_add(0x4a30)); ld75 }));
        // 00b728ef: movaps xmm0, xmm4
        xmm[0] = xmm[4];
        // 00b728f2: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 00b728f6: movaps xmm7, xmm5
        xmm[7] = xmm[5];
        // 00b728f9: mulss xmm7, xmm2
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
        // 00b728fd: addss xmm0, xmm7
        xmm[0][0] = rt::addss(xmm[0][0], xmm[7][0]);
        // 00b72901: movaps xmm7, xmm6
        xmm[7] = xmm[6];
        // 00b72904: mulss xmm7, xmm3
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[3][0]);
        // 00b72908: addss xmm0, xmm7
        xmm[0][0] = rt::addss(xmm[0][0], xmm[7][0]);
        // 00b7290c: xorps xmm0, xmmword ptr [0x161c280]
        { let s = [f32::from_bits({ ld76 = pg.u32(m, 0x0161c280u32.wrapping_add(0)); ld76 }), f32::from_bits({ ld77 = pg.u32(m, 0x0161c280u32.wrapping_add(4)); ld77 }), f32::from_bits({ ld78 = pg.u32(m, 0x0161c280u32.wrapping_add(8)); ld78 }), f32::from_bits({ ld79 = pg.u32(m, 0x0161c280u32.wrapping_add(12)); ld79 })]; for k in 0..4 { xmm[0][k] = f32::from_bits(xmm[0][k].to_bits() ^ s[k].to_bits()); } }
        // 00b72913: movss xmm7, dword ptr [esi + 0x4a30]
        xmm[7] = [f32::from_bits(ld75), 0.0, 0.0, 0.0];
        // 00b7291b: mulss xmm1, xmm0
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
        // 00b7291f: mulss xmm2, xmm0
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[0][0]);
        // 00b72923: mulss xmm3, xmm0
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[0][0]);
        // 00b72927: movss xmm0, dword ptr [esi + 0x4a34]
        xmm[0] = [f32::from_bits(ld69), 0.0, 0.0, 0.0];
        // 00b7292f: addss xmm2, xmm5
        xmm[2][0] = rt::addss(xmm[2][0], xmm[5][0]);
        // 00b72933: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 00b72937: movss xmm2, dword ptr [0x1629548]
        xmm[2] = [f32::from_bits({ ld80 = pg.u32(m, 0x01629548u32); ld80 }), 0.0, 0.0, 0.0];
        // 00b7293f: addss xmm3, xmm6
        xmm[3][0] = rt::addss(xmm[3][0], xmm[6][0]);
        // 00b72943: movss xmm6, dword ptr [0x14f7270]
        xmm[6] = [f32::from_bits({ ld81 = pg.u32(m, 0x014f7270u32); ld81 }), 0.0, 0.0, 0.0];
        // 00b7294b: addss xmm1, xmm4
        xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
        // 00b7294f: movss xmm4, dword ptr [esi + 0x4a38]
        xmm[4] = [f32::from_bits(ld71), 0.0, 0.0, 0.0];
        // 00b72957: movss dword ptr [esp + 0x44], xmm0
        pg.set_u32(m, esp.wrapping_add(0x44), xmm[0][0].to_bits());
        // 00b7295d: movss xmm0, dword ptr [0x1629540]
        xmm[0] = [f32::from_bits({ ld82 = pg.u32(m, 0x01629540u32); ld82 }), 0.0, 0.0, 0.0];
        // 00b72965: addss xmm1, xmm7
        xmm[1][0] = rt::addss(xmm[1][0], xmm[7][0]);
        // 00b72969: addss xmm4, xmm3
        xmm[4][0] = rt::addss(xmm[4][0], xmm[3][0]);
        // 00b7296d: movss dword ptr [esp + 0x40], xmm1
        pg.set_u32(m, esp.wrapping_add(0x40), xmm[1][0].to_bits());
        // 00b72973: movss xmm1, dword ptr [0x1629544]
        xmm[1] = [f32::from_bits({ ld83 = pg.u32(m, 0x01629544u32); ld83 }), 0.0, 0.0, 0.0];
        // 00b7297b: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 00b7297f: mulss xmm1, xmm6
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
        // 00b72983: movaps xmm3, xmm0
        xmm[3] = xmm[0];
        // 00b72986: addss xmm0, dword ptr [esp + 0x40]
        xmm[0][0] = rt::addss(xmm[0][0], f32::from_bits({ ld84 = pg.u32(m, esp.wrapping_add(0x40)); ld84 }));
        // 00b7298c: mulss xmm2, xmm6
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[6][0]);
        // 00b72990: addss xmm3, xmm7
        xmm[3][0] = rt::addss(xmm[3][0], xmm[7][0]);
        // 00b72994: movaps xmm5, xmm1
        xmm[5] = xmm[1];
        // 00b72997: addss xmm5, dword ptr [esi + 0x4a34]
        xmm[5][0] = rt::addss(xmm[5][0], f32::from_bits({ ld85 = pg.u32(m, esi.wrapping_add(0x4a34)); ld85 }));
        // 00b7299f: addss xmm1, dword ptr [esp + 0x44]
        xmm[1][0] = rt::addss(xmm[1][0], f32::from_bits({ ld86 = pg.u32(m, esp.wrapping_add(0x44)); ld86 }));
        // 00b729a5: movaps xmm6, xmm2
        xmm[6] = xmm[2];
        // 00b729a8: addss xmm6, dword ptr [esi + 0x4a38]
        xmm[6][0] = rt::addss(xmm[6][0], f32::from_bits({ ld87 = pg.u32(m, esi.wrapping_add(0x4a38)); ld87 }));
        // 00b729b0: movss dword ptr [esp + 0x10], xmm3
        pg.set_u32(m, esp.wrapping_add(0x10), xmm[3][0].to_bits());
        // 00b729b6: xorps xmm3, xmm3
        { let s = xmm[3]; for k in 0..4 { xmm[3][k] = f32::from_bits(xmm[3][k].to_bits() ^ s[k].to_bits()); } }
        // 00b729b9: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 00b729bd: movss dword ptr [esp + 0x48], xmm4
        pg.set_u32(m, esp.wrapping_add(0x48), xmm[4][0].to_bits());
        // 00b729c3: movss dword ptr [esp + 0x14], xmm5
        pg.set_u32(m, esp.wrapping_add(0x14), xmm[5][0].to_bits());
        // 00b729c9: movss dword ptr [esp + 0x18], xmm6
        pg.set_u32(m, esp.wrapping_add(0x18), xmm[6][0].to_bits());
        // 00b729cf: movss dword ptr [esp + 0x1c], xmm3
        pg.set_u32(m, esp.wrapping_add(0x1c), xmm[3][0].to_bits());
        // 00b729d5: movss dword ptr [esp + 0x20], xmm0
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[0][0].to_bits());
        // 00b729db: movss dword ptr [esp + 0x24], xmm1
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[1][0].to_bits());
        // 00b729e1: movss dword ptr [esp + 0x28], xmm2
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[2][0].to_bits());
        // 00b729e7: movss dword ptr [esp + 0x2c], xmm3
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[3][0].to_bits());
        // 00b729ed: mov eax, 0x3f74e9c7
        eax = 0x3f74e9c7u32;
        // 00b729f2: je 0xb72a18
        if zf { break 'b00b72a18; }
        // 00b729f4: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b729f7: je 0xb72a00
        if zf { break 'b00b72a00; }
        // 00b729f9: mov eax, 0x3f54e9c7
        eax = 0x3f54e9c7u32;
        // 00b729fe: jmp 0xb72a33
        break 'b00b72a33;
    } // end of the jumps to block 00b72a00
        // 00b72a00: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72a03: je 0xb72a0c
        if zf { break 'b00b72a0c; }
        // 00b72a05: mov eax, 0x3f5469c7
        eax = 0x3f5469c7u32;
        // 00b72a0a: jmp 0xb72a33
        break 'b00b72a33;
    } // end of the jumps to block 00b72a0c
        // 00b72a0c: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72a0f: je 0xb72a33
        if zf { break 'b00b72a33; }
        // 00b72a11: mov eax, 0x3f5429c7
        eax = 0x3f5429c7u32;
        // 00b72a16: jmp 0xb72a33
        break 'b00b72a33;
    } // end of the jumps to block 00b72a18
        // 00b72a18: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72a1b: jne 0xb72a33
        if !zf { break 'b00b72a33; }
        // 00b72a1d: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72a20: je 0xb72a29
        if zf { break 'b00b72a29; }
        // 00b72a22: mov eax, 0x3f7469c7
        eax = 0x3f7469c7u32;
        // 00b72a27: jmp 0xb72a33
        break 'b00b72a33;
    } // end of the jumps to block 00b72a29
        // 00b72a29: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72a2c: je 0xb72a33
        if zf { break 'b00b72a33; }
        // 00b72a2e: mov eax, 0x3f7429c7
        eax = 0x3f7429c7u32;
    } // end of the jumps to block 00b72a33
        // 00b72a33: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a35: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a37: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a38: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a3a: lea ecx, [esp + 0x120]
        ecx = esp.wrapping_add(0x120);
        // 00b72a41: call 0xde4710
        let target: u32 = 0x00de4710u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72a46);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72a46: movss xmm0, dword ptr [esp + 0x10]
        xmm[0] = [f32::from_bits({ ld88 = pg.u32(m, esp.wrapping_add(0x10)); ld88 }), 0.0, 0.0, 0.0];
        // 00b72a4c: subss xmm0, dword ptr [esp + 0x20]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld89 = pg.u32(m, esp.wrapping_add(0x20)); ld89 }));
        // 00b72a52: movss xmm1, dword ptr [esp + 0x14]
        xmm[1] = [f32::from_bits({ ld90 = pg.u32(m, esp.wrapping_add(0x14)); ld90 }), 0.0, 0.0, 0.0];
        // 00b72a58: subss xmm1, dword ptr [esp + 0x24]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld91 = pg.u32(m, esp.wrapping_add(0x24)); ld91 }));
        // 00b72a5e: movss xmm2, dword ptr [esp + 0x18]
        xmm[2] = [f32::from_bits({ ld92 = pg.u32(m, esp.wrapping_add(0x18)); ld92 }), 0.0, 0.0, 0.0];
        // 00b72a64: subss xmm2, dword ptr [esp + 0x28]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld93 = pg.u32(m, esp.wrapping_add(0x28)); ld93 }));
        // 00b72a6a: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b72a6e: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b72a72: addss xmm0, xmm1
        xmm[0][0] = rt::addss(xmm[0][0], xmm[1][0]);
        // 00b72a76: movss xmm1, dword ptr [0x152cf28]
        xmm[1] = [f32::from_bits({ ld94 = pg.u32(m, 0x0152cf28u32); ld94 }), 0.0, 0.0, 0.0];
        // 00b72a7e: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b72a82: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 00b72a86: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72a89: ja 0xb72b6d
        if (!cf && !zf) { break 'b00b72b6d; }
        // 00b72a8f: lea eax, [esp + 0x110]
        eax = esp.wrapping_add(0x110);
        // 00b72a96: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a97: lea ecx, [esp + 0x74]
        ecx = esp.wrapping_add(0x74);
        // 00b72a9b: push ecx
        { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72a9c: mov ecx, dword ptr [0x18d0dd0]
        ecx = { ld95 = pg.u32(m, 0x018d0dd0u32); ld95 };
        // 00b72aa2: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72aa4: lea edx, [esp + 0x1c]
        edx = esp.wrapping_add(0x1c);
        // 00b72aa8: push edx
        { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72aa9: call 0xdea870
        let target: u32 = 0x00dea870u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72aae);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72aae: test eax, eax
        { let (a, b) = (eax, eax);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        }
        // 00b72ab0: je 0xb72b6d
        if zf { break 'b00b72b6d; }
        // 00b72ab6: movss xmm4, dword ptr [esp + 0x20]
        xmm[4] = [f32::from_bits({ ld96 = pg.u32(m, esp.wrapping_add(0x20)); ld96 }), 0.0, 0.0, 0.0];
        // 00b72abc: subss xmm4, dword ptr [esp + 0x10]
        xmm[4][0] = rt::subss(xmm[4][0], f32::from_bits({ ld97 = pg.u32(m, esp.wrapping_add(0x10)); ld97 }));
        // 00b72ac2: movss xmm1, dword ptr [esp + 0x24]
        xmm[1] = [f32::from_bits({ ld98 = pg.u32(m, esp.wrapping_add(0x24)); ld98 }), 0.0, 0.0, 0.0];
        // 00b72ac8: subss xmm1, dword ptr [esp + 0x14]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld99 = pg.u32(m, esp.wrapping_add(0x14)); ld99 }));
        // 00b72ace: movss xmm2, dword ptr [esp + 0x28]
        xmm[2] = [f32::from_bits({ ld100 = pg.u32(m, esp.wrapping_add(0x28)); ld100 }), 0.0, 0.0, 0.0];
        // 00b72ad4: subss xmm2, dword ptr [esp + 0x18]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld101 = pg.u32(m, esp.wrapping_add(0x18)); ld101 }));
        // 00b72ada: xorps xmm6, xmm6
        { let s = xmm[6]; for k in 0..4 { xmm[6][k] = f32::from_bits(xmm[6][k].to_bits() ^ s[k].to_bits()); } }
        // 00b72add: movaps xmm0, xmm4
        xmm[0] = xmm[4];
        // 00b72ae0: mulss xmm0, xmm4
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[4][0]);
        // 00b72ae4: movaps xmm3, xmm1
        xmm[3] = xmm[1];
        // 00b72ae7: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 00b72aeb: addss xmm0, xmm3
        xmm[0][0] = rt::addss(xmm[0][0], xmm[3][0]);
        // 00b72aef: movaps xmm3, xmm2
        xmm[3] = xmm[2];
        // 00b72af2: mulss xmm3, xmm2
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
        // 00b72af6: addss xmm0, xmm3
        xmm[0][0] = rt::addss(xmm[0][0], xmm[3][0]);
        // 00b72afa: comiss xmm0, xmm6
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[6][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72afd: jbe 0xb72b70
        if (cf || zf) { break 'b00b72b70; }
        // 00b72aff: movss xmm3, dword ptr [0x15319b8]
        xmm[3] = [f32::from_bits({ ld102 = pg.u32(m, 0x015319b8u32); ld102 }), 0.0, 0.0, 0.0];
        // 00b72b07: sqrtss xmm0, xmm0
        xmm[0][0] = xmm[0][0].sqrt();
        // 00b72b0b: divss xmm3, xmm0
        xmm[3][0] = rt::divss(xmm[3][0], xmm[0][0]);
        // 00b72b0f: mulss xmm1, xmm3
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
        // 00b72b13: movaps xmm0, xmm3
        xmm[0] = xmm[3];
        // 00b72b16: mulss xmm2, xmm3
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[3][0]);
        // 00b72b1a: movss xmm3, dword ptr [0x14f7270]
        xmm[3] = [f32::from_bits({ ld103 = pg.u32(m, 0x014f7270u32); ld103 }), 0.0, 0.0, 0.0];
        // 00b72b22: mulss xmm1, xmm3
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
        // 00b72b26: mulss xmm0, xmm4
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[4][0]);
        // 00b72b2a: mulss xmm0, xmm3
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[3][0]);
        // 00b72b2e: mulss xmm2, xmm3
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[3][0]);
        // 00b72b32: movss xmm3, dword ptr [esp + 0xe0]
        xmm[3] = [f32::from_bits({ ld104 = pg.u32(m, esp.wrapping_add(0xe0)); ld104 }), 0.0, 0.0, 0.0];
        // 00b72b3b: subss xmm3, xmm0
        xmm[3][0] = rt::subss(xmm[3][0], xmm[0][0]);
        // 00b72b3f: movss xmm0, dword ptr [esp + 0xe4]
        xmm[0] = [f32::from_bits({ ld105 = pg.u32(m, esp.wrapping_add(0xe4)); ld105 }), 0.0, 0.0, 0.0];
        // 00b72b48: subss xmm0, xmm1
        xmm[0][0] = rt::subss(xmm[0][0], xmm[1][0]);
        // 00b72b4c: movss xmm1, dword ptr [esp + 0xe8]
        xmm[1] = [f32::from_bits({ ld106 = pg.u32(m, esp.wrapping_add(0xe8)); ld106 }), 0.0, 0.0, 0.0];
        // 00b72b55: subss xmm1, xmm2
        xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
        // 00b72b59: movss dword ptr [esp + 0x40], xmm3
        pg.set_u32(m, esp.wrapping_add(0x40), xmm[3][0].to_bits());
        // 00b72b5f: movss dword ptr [esp + 0x44], xmm0
        pg.set_u32(m, esp.wrapping_add(0x44), xmm[0][0].to_bits());
        // 00b72b65: movss dword ptr [esp + 0x48], xmm1
        pg.set_u32(m, esp.wrapping_add(0x48), xmm[1][0].to_bits());
        // 00b72b6b: jmp 0xb72b70
        break 'b00b72b70;
    } // end of the jumps to block 00b72b6d
        // 00b72b6d: xorps xmm6, xmm6
        { let s = xmm[6]; for k in 0..4 { xmm[6][k] = f32::from_bits(xmm[6][k].to_bits() ^ s[k].to_bits()); } }
    } // end of the jumps to block 00b72b70
        // 00b72b70: cmp byte ptr [esi + 0x2f84], 0
        { let (a, b) = ({ ld107 = pg.u8(m, esi.wrapping_add(0x2f84)) as u32; ld107 }, 0x0u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 00b72b77: movss xmm5, dword ptr [0x1531ae8]
        xmm[5] = [f32::from_bits({ ld108 = pg.u32(m, 0x01531ae8u32); ld108 }), 0.0, 0.0, 0.0];
        // 00b72b7f: movss xmm0, dword ptr [0x1629540]
        xmm[0] = [f32::from_bits({ ld109 = pg.u32(m, 0x01629540u32); ld109 }), 0.0, 0.0, 0.0];
        // 00b72b87: movss xmm1, dword ptr [0x1629544]
        xmm[1] = [f32::from_bits({ ld110 = pg.u32(m, 0x01629544u32); ld110 }), 0.0, 0.0, 0.0];
        // 00b72b8f: movss xmm2, dword ptr [0x1629548]
        xmm[2] = [f32::from_bits({ ld111 = pg.u32(m, 0x01629548u32); ld111 }), 0.0, 0.0, 0.0];
        // 00b72b97: mov cl, byte ptr [esi + 0x206d]
        ecx = (ecx & !(0xffu32 << 0)) | ((({ ld112 = pg.u8(m, esi.wrapping_add(0x206d)) as u32; ld112 }) & 0xff) << 0);
        // 00b72b9d: mulss xmm0, xmm5
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[5][0]);
        // 00b72ba1: movaps xmm3, xmm0
        xmm[3] = xmm[0];
        // 00b72ba4: addss xmm3, dword ptr [esp + 0x40]
        xmm[3][0] = rt::addss(xmm[3][0], f32::from_bits({ ld113 = pg.u32(m, esp.wrapping_add(0x40)); ld113 }));
        // 00b72baa: mulss xmm1, xmm5
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
        // 00b72bae: movss dword ptr [esp + 0x10], xmm3
        pg.set_u32(m, esp.wrapping_add(0x10), xmm[3][0].to_bits());
        // 00b72bb4: movss xmm3, dword ptr [esp + 0x40]
        xmm[3] = [f32::from_bits({ ld114 = pg.u32(m, esp.wrapping_add(0x40)); ld114 }), 0.0, 0.0, 0.0];
        // 00b72bba: subss xmm3, xmm0
        xmm[3][0] = rt::subss(xmm[3][0], xmm[0][0]);
        // 00b72bbe: movss xmm0, dword ptr [esp + 0x44]
        xmm[0] = [f32::from_bits({ ld115 = pg.u32(m, esp.wrapping_add(0x44)); ld115 }), 0.0, 0.0, 0.0];
        // 00b72bc4: movaps xmm4, xmm1
        xmm[4] = xmm[1];
        // 00b72bc7: addss xmm4, dword ptr [esp + 0x44]
        xmm[4][0] = rt::addss(xmm[4][0], f32::from_bits(ld115));
        // 00b72bcd: mulss xmm2, xmm5
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[5][0]);
        // 00b72bd1: subss xmm0, xmm1
        xmm[0][0] = rt::subss(xmm[0][0], xmm[1][0]);
        // 00b72bd5: movss xmm1, dword ptr [esp + 0x48]
        xmm[1] = [f32::from_bits({ ld116 = pg.u32(m, esp.wrapping_add(0x48)); ld116 }), 0.0, 0.0, 0.0];
        // 00b72bdb: movaps xmm5, xmm2
        xmm[5] = xmm[2];
        // 00b72bde: addss xmm5, dword ptr [esp + 0x48]
        xmm[5][0] = rt::addss(xmm[5][0], f32::from_bits(ld116));
        // 00b72be4: subss xmm1, xmm2
        xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
        // 00b72be8: movss dword ptr [esp + 0x14], xmm4
        pg.set_u32(m, esp.wrapping_add(0x14), xmm[4][0].to_bits());
        // 00b72bee: movss dword ptr [esp + 0x18], xmm5
        pg.set_u32(m, esp.wrapping_add(0x18), xmm[5][0].to_bits());
        // 00b72bf4: movss dword ptr [esp + 0x1c], xmm6
        pg.set_u32(m, esp.wrapping_add(0x1c), xmm[6][0].to_bits());
        // 00b72bfa: movss dword ptr [esp + 0x20], xmm3
        pg.set_u32(m, esp.wrapping_add(0x20), xmm[3][0].to_bits());
        // 00b72c00: movss dword ptr [esp + 0x24], xmm0
        pg.set_u32(m, esp.wrapping_add(0x24), xmm[0][0].to_bits());
        // 00b72c06: movss dword ptr [esp + 0x28], xmm1
        pg.set_u32(m, esp.wrapping_add(0x28), xmm[1][0].to_bits());
        // 00b72c0c: movss dword ptr [esp + 0x2c], xmm6
        pg.set_u32(m, esp.wrapping_add(0x2c), xmm[6][0].to_bits());
        // 00b72c12: mov eax, 0x3f74e9c7
        eax = 0x3f74e9c7u32;
        // 00b72c17: je 0xb72c3d
        if zf { break 'b00b72c3d; }
        // 00b72c19: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c1c: je 0xb72c25
        if zf { break 'b00b72c25; }
        // 00b72c1e: mov eax, 0x3f54e9c7
        eax = 0x3f54e9c7u32;
        // 00b72c23: jmp 0xb72c58
        break 'b00b72c58;
    } // end of the jumps to block 00b72c25
        // 00b72c25: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c28: je 0xb72c31
        if zf { break 'b00b72c31; }
        // 00b72c2a: mov eax, 0x3f5469c7
        eax = 0x3f5469c7u32;
        // 00b72c2f: jmp 0xb72c58
        break 'b00b72c58;
    } // end of the jumps to block 00b72c31
        // 00b72c31: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c34: je 0xb72c58
        if zf { break 'b00b72c58; }
        // 00b72c36: mov eax, 0x3f5429c7
        eax = 0x3f5429c7u32;
        // 00b72c3b: jmp 0xb72c58
        break 'b00b72c58;
    } // end of the jumps to block 00b72c3d
        // 00b72c3d: test cl, 2
        { let (a, b) = (((ecx >> 0) & 0xff), 0x2u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c40: jne 0xb72c58
        if !zf { break 'b00b72c58; }
        // 00b72c42: test cl, 4
        { let (a, b) = (((ecx >> 0) & 0xff), 0x4u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c45: je 0xb72c4e
        if zf { break 'b00b72c4e; }
        // 00b72c47: mov eax, 0x3f7469c7
        eax = 0x3f7469c7u32;
        // 00b72c4c: jmp 0xb72c58
        break 'b00b72c58;
    } // end of the jumps to block 00b72c4e
        // 00b72c4e: test cl, 8
        { let (a, b) = (((ecx >> 0) & 0xff), 0x8u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72c51: je 0xb72c58
        if zf { break 'b00b72c58; }
        // 00b72c53: mov eax, 0x3f7429c7
        eax = 0x3f7429c7u32;
    } // end of the jumps to block 00b72c58
        // 00b72c58: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72c5a: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72c5c: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72c5d: push 1
        { let v = 0x1u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72c5f: lea ecx, [esp + 0x120]
        ecx = esp.wrapping_add(0x120);
        // 00b72c66: call 0xde4710
        let target: u32 = 0x00de4710u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72c6b);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72c6b: movss xmm0, dword ptr [esp + 0x10]
        xmm[0] = [f32::from_bits({ ld117 = pg.u32(m, esp.wrapping_add(0x10)); ld117 }), 0.0, 0.0, 0.0];
        // 00b72c71: subss xmm0, dword ptr [esp + 0x20]
        xmm[0][0] = rt::subss(xmm[0][0], f32::from_bits({ ld118 = pg.u32(m, esp.wrapping_add(0x20)); ld118 }));
        // 00b72c77: movss xmm1, dword ptr [esp + 0x14]
        xmm[1] = [f32::from_bits({ ld119 = pg.u32(m, esp.wrapping_add(0x14)); ld119 }), 0.0, 0.0, 0.0];
        // 00b72c7d: subss xmm1, dword ptr [esp + 0x24]
        xmm[1][0] = rt::subss(xmm[1][0], f32::from_bits({ ld120 = pg.u32(m, esp.wrapping_add(0x24)); ld120 }));
        // 00b72c83: movss xmm2, dword ptr [esp + 0x18]
        xmm[2] = [f32::from_bits({ ld121 = pg.u32(m, esp.wrapping_add(0x18)); ld121 }), 0.0, 0.0, 0.0];
        // 00b72c89: subss xmm2, dword ptr [esp + 0x28]
        xmm[2][0] = rt::subss(xmm[2][0], f32::from_bits({ ld122 = pg.u32(m, esp.wrapping_add(0x28)); ld122 }));
        // 00b72c8f: mulss xmm0, xmm0
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[0][0]);
        // 00b72c93: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 00b72c97: addss xmm0, xmm1
        xmm[0][0] = rt::addss(xmm[0][0], xmm[1][0]);
        // 00b72c9b: movss xmm1, dword ptr [0x152cf28]
        xmm[1] = [f32::from_bits({ ld123 = pg.u32(m, 0x0152cf28u32); ld123 }), 0.0, 0.0, 0.0];
        // 00b72ca3: mulss xmm2, xmm2
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[2][0]);
        // 00b72ca7: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 00b72cab: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 00b72cae: jbe 0xb72cb4
        if (cf || zf) { break 'b00b72cb4; }
        // 00b72cb0: xor eax, eax
        { let (a, b) = (eax, eax);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
        let r = (a ^ b) & 0xffffffffu32;
        eax = r;
        }
        // 00b72cb2: jmp 0xb72cd3
        break 'b00b72cd3;
    } // end of the jumps to block 00b72cb4
        // 00b72cb4: lea eax, [esp + 0x110]
        eax = esp.wrapping_add(0x110);
        // 00b72cbb: push eax
        { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72cbc: lea ecx, [esp + 0x74]
        ecx = esp.wrapping_add(0x74);
        // 00b72cc0: push ecx
        { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72cc1: mov ecx, dword ptr [0x18d0dd0]
        ecx = { ld124 = pg.u32(m, 0x018d0dd0u32); ld124 };
        // 00b72cc7: push 0
        { let v = 0x0u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72cc9: lea edx, [esp + 0x1c]
        edx = esp.wrapping_add(0x1c);
        // 00b72ccd: push edx
        { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 00b72cce: call 0xdea870
        let target: u32 = 0x00dea870u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72cd3);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
    } // end of the jumps to block 00b72cd3
        // 00b72cd3: test al, 1
        { let (a, b) = (((eax >> 0) & 0xff), 0x1u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 00b72cd5: je 0xb72d0c
        if zf { break 'b00b72d0c; }
        // 00b72cd7: movss xmm0, dword ptr [esp + 0x40]
        xmm[0] = [f32::from_bits({ ld125 = pg.u32(m, esp.wrapping_add(0x40)); ld125 }), 0.0, 0.0, 0.0];
        // 00b72cdd: movss dword ptr [esi + 0x4a30], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a30), xmm[0][0].to_bits());
        // 00b72ce5: movss xmm0, dword ptr [esp + 0x44]
        xmm[0] = [f32::from_bits({ ld126 = pg.u32(m, esp.wrapping_add(0x44)); ld126 }), 0.0, 0.0, 0.0];
        // 00b72ceb: movss dword ptr [esi + 0x4a34], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a34), xmm[0][0].to_bits());
        // 00b72cf3: movss xmm0, dword ptr [esp + 0x48]
        xmm[0] = [f32::from_bits({ ld127 = pg.u32(m, esp.wrapping_add(0x48)); ld127 }), 0.0, 0.0, 0.0];
        // 00b72cf9: movss dword ptr [esi + 0x4a38], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a38), xmm[0][0].to_bits());
        // 00b72d01: xorps xmm0, xmm0
        { let s = xmm[0]; for k in 0..4 { xmm[0][k] = f32::from_bits(xmm[0][k].to_bits() ^ s[k].to_bits()); } }
        // 00b72d04: movss dword ptr [esi + 0x4a3c], xmm0
        pg.set_u32(m, esi.wrapping_add(0x4a3c), xmm[0][0].to_bits());
    } // end of the jumps to block 00b72d0c
        // 00b72d0c: lea ecx, [esp + 0x110]
        ecx = esp.wrapping_add(0x110);
        // 00b72d13: call 0x4038f0
        let target: u32 = 0x004038f0u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72d18);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72d18: lea ecx, [esp + 0x70]
        ecx = esp.wrapping_add(0x70);
        // 00b72d1c: call 0xde5bd0
        let target: u32 = 0x00de5bd0u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00b72d21);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 00b72d21: pop edi
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); edi = v; }
        // 00b72d22: mov al, bl
        eax = (eax & !(0xffu32 << 0)) | (((((ebx >> 0) & 0xff)) & 0xff) << 0);
        // 00b72d24: pop ebx
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebx = v; }
        // 00b72d25: mov esp, ebp
        esp = ebp;
        // 00b72d27: pop ebp
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
        // 00b72d28: ret 
        esp = esp.wrapping_add(4 + 0);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        return;
    unreachable!();
}
