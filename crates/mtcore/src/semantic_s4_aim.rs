//! L184 (s4): mechanically translated semantic port (the aim block update 0x0078A730). Regenerate with
//! `research/native/semport.py 0x78a730 0x78b4ba aim_update out.rs doc.txt`; the generated
//! body is below, one commented Rust statement per original instruction.
//! Verified against the original's recorded behaviour
//! (crates/mtcore/tests/semantic_s4.rs).
use super::semantic_s4_rt as rt;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

// Translated from 0x0078a730 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport.py (operation order, widths and memory accesses
// of the original; registers, XMM registers and flags in locals).
/// 0x0078A730, `aim->update()` (ESI = the aim block at player + 0x4760,
/// void; the names are guesses): marks the block's three flag bytes (+0x71,
/// +0x101, +0x191), finds two joints of the player model (+0x1C4) from the
/// joint ids at +0x1E0 and in the index map, builds their orientation
/// frames, derives the yaw and pitch of the aim direction with the CRT
/// sin/cos/asin-style functions (0x012F0906, 0x012F0A55, 0x012F0D4C) and
/// atan2 (0x012F1109), clamps them to the limits stored in the block, and
/// sets the in-range flags at +0x20C, +0x230, +0x231 and +0x232.

#[allow(unused_assignments, unused_variables, unused_mut, unused_labels, unreachable_code, clippy::all)]
pub fn aim_update(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));
    let mut fr = [0f32; 74];
    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);
    let mut ld0 = 0u32; let mut ld1 = 0u32; let mut ld2 = 0u32; let mut ld3 = 0u32; let mut ld4 = 0u32; let mut ld5 = 0u32; let mut ld6 = 0u32; let mut ld7 = 0u32; let mut ld8 = 0u32; let mut ld9 = 0u32; let mut ld10 = 0u32; let mut ld11 = 0u32; let mut ld12 = 0u32; let mut ld13 = 0u32; let mut ld14 = 0u32; let mut ld15 = 0u32; let mut ld16 = 0u32; let mut ld17 = 0u32; let mut ld18 = 0u32; let mut ld19 = 0u32; let mut ld20 = 0u32; let mut ld21 = 0u32; let mut ld22 = 0u32; let mut ld23 = 0u32; let mut ld24 = 0u32; let mut ld25 = 0u32; let mut ld26 = 0u32; let mut ld27 = 0u32; let mut ld28 = 0u32; let mut ld29 = 0u32; let mut ld30 = 0u32; let mut ld31 = 0u32; let mut ld32 = 0u32; let mut ld33 = 0u32; let mut ld34 = 0u32; let mut ld35 = 0u32; let mut ld36 = 0u32; let mut ld37 = 0u32; let mut ld38 = 0u32; let mut ld39 = 0u32; let mut ld40 = 0u32; let mut ld41 = 0u32; let mut ld42 = 0u32; let mut ld43 = 0u32; let mut ld44 = 0u32; let mut ld45 = 0u32; let mut ld46 = 0u32; let mut ld47 = 0u32; let mut ld48 = 0u32; let mut ld49 = 0u32; let mut ld50 = 0u32; let mut ld51 = 0u32; let mut ld52 = 0u32; let mut ld53 = 0u32; let mut ld54 = 0u32; let mut ld55 = 0u32; let mut ld56 = 0u32; let mut ld57 = 0u32; let mut ld58 = 0u32; let mut ld59 = 0u32; let mut ld60 = 0u32; let mut ld61 = 0u32; let mut ld62 = 0u32; let mut ld63 = 0u32; let mut ld64 = 0u32; let mut ld65 = 0u32; let mut ld66 = 0u32; let mut ld67 = 0u32; let mut ld68 = 0u32; let mut ld69 = 0u32; let mut ld70 = 0u32; let mut ld71 = 0u32; let mut ld72 = 0u32; let mut ld73 = 0u32; let mut ld74 = 0u32; let mut ld75 = 0u32; let mut ld76 = 0u32; let mut ld77 = 0u32; let mut ld78 = 0u32; let mut ld79 = 0u32; let mut ld80 = 0u32; let mut ld81 = 0u32; let mut ld82 = 0u32; let mut ld83 = 0u32; let mut ld84 = 0u32; let mut ld85 = 0u32; let mut ld86 = 0u32; let mut ld87 = 0u32; let mut ld88 = 0u32; let mut ld89 = 0u32; let mut ld90 = 0u32; let mut ld91 = 0u32;
    'b0078b4b5: {
    'b0078b436: {
    'b0078b430: {
    'b0078b3f3: {
    'b0078b3ad: {
    'b0078b363: {
    'b0078b315: {
    'b0078b2f8: {
    'b0078b2c0: {
    'b0078b27d: {
    'b0078b237: {
    'b0078b22f: {
    'b0078b21c: {
    'b0078b20e: {
    'b0078b205: {
    'b0078b1f5: {
    'b0078b1f1: {
    'b0078b1e7: {
    'b0078b1c5: {
    'b0078b1c2: {
    'b0078b182: {
    'b0078b16f: {
    'b0078b0fd: {
    'b0078b0ee: {
    'b0078b0b8: {
    'b0078b0a0: {
    'b0078afbb: {
    'b0078afb3: {
    'b0078af93: {
    'b0078af8b: {
    'b0078aeb0: {
    'b0078aea2: {
    'b0078ae92: {
    'b0078ae8a: {
    'b0078ad36: {
    'b0078ac54: {
    'b0078ac51: {
        // 0078a730: push ebp
        { let v = ebp; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 0078a731: mov ebp, esp
        ebp = esp;
        // 0078a733: and esp, 0xfffffff0
        { let (a, b) = (esp, 0xfffffff0u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        let r = (a & b) & 0xffffffffu32;
        esp = r;
        }
        // 0078a736: mov eax, dword ptr [esi + 0x1e0]
        eax = { ld0 = pg.u32(m, esi.wrapping_add(0x1e0)); ld0 };
        // 0078a73c: sub esp, 0x128
        { let (a, b) = (esp, 0x128u32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
        let r = a.wrapping_sub(b) & 0xffffffffu32;
        esp = r;
        }
        // 0078a742: push ebx
        { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 0078a743: mov ebx, dword ptr [esi + 0x1c4]
        ebx = { ld1 = pg.u32(m, esi.wrapping_add(0x1c4)); ld1 };
        // 0078a749: mov byte ptr [esi + 0x71], 1
        pg.set_u8(m, esi.wrapping_add(0x71), (0x1u32) as u8);
        // 0078a74d: mov byte ptr [esi + 0x101], 1
        pg.set_u8(m, esi.wrapping_add(0x101), (0x1u32) as u8);
        // 0078a754: mov byte ptr [esi + 0x191], 1
        pg.set_u8(m, esi.wrapping_add(0x191), (0x1u32) as u8);
        // 0078a75b: mov ecx, dword ptr [ebx + 0x368]
        ecx = { ld2 = pg.u32(m, ebx.wrapping_add(0x368)); ld2 };
        // 0078a761: and eax, 0xff
        { let (a, b) = (eax, 0xffu32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
        let r = (a & b) & 0xffffffffu32;
        eax = r;
        }
        // 0078a766: movzx eax, byte ptr [eax + ecx]
        eax = { ld3 = pg.u8(m, eax.wrapping_add(ecx)) as u32; ld3 };
        // 0078a76a: push edi
        { let v = edi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
        // 0078a76b: cmp eax, 0xff
        { let (a, b) = (eax, 0xffu32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
        }
        // 0078a770: je 0x78b4b5
        if zf { break 'b0078b4b5; }
        // 0078a776: lea edi, [eax + eax*4]
        edi = eax.wrapping_add(eax.wrapping_mul(4));
        // 0078a779: shl edi, 5
        { let a = edi;
        let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
        (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
        edi = r;
        }
        // 0078a77c: add edi, dword ptr [ebx + 0x364]
        { let (a, b) = (edi, { ld4 = pg.u32(m, ebx.wrapping_add(0x364)); ld4 });
        (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
        let r = a.wrapping_add(b) & 0xffffffffu32;
        edi = r;
        }
        // 0078a782: je 0x78b4b5
        if zf { break 'b0078b4b5; }
        // 0078a788: movss xmm0, dword ptr [edi + 0x10]
        xmm[0] = [f32::from_bits({ ld5 = pg.u32(m, edi.wrapping_add(0x10)); ld5 }), 0.0, 0.0, 0.0];
        // 0078a78d: movss dword ptr [esp + 0x60], xmm0
        fr[22] = xmm[0][0];
        // 0078a793: movss xmm0, dword ptr [edi + 0x14]
        xmm[0] = [f32::from_bits({ ld6 = pg.u32(m, edi.wrapping_add(0x14)); ld6 }), 0.0, 0.0, 0.0];
        // 0078a798: movss dword ptr [esp + 0x64], xmm0
        fr[23] = xmm[0][0];
        // 0078a79e: movss xmm0, dword ptr [edi + 0x18]
        xmm[0] = [f32::from_bits({ ld7 = pg.u32(m, edi.wrapping_add(0x18)); ld7 }), 0.0, 0.0, 0.0];
        // 0078a7a3: movss dword ptr [esp + 0x68], xmm0
        fr[24] = xmm[0][0];
        // 0078a7a9: movss xmm0, dword ptr [edi + 0x1c]
        xmm[0] = [f32::from_bits({ ld8 = pg.u32(m, edi.wrapping_add(0x1c)); ld8 }), 0.0, 0.0, 0.0];
        // 0078a7ae: movss dword ptr [esp + 0x6c], xmm0
        fr[25] = xmm[0][0];
        // 0078a7b4: movss xmm0, dword ptr [edi + 0x20]
        xmm[0] = [f32::from_bits({ ld9 = pg.u32(m, edi.wrapping_add(0x20)); ld9 }), 0.0, 0.0, 0.0];
        // 0078a7b9: movss dword ptr [esp + 0x70], xmm0
        fr[26] = xmm[0][0];
        // 0078a7bf: movss xmm0, dword ptr [edi + 0x24]
        xmm[0] = [f32::from_bits({ ld10 = pg.u32(m, edi.wrapping_add(0x24)); ld10 }), 0.0, 0.0, 0.0];
        // 0078a7c4: movss dword ptr [esp + 0x74], xmm0
        fr[27] = xmm[0][0];
        // 0078a7ca: movss xmm0, dword ptr [edi + 0x28]
        xmm[0] = [f32::from_bits({ ld11 = pg.u32(m, edi.wrapping_add(0x28)); ld11 }), 0.0, 0.0, 0.0];
        // 0078a7cf: movss dword ptr [esp + 0x78], xmm0
        fr[28] = xmm[0][0];
        // 0078a7d5: movss xmm0, dword ptr [edi + 0x2c]
        xmm[0] = [f32::from_bits({ ld12 = pg.u32(m, edi.wrapping_add(0x2c)); ld12 }), 0.0, 0.0, 0.0];
        // 0078a7da: movss dword ptr [esp + 0x7c], xmm0
        fr[29] = xmm[0][0];
        // 0078a7e0: movss xmm0, dword ptr [edi + 0x30]
        xmm[0] = [f32::from_bits({ ld13 = pg.u32(m, edi.wrapping_add(0x30)); ld13 }), 0.0, 0.0, 0.0];
        // 0078a7e5: movss dword ptr [esp + 0x80], xmm0
        fr[30] = xmm[0][0];
        // 0078a7ee: movss xmm0, dword ptr [edi + 0x34]
        xmm[0] = [f32::from_bits({ ld14 = pg.u32(m, edi.wrapping_add(0x34)); ld14 }), 0.0, 0.0, 0.0];
        // 0078a7f3: movss dword ptr [esp + 0x84], xmm0
        fr[31] = xmm[0][0];
        // 0078a7fc: movss xmm0, dword ptr [edi + 0x38]
        xmm[0] = [f32::from_bits({ ld15 = pg.u32(m, edi.wrapping_add(0x38)); ld15 }), 0.0, 0.0, 0.0];
        // 0078a801: movss dword ptr [esp + 0x88], xmm0
        fr[32] = xmm[0][0];
        // 0078a80a: movss xmm0, dword ptr [edi + 0x3c]
        xmm[0] = [f32::from_bits({ ld16 = pg.u32(m, edi.wrapping_add(0x3c)); ld16 }), 0.0, 0.0, 0.0];
        // 0078a80f: movss dword ptr [esp + 0x8c], xmm0
        fr[33] = xmm[0][0];
        // 0078a818: movss xmm0, dword ptr [edi + 0x40]
        xmm[0] = [f32::from_bits({ ld17 = pg.u32(m, edi.wrapping_add(0x40)); ld17 }), 0.0, 0.0, 0.0];
        // 0078a81d: movss dword ptr [esp + 0x90], xmm0
        fr[34] = xmm[0][0];
        // 0078a826: movss xmm0, dword ptr [edi + 0x44]
        xmm[0] = [f32::from_bits({ ld18 = pg.u32(m, edi.wrapping_add(0x44)); ld18 }), 0.0, 0.0, 0.0];
        // 0078a82b: movss dword ptr [esp + 0x94], xmm0
        fr[35] = xmm[0][0];
        // 0078a834: movss xmm0, dword ptr [edi + 0x48]
        xmm[0] = [f32::from_bits({ ld19 = pg.u32(m, edi.wrapping_add(0x48)); ld19 }), 0.0, 0.0, 0.0];
        // 0078a839: movss dword ptr [esp + 0x98], xmm0
        fr[36] = xmm[0][0];
        // 0078a842: movss xmm0, dword ptr [edi + 0x4c]
        xmm[0] = [f32::from_bits({ ld20 = pg.u32(m, edi.wrapping_add(0x4c)); ld20 }), 0.0, 0.0, 0.0];
        // 0078a847: movss dword ptr [esp + 0x9c], xmm0
        fr[37] = xmm[0][0];
        // 0078a850: movss xmm0, dword ptr [esi + 0x210]
        xmm[0] = [f32::from_bits({ ld21 = pg.u32(m, esi.wrapping_add(0x210)); ld21 }), 0.0, 0.0, 0.0];
        // 0078a858: mulss xmm0, dword ptr [0x161c22c]
        xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld22 = pg.u32(m, 0x0161c22cu32); ld22 }));
        // 0078a860: movss dword ptr [esp + 0x14], xmm0
        fr[3] = xmm[0][0];
        // 0078a866: call 0x12f0906
        let target: u32 = 0x012f0906u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078a86b);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078a86b: movss dword ptr [esp + 0x1c], xmm0
        fr[5] = xmm[0][0];
        // 0078a871: movss xmm0, dword ptr [esp + 0x14]
        xmm[0] = [fr[3], 0.0, 0.0, 0.0];
        // 0078a877: call 0x12f0a55
        let target: u32 = 0x012f0a55u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078a87c);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078a87c: movss xmm7, dword ptr [esp + 0x1c]
        xmm[7] = [fr[5], 0.0, 0.0, 0.0];
        // 0078a882: movss xmm3, dword ptr [esp + 0x70]
        xmm[3] = [fr[26], 0.0, 0.0, 0.0];
        // 0078a888: movss xmm4, dword ptr [esp + 0x80]
        xmm[4] = [fr[30], 0.0, 0.0, 0.0];
        // 0078a891: movss xmm2, dword ptr [esp + 0x90]
        xmm[2] = [fr[34], 0.0, 0.0, 0.0];
        // 0078a89a: movss xmm5, dword ptr [esp + 0x74]
        xmm[5] = [fr[27], 0.0, 0.0, 0.0];
        // 0078a8a0: movss xmm6, dword ptr [esp + 0x78]
        xmm[6] = [fr[28], 0.0, 0.0, 0.0];
        // 0078a8a6: movaps xmm1, xmm7
        xmm[1] = xmm[7];
        // 0078a8a9: xorps xmm1, xmmword ptr [0x161c280]
        { let s = [f32::from_bits({ ld23 = pg.u32(m, 0x0161c280u32.wrapping_add(0)); ld23 }), f32::from_bits({ ld24 = pg.u32(m, 0x0161c280u32.wrapping_add(4)); ld24 }), f32::from_bits({ ld25 = pg.u32(m, 0x0161c280u32.wrapping_add(8)); ld25 }), f32::from_bits({ ld26 = pg.u32(m, 0x0161c280u32.wrapping_add(12)); ld26 })]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
        // 0078a8b0: movss dword ptr [esp + 0x114], xmm1
        fr[67] = xmm[1][0];
        // 0078a8b9: xorps xmm1, xmm1
        { let s = xmm[1]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
        // 0078a8bc: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078a8c0: movss dword ptr [esp + 0xcc], xmm3
        fr[49] = xmm[3][0];
        // 0078a8c9: addss xmm3, dword ptr [esp + 0x60]
        xmm[3][0] = rt::addss(xmm[3][0], fr[22]);
        // 0078a8cf: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078a8d3: addss xmm3, xmm4
        xmm[3][0] = rt::addss(xmm[3][0], xmm[4][0]);
        // 0078a8d7: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078a8db: addss xmm3, xmm2
        xmm[3][0] = rt::addss(xmm[3][0], xmm[2][0]);
        // 0078a8df: movss dword ptr [esp + 0x44], xmm3
        fr[15] = xmm[3][0];
        // 0078a8e5: movss dword ptr [esp + 0xd8], xmm4
        fr[52] = xmm[4][0];
        // 0078a8ee: movss xmm4, dword ptr [esp + 0x84]
        xmm[4] = [fr[31], 0.0, 0.0, 0.0];
        // 0078a8f7: movss dword ptr [esp + 0x20], xmm2
        fr[6] = xmm[2][0];
        // 0078a8fd: movss xmm2, dword ptr [esp + 0x94]
        xmm[2] = [fr[35], 0.0, 0.0, 0.0];
        // 0078a906: movaps xmm3, xmm5
        xmm[3] = xmm[5];
        // 0078a909: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078a90d: movss dword ptr [esp + 0xd0], xmm3
        fr[50] = xmm[3][0];
        // 0078a916: addss xmm3, dword ptr [esp + 0x64]
        xmm[3][0] = rt::addss(xmm[3][0], fr[23]);
        // 0078a91c: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078a920: addss xmm3, xmm4
        xmm[3][0] = rt::addss(xmm[3][0], xmm[4][0]);
        // 0078a924: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078a928: addss xmm3, xmm2
        xmm[3][0] = rt::addss(xmm[3][0], xmm[2][0]);
        // 0078a92c: movss dword ptr [esp + 0xd4], xmm4
        fr[51] = xmm[4][0];
        // 0078a935: movss xmm4, dword ptr [esp + 0x88]
        xmm[4] = [fr[32], 0.0, 0.0, 0.0];
        // 0078a93e: movss dword ptr [esp + 0x38], xmm2
        fr[12] = xmm[2][0];
        // 0078a944: movss xmm2, dword ptr [esp + 0x98]
        xmm[2] = [fr[36], 0.0, 0.0, 0.0];
        // 0078a94d: movss dword ptr [esp + 0x40], xmm3
        fr[14] = xmm[3][0];
        // 0078a953: movaps xmm3, xmm6
        xmm[3] = xmm[6];
        // 0078a956: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078a95a: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078a95e: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078a962: movss dword ptr [esp + 0xdc], xmm3
        fr[53] = xmm[3][0];
        // 0078a96b: movss dword ptr [esp + 0xc8], xmm4
        fr[48] = xmm[4][0];
        // 0078a974: movss dword ptr [esp + 0xac], xmm2
        fr[41] = xmm[2][0];
        // 0078a97d: addss xmm3, dword ptr [esp + 0x68]
        xmm[3][0] = rt::addss(xmm[3][0], fr[24]);
        // 0078a983: addss xmm3, xmm4
        xmm[3][0] = rt::addss(xmm[3][0], xmm[4][0]);
        // 0078a987: addss xmm3, xmm2
        xmm[3][0] = rt::addss(xmm[3][0], xmm[2][0]);
        // 0078a98b: movss xmm2, dword ptr [esp + 0x8c]
        xmm[2] = [fr[33], 0.0, 0.0, 0.0];
        // 0078a994: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078a998: movss xmm4, dword ptr [esp + 0x9c]
        xmm[4] = [fr[37], 0.0, 0.0, 0.0];
        // 0078a9a1: movss dword ptr [esp + 0x24], xmm2
        fr[7] = xmm[2][0];
        // 0078a9a7: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078a9ab: movss dword ptr [esp + 0x2c], xmm4
        fr[9] = xmm[4][0];
        // 0078a9b1: movss dword ptr [esp + 0x30], xmm3
        fr[10] = xmm[3][0];
        // 0078a9b7: movss xmm3, dword ptr [esp + 0x7c]
        xmm[3] = [fr[29], 0.0, 0.0, 0.0];
        // 0078a9bd: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078a9c1: movss dword ptr [esp + 0x3c], xmm3
        fr[13] = xmm[3][0];
        // 0078a9c7: addss xmm3, dword ptr [esp + 0x6c]
        xmm[3][0] = rt::addss(xmm[3][0], fr[25]);
        // 0078a9cd: addss xmm3, xmm2
        xmm[3][0] = rt::addss(xmm[3][0], xmm[2][0]);
        // 0078a9d1: movss xmm2, dword ptr [esp + 0x60]
        xmm[2] = [fr[22], 0.0, 0.0, 0.0];
        // 0078a9d7: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078a9db: addss xmm3, xmm4
        xmm[3][0] = rt::addss(xmm[3][0], xmm[4][0]);
        // 0078a9df: movaps xmm4, xmm2
        xmm[4] = xmm[2];
        // 0078a9e2: movss xmm2, dword ptr [esp + 0x70]
        xmm[2] = [fr[26], 0.0, 0.0, 0.0];
        // 0078a9e8: mulss xmm2, xmm0
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[0][0]);
        // 0078a9ec: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078a9f0: movss dword ptr [esp + 0x1c], xmm4
        fr[5] = xmm[4][0];
        // 0078a9f6: movss xmm4, dword ptr [esp + 0x80]
        xmm[4] = [fr[30], 0.0, 0.0, 0.0];
        // 0078a9ff: mulss xmm4, xmm7
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[7][0]);
        // 0078aa03: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078aa07: addss xmm2, dword ptr [esp + 0x20]
        xmm[2][0] = rt::addss(xmm[2][0], fr[6]);
        // 0078aa0d: movss dword ptr [esp + 0x28], xmm2
        fr[8] = xmm[2][0];
        // 0078aa13: movss xmm2, dword ptr [esp + 0x64]
        xmm[2] = [fr[23], 0.0, 0.0, 0.0];
        // 0078aa19: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078aa1d: movaps xmm4, xmm2
        xmm[4] = xmm[2];
        // 0078aa20: movss dword ptr [esp + 0x14], xmm4
        fr[3] = xmm[4][0];
        // 0078aa26: movaps xmm2, xmm5
        xmm[2] = xmm[5];
        // 0078aa29: mulss xmm2, xmm0
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[0][0]);
        // 0078aa2d: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078aa31: movss xmm4, dword ptr [esp + 0x84]
        xmm[4] = [fr[31], 0.0, 0.0, 0.0];
        // 0078aa3a: mulss xmm4, xmm7
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[7][0]);
        // 0078aa3e: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078aa42: addss xmm2, dword ptr [esp + 0x38]
        xmm[2][0] = rt::addss(xmm[2][0], fr[12]);
        // 0078aa48: movss dword ptr [esp + 0x34], xmm2
        fr[11] = xmm[2][0];
        // 0078aa4e: movss xmm2, dword ptr [esp + 0x68]
        xmm[2] = [fr[24], 0.0, 0.0, 0.0];
        // 0078aa54: mulss xmm5, dword ptr [esp + 0x114]
        xmm[5][0] = rt::mulss(xmm[5][0], fr[67]);
        // 0078aa5d: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078aa61: movaps xmm4, xmm2
        xmm[4] = xmm[2];
        // 0078aa64: addss xmm5, dword ptr [esp + 0x14]
        xmm[5][0] = rt::addss(xmm[5][0], fr[3]);
        // 0078aa6a: movss dword ptr [esp + 0x4c], xmm4
        fr[17] = xmm[4][0];
        // 0078aa70: movaps xmm2, xmm6
        xmm[2] = xmm[6];
        // 0078aa73: mulss xmm2, xmm0
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[0][0]);
        // 0078aa77: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078aa7b: movss xmm4, dword ptr [esp + 0x88]
        xmm[4] = [fr[32], 0.0, 0.0, 0.0];
        // 0078aa84: mulss xmm6, dword ptr [esp + 0x114]
        xmm[6][0] = rt::mulss(xmm[6][0], fr[67]);
        // 0078aa8d: mulss xmm4, xmm7
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[7][0]);
        // 0078aa91: addss xmm2, xmm4
        xmm[2][0] = rt::addss(xmm[2][0], xmm[4][0]);
        // 0078aa95: addss xmm2, dword ptr [esp + 0xac]
        xmm[2][0] = rt::addss(xmm[2][0], fr[41]);
        // 0078aa9e: movss xmm4, dword ptr [esp + 0x7c]
        xmm[4] = [fr[29], 0.0, 0.0, 0.0];
        // 0078aaa4: movss dword ptr [esp + 0x18], xmm2
        fr[4] = xmm[2][0];
        // 0078aaaa: movss xmm2, dword ptr [esp + 0x6c]
        xmm[2] = [fr[25], 0.0, 0.0, 0.0];
        // 0078aab0: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078aab4: movss dword ptr [esp + 0x48], xmm2
        fr[16] = xmm[2][0];
        // 0078aaba: mulss xmm4, xmm0
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[0][0]);
        // 0078aabe: addss xmm4, xmm2
        xmm[4][0] = rt::addss(xmm[4][0], xmm[2][0]);
        // 0078aac2: movss xmm2, dword ptr [esp + 0x8c]
        xmm[2] = [fr[33], 0.0, 0.0, 0.0];
        // 0078aacb: mulss xmm2, xmm7
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[7][0]);
        // 0078aacf: movss xmm7, dword ptr [esp + 0x80]
        xmm[7] = [fr[30], 0.0, 0.0, 0.0];
        // 0078aad8: addss xmm4, xmm2
        xmm[4][0] = rt::addss(xmm[4][0], xmm[2][0]);
        // 0078aadc: movss xmm2, dword ptr [esp + 0x114]
        xmm[2] = [fr[67], 0.0, 0.0, 0.0];
        // 0078aae5: mulss xmm2, dword ptr [esp + 0x70]
        xmm[2][0] = rt::mulss(xmm[2][0], fr[26]);
        // 0078aaeb: addss xmm2, dword ptr [esp + 0x1c]
        xmm[2][0] = rt::addss(xmm[2][0], fr[5]);
        // 0078aaf1: addss xmm4, dword ptr [esp + 0x2c]
        xmm[4][0] = rt::addss(xmm[4][0], fr[9]);
        // 0078aaf7: mulss xmm7, xmm0
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
        // 0078aafb: addss xmm2, xmm7
        xmm[2][0] = rt::addss(xmm[2][0], xmm[7][0]);
        // 0078aaff: movss xmm7, dword ptr [esp + 0x84]
        xmm[7] = [fr[31], 0.0, 0.0, 0.0];
        // 0078ab08: addss xmm2, dword ptr [esp + 0x20]
        xmm[2][0] = rt::addss(xmm[2][0], fr[6]);
        // 0078ab0e: mulss xmm7, xmm0
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[0][0]);
        // 0078ab12: addss xmm5, xmm7
        xmm[5][0] = rt::addss(xmm[5][0], xmm[7][0]);
        // 0078ab16: addss xmm5, dword ptr [esp + 0x38]
        xmm[5][0] = rt::addss(xmm[5][0], fr[12]);
        // 0078ab1c: movss xmm7, dword ptr [esp + 0x4c]
        xmm[7] = [fr[17], 0.0, 0.0, 0.0];
        // 0078ab22: movss dword ptr [esp + 0x38], xmm5
        fr[12] = xmm[5][0];
        // 0078ab28: movss xmm5, dword ptr [esp + 0x88]
        xmm[5] = [fr[32], 0.0, 0.0, 0.0];
        // 0078ab31: addss xmm6, xmm7
        xmm[6][0] = rt::addss(xmm[6][0], xmm[7][0]);
        // 0078ab35: mulss xmm5, xmm0
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[0][0]);
        // 0078ab39: addss xmm6, xmm5
        xmm[6][0] = rt::addss(xmm[6][0], xmm[5][0]);
        // 0078ab3d: addss xmm6, dword ptr [esp + 0xac]
        xmm[6][0] = rt::addss(xmm[6][0], fr[41]);
        // 0078ab46: movss xmm5, dword ptr [esp + 0x48]
        xmm[5] = [fr[16], 0.0, 0.0, 0.0];
        // 0078ab4c: movss dword ptr [esp + 0x4c], xmm6
        fr[17] = xmm[6][0];
        // 0078ab52: movss xmm6, dword ptr [esp + 0x7c]
        xmm[6] = [fr[29], 0.0, 0.0, 0.0];
        // 0078ab58: mulss xmm6, dword ptr [esp + 0x114]
        xmm[6][0] = rt::mulss(xmm[6][0], fr[67]);
        // 0078ab61: addss xmm7, dword ptr [esp + 0xdc]
        xmm[7][0] = rt::addss(xmm[7][0], fr[53]);
        // 0078ab6a: addss xmm6, xmm5
        xmm[6][0] = rt::addss(xmm[6][0], xmm[5][0]);
        // 0078ab6e: addss xmm5, dword ptr [esp + 0x3c]
        xmm[5][0] = rt::addss(xmm[5][0], fr[13]);
        // 0078ab74: addss xmm5, dword ptr [esp + 0x24]
        xmm[5][0] = rt::addss(xmm[5][0], fr[7]);
        // 0078ab7a: addss xmm5, dword ptr [esp + 0x9c]
        xmm[5][0] = rt::addss(xmm[5][0], fr[37]);
        // 0078ab83: addss xmm7, dword ptr [esp + 0xc8]
        xmm[7][0] = rt::addss(xmm[7][0], fr[48]);
        // 0078ab8c: addss xmm7, dword ptr [esp + 0x98]
        xmm[7][0] = rt::addss(xmm[7][0], fr[36]);
        // 0078ab95: movss dword ptr [esp + 0x48], xmm6
        fr[16] = xmm[6][0];
        // 0078ab9b: movss xmm6, dword ptr [esp + 0x8c]
        xmm[6] = [fr[33], 0.0, 0.0, 0.0];
        // 0078aba4: mulss xmm6, xmm0
        xmm[6][0] = rt::mulss(xmm[6][0], xmm[0][0]);
        // 0078aba8: movss xmm0, dword ptr [esp + 0x1c]
        xmm[0] = [fr[5], 0.0, 0.0, 0.0];
        // 0078abae: addss xmm0, dword ptr [esp + 0xcc]
        xmm[0][0] = rt::addss(xmm[0][0], fr[49]);
        // 0078abb7: addss xmm0, dword ptr [esp + 0xd8]
        xmm[0][0] = rt::addss(xmm[0][0], fr[52]);
        // 0078abc0: addss xmm0, dword ptr [esp + 0x90]
        xmm[0][0] = rt::addss(xmm[0][0], fr[34]);
        // 0078abc9: addss xmm6, dword ptr [esp + 0x48]
        xmm[6][0] = rt::addss(xmm[6][0], fr[16]);
        // 0078abcf: addss xmm6, dword ptr [esp + 0x2c]
        xmm[6][0] = rt::addss(xmm[6][0], fr[9]);
        // 0078abd5: movss dword ptr [esp + 0x1c], xmm0
        fr[5] = xmm[0][0];
        // 0078abdb: movss xmm0, dword ptr [esp + 0x14]
        xmm[0] = [fr[3], 0.0, 0.0, 0.0];
        // 0078abe1: addss xmm0, dword ptr [esp + 0xd0]
        xmm[0][0] = rt::addss(xmm[0][0], fr[50]);
        // 0078abea: addss xmm0, dword ptr [esp + 0xd4]
        xmm[0][0] = rt::addss(xmm[0][0], fr[51]);
        // 0078abf3: addss xmm0, dword ptr [esp + 0x94]
        xmm[0][0] = rt::addss(xmm[0][0], fr[35]);
        // 0078abfc: movss dword ptr [esp + 0x14], xmm0
        fr[3] = xmm[0][0];
        // 0078ac02: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 0078ac05: movss xmm5, dword ptr [0x14f7270]
        xmm[5] = [f32::from_bits({ ld27 = pg.u32(m, 0x014f7270u32); ld27 }), 0.0, 0.0, 0.0];
        // 0078ac0d: mulss xmm4, xmm5
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[5][0]);
        // 0078ac11: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078ac15: addss xmm3, xmm4
        xmm[3][0] = rt::addss(xmm[3][0], xmm[4][0]);
        // 0078ac19: movaps xmm4, xmm6
        xmm[4] = xmm[6];
        // 0078ac1c: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078ac20: addss xmm4, xmm3
        xmm[4][0] = rt::addss(xmm[4][0], xmm[3][0]);
        // 0078ac24: addss xmm4, xmm0
        xmm[4][0] = rt::addss(xmm[4][0], xmm[0][0]);
        // 0078ac28: ucomiss xmm4, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[4][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078ac2b: lahf 
        eax = (eax & 0xffff_00ff) | ((((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32)) << 8);
        // 0078ac2c: movss dword ptr [esp + 0x2c], xmm7
        fr[9] = xmm[7][0];
        // 0078ac32: movss dword ptr [esp + 0x3c], xmm0
        fr[13] = xmm[0][0];
        // 0078ac38: movss dword ptr [esp + 0x24], xmm3
        fr[7] = xmm[3][0];
        // 0078ac3e: test ah, 0x44
        { let (a, b) = (((eax >> 8) & 0xff), 0x44u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 0078ac41: jnp 0x78ac51
        if !pf { break 'b0078ac51; }
        // 0078ac43: movss xmm3, dword ptr [0x15319b8]
        xmm[3] = [f32::from_bits({ ld28 = pg.u32(m, 0x015319b8u32); ld28 }), 0.0, 0.0, 0.0];
        // 0078ac4b: divss xmm3, xmm4
        xmm[3][0] = rt::divss(xmm[3][0], xmm[4][0]);
        // 0078ac4f: jmp 0x78ac54
        break 'b0078ac54;
    } // end of the jumps to block 0078ac51
        // 0078ac51: movaps xmm3, xmm1
        xmm[3] = xmm[1];
    } // end of the jumps to block 0078ac54
        // 0078ac54: movss xmm4, dword ptr [esp + 0x28]
        xmm[4] = [fr[8], 0.0, 0.0, 0.0];
        // 0078ac5a: movss xmm0, dword ptr [esp + 0x44]
        xmm[0] = [fr[15], 0.0, 0.0, 0.0];
        // 0078ac60: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078ac64: movss dword ptr [esp + 0x20], xmm3
        fr[6] = xmm[3][0];
        // 0078ac6a: mulss xmm4, xmm5
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[5][0]);
        // 0078ac6e: addss xmm0, xmm4
        xmm[0][0] = rt::addss(xmm[0][0], xmm[4][0]);
        // 0078ac72: movss xmm4, dword ptr [esp + 0x38]
        xmm[4] = [fr[12], 0.0, 0.0, 0.0];
        // 0078ac78: movss dword ptr [esp + 0x28], xmm0
        fr[8] = xmm[0][0];
        // 0078ac7e: movaps xmm7, xmm2
        xmm[7] = xmm[2];
        // 0078ac81: mulss xmm7, xmm1
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[1][0]);
        // 0078ac85: addss xmm7, xmm0
        xmm[7][0] = rt::addss(xmm[7][0], xmm[0][0]);
        // 0078ac89: addss xmm7, dword ptr [esp + 0x1c]
        xmm[7][0] = rt::addss(xmm[7][0], fr[5]);
        // 0078ac8f: movss xmm0, dword ptr [esp + 0x40]
        xmm[0] = [fr[14], 0.0, 0.0, 0.0];
        // 0078ac95: mulss xmm7, xmm3
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[3][0]);
        // 0078ac99: movss xmm3, dword ptr [esp + 0x34]
        xmm[3] = [fr[11], 0.0, 0.0, 0.0];
        // 0078ac9f: mulss xmm3, xmm5
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[5][0]);
        // 0078aca3: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078aca7: addss xmm0, xmm3
        xmm[0][0] = rt::addss(xmm[0][0], xmm[3][0]);
        // 0078acab: movss dword ptr [esp + 0x34], xmm0
        fr[11] = xmm[0][0];
        // 0078acb1: movaps xmm3, xmm4
        xmm[3] = xmm[4];
        // 0078acb4: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078acb8: addss xmm3, xmm0
        xmm[3][0] = rt::addss(xmm[3][0], xmm[0][0]);
        // 0078acbc: addss xmm3, dword ptr [esp + 0x14]
        xmm[3][0] = rt::addss(xmm[3][0], fr[3]);
        // 0078acc2: mulss xmm3, dword ptr [esp + 0x20]
        xmm[3][0] = rt::mulss(xmm[3][0], fr[6]);
        // 0078acc8: movss xmm0, dword ptr [esp + 0x30]
        xmm[0] = [fr[10], 0.0, 0.0, 0.0];
        // 0078acce: movss dword ptr [esp + 0x54], xmm3
        fr[19] = xmm[3][0];
        // 0078acd4: movss xmm3, dword ptr [esp + 0x18]
        xmm[3] = [fr[4], 0.0, 0.0, 0.0];
        // 0078acda: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078acde: mulss xmm3, xmm5
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[5][0]);
        // 0078ace2: movss xmm5, dword ptr [esp + 0x4c]
        xmm[5] = [fr[17], 0.0, 0.0, 0.0];
        // 0078ace8: addss xmm3, xmm0
        xmm[3][0] = rt::addss(xmm[3][0], xmm[0][0]);
        // 0078acec: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 0078acef: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078acf3: addss xmm0, xmm3
        xmm[0][0] = rt::addss(xmm[0][0], xmm[3][0]);
        // 0078acf7: addss xmm0, dword ptr [esp + 0x2c]
        xmm[0][0] = rt::addss(xmm[0][0], fr[9]);
        // 0078acfd: mulss xmm0, dword ptr [esp + 0x20]
        xmm[0][0] = rt::mulss(xmm[0][0], fr[6]);
        // 0078ad03: movss dword ptr [esp + 0x18], xmm3
        fr[4] = xmm[3][0];
        // 0078ad09: movss xmm3, dword ptr [0x1516e7c]
        xmm[3] = [f32::from_bits({ ld29 = pg.u32(m, 0x01516e7cu32); ld29 }), 0.0, 0.0, 0.0];
        // 0078ad11: mulss xmm6, xmm3
        xmm[6][0] = rt::mulss(xmm[6][0], xmm[3][0]);
        // 0078ad15: addss xmm6, dword ptr [esp + 0x24]
        xmm[6][0] = rt::addss(xmm[6][0], fr[7]);
        // 0078ad1b: addss xmm6, dword ptr [esp + 0x3c]
        xmm[6][0] = rt::addss(xmm[6][0], fr[13]);
        // 0078ad21: ucomiss xmm6, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[6][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078ad24: lahf 
        eax = (eax & 0xffff_00ff) | ((((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32)) << 8);
        // 0078ad25: test ah, 0x44
        { let (a, b) = (((eax >> 8) & 0xff), 0x44u32);
        (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
        }
        // 0078ad28: jnp 0x78ad36
        if !pf { break 'b0078ad36; }
        // 0078ad2a: movss xmm1, dword ptr [0x15319b8]
        xmm[1] = [f32::from_bits({ ld30 = pg.u32(m, 0x015319b8u32); ld30 }), 0.0, 0.0, 0.0];
        // 0078ad32: divss xmm1, xmm6
        xmm[1][0] = rt::divss(xmm[1][0], xmm[6][0]);
    } // end of the jumps to block 0078ad36
        // 0078ad36: movss xmm6, dword ptr [esi + 0x1d4]
        xmm[6] = [f32::from_bits({ ld31 = pg.u32(m, esi.wrapping_add(0x1d4)); ld31 }), 0.0, 0.0, 0.0];
        // 0078ad3e: subss xmm6, dword ptr [esp + 0x54]
        xmm[6][0] = rt::subss(xmm[6][0], fr[19]);
        // 0078ad44: mulss xmm2, xmm3
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[3][0]);
        // 0078ad48: addss xmm2, dword ptr [esp + 0x28]
        xmm[2][0] = rt::addss(xmm[2][0], fr[8]);
        // 0078ad4e: addss xmm2, dword ptr [esp + 0x1c]
        xmm[2][0] = rt::addss(xmm[2][0], fr[5]);
        // 0078ad54: mulss xmm5, xmm3
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[3][0]);
        // 0078ad58: addss xmm5, dword ptr [esp + 0x18]
        xmm[5][0] = rt::addss(xmm[5][0], fr[4]);
        // 0078ad5e: addss xmm5, dword ptr [esp + 0x2c]
        xmm[5][0] = rt::addss(xmm[5][0], fr[9]);
        // 0078ad64: mulss xmm4, xmm3
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[3][0]);
        // 0078ad68: addss xmm4, dword ptr [esp + 0x34]
        xmm[4][0] = rt::addss(xmm[4][0], fr[11]);
        // 0078ad6e: addss xmm4, dword ptr [esp + 0x14]
        xmm[4][0] = rt::addss(xmm[4][0], fr[3]);
        // 0078ad74: movss xmm3, dword ptr [esi + 0x1d0]
        xmm[3] = [f32::from_bits({ ld32 = pg.u32(m, esi.wrapping_add(0x1d0)); ld32 }), 0.0, 0.0, 0.0];
        // 0078ad7c: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078ad80: mulss xmm5, xmm1
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[1][0]);
        // 0078ad84: subss xmm5, xmm0
        xmm[5][0] = rt::subss(xmm[5][0], xmm[0][0]);
        // 0078ad88: subss xmm2, xmm7
        xmm[2][0] = rt::subss(xmm[2][0], xmm[7][0]);
        // 0078ad8c: mulss xmm4, xmm1
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[1][0]);
        // 0078ad90: subss xmm4, dword ptr [esp + 0x54]
        xmm[4][0] = rt::subss(xmm[4][0], fr[19]);
        // 0078ad96: movss xmm1, dword ptr [esi + 0x1d8]
        xmm[1] = [f32::from_bits({ ld33 = pg.u32(m, esi.wrapping_add(0x1d8)); ld33 }), 0.0, 0.0, 0.0];
        // 0078ad9e: movss dword ptr [esp + 0xe0], xmm2
        fr[54] = xmm[2][0];
        // 0078ada7: movaps xmm2, xmm5
        xmm[2] = xmm[5];
        // 0078adaa: movss dword ptr [esp + 0xb8], xmm1
        fr[44] = xmm[1][0];
        // 0078adb3: movaps xmm1, xmm4
        xmm[1] = xmm[4];
        // 0078adb6: movss xmm4, dword ptr [esp + 0xb8]
        xmm[4] = [fr[44], 0.0, 0.0, 0.0];
        // 0078adbf: subss xmm4, xmm0
        xmm[4][0] = rt::subss(xmm[4][0], xmm[0][0]);
        // 0078adc3: subss xmm3, xmm7
        xmm[3][0] = rt::subss(xmm[3][0], xmm[7][0]);
        // 0078adc7: movaps xmm7, xmm3
        xmm[7] = xmm[3];
        // 0078adca: movaps xmm3, xmm6
        xmm[3] = xmm[6];
        // 0078adcd: movss xmm6, dword ptr [esp + 0xe0]
        xmm[6] = [fr[54], 0.0, 0.0, 0.0];
        // 0078add6: movaps xmm5, xmm1
        xmm[5] = xmm[1];
        // 0078add9: mulss xmm5, xmm1
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[1][0]);
        // 0078addd: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078ade0: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 0078ade4: addss xmm0, xmm5
        xmm[0][0] = rt::addss(xmm[0][0], xmm[5][0]);
        // 0078ade8: movaps xmm5, xmm2
        xmm[5] = xmm[2];
        // 0078adeb: mulss xmm5, xmm2
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[2][0]);
        // 0078adef: addss xmm0, xmm5
        xmm[0][0] = rt::addss(xmm[0][0], xmm[5][0]);
        // 0078adf3: movss xmm5, dword ptr [0x15319b8]
        xmm[5] = [f32::from_bits({ ld34 = pg.u32(m, 0x015319b8u32); ld34 }), 0.0, 0.0, 0.0];
        // 0078adfb: sqrtss xmm0, xmm0
        xmm[0][0] = xmm[0][0].sqrt();
        // 0078adff: divss xmm5, xmm0
        xmm[5][0] = rt::divss(xmm[5][0], xmm[0][0]);
        // 0078ae03: mulss xmm2, xmm5
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[5][0]);
        // 0078ae07: movss dword ptr [esp + 0x58], xmm2
        fr[20] = xmm[2][0];
        // 0078ae0d: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 0078ae10: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 0078ae14: movss dword ptr [esp + 0x50], xmm0
        fr[18] = xmm[0][0];
        // 0078ae1a: movaps xmm2, xmm3
        xmm[2] = xmm[3];
        // 0078ae1d: mulss xmm2, xmm3
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[3][0]);
        // 0078ae21: movaps xmm0, xmm7
        xmm[0] = xmm[7];
        // 0078ae24: mulss xmm0, xmm7
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[7][0]);
        // 0078ae28: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 0078ae2c: mulss xmm1, xmm5
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[5][0]);
        // 0078ae30: movaps xmm2, xmm4
        xmm[2] = xmm[4];
        // 0078ae33: mulss xmm2, xmm4
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[4][0]);
        // 0078ae37: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 0078ae3b: sqrtss xmm5, xmm0
        xmm[5][0] = xmm[0][0].sqrt();
        // 0078ae3f: movss xmm0, dword ptr [0x15319b8]
        xmm[0] = [f32::from_bits(ld34), 0.0, 0.0, 0.0];
        // 0078ae47: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078ae4a: movaps xmm2, xmm0
        xmm[2] = xmm[0];
        // 0078ae4d: divss xmm2, xmm5
        xmm[2][0] = rt::divss(xmm[2][0], xmm[5][0]);
        // 0078ae51: movaps xmm5, xmm2
        xmm[5] = xmm[2];
        // 0078ae54: mulss xmm3, xmm2
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
        // 0078ae58: mulss xmm4, xmm2
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[2][0]);
        // 0078ae5c: movss xmm2, dword ptr [0x161c20c]
        xmm[2] = [f32::from_bits({ ld35 = pg.u32(m, 0x0161c20cu32); ld35 }), 0.0, 0.0, 0.0];
        // 0078ae64: mulss xmm5, xmm7
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[7][0]);
        // 0078ae68: movss dword ptr [esp + 0xb0], xmm5
        fr[42] = xmm[5][0];
        // 0078ae71: movss dword ptr [esp + 0xb4], xmm3
        fr[43] = xmm[3][0];
        // 0078ae7a: movss dword ptr [esp + 0xb8], xmm4
        fr[44] = xmm[4][0];
        // 0078ae83: jb 0x78ae8a
        if cf { break 'b0078ae8a; }
        // 0078ae85: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078ae88: jmp 0x78ae92
        break 'b0078ae92;
    } // end of the jumps to block 0078ae8a
        // 0078ae8a: comiss xmm2, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[2][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078ae8d: jb 0x78ae92
        if cf { break 'b0078ae92; }
        // 0078ae8f: movaps xmm1, xmm2
        xmm[1] = xmm[2];
    } // end of the jumps to block 0078ae92
        // 0078ae92: comiss xmm3, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[3][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078ae95: jb 0x78aea2
        if cf { break 'b0078aea2; }
        // 0078ae97: movss dword ptr [esp + 0xb4], xmm0
        fr[43] = xmm[0][0];
        // 0078aea0: jmp 0x78aeb0
        break 'b0078aeb0;
    } // end of the jumps to block 0078aea2
        // 0078aea2: comiss xmm2, xmm3
        { let (c_f, z_f, p_f) = rt::comiss(xmm[2][0], xmm[3][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078aea5: jb 0x78aeb0
        if cf { break 'b0078aeb0; }
        // 0078aea7: movss dword ptr [esp + 0xb4], xmm2
        fr[43] = xmm[2][0];
    } // end of the jumps to block 0078aeb0
        // 0078aeb0: movaps xmm0, xmm1
        xmm[0] = xmm[1];
        // 0078aeb3: call 0x12f0d4c
        let target: u32 = 0x012f0d4cu32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078aeb8);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078aeb8: movss dword ptr [esp + 0x30], xmm0
        fr[10] = xmm[0][0];
        // 0078aebe: movss xmm0, dword ptr [esp + 0xb4]
        xmm[0] = [fr[43], 0.0, 0.0, 0.0];
        // 0078aec7: call 0x12f0d4c
        let target: u32 = 0x012f0d4cu32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078aecc);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078aecc: movss xmm3, dword ptr [esp + 0x50]
        xmm[3] = [fr[18], 0.0, 0.0, 0.0];
        // 0078aed2: movss xmm1, dword ptr [esp + 0x58]
        xmm[1] = [fr[20], 0.0, 0.0, 0.0];
        // 0078aed8: movss xmm6, dword ptr [0x15319b8]
        xmm[6] = [f32::from_bits({ ld36 = pg.u32(m, 0x015319b8u32); ld36 }), 0.0, 0.0, 0.0];
        // 0078aee0: movss xmm4, dword ptr [esp + 0xb0]
        xmm[4] = [fr[42], 0.0, 0.0, 0.0];
        // 0078aee9: movss dword ptr [esp + 0x18], xmm0
        fr[4] = xmm[0][0];
        // 0078aeef: movaps xmm2, xmm1
        xmm[2] = xmm[1];
        // 0078aef2: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078aef6: movaps xmm0, xmm3
        xmm[0] = xmm[3];
        // 0078aef9: mulss xmm0, xmm3
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[3][0]);
        // 0078aefd: addss xmm0, xmm2
        xmm[0][0] = rt::addss(xmm[0][0], xmm[2][0]);
        // 0078af01: sqrtss xmm2, xmm0
        xmm[2][0] = xmm[0][0].sqrt();
        // 0078af05: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078af08: divss xmm0, xmm2
        xmm[0][0] = rt::divss(xmm[0][0], xmm[2][0]);
        // 0078af0c: mulss xmm1, xmm0
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[0][0]);
        // 0078af10: movaps xmm2, xmm0
        xmm[2] = xmm[0];
        // 0078af13: movaps xmm5, xmm0
        xmm[5] = xmm[0];
        // 0078af16: mulss xmm5, dword ptr [0x1521690]
        xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits({ ld37 = pg.u32(m, 0x01521690u32); ld37 }));
        // 0078af1e: movss dword ptr [esp + 0x58], xmm1
        fr[20] = xmm[1][0];
        // 0078af24: mulss xmm2, xmm3
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[3][0]);
        // 0078af28: movss xmm3, dword ptr [esp + 0xb8]
        xmm[3] = [fr[44], 0.0, 0.0, 0.0];
        // 0078af31: movaps xmm0, xmm4
        xmm[0] = xmm[4];
        // 0078af34: mulss xmm0, xmm4
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[4][0]);
        // 0078af38: movaps xmm1, xmm3
        xmm[1] = xmm[3];
        // 0078af3b: mulss xmm1, xmm3
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[3][0]);
        // 0078af3f: addss xmm0, xmm1
        xmm[0][0] = rt::addss(xmm[0][0], xmm[1][0]);
        // 0078af43: sqrtss xmm1, xmm0
        xmm[1][0] = xmm[0][0].sqrt();
        // 0078af47: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078af4a: divss xmm0, xmm1
        xmm[0][0] = rt::divss(xmm[0][0], xmm[1][0]);
        // 0078af4e: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078af51: mulss xmm1, xmm4
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[4][0]);
        // 0078af55: movaps xmm4, xmm0
        xmm[4] = xmm[0];
        // 0078af58: mulss xmm4, dword ptr [0x1521690]
        xmm[4][0] = rt::mulss(xmm[4][0], f32::from_bits(ld37));
        // 0078af60: mulss xmm3, xmm0
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[0][0]);
        // 0078af64: movss xmm0, dword ptr [esp + 0x58]
        xmm[0] = [fr[20], 0.0, 0.0, 0.0];
        // 0078af6a: movaps xmm7, xmm3
        xmm[7] = xmm[3];
        // 0078af6d: mulss xmm7, xmm2
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[2][0]);
        // 0078af71: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078af75: subss xmm0, xmm7
        xmm[0][0] = rt::subss(xmm[0][0], xmm[7][0]);
        // 0078af79: comiss xmm0, xmm6
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[6][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078af7c: movss xmm7, dword ptr [0x161c20c]
        xmm[7] = [f32::from_bits({ ld38 = pg.u32(m, 0x0161c20cu32); ld38 }), 0.0, 0.0, 0.0];
        // 0078af84: jb 0x78af8b
        if cf { break 'b0078af8b; }
        // 0078af86: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078af89: jmp 0x78af93
        break 'b0078af93;
    } // end of the jumps to block 0078af8b
        // 0078af8b: comiss xmm7, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[7][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078af8e: jb 0x78af93
        if cf { break 'b0078af93; }
        // 0078af90: movaps xmm0, xmm7
        xmm[0] = xmm[7];
    } // end of the jumps to block 0078af93
        // 0078af93: mulss xmm3, dword ptr [esp + 0x58]
        xmm[3][0] = rt::mulss(xmm[3][0], fr[20]);
        // 0078af99: mulss xmm1, xmm2
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[2][0]);
        // 0078af9d: mulss xmm4, xmm5
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[5][0]);
        // 0078afa1: addss xmm1, xmm4
        xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
        // 0078afa5: addss xmm1, xmm3
        xmm[1][0] = rt::addss(xmm[1][0], xmm[3][0]);
        // 0078afa9: comiss xmm1, xmm6
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[6][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078afac: jb 0x78afb3
        if cf { break 'b0078afb3; }
        // 0078afae: movaps xmm1, xmm6
        xmm[1] = xmm[6];
        // 0078afb1: jmp 0x78afbb
        break 'b0078afbb;
    } // end of the jumps to block 0078afb3
        // 0078afb3: comiss xmm7, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[7][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078afb6: jb 0x78afbb
        if cf { break 'b0078afbb; }
        // 0078afb8: movaps xmm1, xmm7
        xmm[1] = xmm[7];
    } // end of the jumps to block 0078afbb
        // 0078afbb: cvtps2pd xmm0, xmm0
        { let s = xmm[0]; xmm[0] = rt::cvtps2pd(s[0], s[1]); }
        // 0078afbe: cvtps2pd xmm1, xmm1
        { let s = xmm[1]; xmm[1] = rt::cvtps2pd(s[0], s[1]); }
        // 0078afc1: call 0x12f1109
        let target: u32 = 0x012f1109u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078afc6);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078afc6: mov edx, dword ptr [ebx + 0x368]
        edx = { ld39 = pg.u32(m, ebx.wrapping_add(0x368)); ld39 };
        // 0078afcc: movzx eax, byte ptr [edx + 1]
        eax = { ld40 = pg.u8(m, edx.wrapping_add(0x1)) as u32; ld40 };
        // 0078afd0: movss xmm3, dword ptr [esi + 0x1fc]
        xmm[3] = [f32::from_bits({ ld41 = pg.u32(m, esi.wrapping_add(0x1fc)); ld41 }), 0.0, 0.0, 0.0];
        // 0078afd8: movss xmm4, dword ptr [esi + 0x200]
        xmm[4] = [f32::from_bits({ ld42 = pg.u32(m, esi.wrapping_add(0x200)); ld42 }), 0.0, 0.0, 0.0];
        // 0078afe0: movss xmm2, dword ptr [esi + 0x214]
        xmm[2] = [f32::from_bits({ ld43 = pg.u32(m, esi.wrapping_add(0x214)); ld43 }), 0.0, 0.0, 0.0];
        // 0078afe8: xorps xmm1, xmm1
        { let s = xmm[1]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
        // 0078afeb: cvtsd2ss xmm1, xmm0
        xmm[1][0] = f64::from_bits((((xmm[0][1].to_bits() as u64) << 32) | xmm[0][0].to_bits() as u64)) as f32;
        // 0078afef: movss xmm0, dword ptr [esp + 0x18]
        xmm[0] = [fr[4], 0.0, 0.0, 0.0];
        // 0078aff5: subss xmm0, dword ptr [esp + 0x30]
        xmm[0][0] = rt::subss(xmm[0][0], fr[10]);
        // 0078affb: movaps xmm5, xmm0
        xmm[5] = xmm[0];
        // 0078affe: movss xmm0, dword ptr [esi + 0x218]
        xmm[0] = [f32::from_bits({ ld44 = pg.u32(m, esi.wrapping_add(0x218)); ld44 }), 0.0, 0.0, 0.0];
        // 0078b006: movss dword ptr [esp + 0x24], xmm1
        fr[7] = xmm[1][0];
        // 0078b00c: movss dword ptr [esp + 0x44], xmm5
        fr[15] = xmm[5][0];
        // 0078b012: movss dword ptr [esp + 0x30], xmm3
        fr[10] = xmm[3][0];
        // 0078b018: movss dword ptr [esp + 0x34], xmm4
        fr[11] = xmm[4][0];
        // 0078b01e: movss dword ptr [esp + 0x40], xmm2
        fr[14] = xmm[2][0];
        // 0078b024: movss dword ptr [esp + 0x28], xmm0
        fr[8] = xmm[0][0];
        // 0078b02a: cmp eax, 0xff
        { let (a, b) = (eax, 0xffu32);
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
        }
        // 0078b02f: je 0x78b1c2
        if zf { break 'b0078b1c2; }
        // 0078b035: lea eax, [eax + eax*4]
        eax = eax.wrapping_add(eax.wrapping_mul(4));
        // 0078b038: shl eax, 5
        { let a = eax;
        let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, 5, 32);
        (cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);
        eax = r;
        }
        // 0078b03b: add eax, dword ptr [ebx + 0x364]
        { let (a, b) = (eax, { ld45 = pg.u32(m, ebx.wrapping_add(0x364)); ld45 });
        (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
        let r = a.wrapping_add(b) & 0xffffffffu32;
        eax = r;
        }
        // 0078b041: je 0x78b1c2
        if zf { break 'b0078b1c2; }
        // 0078b047: movss xmm3, dword ptr [edi + 0x30]
        xmm[3] = [f32::from_bits({ ld46 = pg.u32(m, edi.wrapping_add(0x30)); ld46 }), 0.0, 0.0, 0.0];
        // 0078b04c: movss xmm1, dword ptr [edi + 0x34]
        xmm[1] = [f32::from_bits({ ld47 = pg.u32(m, edi.wrapping_add(0x34)); ld47 }), 0.0, 0.0, 0.0];
        // 0078b051: movss xmm0, dword ptr [edi + 0x38]
        xmm[0] = [f32::from_bits({ ld48 = pg.u32(m, edi.wrapping_add(0x38)); ld48 }), 0.0, 0.0, 0.0];
        // 0078b056: movss xmm6, dword ptr [eax + 0x30]
        xmm[6] = [f32::from_bits({ ld49 = pg.u32(m, eax.wrapping_add(0x30)); ld49 }), 0.0, 0.0, 0.0];
        // 0078b05b: movss xmm7, dword ptr [eax + 0x34]
        xmm[7] = [f32::from_bits({ ld50 = pg.u32(m, eax.wrapping_add(0x34)); ld50 }), 0.0, 0.0, 0.0];
        // 0078b060: movss xmm2, dword ptr [eax + 0x38]
        xmm[2] = [f32::from_bits({ ld51 = pg.u32(m, eax.wrapping_add(0x38)); ld51 }), 0.0, 0.0, 0.0];
        // 0078b065: movss xmm5, dword ptr [0x15319b8]
        xmm[5] = [f32::from_bits({ ld52 = pg.u32(m, 0x015319b8u32); ld52 }), 0.0, 0.0, 0.0];
        // 0078b06d: mulss xmm1, xmm1
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[1][0]);
        // 0078b071: movaps xmm4, xmm3
        xmm[4] = xmm[3];
        // 0078b074: mulss xmm4, xmm3
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[3][0]);
        // 0078b078: addss xmm1, xmm4
        xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
        // 0078b07c: movaps xmm4, xmm0
        xmm[4] = xmm[0];
        // 0078b07f: mulss xmm4, xmm0
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[0][0]);
        // 0078b083: addss xmm1, xmm4
        xmm[1][0] = rt::addss(xmm[1][0], xmm[4][0]);
        // 0078b087: sqrtss xmm4, xmm1
        xmm[4][0] = xmm[1][0].sqrt();
        // 0078b08b: movss xmm1, dword ptr [0x150a350]
        xmm[1] = [f32::from_bits({ ld53 = pg.u32(m, 0x0150a350u32); ld53 }), 0.0, 0.0, 0.0];
        // 0078b093: comiss xmm1, xmm4
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[4][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b096: jbe 0x78b0a0
        if (cf || zf) { break 'b0078b0a0; }
        // 0078b098: movss dword ptr [esp + 0x50], xmm3
        fr[18] = xmm[3][0];
        // 0078b09e: jmp 0x78b0b8
        break 'b0078b0b8;
    } // end of the jumps to block 0078b0a0
        // 0078b0a0: movaps xmm1, xmm5
        xmm[1] = xmm[5];
        // 0078b0a3: divss xmm1, xmm4
        xmm[1][0] = rt::divss(xmm[1][0], xmm[4][0]);
        // 0078b0a7: movaps xmm4, xmm1
        xmm[4] = xmm[1];
        // 0078b0aa: mulss xmm4, xmm3
        xmm[4][0] = rt::mulss(xmm[4][0], xmm[3][0]);
        // 0078b0ae: mulss xmm0, xmm1
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[1][0]);
        // 0078b0b2: movss dword ptr [esp + 0x50], xmm4
        fr[18] = xmm[4][0];
    } // end of the jumps to block 0078b0b8
        // 0078b0b8: movss xmm1, dword ptr [0x150a350]
        xmm[1] = [f32::from_bits({ ld54 = pg.u32(m, 0x0150a350u32); ld54 }), 0.0, 0.0, 0.0];
        // 0078b0c0: movss dword ptr [esp + 0x58], xmm0
        fr[20] = xmm[0][0];
        // 0078b0c6: mulss xmm7, xmm7
        xmm[7][0] = rt::mulss(xmm[7][0], xmm[7][0]);
        // 0078b0ca: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078b0cd: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 0078b0d1: addss xmm7, xmm0
        xmm[7][0] = rt::addss(xmm[7][0], xmm[0][0]);
        // 0078b0d5: movaps xmm0, xmm2
        xmm[0] = xmm[2];
        // 0078b0d8: mulss xmm0, xmm2
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[2][0]);
        // 0078b0dc: addss xmm7, xmm0
        xmm[7][0] = rt::addss(xmm[7][0], xmm[0][0]);
        // 0078b0e0: sqrtss xmm0, xmm7
        xmm[0][0] = xmm[7][0].sqrt();
        // 0078b0e4: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b0e7: jbe 0x78b0ee
        if (cf || zf) { break 'b0078b0ee; }
        // 0078b0e9: movaps xmm0, xmm6
        xmm[0] = xmm[6];
        // 0078b0ec: jmp 0x78b0fd
        break 'b0078b0fd;
    } // end of the jumps to block 0078b0ee
        // 0078b0ee: divss xmm5, xmm0
        xmm[5][0] = rt::divss(xmm[5][0], xmm[0][0]);
        // 0078b0f2: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 0078b0f5: mulss xmm0, xmm6
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[6][0]);
        // 0078b0f9: mulss xmm2, xmm5
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[5][0]);
    } // end of the jumps to block 0078b0fd
        // 0078b0fd: cvtps2pd xmm0, xmm0
        { let s = xmm[0]; xmm[0] = rt::cvtps2pd(s[0], s[1]); }
        // 0078b100: cvtps2pd xmm1, xmm2
        { let s = xmm[2]; xmm[1] = rt::cvtps2pd(s[0], s[1]); }
        // 0078b103: call 0x12f1109
        let target: u32 = 0x012f1109u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078b108);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078b108: movss xmm1, dword ptr [esp + 0x58]
        xmm[1] = [fr[20], 0.0, 0.0, 0.0];
        // 0078b10e: cvtsd2ss xmm0, xmm0
        xmm[0][0] = f64::from_bits((((xmm[0][1].to_bits() as u64) << 32) | xmm[0][0].to_bits() as u64)) as f32;
        // 0078b112: movss dword ptr [esp + 0x18], xmm0
        fr[4] = xmm[0][0];
        // 0078b118: movss xmm0, dword ptr [esp + 0x50]
        xmm[0] = [fr[18], 0.0, 0.0, 0.0];
        // 0078b11e: cvtps2pd xmm0, xmm0
        { let s = xmm[0]; xmm[0] = rt::cvtps2pd(s[0], s[1]); }
        // 0078b121: cvtps2pd xmm1, xmm1
        { let s = xmm[1]; xmm[1] = rt::cvtps2pd(s[0], s[1]); }
        // 0078b124: call 0x12f1109
        let target: u32 = 0x012f1109u32;
        esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x0078b129);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        call_ext(m, c, target);
        (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
        xmm = c.xmm.map(|v| v.map(f32::from_bits));
        // 0078b129: movss xmm2, dword ptr [0x161c274]
        xmm[2] = [f32::from_bits({ ld55 = pg.u32(m, 0x0161c274u32); ld55 }), 0.0, 0.0, 0.0];
        // 0078b131: xorps xmm7, xmm7
        { let s = xmm[7]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
        // 0078b134: xorps xmm1, xmm1
        { let s = xmm[1]; for k in 0..4 { xmm[1][k] = f32::from_bits(xmm[1][k].to_bits() ^ s[k].to_bits()); } }
        // 0078b137: cvtsd2ss xmm1, xmm0
        xmm[1][0] = f64::from_bits((((xmm[0][1].to_bits() as u64) << 32) | xmm[0][0].to_bits() as u64)) as f32;
        // 0078b13b: movss xmm0, dword ptr [esp + 0x18]
        xmm[0] = [fr[4], 0.0, 0.0, 0.0];
        // 0078b141: subss xmm0, xmm1
        xmm[0][0] = rt::subss(xmm[0][0], xmm[1][0]);
        // 0078b145: comiss xmm0, xmm7
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[7][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b148: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b14b: mulss xmm1, dword ptr [0x161c548]
        xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits({ ld56 = pg.u32(m, 0x0161c548u32); ld56 }));
        // 0078b153: cvttss2si eax, xmm1
        eax = rt::cvttss2si(xmm[1][0]);
        // 0078b157: xorps xmm3, xmm3
        { let s = xmm[3]; for k in 0..4 { xmm[3][k] = f32::from_bits(xmm[3][k].to_bits() ^ s[k].to_bits()); } }
        // 0078b15a: cvtsi2ss xmm3, eax
        xmm[3][0] = (eax as i32) as f32;
        // 0078b15e: mulss xmm3, xmm2
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[2][0]);
        // 0078b162: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b165: subss xmm1, xmm3
        xmm[1][0] = rt::subss(xmm[1][0], xmm[3][0]);
        // 0078b169: jae 0x78b16f
        if !cf { break 'b0078b16f; }
        // 0078b16b: addss xmm1, xmm2
        xmm[1][0] = rt::addss(xmm[1][0], xmm[2][0]);
    } // end of the jumps to block 0078b16f
        // 0078b16f: comiss xmm1, dword ptr [0x150a344]
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], f32::from_bits({ ld57 = pg.u32(m, 0x0150a344u32); ld57 })); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b176: movaps xmm0, xmm1
        xmm[0] = xmm[1];
        // 0078b179: jb 0x78b182
        if cf { break 'b0078b182; }
        // 0078b17b: subss xmm1, xmm2
        xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
        // 0078b17f: movaps xmm0, xmm1
        xmm[0] = xmm[1];
    } // end of the jumps to block 0078b182
        // 0078b182: mulss xmm0, dword ptr [0x161c208]
        xmm[0][0] = rt::mulss(xmm[0][0], f32::from_bits({ ld58 = pg.u32(m, 0x0161c208u32); ld58 }));
        // 0078b18a: movss xmm5, dword ptr [esp + 0x44]
        xmm[5] = [fr[15], 0.0, 0.0, 0.0];
        // 0078b190: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b193: addss xmm1, dword ptr [esp + 0x30]
        xmm[1][0] = rt::addss(xmm[1][0], fr[10]);
        // 0078b199: movaps xmm3, xmm1
        xmm[3] = xmm[1];
        // 0078b19c: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b19f: addss xmm1, dword ptr [esp + 0x34]
        xmm[1][0] = rt::addss(xmm[1][0], fr[11]);
        // 0078b1a5: movaps xmm4, xmm1
        xmm[4] = xmm[1];
        // 0078b1a8: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b1ab: addss xmm1, dword ptr [esp + 0x40]
        xmm[1][0] = rt::addss(xmm[1][0], fr[14]);
        // 0078b1b1: addss xmm0, dword ptr [esp + 0x28]
        xmm[0][0] = rt::addss(xmm[0][0], fr[8]);
        // 0078b1b7: movaps xmm2, xmm1
        xmm[2] = xmm[1];
        // 0078b1ba: movss xmm1, dword ptr [esp + 0x24]
        xmm[1] = [fr[7], 0.0, 0.0, 0.0];
        // 0078b1c0: jmp 0x78b1c5
        break 'b0078b1c5;
    } // end of the jumps to block 0078b1c2
        // 0078b1c2: xorps xmm7, xmm7
        { let s = xmm[7]; for k in 0..4 { xmm[7][k] = f32::from_bits(xmm[7][k].to_bits() ^ s[k].to_bits()); } }
    } // end of the jumps to block 0078b1c5
        // 0078b1c5: mulss xmm1, dword ptr [0x161c548]
        xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits({ ld59 = pg.u32(m, 0x0161c548u32); ld59 }));
        // 0078b1cd: movss xmm6, dword ptr [0x161c300]
        xmm[6] = [f32::from_bits({ ld60 = pg.u32(m, 0x0161c300u32); ld60 }), 0.0, 0.0, 0.0];
        // 0078b1d5: mulss xmm1, xmm6
        xmm[1][0] = rt::mulss(xmm[1][0], xmm[6][0]);
        // 0078b1d9: xor cl, cl
        { let (a, b) = (((ecx >> 0) & 0xff), ((ecx >> 0) & 0xff));
        (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffu32, 8);
        let r = (a ^ b) & 0xffu32;
        ecx = (ecx & !(0xffu32 << 0)) | (((r) & 0xff) << 0);
        }
        // 0078b1db: xor al, al
        { let (a, b) = (((eax >> 0) & 0xff), ((eax >> 0) & 0xff));
        (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffu32, 8);
        let r = (a ^ b) & 0xffu32;
        eax = (eax & !(0xffu32 << 0)) | (((r) & 0xff) << 0);
        }
        // 0078b1dd: comiss xmm2, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[2][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b1e0: jb 0x78b1e7
        if cf { break 'b0078b1e7; }
        // 0078b1e2: movaps xmm1, xmm2
        xmm[1] = xmm[2];
        // 0078b1e5: mov al, 1
        eax = (eax & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
    } // end of the jumps to block 0078b1e7
        // 0078b1e7: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b1ea: jb 0x78b1f1
        if cf { break 'b0078b1f1; }
        // 0078b1ec: movaps xmm1, xmm0
        xmm[1] = xmm[0];
        // 0078b1ef: jmp 0x78b1f5
        break 'b0078b1f5;
    } // end of the jumps to block 0078b1f1
        // 0078b1f1: cmp al, cl
        { let (a, b) = (((eax >> 0) & 0xff), ((ecx >> 0) & 0xff));
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 0078b1f3: je 0x78b20e
        if zf { break 'b0078b20e; }
    } // end of the jumps to block 0078b1f5
        // 0078b1f5: cmp byte ptr [esi + 0x20c], cl
        { let (a, b) = ({ ld61 = pg.u8(m, esi.wrapping_add(0x20c)) as u32; ld61 }, ((ecx >> 0) & 0xff));
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 0078b1fb: jne 0x78b205
        if !zf { break 'b0078b205; }
        // 0078b1fd: movss dword ptr [esi + 0x1f4], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1f4), xmm[1][0].to_bits());
    } // end of the jumps to block 0078b205
        // 0078b205: mov byte ptr [esi + 0x20c], 1
        pg.set_u8(m, esi.wrapping_add(0x20c), (0x1u32) as u8);
        // 0078b20c: jmp 0x78b21c
        break 'b0078b21c;
    } // end of the jumps to block 0078b20e
        // 0078b20e: mov byte ptr [esi + 0x20c], cl
        pg.set_u8(m, esi.wrapping_add(0x20c), (((ecx >> 0) & 0xff)) as u8);
        // 0078b214: movss dword ptr [esi + 0x1f4], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1f4), xmm[1][0].to_bits());
    } // end of the jumps to block 0078b21c
        // 0078b21c: movss xmm1, dword ptr [esi + 0x1f4]
        xmm[1] = [f32::from_bits({ ld62 = pg.u32(m, esi.wrapping_add(0x1f4)); ld62 }), 0.0, 0.0, 0.0];
        // 0078b224: comiss xmm3, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[3][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b227: movaps xmm0, xmm1
        xmm[0] = xmm[1];
        // 0078b22a: jb 0x78b22f
        if cf { break 'b0078b22f; }
        // 0078b22c: movaps xmm0, xmm3
        xmm[0] = xmm[3];
    } // end of the jumps to block 0078b22f
        // 0078b22f: comiss xmm1, xmm4
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[4][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b232: jb 0x78b237
        if cf { break 'b0078b237; }
        // 0078b234: movaps xmm0, xmm4
        xmm[0] = xmm[4];
    } // end of the jumps to block 0078b237
        // 0078b237: movss xmm1, dword ptr [esi + 0x1ec]
        xmm[1] = [f32::from_bits({ ld63 = pg.u32(m, esi.wrapping_add(0x1ec)); ld63 }), 0.0, 0.0, 0.0];
        // 0078b23f: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b242: mov byte ptr [esi + 0x231], cl
        pg.set_u8(m, esi.wrapping_add(0x231), (((ecx >> 0) & 0xff)) as u8);
        // 0078b248: jbe 0x78b27d
        if (cf || zf) { break 'b0078b27d; }
        // 0078b24a: movss xmm2, dword ptr [ebx + 0x14]
        xmm[2] = [f32::from_bits({ ld64 = pg.u32(m, ebx.wrapping_add(0x14)); ld64 }), 0.0, 0.0, 0.0];
        // 0078b24f: mulss xmm2, dword ptr [esi + 0x21c]
        xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits({ ld65 = pg.u32(m, esi.wrapping_add(0x21c)); ld65 }));
        // 0078b257: subss xmm1, xmm2
        xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
        // 0078b25b: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b25e: movss dword ptr [esi + 0x1ec], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1ec), xmm[1][0].to_bits());
        // 0078b266: mov byte ptr [esi + 0x231], 1
        pg.set_u8(m, esi.wrapping_add(0x231), (0x1u32) as u8);
        // 0078b26d: jb 0x78b27d
        if cf { break 'b0078b27d; }
        // 0078b26f: movss dword ptr [esi + 0x1ec], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1ec), xmm[0][0].to_bits());
        // 0078b277: mov byte ptr [esi + 0x231], cl
        pg.set_u8(m, esi.wrapping_add(0x231), (((ecx >> 0) & 0xff)) as u8);
    } // end of the jumps to block 0078b27d
        // 0078b27d: movss xmm1, dword ptr [esi + 0x1ec]
        xmm[1] = [f32::from_bits({ ld66 = pg.u32(m, esi.wrapping_add(0x1ec)); ld66 }), 0.0, 0.0, 0.0];
        // 0078b285: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b288: jbe 0x78b2c0
        if (cf || zf) { break 'b0078b2c0; }
        // 0078b28a: movss xmm2, dword ptr [ebx + 0x14]
        xmm[2] = [f32::from_bits({ ld67 = pg.u32(m, ebx.wrapping_add(0x14)); ld67 }), 0.0, 0.0, 0.0];
        // 0078b28f: mulss xmm2, dword ptr [esi + 0x21c]
        xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits({ ld68 = pg.u32(m, esi.wrapping_add(0x21c)); ld68 }));
        // 0078b297: addss xmm2, xmm1
        xmm[2][0] = rt::addss(xmm[2][0], xmm[1][0]);
        // 0078b29b: movaps xmm1, xmm2
        xmm[1] = xmm[2];
        // 0078b29e: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b2a1: movss dword ptr [esi + 0x1ec], xmm2
        pg.set_u32(m, esi.wrapping_add(0x1ec), xmm[2][0].to_bits());
        // 0078b2a9: mov byte ptr [esi + 0x231], 1
        pg.set_u8(m, esi.wrapping_add(0x231), (0x1u32) as u8);
        // 0078b2b0: jb 0x78b2c0
        if cf { break 'b0078b2c0; }
        // 0078b2b2: movss dword ptr [esi + 0x1ec], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1ec), xmm[0][0].to_bits());
        // 0078b2ba: mov byte ptr [esi + 0x231], cl
        pg.set_u8(m, esi.wrapping_add(0x231), (((ecx >> 0) & 0xff)) as u8);
    } // end of the jumps to block 0078b2c0
        // 0078b2c0: cmp byte ptr [esi + 0x232], cl
        { let (a, b) = ({ ld69 = pg.u8(m, esi.wrapping_add(0x232)) as u32; ld69 }, ((ecx >> 0) & 0xff));
        (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
        }
        // 0078b2c6: jne 0x78b3ad
        if !zf { break 'b0078b3ad; }
        // 0078b2cc: mulss xmm5, dword ptr [0x161cfb0]
        xmm[5][0] = rt::mulss(xmm[5][0], f32::from_bits({ ld70 = pg.u32(m, 0x0161cfb0u32); ld70 }));
        // 0078b2d4: movss xmm1, dword ptr [esi + 0x204]
        xmm[1] = [f32::from_bits({ ld71 = pg.u32(m, esi.wrapping_add(0x204)); ld71 }), 0.0, 0.0, 0.0];
        // 0078b2dc: mulss xmm5, xmm6
        xmm[5][0] = rt::mulss(xmm[5][0], xmm[6][0]);
        // 0078b2e0: movaps xmm0, xmm5
        xmm[0] = xmm[5];
        // 0078b2e3: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b2e6: movss dword ptr [esi + 0x1f8], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1f8), xmm[0][0].to_bits());
        // 0078b2ee: jb 0x78b2f8
        if cf { break 'b0078b2f8; }
        // 0078b2f0: movss dword ptr [esi + 0x1f8], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1f8), xmm[1][0].to_bits());
    } // end of the jumps to block 0078b2f8
        // 0078b2f8: movss xmm0, dword ptr [esi + 0x208]
        xmm[0] = [f32::from_bits({ ld72 = pg.u32(m, esi.wrapping_add(0x208)); ld72 }), 0.0, 0.0, 0.0];
        // 0078b300: movss xmm1, dword ptr [esi + 0x1f8]
        xmm[1] = [f32::from_bits({ ld73 = pg.u32(m, esi.wrapping_add(0x1f8)); ld73 }), 0.0, 0.0, 0.0];
        // 0078b308: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b30b: jb 0x78b315
        if cf { break 'b0078b315; }
        // 0078b30d: movss dword ptr [esi + 0x1f8], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1f8), xmm[0][0].to_bits());
    } // end of the jumps to block 0078b315
        // 0078b315: movss xmm0, dword ptr [esi + 0x1f8]
        xmm[0] = [f32::from_bits({ ld74 = pg.u32(m, esi.wrapping_add(0x1f8)); ld74 }), 0.0, 0.0, 0.0];
        // 0078b31d: movss xmm1, dword ptr [esi + 0x1f0]
        xmm[1] = [f32::from_bits({ ld75 = pg.u32(m, esi.wrapping_add(0x1f0)); ld75 }), 0.0, 0.0, 0.0];
        // 0078b325: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b328: mov byte ptr [esi + 0x230], cl
        pg.set_u8(m, esi.wrapping_add(0x230), (((ecx >> 0) & 0xff)) as u8);
        // 0078b32e: jbe 0x78b363
        if (cf || zf) { break 'b0078b363; }
        // 0078b330: movss xmm2, dword ptr [ebx + 0x14]
        xmm[2] = [f32::from_bits({ ld76 = pg.u32(m, ebx.wrapping_add(0x14)); ld76 }), 0.0, 0.0, 0.0];
        // 0078b335: mulss xmm2, dword ptr [esi + 0x220]
        xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits({ ld77 = pg.u32(m, esi.wrapping_add(0x220)); ld77 }));
        // 0078b33d: subss xmm1, xmm2
        xmm[1][0] = rt::subss(xmm[1][0], xmm[2][0]);
        // 0078b341: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b344: movss dword ptr [esi + 0x1f0], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[1][0].to_bits());
        // 0078b34c: mov byte ptr [esi + 0x230], 1
        pg.set_u8(m, esi.wrapping_add(0x230), (0x1u32) as u8);
        // 0078b353: jb 0x78b363
        if cf { break 'b0078b363; }
        // 0078b355: movss dword ptr [esi + 0x1f0], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[0][0].to_bits());
        // 0078b35d: mov byte ptr [esi + 0x230], cl
        pg.set_u8(m, esi.wrapping_add(0x230), (((ecx >> 0) & 0xff)) as u8);
    } // end of the jumps to block 0078b363
        // 0078b363: movss xmm1, dword ptr [esi + 0x1f0]
        xmm[1] = [f32::from_bits({ ld78 = pg.u32(m, esi.wrapping_add(0x1f0)); ld78 }), 0.0, 0.0, 0.0];
        // 0078b36b: comiss xmm0, xmm1
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[1][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b36e: jbe 0x78b436
        if (cf || zf) { break 'b0078b436; }
        // 0078b374: movss xmm2, dword ptr [ebx + 0x14]
        xmm[2] = [f32::from_bits({ ld79 = pg.u32(m, ebx.wrapping_add(0x14)); ld79 }), 0.0, 0.0, 0.0];
        // 0078b379: mulss xmm2, dword ptr [esi + 0x220]
        xmm[2][0] = rt::mulss(xmm[2][0], f32::from_bits({ ld80 = pg.u32(m, esi.wrapping_add(0x220)); ld80 }));
        // 0078b381: addss xmm2, xmm1
        xmm[2][0] = rt::addss(xmm[2][0], xmm[1][0]);
        // 0078b385: movaps xmm1, xmm2
        xmm[1] = xmm[2];
        // 0078b388: comiss xmm1, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[1][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b38b: movss dword ptr [esi + 0x1f0], xmm2
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[2][0].to_bits());
        // 0078b393: mov byte ptr [esi + 0x230], 1
        pg.set_u8(m, esi.wrapping_add(0x230), (0x1u32) as u8);
        // 0078b39a: jb 0x78b436
        if cf { break 'b0078b436; }
        // 0078b3a0: movss dword ptr [esi + 0x1f0], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[0][0].to_bits());
        // 0078b3a8: jmp 0x78b430
        break 'b0078b430;
    } // end of the jumps to block 0078b3ad
        // 0078b3ad: movss xmm0, dword ptr [esi + 0x1f0]
        xmm[0] = [f32::from_bits({ ld81 = pg.u32(m, esi.wrapping_add(0x1f0)); ld81 }), 0.0, 0.0, 0.0];
        // 0078b3b5: comiss xmm0, xmm7
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[7][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b3b8: mov byte ptr [esi + 0x230], cl
        pg.set_u8(m, esi.wrapping_add(0x230), (((ecx >> 0) & 0xff)) as u8);
        // 0078b3be: jbe 0x78b3f3
        if (cf || zf) { break 'b0078b3f3; }
        // 0078b3c0: movss xmm1, dword ptr [ebx + 0x14]
        xmm[1] = [f32::from_bits({ ld82 = pg.u32(m, ebx.wrapping_add(0x14)); ld82 }), 0.0, 0.0, 0.0];
        // 0078b3c5: mulss xmm1, dword ptr [esi + 0x220]
        xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits({ ld83 = pg.u32(m, esi.wrapping_add(0x220)); ld83 }));
        // 0078b3cd: subss xmm0, xmm1
        xmm[0][0] = rt::subss(xmm[0][0], xmm[1][0]);
        // 0078b3d1: comiss xmm7, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[7][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b3d4: movss dword ptr [esi + 0x1f0], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[0][0].to_bits());
        // 0078b3dc: mov byte ptr [esi + 0x230], 1
        pg.set_u8(m, esi.wrapping_add(0x230), (0x1u32) as u8);
        // 0078b3e3: jb 0x78b3f3
        if cf { break 'b0078b3f3; }
        // 0078b3e5: movss dword ptr [esi + 0x1f0], xmm7
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[7][0].to_bits());
        // 0078b3ed: mov byte ptr [esi + 0x230], cl
        pg.set_u8(m, esi.wrapping_add(0x230), (((ecx >> 0) & 0xff)) as u8);
    } // end of the jumps to block 0078b3f3
        // 0078b3f3: movss xmm0, dword ptr [esi + 0x1f0]
        xmm[0] = [f32::from_bits({ ld84 = pg.u32(m, esi.wrapping_add(0x1f0)); ld84 }), 0.0, 0.0, 0.0];
        // 0078b3fb: comiss xmm7, xmm0
        { let (c_f, z_f, p_f) = rt::comiss(xmm[7][0], xmm[0][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b3fe: jbe 0x78b436
        if (cf || zf) { break 'b0078b436; }
        // 0078b400: movss xmm1, dword ptr [ebx + 0x14]
        xmm[1] = [f32::from_bits({ ld85 = pg.u32(m, ebx.wrapping_add(0x14)); ld85 }), 0.0, 0.0, 0.0];
        // 0078b405: mulss xmm1, dword ptr [esi + 0x220]
        xmm[1][0] = rt::mulss(xmm[1][0], f32::from_bits({ ld86 = pg.u32(m, esi.wrapping_add(0x220)); ld86 }));
        // 0078b40d: addss xmm1, xmm0
        xmm[1][0] = rt::addss(xmm[1][0], xmm[0][0]);
        // 0078b411: movaps xmm0, xmm1
        xmm[0] = xmm[1];
        // 0078b414: comiss xmm0, xmm7
        { let (c_f, z_f, p_f) = rt::comiss(xmm[0][0], xmm[7][0]); (cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }
        // 0078b417: movss dword ptr [esi + 0x1f0], xmm1
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[1][0].to_bits());
        // 0078b41f: mov byte ptr [esi + 0x230], 1
        pg.set_u8(m, esi.wrapping_add(0x230), (0x1u32) as u8);
        // 0078b426: jb 0x78b436
        if cf { break 'b0078b436; }
        // 0078b428: movss dword ptr [esi + 0x1f0], xmm7
        pg.set_u32(m, esi.wrapping_add(0x1f0), xmm[7][0].to_bits());
    } // end of the jumps to block 0078b430
        // 0078b430: mov byte ptr [esi + 0x230], cl
        pg.set_u8(m, esi.wrapping_add(0x230), (((ecx >> 0) & 0xff)) as u8);
    } // end of the jumps to block 0078b436
        // 0078b436: movss xmm0, dword ptr [esi + 0x224]
        xmm[0] = [f32::from_bits({ ld87 = pg.u32(m, esi.wrapping_add(0x224)); ld87 }), 0.0, 0.0, 0.0];
        // 0078b43e: movss xmm1, dword ptr [esi + 0x1f0]
        xmm[1] = [f32::from_bits({ ld88 = pg.u32(m, esi.wrapping_add(0x1f0)); ld88 }), 0.0, 0.0, 0.0];
        // 0078b446: movaps xmm2, xmm0
        xmm[2] = xmm[0];
        // 0078b449: mulss xmm2, xmm1
        xmm[2][0] = rt::mulss(xmm[2][0], xmm[1][0]);
        // 0078b44d: movss dword ptr [esi + 0x80], xmm2
        pg.set_u32(m, esi.wrapping_add(0x80), xmm[2][0].to_bits());
        // 0078b455: movss xmm2, dword ptr [esi + 0x1ec]
        xmm[2] = [f32::from_bits({ ld89 = pg.u32(m, esi.wrapping_add(0x1ec)); ld89 }), 0.0, 0.0, 0.0];
        // 0078b45d: mulss xmm0, xmm2
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[2][0]);
        // 0078b461: movss dword ptr [esi + 0x84], xmm0
        pg.set_u32(m, esi.wrapping_add(0x84), xmm[0][0].to_bits());
        // 0078b469: movss xmm0, dword ptr [esi + 0x228]
        xmm[0] = [f32::from_bits({ ld90 = pg.u32(m, esi.wrapping_add(0x228)); ld90 }), 0.0, 0.0, 0.0];
        // 0078b471: movaps xmm3, xmm0
        xmm[3] = xmm[0];
        // 0078b474: mulss xmm0, xmm2
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[2][0]);
        // 0078b478: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078b47c: movss dword ptr [esi + 0x114], xmm0
        pg.set_u32(m, esi.wrapping_add(0x114), xmm[0][0].to_bits());
        // 0078b484: movss xmm0, dword ptr [esi + 0x22c]
        xmm[0] = [f32::from_bits({ ld91 = pg.u32(m, esi.wrapping_add(0x22c)); ld91 }), 0.0, 0.0, 0.0];
        // 0078b48c: movss dword ptr [esi + 0x110], xmm3
        pg.set_u32(m, esi.wrapping_add(0x110), xmm[3][0].to_bits());
        // 0078b494: movaps xmm3, xmm0
        xmm[3] = xmm[0];
        // 0078b497: mulss xmm3, xmm1
        xmm[3][0] = rt::mulss(xmm[3][0], xmm[1][0]);
        // 0078b49b: mulss xmm0, xmm2
        xmm[0][0] = rt::mulss(xmm[0][0], xmm[2][0]);
        // 0078b49f: mov byte ptr [esi + 0x232], cl
        pg.set_u8(m, esi.wrapping_add(0x232), (((ecx >> 0) & 0xff)) as u8);
        // 0078b4a5: movss dword ptr [esi + 0x1a0], xmm3
        pg.set_u32(m, esi.wrapping_add(0x1a0), xmm[3][0].to_bits());
        // 0078b4ad: movss dword ptr [esi + 0x1a4], xmm0
        pg.set_u32(m, esi.wrapping_add(0x1a4), xmm[0][0].to_bits());
    } // end of the jumps to block 0078b4b5
        // 0078b4b5: pop edi
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); edi = v; }
        // 0078b4b6: pop ebx
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebx = v; }
        // 0078b4b7: mov esp, ebp
        esp = ebp;
        // 0078b4b9: pop ebp
        { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
        // 0078b4ba: ret 
        esp = esp.wrapping_add(4 + 0);
        (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
        c.xmm = xmm.map(|v| v.map(f32::to_bits));
        return;
    unreachable!();
}
