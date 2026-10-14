//! L172 P2b (s9): native ports of the sPrimitive singleton methods the
//! shadow job's units reach through `0x0107FF00` / `0x0107FF30`
//! (`*0x018D1848`, vtable `0x01430E08`, slots +0x2C / +0x30): `vf11`
//! 0x00E61010 and `vf12` 0x00E68390, and the two dispatch thunks. The
//! bodies are mechanical translations (registers and flags in locals, the
//! stack in guest memory so every callee sees the original's frame); their
//! callees stay guest calls. Verified by `tests/shadow_primitive.rs`
//! against Unicorn runs of the original (`research/native/
//! record_shadow_primitive.py`). Regenerate with
//! `research/native/gen_shadow_primitive.py`.
use crate::semantic_rt as rt;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

/// The scheduler object the unit callbacks queue into (`[0x018D0B28]`).
/// Every scheduler access of the translated methods goes through here.
#[inline(always)]
fn scheduler(m: &dyn Mem, pg: Pages) -> u32 {
    pg.u32(m, 0x018d_0b28)
}

/// The CRT memcpy (memmove semantics, overlap-safe).
fn copy_guest(m: &mut dyn Mem, pg: Pages, dst: u32, src: u32, len: u32) {
    let backward = dst.wrapping_sub(src) < len;
    let mut done = 0u32;
    while done < len {
        let left = len - done;
        let (offset, chunk) = if backward {
            let since_boundary = |a: u32| match a.wrapping_add(left) & 0xfff {
                0 => 0x1000,
                n => n,
            };
            let chunk = left.min(since_boundary(src)).min(since_boundary(dst));
            (left - chunk, chunk)
        } else {
            let room = |a: u32| 0x1000 - (a.wrapping_add(done) & 0xfff);
            (done, left.min(room(src)).min(room(dst)))
        };
        let (from, to) = (src.wrapping_add(offset), dst.wrapping_add(offset));
        if let (Some(f), Some(t)) = (pg.bytes(from, chunk as usize, 1), pg.bytes(to, chunk as usize, 2)) {
            // SAFETY: `bytes` returned `chunk` bytes inside backed pages.
            unsafe { std::ptr::copy(f, t, chunk as usize) };
        } else if backward {
            for k in (0..chunk).rev() {
                let b = pg.u8(m, from.wrapping_add(k));
                pg.set_u8(m, to.wrapping_add(k), b);
            }
        } else {
            for k in 0..chunk {
                let b = pg.u8(m, from.wrapping_add(k));
                pg.set_u8(m, to.wrapping_add(k), b);
            }
        }
        done += chunk;
    }
}

/// 0x00E61010, `sPrimitive::vf11(this, context, mode, flag, id)` (thiscall, `ret 0x10`):
/// open one nested primitive list. With `mode == 0` the primitive lane of
/// `context + 0x58` (`this + 0x30 + lane * 4`) begins a list (0x0111D610 with
/// the last argument); otherwise every lane (`this + 0xD0` of them) begins
/// one, and the result is true when any of them did not.
// Translated from 0x00E61010 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport_s9.py (one commented statement per instruction).

#[allow(unused_assignments, unused_variables, unused_mut, clippy::all)]
pub fn sprimitive_vf11(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));
    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);
    let mut st0 = 0u32;
    let mut ld0 = 0u32; let mut ld1 = 0u32; let mut ld2 = 0u32; let mut ld3 = 0u32; let mut ld4 = 0u32; let mut ld5 = 0u32; let mut ld6 = 0u32; let mut ld7 = 0u32; let mut ld8 = 0u32;
    let mut pc: u32 = 0x00e61010;
    loop {
        match pc {
            0x00e61010 => {
                // 00e61010: cmp dword ptr [esp + 0xc], 0
                { let (a, b) = ({ ld0 = pg.u32(m, esp.wrapping_add(0xc)); ld0 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e61015: push ebp
                { let v = ebp; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e61016: mov ebp, ecx
                ebp = ecx;
                // 00e61018: jne 0xe61033
                if !zf { pc = 0x00e61033; continue; }
                pc = 0x00e6101a;
                continue;
            }
            0x00e6101a => {
                // 00e6101a: mov eax, dword ptr [esp + 8]
                eax = { ld1 = pg.u32(m, esp.wrapping_add(0x8)); ld1 };
                // 00e6101e: mov ecx, dword ptr [esp + 0x14]
                ecx = { ld2 = pg.u32(m, esp.wrapping_add(0x14)); ld2 };
                // 00e61022: mov eax, dword ptr [eax + 0x58]
                eax = { ld3 = pg.u32(m, eax.wrapping_add(0x58)); ld3 };
                // 00e61025: push ecx
                { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e61026: mov ecx, dword ptr [ebp + eax*4 + 0x30]
                ecx = { ld4 = pg.u32(m, ebp.wrapping_add(eax.wrapping_mul(4)).wrapping_add(0x30)); ld4 };
                // 00e6102a: call 0x111d610
                let target: u32 = 0x0111d610u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e6102f);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e6102f: pop ebp
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
                // 00e61030: ret 0x10
                esp = esp.wrapping_add(4 + 16);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                return;
            }
            0x00e61033 => {
                // 00e61033: push ebx
                { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e61034: push edi
                { let v = edi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e61035: xor edi, edi
                { let (a, b) = (edi, edi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edi = r;
                }
                // 00e61037: mov bl, 1
                ebx = (ebx & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
                // 00e61039: cmp dword ptr [ebp + 0xd0], edi
                { let (a, b) = ({ ld5 = pg.u32(m, ebp.wrapping_add(0xd0)); ld5 }, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6103f: jbe 0xe6106c
                if (cf || zf) { pc = 0x00e6106c; continue; }
                pc = 0x00e61041;
                continue;
            }
            0x00e61041 => {
                // 00e61041: push esi
                { let v = esi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e61042: lea esi, [ebp + 0x30]
                esi = ebp.wrapping_add(0x30);
                pc = 0x00e61045;
                continue;
            }
            0x00e61045 => {
                // 00e61045: mov edx, dword ptr [esp + 0x20]
                edx = { ld6 = pg.u32(m, esp.wrapping_add(0x20)); ld6 };
                // 00e61049: mov ecx, dword ptr [esi]
                ecx = { ld7 = pg.u32(m, esi); ld7 };
                // 00e6104b: push edx
                { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e6104c: call 0x111d610
                let target: u32 = 0x0111d610u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e61051);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e61051: test bl, bl
                { let (a, b) = (((ebx >> 0) & 0xff), ((ebx >> 0) & 0xff));
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
                }
                // 00e61053: je 0xe6105d
                if zf { pc = 0x00e6105d; continue; }
                pc = 0x00e61055;
                continue;
            }
            0x00e61055 => {
                // 00e61055: test eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e61057: jne 0xe6105d
                if !zf { pc = 0x00e6105d; continue; }
                pc = 0x00e61059;
                continue;
            }
            0x00e61059 => {
                // 00e61059: mov bl, 1
                ebx = (ebx & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
                // 00e6105b: jmp 0xe6105f
                pc = 0x00e6105f;
                continue;
            }
            0x00e6105d => {
                // 00e6105d: xor bl, bl
                { let (a, b) = (((ebx >> 0) & 0xff), ((ebx >> 0) & 0xff));
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffu32, 8);
                let r = (a ^ b) & 0xffu32;
                ebx = (ebx & !(0xffu32 << 0)) | (((r) & 0xff) << 0);
                }
                pc = 0x00e6105f;
                continue;
            }
            0x00e6105f => {
                // 00e6105f: inc edi
                { let a = edi; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edi = r;
                }
                // 00e61060: add esi, 4
                { let (a, b) = (esi, 0x4u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e61063: cmp edi, dword ptr [ebp + 0xd0]
                { let (a, b) = (edi, { ld8 = pg.u32(m, ebp.wrapping_add(0xd0)); ld8 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e61069: jb 0xe61045
                if cf { pc = 0x00e61045; continue; }
                pc = 0x00e6106b;
                continue;
            }
            0x00e6106b => {
                // 00e6106b: pop esi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); esi = v; }
                pc = 0x00e6106c;
                continue;
            }
            0x00e6106c => {
                // 00e6106c: xor eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                eax = r;
                }
                // 00e6106e: pop edi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); edi = v; }
                // 00e6106f: cmp bl, 1
                { let (a, b) = (((ebx >> 0) & 0xff), 0x1u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 00e61072: pop ebx
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebx = v; }
                // 00e61073: setne al
                eax = (eax & !(0xffu32 << 0)) | ((((!zf) as u32) & 0xff) << 0);
                // 00e61076: pop ebp
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
                // 00e61077: ret 0x10
                esp = esp.wrapping_add(4 + 16);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                return;
            }
            _ => unreachable!("pc {pc:#x}"),
        }
    }
}

/// 0x00E68390, `sPrimitive::vf12(this, context, lanes, parallel)` (thiscall,
/// `ret 0xc`): close the primitive lists of a unit pass.
///
/// Serial (`parallel == 0`): sort the lane's pool (the allocator vtable at
/// 0x0187663C allocates and frees a scratch copy, 0x0111E180 sorts it,
/// the unit's virtual +0x3C consumes it) and end the list (0x0111D650).
/// Parallel: queue one sorting job (0x0111E180, `DBE560` kind 2) per
/// non-empty pool of the eight lanes' four pools in the scheduler at
/// `[0x018D0B28]` (`+0x8C` job count, jobs from `+0xC0`), run them, merge
/// the sorted runs by descending key into one array, then either hand each
/// lane its slice (serial finish) or queue one draw job per lane
/// (0x010B7440, kind 3) after publishing the lanes' recording contexts at
/// `[0x018D0B28] + 0x1BDD0 + lane * 0x20`; finally end every list.
/// The scheduler is read through [`scheduler`] only.
// Translated from 0x00E68390 (DDDA.exe 2017-12-13 Steam), mechanically by
// research/native/semport_s9.py (one commented statement per instruction).

#[allow(unused_assignments, unused_variables, unused_mut, clippy::all)]
pub fn sprimitive_vf12(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));
    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);
    let mut st0 = 0u32;
    let mut ld0 = 0u32; let mut ld1 = 0u32; let mut ld2 = 0u32; let mut ld3 = 0u32; let mut ld4 = 0u32; let mut ld5 = 0u32; let mut ld6 = 0u32; let mut ld7 = 0u32; let mut ld8 = 0u32; let mut ld9 = 0u32; let mut ld10 = 0u32; let mut ld11 = 0u32; let mut ld12 = 0u32; let mut ld13 = 0u32; let mut ld14 = 0u32; let mut ld15 = 0u32; let mut ld16 = 0u32; let mut ld17 = 0u32; let mut ld18 = 0u32; let mut ld19 = 0u32; let mut ld20 = 0u32; let mut ld21 = 0u32; let mut ld22 = 0u32; let mut ld23 = 0u32; let mut ld24 = 0u32; let mut ld25 = 0u32; let mut ld26 = 0u32; let mut ld27 = 0u32; let mut ld28 = 0u32; let mut ld29 = 0u32; let mut ld30 = 0u32; let mut ld31 = 0u32; let mut ld32 = 0u32; let mut ld33 = 0u32; let mut ld34 = 0u32; let mut ld35 = 0u32; let mut ld36 = 0u32; let mut ld37 = 0u32; let mut ld38 = 0u32; let mut ld39 = 0u32; let mut ld40 = 0u32; let mut ld41 = 0u32; let mut ld42 = 0u32; let mut ld43 = 0u32; let mut ld44 = 0u32; let mut ld45 = 0u32; let mut ld46 = 0u32; let mut ld47 = 0u32; let mut ld48 = 0u32; let mut ld49 = 0u32; let mut ld50 = 0u32; let mut ld51 = 0u32; let mut ld52 = 0u32; let mut ld53 = 0u32; let mut ld54 = 0u32; let mut ld55 = 0u32; let mut ld56 = 0u32; let mut ld57 = 0u32; let mut ld58 = 0u32; let mut ld59 = 0u32; let mut ld60 = 0u32; let mut ld61 = 0u32; let mut ld62 = 0u32; let mut ld63 = 0u32; let mut ld64 = 0u32; let mut ld65 = 0u32; let mut ld66 = 0u32; let mut ld67 = 0u32; let mut ld68 = 0u32; let mut ld69 = 0u32; let mut ld70 = 0u32; let mut ld71 = 0u32; let mut ld72 = 0u32; let mut ld73 = 0u32; let mut ld74 = 0u32; let mut ld75 = 0u32; let mut ld76 = 0u32; let mut ld77 = 0u32; let mut ld78 = 0u32; let mut ld79 = 0u32; let mut ld80 = 0u32; let mut ld81 = 0u32; let mut ld82 = 0u32; let mut ld83 = 0u32; let mut ld84 = 0u32; let mut ld85 = 0u32; let mut ld86 = 0u32; let mut ld87 = 0u32; let mut ld88 = 0u32; let mut ld89 = 0u32; let mut ld90 = 0u32; let mut ld91 = 0u32; let mut ld92 = 0u32; let mut ld93 = 0u32; let mut ld94 = 0u32; let mut ld95 = 0u32; let mut ld96 = 0u32; let mut ld97 = 0u32; let mut ld98 = 0u32; let mut ld99 = 0u32; let mut ld100 = 0u32; let mut ld101 = 0u32; let mut ld102 = 0u32; let mut ld103 = 0u32; let mut ld104 = 0u32; let mut ld105 = 0u32; let mut ld106 = 0u32; let mut ld107 = 0u32; let mut ld108 = 0u32; let mut ld109 = 0u32; let mut ld110 = 0u32; let mut ld111 = 0u32; let mut ld112 = 0u32; let mut ld113 = 0u32; let mut ld114 = 0u32; let mut ld115 = 0u32; let mut ld116 = 0u32; let mut ld117 = 0u32; let mut ld118 = 0u32; let mut ld119 = 0u32; let mut ld120 = 0u32; let mut ld121 = 0u32; let mut ld122 = 0u32; let mut ld123 = 0u32; let mut ld124 = 0u32; let mut ld125 = 0u32; let mut ld126 = 0u32; let mut ld127 = 0u32; let mut ld128 = 0u32; let mut ld129 = 0u32; let mut ld130 = 0u32; let mut ld131 = 0u32; let mut ld132 = 0u32; let mut ld133 = 0u32; let mut ld134 = 0u32; let mut ld135 = 0u32; let mut ld136 = 0u32; let mut ld137 = 0u32; let mut ld138 = 0u32; let mut ld139 = 0u32; let mut ld140 = 0u32; let mut ld141 = 0u32; let mut ld142 = 0u32; let mut ld143 = 0u32; let mut ld144 = 0u32; let mut ld145 = 0u32; let mut ld146 = 0u32; let mut ld147 = 0u32; let mut ld148 = 0u32; let mut ld149 = 0u32; let mut ld150 = 0u32; let mut ld151 = 0u32; let mut ld152 = 0u32; let mut ld153 = 0u32; let mut ld154 = 0u32; let mut ld155 = 0u32; let mut ld156 = 0u32; let mut ld157 = 0u32; let mut ld158 = 0u32; let mut ld159 = 0u32; let mut ld160 = 0u32; let mut ld161 = 0u32; let mut ld162 = 0u32; let mut ld163 = 0u32; let mut ld164 = 0u32; let mut ld165 = 0u32; let mut ld166 = 0u32; let mut ld167 = 0u32; let mut ld168 = 0u32; let mut ld169 = 0u32; let mut ld170 = 0u32; let mut ld171 = 0u32; let mut ld172 = 0u32; let mut ld173 = 0u32; let mut ld174 = 0u32; let mut ld175 = 0u32;
    let mut pc: u32 = 0x00e68390;
    loop {
        match pc {
            0x00e68390 => {
                // 00e68390: sub esp, 0xb4
                { let (a, b) = (esp, 0xb4u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                let r = a.wrapping_sub(b) & 0xffffffffu32;
                esp = r;
                }
                // 00e68396: cmp dword ptr [esp + 0xc0], 0
                { let (a, b) = ({ ld0 = pg.u32(m, esp.wrapping_add(0xc0)); ld0 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6839e: push ebx
                { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e6839f: push ebp
                { let v = ebp; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683a0: push esi
                { let v = esi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683a1: mov esi, ecx
                esi = ecx;
                // 00e683a3: push edi
                { let v = edi; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683a4: mov dword ptr [esp + 0x28], esi
                pg.set_u32(m, esp.wrapping_add(0x28), esi);
                // 00e683a8: jne 0xe68467
                if !zf { pc = 0x00e68467; continue; }
                pc = 0x00e683ae;
                continue;
            }
            0x00e683ae => {
                // 00e683ae: mov eax, dword ptr [esp + 0xc8]
                eax = { ld1 = pg.u32(m, esp.wrapping_add(0xc8)); ld1 };
                // 00e683b5: mov ecx, dword ptr [eax + 0x58]
                ecx = { ld2 = pg.u32(m, eax.wrapping_add(0x58)); ld2 };
                // 00e683b8: mov edi, dword ptr [esi + ecx*4 + 0x30]
                edi = { ld3 = pg.u32(m, esi.wrapping_add(ecx.wrapping_mul(4)).wrapping_add(0x30)); ld3 };
                // 00e683bc: mov eax, dword ptr [edi + 4]
                eax = { ld4 = pg.u32(m, edi.wrapping_add(0x4)); ld4 };
                // 00e683bf: mov edx, dword ptr [eax + 0x64]
                edx = { ld5 = pg.u32(m, eax.wrapping_add(0x64)); ld5 };
                // 00e683c2: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e683c4: mov ebp, dword ptr [eax + edx*8 - 4]
                ebp = { ld6 = pg.u32(m, eax.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld6 };
                // 00e683c8: test ebp, ebp
                { let (a, b) = (ebp, ebp);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e683ca: je 0xe68453
                if zf { pc = 0x00e68453; continue; }
                pc = 0x00e683d0;
                continue;
            }
            0x00e683d0 => {
                // 00e683d0: mov ecx, dword ptr [0x187663c]
                ecx = { ld7 = pg.u32(m, 0x0187663cu32); ld7 };
                // 00e683d6: mov edx, dword ptr [ecx]
                edx = { ld8 = pg.u32(m, ecx); ld8 };
                // 00e683d8: lea eax, [ebp*8]
                eax = ebp.wrapping_mul(8);
                // 00e683df: push 0x10
                { let v = 0x10u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683e1: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683e2: mov eax, dword ptr [edx + 0x20]
                eax = { ld9 = pg.u32(m, edx.wrapping_add(0x20)); ld9 };
                // 00e683e5: call eax
                let target: u32 = eax;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e683e7);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e683e7: mov ebx, eax
                ebx = eax;
                // 00e683e9: push ebx
                { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e683ea: mov ecx, edi
                ecx = edi;
                // 00e683ec: call 0x111e180
                let target: u32 = 0x0111e180u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e683f1);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e683f1: mov eax, dword ptr [edi + 4]
                eax = { ld10 = pg.u32(m, edi.wrapping_add(0x4)); ld10 };
                // 00e683f4: mov ecx, dword ptr [eax + 0x64]
                ecx = { ld11 = pg.u32(m, eax.wrapping_add(0x64)); ld11 };
                // 00e683f7: add ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ecx = r;
                }
                // 00e683f9: cmp dword ptr [eax + ecx*8 - 4], 0
                { let (a, b) = ({ ld12 = pg.u32(m, eax.wrapping_add(ecx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld12 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e683fe: jbe 0xe68406
                if (cf || zf) { pc = 0x00e68406; continue; }
                pc = 0x00e68400;
                continue;
            }
            0x00e68400 => {
                // 00e68400: mov eax, dword ptr [eax + ecx*8 - 8]
                eax = { ld13 = pg.u32(m, eax.wrapping_add(ecx.wrapping_mul(8)).wrapping_add(0xfffffff8)); ld13 };
                // 00e68404: jmp 0xe68408
                pc = 0x00e68408;
                continue;
            }
            0x00e68406 => {
                // 00e68406: xor eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                eax = r;
                }
                pc = 0x00e68408;
                continue;
            }
            0x00e68408 => {
                // 00e68408: lea ecx, [ebp*8]
                ecx = ebp.wrapping_mul(8);
                // 00e6840f: push ecx
                { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68410: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68411: push ebx
                { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68412: call 0x12efea0
                { let (d, s, n) = (pg.u32(m, esp), pg.u32(m, esp.wrapping_add(4)), pg.u32(m, esp.wrapping_add(8))); copy_guest(m, pg, d, s, n); eax = d; }
                // 00e68417: mov eax, dword ptr [edi + 0x54]
                eax = { ld14 = pg.u32(m, edi.wrapping_add(0x54)); ld14 };
                // 00e6841a: mov ecx, dword ptr [esp + 0xd4]
                ecx = { ld15 = pg.u32(m, esp.wrapping_add(0xd4)); ld15 };
                // 00e68421: lea edx, [eax + eax*2]
                edx = eax.wrapping_add(eax.wrapping_mul(2));
                // 00e68424: mov dword ptr [esp + 0x44], ebx
                pg.set_u32(m, esp.wrapping_add(0x44), ebx);
                // 00e68428: mov dword ptr [esp + 0x48], ebp
                pg.set_u32(m, esp.wrapping_add(0x48), ebp);
                // 00e6842c: mov al, byte ptr [edi + edx*4]
                eax = (eax & !(0xffu32 << 0)) | ((({ ld16 = pg.u8(m, edi.wrapping_add(edx.wrapping_mul(4))) as u32; ld16 }) & 0xff) << 0);
                // 00e6842f: mov edx, dword ptr [esi]
                edx = { ld17 = pg.u32(m, esi); ld17 };
                // 00e68431: mov edx, dword ptr [edx + 0x3c]
                edx = { ld18 = pg.u32(m, edx.wrapping_add(0x3c)); ld18 };
                // 00e68434: add esp, 0xc
                { let (a, b) = (esp, 0xcu32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esp = r;
                }
                // 00e68437: mov byte ptr [esp + 0x40], al
                pg.set_u8(m, esp.wrapping_add(0x40), (((eax >> 0) & 0xff)) as u8);
                // 00e6843b: lea eax, [esp + 0x34]
                eax = esp.wrapping_add(0x34);
                // 00e6843f: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68440: push ecx
                { let v = ecx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68441: mov ecx, esi
                ecx = esi;
                // 00e68443: call edx
                let target: u32 = edx;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e68445);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e68445: mov ecx, dword ptr [0x187663c]
                ecx = { ld19 = pg.u32(m, 0x0187663cu32); ld19 };
                // 00e6844b: mov eax, dword ptr [ecx]
                eax = { ld20 = pg.u32(m, ecx); ld20 };
                // 00e6844d: mov edx, dword ptr [eax + 0x24]
                edx = { ld21 = pg.u32(m, eax.wrapping_add(0x24)); ld21 };
                // 00e68450: push ebx
                { let v = ebx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68451: call edx
                let target: u32 = edx;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e68453);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                pc = 0x00e68453;
                continue;
            }
            0x00e68453 => {
                // 00e68453: mov ecx, edi
                ecx = edi;
                // 00e68455: call 0x111d650
                let target: u32 = 0x0111d650u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e6845a);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e6845a: pop edi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); edi = v; }
                // 00e6845b: pop esi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); esi = v; }
                // 00e6845c: pop ebp
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
                // 00e6845d: pop ebx
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebx = v; }
                // 00e6845e: add esp, 0xb4
                { let (a, b) = (esp, 0xb4u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esp = r;
                }
                // 00e68464: ret 0xc
                esp = esp.wrapping_add(4 + 12);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                return;
            }
            0x00e68467 => {
                // 00e68467: mov ecx, dword ptr [esi + 0x30]
                ecx = { ld22 = pg.u32(m, esi.wrapping_add(0x30)); ld22 };
                // 00e6846a: mov edx, dword ptr [ecx + 0x54]
                edx = { ld23 = pg.u32(m, ecx.wrapping_add(0x54)); ld23 };
                // 00e6846d: lea edx, [edx + edx*2]
                edx = edx.wrapping_add(edx.wrapping_mul(2));
                // 00e68470: mov dl, byte ptr [ecx + edx*4]
                edx = (edx & !(0xffu32 << 0)) | ((({ ld24 = pg.u8(m, ecx.wrapping_add(edx.wrapping_mul(4))) as u32; ld24 }) & 0xff) << 0);
                // 00e68473: xor eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68475: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                // 00e68479: mov byte ptr [esp + 0x27], dl
                pg.set_u8(m, esp.wrapping_add(0x27), (((edx >> 0) & 0xff)) as u8);
                // 00e6847d: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e6847f: je 0xe68491
                if zf { pc = 0x00e68491; continue; }
                pc = 0x00e68481;
                continue;
            }
            0x00e68481 => {
                // 00e68481: mov ecx, dword ptr [ecx + 4]
                ecx = { ld25 = pg.u32(m, ecx.wrapping_add(0x4)); ld25 };
                // 00e68484: mov eax, dword ptr [ecx + 0x64]
                eax = { ld26 = pg.u32(m, ecx.wrapping_add(0x64)); ld26 };
                // 00e68487: add eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68489: mov eax, dword ptr [ecx + eax*8 - 4]
                eax = { ld27 = pg.u32(m, ecx.wrapping_add(eax.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld27 };
                // 00e6848d: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e68491;
                continue;
            }
            0x00e68491 => {
                // 00e68491: mov ecx, dword ptr [esi + 0x34]
                ecx = { ld28 = pg.u32(m, esi.wrapping_add(0x34)); ld28 };
                // 00e68494: lea edi, [esi + 0x34]
                edi = esi.wrapping_add(0x34);
                // 00e68497: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68499: je 0xe684ab
                if zf { pc = 0x00e684ab; continue; }
                pc = 0x00e6849b;
                continue;
            }
            0x00e6849b => {
                // 00e6849b: mov ecx, dword ptr [ecx + 4]
                ecx = { ld29 = pg.u32(m, ecx.wrapping_add(0x4)); ld29 };
                // 00e6849e: mov edx, dword ptr [ecx + 0x64]
                edx = { ld30 = pg.u32(m, ecx.wrapping_add(0x64)); ld30 };
                // 00e684a1: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e684a3: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld31 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld31 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e684a7: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e684ab;
                continue;
            }
            0x00e684ab => {
                // 00e684ab: mov ecx, dword ptr [esi + 0x38]
                ecx = { ld32 = pg.u32(m, esi.wrapping_add(0x38)); ld32 };
                // 00e684ae: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e684b0: je 0xe684c2
                if zf { pc = 0x00e684c2; continue; }
                pc = 0x00e684b2;
                continue;
            }
            0x00e684b2 => {
                // 00e684b2: mov ecx, dword ptr [ecx + 4]
                ecx = { ld33 = pg.u32(m, ecx.wrapping_add(0x4)); ld33 };
                // 00e684b5: mov edx, dword ptr [ecx + 0x64]
                edx = { ld34 = pg.u32(m, ecx.wrapping_add(0x64)); ld34 };
                // 00e684b8: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e684ba: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld35 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld35 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e684be: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e684c2;
                continue;
            }
            0x00e684c2 => {
                // 00e684c2: mov ecx, dword ptr [esi + 0x3c]
                ecx = { ld36 = pg.u32(m, esi.wrapping_add(0x3c)); ld36 };
                // 00e684c5: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e684c7: je 0xe684d9
                if zf { pc = 0x00e684d9; continue; }
                pc = 0x00e684c9;
                continue;
            }
            0x00e684c9 => {
                // 00e684c9: mov ecx, dword ptr [ecx + 4]
                ecx = { ld37 = pg.u32(m, ecx.wrapping_add(0x4)); ld37 };
                // 00e684cc: mov edx, dword ptr [ecx + 0x64]
                edx = { ld38 = pg.u32(m, ecx.wrapping_add(0x64)); ld38 };
                // 00e684cf: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e684d1: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld39 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld39 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e684d5: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e684d9;
                continue;
            }
            0x00e684d9 => {
                // 00e684d9: mov ecx, dword ptr [esi + 0x40]
                ecx = { ld40 = pg.u32(m, esi.wrapping_add(0x40)); ld40 };
                // 00e684dc: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e684de: je 0xe684f0
                if zf { pc = 0x00e684f0; continue; }
                pc = 0x00e684e0;
                continue;
            }
            0x00e684e0 => {
                // 00e684e0: mov ecx, dword ptr [ecx + 4]
                ecx = { ld41 = pg.u32(m, ecx.wrapping_add(0x4)); ld41 };
                // 00e684e3: mov edx, dword ptr [ecx + 0x64]
                edx = { ld42 = pg.u32(m, ecx.wrapping_add(0x64)); ld42 };
                // 00e684e6: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e684e8: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld43 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld43 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e684ec: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e684f0;
                continue;
            }
            0x00e684f0 => {
                // 00e684f0: mov ecx, dword ptr [esi + 0x44]
                ecx = { ld44 = pg.u32(m, esi.wrapping_add(0x44)); ld44 };
                // 00e684f3: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e684f5: je 0xe68507
                if zf { pc = 0x00e68507; continue; }
                pc = 0x00e684f7;
                continue;
            }
            0x00e684f7 => {
                // 00e684f7: mov ecx, dword ptr [ecx + 4]
                ecx = { ld45 = pg.u32(m, ecx.wrapping_add(0x4)); ld45 };
                // 00e684fa: mov edx, dword ptr [ecx + 0x64]
                edx = { ld46 = pg.u32(m, ecx.wrapping_add(0x64)); ld46 };
                // 00e684fd: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e684ff: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld47 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld47 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68503: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e68507;
                continue;
            }
            0x00e68507 => {
                // 00e68507: mov ecx, dword ptr [esi + 0x48]
                ecx = { ld48 = pg.u32(m, esi.wrapping_add(0x48)); ld48 };
                // 00e6850a: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e6850c: je 0xe6851e
                if zf { pc = 0x00e6851e; continue; }
                pc = 0x00e6850e;
                continue;
            }
            0x00e6850e => {
                // 00e6850e: mov ecx, dword ptr [ecx + 4]
                ecx = { ld49 = pg.u32(m, ecx.wrapping_add(0x4)); ld49 };
                // 00e68511: mov edx, dword ptr [ecx + 0x64]
                edx = { ld50 = pg.u32(m, ecx.wrapping_add(0x64)); ld50 };
                // 00e68514: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e68516: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld51 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld51 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e6851a: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e6851e;
                continue;
            }
            0x00e6851e => {
                // 00e6851e: mov ecx, dword ptr [esi + 0x4c]
                ecx = { ld52 = pg.u32(m, esi.wrapping_add(0x4c)); ld52 };
                // 00e68521: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68523: je 0xe68535
                if zf { pc = 0x00e68535; continue; }
                pc = 0x00e68525;
                continue;
            }
            0x00e68525 => {
                // 00e68525: mov ecx, dword ptr [ecx + 4]
                ecx = { ld53 = pg.u32(m, ecx.wrapping_add(0x4)); ld53 };
                // 00e68528: mov edx, dword ptr [ecx + 0x64]
                edx = { ld54 = pg.u32(m, ecx.wrapping_add(0x64)); ld54 };
                // 00e6852b: add edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e6852d: add eax, dword ptr [ecx + edx*8 - 4]
                { let (a, b) = (eax, { ld55 = pg.u32(m, ecx.wrapping_add(edx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld55 });
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68531: mov dword ptr [esp + 0x20], eax
                pg.set_u32(m, esp.wrapping_add(0x20), eax);
                pc = 0x00e68535;
                continue;
            }
            0x00e68535 => {
                // 00e68535: test eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68537: je 0xe689da
                if zf { pc = 0x00e689da; continue; }
                pc = 0x00e6853d;
                continue;
            }
            0x00e6853d => {
                // 00e6853d: mov ecx, dword ptr [0x187663c]
                ecx = { ld56 = pg.u32(m, 0x0187663cu32); ld56 };
                // 00e68543: mov edx, dword ptr [ecx]
                edx = { ld57 = pg.u32(m, ecx); ld57 };
                // 00e68545: mov edx, dword ptr [edx + 0x20]
                edx = { ld58 = pg.u32(m, edx.wrapping_add(0x20)); ld58 };
                // 00e68548: add eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e6854a: add eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e6854c: push 0x10
                { let v = 0x10u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e6854e: add eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68550: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68551: call edx
                let target: u32 = edx;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e68553);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e68553: mov dword ptr [esp + 0x2c], eax
                pg.set_u32(m, esp.wrapping_add(0x2c), eax);
                // 00e68557: mov ecx, eax
                ecx = eax;
                // 00e68559: mov dword ptr [esp + 0x14], 0
                pg.set_u32(m, esp.wrapping_add(0x14), 0x0u32);
                // 00e68561: lea eax, [esi + 0x58]
                eax = esi.wrapping_add(0x58);
                // 00e68564: mov dword ptr [esp + 0x30], 2
                pg.set_u32(m, esp.wrapping_add(0x30), 0x2u32);
                // 00e6856c: lea esp, [esp]
                esp = esp;
                pc = 0x00e68570;
                continue;
            }
            0x00e68570 => {
                // 00e68570: mov edx, dword ptr [edi - 4]
                edx = { ld59 = pg.u32(m, edi.wrapping_add(0xfffffffc)); ld59 };
                // 00e68573: xor ebx, ebx
                { let (a, b) = (ebx, ebx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                ebx = r;
                }
                // 00e68575: cmp edx, ebx
                { let (a, b) = (edx, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68577: je 0xe6858b
                if zf { pc = 0x00e6858b; continue; }
                pc = 0x00e68579;
                continue;
            }
            0x00e68579 => {
                // 00e68579: mov esi, dword ptr [edx + 4]
                esi = { ld60 = pg.u32(m, edx.wrapping_add(0x4)); ld60 };
                // 00e6857c: mov ebp, dword ptr [esi + 0x64]
                ebp = { ld61 = pg.u32(m, esi.wrapping_add(0x64)); ld61 };
                // 00e6857f: add ebp, ebp
                { let (a, b) = (ebp, ebp);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebp = r;
                }
                // 00e68581: mov esi, dword ptr [esi + ebp*8 - 4]
                esi = { ld62 = pg.u32(m, esi.wrapping_add(ebp.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld62 };
                // 00e68585: mov dword ptr [esp + 0x18], esi
                pg.set_u32(m, esp.wrapping_add(0x18), esi);
                // 00e68589: jmp 0xe6858f
                pc = 0x00e6858f;
                continue;
            }
            0x00e6858b => {
                // 00e6858b: mov dword ptr [esp + 0x18], ebx
                pg.set_u32(m, esp.wrapping_add(0x18), ebx);
                pc = 0x00e6858f;
                continue;
            }
            0x00e6858f => {
                // 00e6858f: cmp dword ptr [esp + 0x18], ebx
                { let (a, b) = ({ ld63 = pg.u32(m, esp.wrapping_add(0x18)); ld63 }, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68593: jbe 0xe685f9
                if (cf || zf) { pc = 0x00e685f9; continue; }
                pc = 0x00e68595;
                continue;
            }
            0x00e68595 => {
                // 00e68595: mov edx, dword ptr [edx + 4]
                edx = { ld64 = pg.u32(m, edx.wrapping_add(0x4)); ld64 };
                // 00e68598: mov esi, dword ptr [edx + 0x64]
                esi = { ld65 = pg.u32(m, edx.wrapping_add(0x64)); ld65 };
                // 00e6859b: add esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e6859d: cmp dword ptr [edx + esi*8 - 4], ebx
                { let (a, b) = ({ ld66 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld66 }, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e685a1: jbe 0xe685a9
                if (cf || zf) { pc = 0x00e685a9; continue; }
                pc = 0x00e685a3;
                continue;
            }
            0x00e685a3 => {
                // 00e685a3: mov edx, dword ptr [edx + esi*8 - 8]
                edx = { ld67 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffff8)); ld67 };
                // 00e685a7: jmp 0xe685ab
                pc = 0x00e685ab;
                continue;
            }
            0x00e685a9 => {
                // 00e685a9: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                pc = 0x00e685ab;
                continue;
            }
            0x00e685ab => {
                // 00e685ab: mov dword ptr [eax - 8], edx
                pg.set_u32(m, eax.wrapping_add(0xfffffff8), edx);
                // 00e685ae: mov edx, dword ptr [esp + 0x18]
                edx = { ld68 = pg.u32(m, esp.wrapping_add(0x18)); ld68 };
                // 00e685b2: mov dword ptr [eax + 4], edx
                pg.set_u32(m, eax.wrapping_add(0x4), edx);
                // 00e685b5: mov dword ptr [eax - 4], ebx
                pg.set_u32(m, eax.wrapping_add(0xfffffffc), ebx);
                // 00e685b8: mov dword ptr [eax], ebx
                pg.set_u32(m, eax, ebx);
                // 00e685ba: mov edx, dword ptr [edi - 4]
                edx = { ld69 = pg.u32(m, edi.wrapping_add(0xfffffffc)); ld69 };
                // 00e685bd: mov ebp, dword ptr [0x18d0b28]
                ebp = { ld70 = scheduler(m, pg); ld70 };
                // 00e685c3: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                // 00e685c7: mov edx, dword ptr [ebp + 0x8c]
                edx = { ld71 = pg.u32(m, ebp.wrapping_add(0x8c)); ld71 };
                // 00e685cd: lea esi, [edx + edx*2 + 0x30]
                esi = edx.wrapping_add(edx.wrapping_mul(2)).wrapping_add(0x30);
                // 00e685d1: inc edx
                { let a = edx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edx = r;
                }
                // 00e685d2: inc dword ptr [esp + 0x14]
                { let a = { ld72 = pg.u32(m, esp.wrapping_add(0x14)); ld72 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, esp.wrapping_add(0x14), r);
                }
                // 00e685d6: mov dword ptr [ebp + 0x8c], edx
                pg.set_u32(m, ebp.wrapping_add(0x8c), edx);
                // 00e685dc: mov edx, dword ptr [esp + 0x10]
                edx = { ld73 = pg.u32(m, esp.wrapping_add(0x10)); ld73 };
                // 00e685e0: lea esi, [ebp + esi*4]
                esi = ebp.wrapping_add(esi.wrapping_mul(4));
                // 00e685e4: mov dword ptr [esi], edx
                pg.set_u32(m, esi, edx);
                // 00e685e6: mov edx, dword ptr [esp + 0x18]
                edx = { ld74 = pg.u32(m, esp.wrapping_add(0x18)); ld74 };
                // 00e685ea: mov dword ptr [esi + 8], ecx
                pg.set_u32(m, esi.wrapping_add(0x8), ecx);
                // 00e685ed: mov dword ptr [esi + 4], 0x111e180
                pg.set_u32(m, esi.wrapping_add(0x4), 0x111e180u32);
                // 00e685f4: lea ecx, [ecx + edx*8]
                ecx = ecx.wrapping_add(edx.wrapping_mul(8));
                // 00e685f7: jmp 0xe685fc
                pc = 0x00e685fc;
                continue;
            }
            0x00e685f9 => {
                // 00e685f9: mov dword ptr [eax - 8], ebx
                pg.set_u32(m, eax.wrapping_add(0xfffffff8), ebx);
                pc = 0x00e685fc;
                continue;
            }
            0x00e685fc => {
                // 00e685fc: mov edx, dword ptr [edi]
                edx = { ld75 = pg.u32(m, edi); ld75 };
                // 00e685fe: cmp edx, ebx
                { let (a, b) = (edx, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68600: je 0xe68610
                if zf { pc = 0x00e68610; continue; }
                pc = 0x00e68602;
                continue;
            }
            0x00e68602 => {
                // 00e68602: mov esi, dword ptr [edx + 4]
                esi = { ld76 = pg.u32(m, edx.wrapping_add(0x4)); ld76 };
                // 00e68605: mov ebp, dword ptr [esi + 0x64]
                ebp = { ld77 = pg.u32(m, esi.wrapping_add(0x64)); ld77 };
                // 00e68608: add ebp, ebp
                { let (a, b) = (ebp, ebp);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebp = r;
                }
                // 00e6860a: mov esi, dword ptr [esi + ebp*8 - 4]
                esi = { ld78 = pg.u32(m, esi.wrapping_add(ebp.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld78 };
                // 00e6860e: jmp 0xe68612
                pc = 0x00e68612;
                continue;
            }
            0x00e68610 => {
                // 00e68610: xor esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                esi = r;
                }
                pc = 0x00e68612;
                continue;
            }
            0x00e68612 => {
                // 00e68612: mov dword ptr [esp + 0x18], esi
                pg.set_u32(m, esp.wrapping_add(0x18), esi);
                // 00e68616: cmp esi, ebx
                { let (a, b) = (esi, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68618: jbe 0xe6867e
                if (cf || zf) { pc = 0x00e6867e; continue; }
                pc = 0x00e6861a;
                continue;
            }
            0x00e6861a => {
                // 00e6861a: mov edx, dword ptr [edx + 4]
                edx = { ld79 = pg.u32(m, edx.wrapping_add(0x4)); ld79 };
                // 00e6861d: mov esi, dword ptr [edx + 0x64]
                esi = { ld80 = pg.u32(m, edx.wrapping_add(0x64)); ld80 };
                // 00e68620: add esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e68622: cmp dword ptr [edx + esi*8 - 4], ebx
                { let (a, b) = ({ ld81 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld81 }, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68626: jbe 0xe6862e
                if (cf || zf) { pc = 0x00e6862e; continue; }
                pc = 0x00e68628;
                continue;
            }
            0x00e68628 => {
                // 00e68628: mov edx, dword ptr [edx + esi*8 - 8]
                edx = { ld82 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffff8)); ld82 };
                // 00e6862c: jmp 0xe68630
                pc = 0x00e68630;
                continue;
            }
            0x00e6862e => {
                // 00e6862e: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                pc = 0x00e68630;
                continue;
            }
            0x00e68630 => {
                // 00e68630: mov dword ptr [eax + 8], edx
                pg.set_u32(m, eax.wrapping_add(0x8), edx);
                // 00e68633: mov edx, dword ptr [esp + 0x18]
                edx = { ld83 = pg.u32(m, esp.wrapping_add(0x18)); ld83 };
                // 00e68637: mov dword ptr [eax + 0x14], edx
                pg.set_u32(m, eax.wrapping_add(0x14), edx);
                // 00e6863a: mov dword ptr [eax + 0xc], ebx
                pg.set_u32(m, eax.wrapping_add(0xc), ebx);
                // 00e6863d: mov dword ptr [eax + 0x10], ebx
                pg.set_u32(m, eax.wrapping_add(0x10), ebx);
                // 00e68640: mov edx, dword ptr [edi]
                edx = { ld84 = pg.u32(m, edi); ld84 };
                // 00e68642: mov ebp, dword ptr [0x18d0b28]
                ebp = { ld85 = scheduler(m, pg); ld85 };
                // 00e68648: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                // 00e6864c: mov edx, dword ptr [ebp + 0x8c]
                edx = { ld86 = pg.u32(m, ebp.wrapping_add(0x8c)); ld86 };
                // 00e68652: lea esi, [edx + edx*2 + 0x30]
                esi = edx.wrapping_add(edx.wrapping_mul(2)).wrapping_add(0x30);
                // 00e68656: inc edx
                { let a = edx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edx = r;
                }
                // 00e68657: inc dword ptr [esp + 0x14]
                { let a = { ld87 = pg.u32(m, esp.wrapping_add(0x14)); ld87 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, esp.wrapping_add(0x14), r);
                }
                // 00e6865b: mov dword ptr [ebp + 0x8c], edx
                pg.set_u32(m, ebp.wrapping_add(0x8c), edx);
                // 00e68661: mov edx, dword ptr [esp + 0x10]
                edx = { ld88 = pg.u32(m, esp.wrapping_add(0x10)); ld88 };
                // 00e68665: lea esi, [ebp + esi*4]
                esi = ebp.wrapping_add(esi.wrapping_mul(4));
                // 00e68669: mov dword ptr [esi], edx
                pg.set_u32(m, esi, edx);
                // 00e6866b: mov edx, dword ptr [esp + 0x18]
                edx = { ld89 = pg.u32(m, esp.wrapping_add(0x18)); ld89 };
                // 00e6866f: mov dword ptr [esi + 4], 0x111e180
                pg.set_u32(m, esi.wrapping_add(0x4), 0x111e180u32);
                // 00e68676: mov dword ptr [esi + 8], ecx
                pg.set_u32(m, esi.wrapping_add(0x8), ecx);
                // 00e68679: lea ecx, [ecx + edx*8]
                ecx = ecx.wrapping_add(edx.wrapping_mul(8));
                // 00e6867c: jmp 0xe68681
                pc = 0x00e68681;
                continue;
            }
            0x00e6867e => {
                // 00e6867e: mov dword ptr [eax + 8], ebx
                pg.set_u32(m, eax.wrapping_add(0x8), ebx);
                pc = 0x00e68681;
                continue;
            }
            0x00e68681 => {
                // 00e68681: mov edx, dword ptr [edi + 4]
                edx = { ld90 = pg.u32(m, edi.wrapping_add(0x4)); ld90 };
                // 00e68684: cmp edx, ebx
                { let (a, b) = (edx, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68686: je 0xe68696
                if zf { pc = 0x00e68696; continue; }
                pc = 0x00e68688;
                continue;
            }
            0x00e68688 => {
                // 00e68688: mov esi, dword ptr [edx + 4]
                esi = { ld91 = pg.u32(m, edx.wrapping_add(0x4)); ld91 };
                // 00e6868b: mov ebp, dword ptr [esi + 0x64]
                ebp = { ld92 = pg.u32(m, esi.wrapping_add(0x64)); ld92 };
                // 00e6868e: add ebp, ebp
                { let (a, b) = (ebp, ebp);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebp = r;
                }
                // 00e68690: mov esi, dword ptr [esi + ebp*8 - 4]
                esi = { ld93 = pg.u32(m, esi.wrapping_add(ebp.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld93 };
                // 00e68694: jmp 0xe68698
                pc = 0x00e68698;
                continue;
            }
            0x00e68696 => {
                // 00e68696: xor esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                esi = r;
                }
                pc = 0x00e68698;
                continue;
            }
            0x00e68698 => {
                // 00e68698: mov dword ptr [esp + 0x18], esi
                pg.set_u32(m, esp.wrapping_add(0x18), esi);
                // 00e6869c: cmp esi, ebx
                { let (a, b) = (esi, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6869e: jbe 0xe68705
                if (cf || zf) { pc = 0x00e68705; continue; }
                pc = 0x00e686a0;
                continue;
            }
            0x00e686a0 => {
                // 00e686a0: mov edx, dword ptr [edx + 4]
                edx = { ld94 = pg.u32(m, edx.wrapping_add(0x4)); ld94 };
                // 00e686a3: mov esi, dword ptr [edx + 0x64]
                esi = { ld95 = pg.u32(m, edx.wrapping_add(0x64)); ld95 };
                // 00e686a6: add esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e686a8: cmp dword ptr [edx + esi*8 - 4], ebx
                { let (a, b) = ({ ld96 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld96 }, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e686ac: jbe 0xe686b4
                if (cf || zf) { pc = 0x00e686b4; continue; }
                pc = 0x00e686ae;
                continue;
            }
            0x00e686ae => {
                // 00e686ae: mov edx, dword ptr [edx + esi*8 - 8]
                edx = { ld97 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffff8)); ld97 };
                // 00e686b2: jmp 0xe686b6
                pc = 0x00e686b6;
                continue;
            }
            0x00e686b4 => {
                // 00e686b4: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                pc = 0x00e686b6;
                continue;
            }
            0x00e686b6 => {
                // 00e686b6: mov dword ptr [eax + 0x18], edx
                pg.set_u32(m, eax.wrapping_add(0x18), edx);
                // 00e686b9: mov edx, dword ptr [esp + 0x18]
                edx = { ld98 = pg.u32(m, esp.wrapping_add(0x18)); ld98 };
                // 00e686bd: mov dword ptr [eax + 0x24], edx
                pg.set_u32(m, eax.wrapping_add(0x24), edx);
                // 00e686c0: mov dword ptr [eax + 0x1c], ebx
                pg.set_u32(m, eax.wrapping_add(0x1c), ebx);
                // 00e686c3: mov dword ptr [eax + 0x20], ebx
                pg.set_u32(m, eax.wrapping_add(0x20), ebx);
                // 00e686c6: mov edx, dword ptr [edi + 4]
                edx = { ld99 = pg.u32(m, edi.wrapping_add(0x4)); ld99 };
                // 00e686c9: mov ebp, dword ptr [0x18d0b28]
                ebp = { ld100 = scheduler(m, pg); ld100 };
                // 00e686cf: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                // 00e686d3: mov edx, dword ptr [ebp + 0x8c]
                edx = { ld101 = pg.u32(m, ebp.wrapping_add(0x8c)); ld101 };
                // 00e686d9: lea esi, [edx + edx*2 + 0x30]
                esi = edx.wrapping_add(edx.wrapping_mul(2)).wrapping_add(0x30);
                // 00e686dd: inc edx
                { let a = edx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edx = r;
                }
                // 00e686de: inc dword ptr [esp + 0x14]
                { let a = { ld102 = pg.u32(m, esp.wrapping_add(0x14)); ld102 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, esp.wrapping_add(0x14), r);
                }
                // 00e686e2: mov dword ptr [ebp + 0x8c], edx
                pg.set_u32(m, ebp.wrapping_add(0x8c), edx);
                // 00e686e8: mov edx, dword ptr [esp + 0x10]
                edx = { ld103 = pg.u32(m, esp.wrapping_add(0x10)); ld103 };
                // 00e686ec: lea esi, [ebp + esi*4]
                esi = ebp.wrapping_add(esi.wrapping_mul(4));
                // 00e686f0: mov dword ptr [esi], edx
                pg.set_u32(m, esi, edx);
                // 00e686f2: mov edx, dword ptr [esp + 0x18]
                edx = { ld104 = pg.u32(m, esp.wrapping_add(0x18)); ld104 };
                // 00e686f6: mov dword ptr [esi + 4], 0x111e180
                pg.set_u32(m, esi.wrapping_add(0x4), 0x111e180u32);
                // 00e686fd: mov dword ptr [esi + 8], ecx
                pg.set_u32(m, esi.wrapping_add(0x8), ecx);
                // 00e68700: lea ecx, [ecx + edx*8]
                ecx = ecx.wrapping_add(edx.wrapping_mul(8));
                // 00e68703: jmp 0xe68708
                pc = 0x00e68708;
                continue;
            }
            0x00e68705 => {
                // 00e68705: mov dword ptr [eax + 0x18], ebx
                pg.set_u32(m, eax.wrapping_add(0x18), ebx);
                pc = 0x00e68708;
                continue;
            }
            0x00e68708 => {
                // 00e68708: mov edx, dword ptr [edi + 8]
                edx = { ld105 = pg.u32(m, edi.wrapping_add(0x8)); ld105 };
                // 00e6870b: cmp edx, ebx
                { let (a, b) = (edx, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6870d: je 0xe6871d
                if zf { pc = 0x00e6871d; continue; }
                pc = 0x00e6870f;
                continue;
            }
            0x00e6870f => {
                // 00e6870f: mov esi, dword ptr [edx + 4]
                esi = { ld106 = pg.u32(m, edx.wrapping_add(0x4)); ld106 };
                // 00e68712: mov ebx, dword ptr [esi + 0x64]
                ebx = { ld107 = pg.u32(m, esi.wrapping_add(0x64)); ld107 };
                // 00e68715: add ebx, ebx
                { let (a, b) = (ebx, ebx);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebx = r;
                }
                // 00e68717: mov ebx, dword ptr [esi + ebx*8 - 4]
                ebx = { ld108 = pg.u32(m, esi.wrapping_add(ebx.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld108 };
                // 00e6871b: jmp 0xe6871f
                pc = 0x00e6871f;
                continue;
            }
            0x00e6871d => {
                // 00e6871d: xor ebx, ebx
                { let (a, b) = (ebx, ebx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                ebx = r;
                }
                pc = 0x00e6871f;
                continue;
            }
            0x00e6871f => {
                // 00e6871f: test ebx, ebx
                { let (a, b) = (ebx, ebx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68721: je 0xe68783
                if zf { pc = 0x00e68783; continue; }
                pc = 0x00e68723;
                continue;
            }
            0x00e68723 => {
                // 00e68723: mov edx, dword ptr [edx + 4]
                edx = { ld109 = pg.u32(m, edx.wrapping_add(0x4)); ld109 };
                // 00e68726: mov esi, dword ptr [edx + 0x64]
                esi = { ld110 = pg.u32(m, edx.wrapping_add(0x64)); ld110 };
                // 00e68729: add esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e6872b: cmp dword ptr [edx + esi*8 - 4], 0
                { let (a, b) = ({ ld111 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffffc)); ld111 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68730: jbe 0xe68738
                if (cf || zf) { pc = 0x00e68738; continue; }
                pc = 0x00e68732;
                continue;
            }
            0x00e68732 => {
                // 00e68732: mov edx, dword ptr [edx + esi*8 - 8]
                edx = { ld112 = pg.u32(m, edx.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0xfffffff8)); ld112 };
                // 00e68736: jmp 0xe6873a
                pc = 0x00e6873a;
                continue;
            }
            0x00e68738 => {
                // 00e68738: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                pc = 0x00e6873a;
                continue;
            }
            0x00e6873a => {
                // 00e6873a: mov dword ptr [eax + 0x28], edx
                pg.set_u32(m, eax.wrapping_add(0x28), edx);
                // 00e6873d: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                // 00e6873f: mov dword ptr [eax + 0x2c], edx
                pg.set_u32(m, eax.wrapping_add(0x2c), edx);
                // 00e68742: mov dword ptr [eax + 0x30], edx
                pg.set_u32(m, eax.wrapping_add(0x30), edx);
                // 00e68745: mov dword ptr [eax + 0x34], ebx
                pg.set_u32(m, eax.wrapping_add(0x34), ebx);
                // 00e68748: mov edx, dword ptr [edi + 8]
                edx = { ld113 = pg.u32(m, edi.wrapping_add(0x8)); ld113 };
                // 00e6874b: mov ebp, dword ptr [0x18d0b28]
                ebp = { ld114 = scheduler(m, pg); ld114 };
                // 00e68751: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                // 00e68755: mov edx, dword ptr [ebp + 0x8c]
                edx = { ld115 = pg.u32(m, ebp.wrapping_add(0x8c)); ld115 };
                // 00e6875b: lea esi, [edx + edx*2 + 0x30]
                esi = edx.wrapping_add(edx.wrapping_mul(2)).wrapping_add(0x30);
                // 00e6875f: inc edx
                { let a = edx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edx = r;
                }
                // 00e68760: inc dword ptr [esp + 0x14]
                { let a = { ld116 = pg.u32(m, esp.wrapping_add(0x14)); ld116 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, esp.wrapping_add(0x14), r);
                }
                // 00e68764: mov dword ptr [ebp + 0x8c], edx
                pg.set_u32(m, ebp.wrapping_add(0x8c), edx);
                // 00e6876a: mov edx, dword ptr [esp + 0x10]
                edx = { ld117 = pg.u32(m, esp.wrapping_add(0x10)); ld117 };
                // 00e6876e: lea esi, [ebp + esi*4]
                esi = ebp.wrapping_add(esi.wrapping_mul(4));
                // 00e68772: mov dword ptr [esi], edx
                pg.set_u32(m, esi, edx);
                // 00e68774: mov dword ptr [esi + 4], 0x111e180
                pg.set_u32(m, esi.wrapping_add(0x4), 0x111e180u32);
                // 00e6877b: mov dword ptr [esi + 8], ecx
                pg.set_u32(m, esi.wrapping_add(0x8), ecx);
                // 00e6877e: lea ecx, [ecx + ebx*8]
                ecx = ecx.wrapping_add(ebx.wrapping_mul(8));
                // 00e68781: jmp 0xe6878a
                pc = 0x00e6878a;
                continue;
            }
            0x00e68783 => {
                // 00e68783: mov dword ptr [eax + 0x28], 0
                pg.set_u32(m, eax.wrapping_add(0x28), 0x0u32);
                pc = 0x00e6878a;
                continue;
            }
            0x00e6878a => {
                // 00e6878a: add edi, 0x10
                { let (a, b) = (edi, 0x10u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edi = r;
                }
                // 00e6878d: add eax, 0x40
                { let (a, b) = (eax, 0x40u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68790: dec dword ptr [esp + 0x30]
                { let a = { ld118 = pg.u32(m, esp.wrapping_add(0x30)); ld118 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, 1, 32);
                let r = a.wrapping_sub(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, esp.wrapping_add(0x30), r);
                }
                // 00e68794: jne 0xe68570
                if !zf { pc = 0x00e68570; continue; }
                pc = 0x00e6879a;
                continue;
            }
            0x00e6879a => {
                // 00e6879a: xor edi, edi
                { let (a, b) = (edi, edi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edi = r;
                }
                // 00e6879c: cmp dword ptr [esp + 0x14], edi
                { let (a, b) = ({ ld119 = pg.u32(m, esp.wrapping_add(0x14)); ld119 }, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e687a0: jbe 0xe687af
                if (cf || zf) { pc = 0x00e687af; continue; }
                pc = 0x00e687a2;
                continue;
            }
            0x00e687a2 => {
                // 00e687a2: mov ecx, dword ptr [0x18d0b28]
                ecx = { ld120 = scheduler(m, pg); ld120 };
                // 00e687a8: push 2
                { let v = 0x2u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e687aa: call 0xdbe560
                let target: u32 = 0x00dbe560u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e687af);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                pc = 0x00e687af;
                continue;
            }
            0x00e687af => {
                // 00e687af: mov eax, dword ptr [esp + 0x20]
                eax = { ld121 = pg.u32(m, esp.wrapping_add(0x20)); ld121 };
                // 00e687b3: mov dword ptr [esp + 0x10], edi
                pg.set_u32(m, esp.wrapping_add(0x10), edi);
                // 00e687b7: cmp eax, edi
                { let (a, b) = (eax, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e687b9: jbe 0xe688c1
                if (cf || zf) { pc = 0x00e688c1; continue; }
                pc = 0x00e687bf;
                continue;
            }
            0x00e687bf => {
                // 00e687bf: nop 
                pc = 0x00e687c0;
                continue;
            }
            0x00e687c0 => {
                // 00e687c0: mov ebx, dword ptr [esp + 0x28]
                ebx = { ld122 = pg.u32(m, esp.wrapping_add(0x28)); ld122 };
                // 00e687c4: xor eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                eax = r;
                }
                // 00e687c6: xor ebp, ebp
                { let (a, b) = (ebp, ebp);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                ebp = r;
                }
                // 00e687c8: or esi, 0xffffffff
                { let (a, b) = (esi, 0xffffffffu32);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a | b) & 0xffffffffu32, 32);
                let r = (a | b) & 0xffffffffu32;
                esi = r;
                }
                // 00e687cb: lea edx, [eax + 2]
                edx = eax.wrapping_add(0x2);
                // 00e687ce: add ebx, 0x58
                { let (a, b) = (ebx, 0x58u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebx = r;
                }
                pc = 0x00e687d1;
                continue;
            }
            0x00e687d1 => {
                // 00e687d1: mov ecx, dword ptr [ebx - 8]
                ecx = { ld123 = pg.u32(m, ebx.wrapping_add(0xfffffff8)); ld123 };
                // 00e687d4: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e687d6: je 0xe687f4
                if zf { pc = 0x00e687f4; continue; }
                pc = 0x00e687d8;
                continue;
            }
            0x00e687d8 => {
                // 00e687d8: mov edi, dword ptr [ebx]
                edi = { ld124 = pg.u32(m, ebx); ld124 };
                // 00e687da: cmp edi, dword ptr [ebx + 4]
                { let (a, b) = (edi, { ld125 = pg.u32(m, ebx.wrapping_add(0x4)); ld125 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e687dd: je 0xe687f4
                if zf { pc = 0x00e687f4; continue; }
                pc = 0x00e687df;
                continue;
            }
            0x00e687df => {
                // 00e687df: lea ecx, [ecx + edi*8]
                ecx = ecx.wrapping_add(edi.wrapping_mul(8));
                // 00e687e2: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e687e4: je 0xe687f4
                if zf { pc = 0x00e687f4; continue; }
                pc = 0x00e687e6;
                continue;
            }
            0x00e687e6 => {
                // 00e687e6: mov edi, dword ptr [ecx + 4]
                edi = { ld126 = pg.u32(m, ecx.wrapping_add(0x4)); ld126 };
                // 00e687e9: cmp edi, eax
                { let (a, b) = (edi, eax);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e687eb: jb 0xe687f4
                if cf { pc = 0x00e687f4; continue; }
                pc = 0x00e687ed;
                continue;
            }
            0x00e687ed => {
                // 00e687ed: lea esi, [edx - 2]
                esi = edx.wrapping_add(0xfffffffe);
                // 00e687f0: mov eax, edi
                eax = edi;
                // 00e687f2: mov ebp, ecx
                ebp = ecx;
                pc = 0x00e687f4;
                continue;
            }
            0x00e687f4 => {
                // 00e687f4: mov ecx, dword ptr [ebx + 8]
                ecx = { ld127 = pg.u32(m, ebx.wrapping_add(0x8)); ld127 };
                // 00e687f7: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e687f9: je 0xe68818
                if zf { pc = 0x00e68818; continue; }
                pc = 0x00e687fb;
                continue;
            }
            0x00e687fb => {
                // 00e687fb: mov edi, dword ptr [ebx + 0x10]
                edi = { ld128 = pg.u32(m, ebx.wrapping_add(0x10)); ld128 };
                // 00e687fe: cmp edi, dword ptr [ebx + 0x14]
                { let (a, b) = (edi, { ld129 = pg.u32(m, ebx.wrapping_add(0x14)); ld129 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68801: je 0xe68818
                if zf { pc = 0x00e68818; continue; }
                pc = 0x00e68803;
                continue;
            }
            0x00e68803 => {
                // 00e68803: lea ecx, [ecx + edi*8]
                ecx = ecx.wrapping_add(edi.wrapping_mul(8));
                // 00e68806: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68808: je 0xe68818
                if zf { pc = 0x00e68818; continue; }
                pc = 0x00e6880a;
                continue;
            }
            0x00e6880a => {
                // 00e6880a: mov edi, dword ptr [ecx + 4]
                edi = { ld130 = pg.u32(m, ecx.wrapping_add(0x4)); ld130 };
                // 00e6880d: cmp edi, eax
                { let (a, b) = (edi, eax);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6880f: jb 0xe68818
                if cf { pc = 0x00e68818; continue; }
                pc = 0x00e68811;
                continue;
            }
            0x00e68811 => {
                // 00e68811: lea esi, [edx - 1]
                esi = edx.wrapping_add(0xffffffff);
                // 00e68814: mov eax, edi
                eax = edi;
                // 00e68816: mov ebp, ecx
                ebp = ecx;
                pc = 0x00e68818;
                continue;
            }
            0x00e68818 => {
                // 00e68818: mov ecx, dword ptr [ebx + 0x18]
                ecx = { ld131 = pg.u32(m, ebx.wrapping_add(0x18)); ld131 };
                // 00e6881b: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e6881d: je 0xe6883b
                if zf { pc = 0x00e6883b; continue; }
                pc = 0x00e6881f;
                continue;
            }
            0x00e6881f => {
                // 00e6881f: mov edi, dword ptr [ebx + 0x20]
                edi = { ld132 = pg.u32(m, ebx.wrapping_add(0x20)); ld132 };
                // 00e68822: cmp edi, dword ptr [ebx + 0x24]
                { let (a, b) = (edi, { ld133 = pg.u32(m, ebx.wrapping_add(0x24)); ld133 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68825: je 0xe6883b
                if zf { pc = 0x00e6883b; continue; }
                pc = 0x00e68827;
                continue;
            }
            0x00e68827 => {
                // 00e68827: lea ecx, [ecx + edi*8]
                ecx = ecx.wrapping_add(edi.wrapping_mul(8));
                // 00e6882a: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e6882c: je 0xe6883b
                if zf { pc = 0x00e6883b; continue; }
                pc = 0x00e6882e;
                continue;
            }
            0x00e6882e => {
                // 00e6882e: mov edi, dword ptr [ecx + 4]
                edi = { ld134 = pg.u32(m, ecx.wrapping_add(0x4)); ld134 };
                // 00e68831: cmp edi, eax
                { let (a, b) = (edi, eax);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68833: jb 0xe6883b
                if cf { pc = 0x00e6883b; continue; }
                pc = 0x00e68835;
                continue;
            }
            0x00e68835 => {
                // 00e68835: mov esi, edx
                esi = edx;
                // 00e68837: mov eax, edi
                eax = edi;
                // 00e68839: mov ebp, ecx
                ebp = ecx;
                pc = 0x00e6883b;
                continue;
            }
            0x00e6883b => {
                // 00e6883b: mov ecx, dword ptr [ebx + 0x28]
                ecx = { ld135 = pg.u32(m, ebx.wrapping_add(0x28)); ld135 };
                // 00e6883e: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e68840: je 0xe6885f
                if zf { pc = 0x00e6885f; continue; }
                pc = 0x00e68842;
                continue;
            }
            0x00e68842 => {
                // 00e68842: mov edi, dword ptr [ebx + 0x30]
                edi = { ld136 = pg.u32(m, ebx.wrapping_add(0x30)); ld136 };
                // 00e68845: cmp edi, dword ptr [ebx + 0x34]
                { let (a, b) = (edi, { ld137 = pg.u32(m, ebx.wrapping_add(0x34)); ld137 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68848: je 0xe6885f
                if zf { pc = 0x00e6885f; continue; }
                pc = 0x00e6884a;
                continue;
            }
            0x00e6884a => {
                // 00e6884a: lea ecx, [ecx + edi*8]
                ecx = ecx.wrapping_add(edi.wrapping_mul(8));
                // 00e6884d: test ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e6884f: je 0xe6885f
                if zf { pc = 0x00e6885f; continue; }
                pc = 0x00e68851;
                continue;
            }
            0x00e68851 => {
                // 00e68851: mov edi, dword ptr [ecx + 4]
                edi = { ld138 = pg.u32(m, ecx.wrapping_add(0x4)); ld138 };
                // 00e68854: cmp edi, eax
                { let (a, b) = (edi, eax);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68856: jb 0xe6885f
                if cf { pc = 0x00e6885f; continue; }
                pc = 0x00e68858;
                continue;
            }
            0x00e68858 => {
                // 00e68858: lea esi, [edx + 1]
                esi = edx.wrapping_add(0x1);
                // 00e6885b: mov eax, edi
                eax = edi;
                // 00e6885d: mov ebp, ecx
                ebp = ecx;
                pc = 0x00e6885f;
                continue;
            }
            0x00e6885f => {
                // 00e6885f: add edx, 4
                { let (a, b) = (edx, 0x4u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                edx = r;
                }
                // 00e68862: lea ecx, [edx - 2]
                ecx = edx.wrapping_add(0xfffffffe);
                // 00e68865: add ebx, 0x40
                { let (a, b) = (ebx, 0x40u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebx = r;
                }
                // 00e68868: cmp ecx, 8
                { let (a, b) = (ecx, 0x8u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6886b: jb 0xe687d1
                if cf { pc = 0x00e687d1; continue; }
                pc = 0x00e68871;
                continue;
            }
            0x00e68871 => {
                // 00e68871: xor edi, edi
                { let (a, b) = (edi, edi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edi = r;
                }
                // 00e68873: cmp ebp, edi
                { let (a, b) = (ebp, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68875: je 0xe688a1
                if zf { pc = 0x00e688a1; continue; }
                pc = 0x00e68877;
                continue;
            }
            0x00e68877 => {
                // 00e68877: cmp esi, -1
                { let (a, b) = (esi, 0xffffffffu32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6887a: je 0xe688a1
                if zf { pc = 0x00e688a1; continue; }
                pc = 0x00e6887c;
                continue;
            }
            0x00e6887c => {
                // 00e6887c: mov edx, dword ptr [ebp]
                edx = { ld139 = pg.u32(m, ebp); ld139 };
                // 00e6887f: mov eax, dword ptr [esp + 0x2c]
                eax = { ld140 = pg.u32(m, esp.wrapping_add(0x2c)); ld140 };
                // 00e68883: mov ecx, dword ptr [esp + 0x10]
                ecx = { ld141 = pg.u32(m, esp.wrapping_add(0x10)); ld141 };
                // 00e68887: mov dword ptr [eax + ecx*8], edx
                pg.set_u32(m, eax.wrapping_add(ecx.wrapping_mul(8)), edx);
                // 00e6888a: mov edx, dword ptr [ebp + 4]
                edx = { ld142 = pg.u32(m, ebp.wrapping_add(0x4)); ld142 };
                // 00e6888d: mov dword ptr [eax + ecx*8 + 4], edx
                pg.set_u32(m, eax.wrapping_add(ecx.wrapping_mul(8)).wrapping_add(0x4), edx);
                // 00e68891: mov eax, dword ptr [esp + 0x28]
                eax = { ld143 = pg.u32(m, esp.wrapping_add(0x28)); ld143 };
                // 00e68895: add esi, esi
                { let (a, b) = (esi, esi);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e68897: inc dword ptr [eax + esi*8 + 0x58]
                { let a = { ld144 = pg.u32(m, eax.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0x58)); ld144 }; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                pg.set_u32(m, eax.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0x58), r);
                }
                // 00e6889b: lea eax, [eax + esi*8 + 0x58]
                eax = eax.wrapping_add(esi.wrapping_mul(8)).wrapping_add(0x58);
                // 00e6889f: jmp 0xe688b0
                pc = 0x00e688b0;
                continue;
            }
            0x00e688a1 => {
                // 00e688a1: mov eax, dword ptr [esp + 0x2c]
                eax = { ld145 = pg.u32(m, esp.wrapping_add(0x2c)); ld145 };
                // 00e688a5: mov ecx, dword ptr [esp + 0x10]
                ecx = { ld146 = pg.u32(m, esp.wrapping_add(0x10)); ld146 };
                // 00e688a9: mov dword ptr [eax + ecx*8], edi
                pg.set_u32(m, eax.wrapping_add(ecx.wrapping_mul(8)), edi);
                // 00e688ac: mov dword ptr [eax + ecx*8 + 4], edi
                pg.set_u32(m, eax.wrapping_add(ecx.wrapping_mul(8)).wrapping_add(0x4), edi);
                pc = 0x00e688b0;
                continue;
            }
            0x00e688b0 => {
                // 00e688b0: mov eax, dword ptr [esp + 0x20]
                eax = { ld147 = pg.u32(m, esp.wrapping_add(0x20)); ld147 };
                // 00e688b4: inc ecx
                { let a = ecx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                ecx = r;
                }
                // 00e688b5: mov dword ptr [esp + 0x10], ecx
                pg.set_u32(m, esp.wrapping_add(0x10), ecx);
                // 00e688b9: cmp ecx, eax
                { let (a, b) = (ecx, eax);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e688bb: jb 0xe687c0
                if cf { pc = 0x00e687c0; continue; }
                pc = 0x00e688c1;
                continue;
            }
            0x00e688c1 => {
                // 00e688c1: mov ebx, dword ptr [esp + 0xd0]
                ebx = { ld148 = pg.u32(m, esp.wrapping_add(0xd0)); ld148 };
                // 00e688c8: xor edx, edx
                { let (a, b) = (edx, edx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edx = r;
                }
                // 00e688ca: mov dword ptr [esp + 0x14], eax
                pg.set_u32(m, esp.wrapping_add(0x14), eax);
                // 00e688ce: div ebx
                { let d = ebx as u64; assert!(d != 0, "divide error"); let n = ((edx as u64) << 32) | eax as u64; eax = (n / d) as u32; edx = (n % d) as u32; }
                // 00e688d0: mov ebp, dword ptr [esp + 0x28]
                ebp = { ld149 = pg.u32(m, esp.wrapping_add(0x28)); ld149 };
                // 00e688d4: mov cl, byte ptr [ebp + 0x1b0]
                ecx = (ecx & !(0xffu32 << 0)) | ((({ ld150 = pg.u8(m, ebp.wrapping_add(0x1b0)) as u32; ld150 }) & 0xff) << 0);
                // 00e688da: mov edx, dword ptr [esp + 0x2c]
                edx = { ld151 = pg.u32(m, esp.wrapping_add(0x2c)); ld151 };
                // 00e688de: mov byte ptr [esp + 0x1f], cl
                pg.set_u8(m, esp.wrapping_add(0x1f), (((ecx >> 0) & 0xff)) as u8);
                // 00e688e2: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                // 00e688e6: mov dword ptr [esp + 0x30], eax
                pg.set_u32(m, esp.wrapping_add(0x30), eax);
                // 00e688ea: test cl, cl
                { let (a, b) = (((ecx >> 0) & 0xff), ((ecx >> 0) & 0xff));
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
                }
                // 00e688ec: je 0xe68919
                if zf { pc = 0x00e68919; continue; }
                pc = 0x00e688ee;
                continue;
            }
            0x00e688ee => {
                // 00e688ee: cmp ebx, edi
                { let (a, b) = (ebx, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e688f0: jbe 0xe689b0
                if (cf || zf) { pc = 0x00e689b0; continue; }
                pc = 0x00e688f6;
                continue;
            }
            0x00e688f6 => {
                // 00e688f6: mov eax, dword ptr [esp + 0xcc]
                eax = { ld152 = pg.u32(m, esp.wrapping_add(0xcc)); ld152 };
                // 00e688fd: xor ecx, ecx
                { let (a, b) = (ecx, ecx);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                ecx = r;
                }
                // 00e688ff: mov edx, ebx
                edx = ebx;
                pc = 0x00e68901;
                continue;
            }
            0x00e68901 => {
                // 00e68901: mov esi, dword ptr [0x18d0b28]
                esi = { ld153 = scheduler(m, pg); ld153 };
                // 00e68907: mov dword ptr [ecx + esi + 0x1bdd0], eax
                pg.set_u32(m, ecx.wrapping_add(esi).wrapping_add(0x1bdd0), eax);
                // 00e6890e: add eax, 0x33c84
                { let (a, b) = (eax, 0x33c84u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68913: add ecx, 0x20
                { let (a, b) = (ecx, 0x20u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ecx = r;
                }
                // 00e68916: dec edx
                { let a = edx; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, 1, 32);
                let r = a.wrapping_sub(1) & 0xffffffffu32;
                cf = old_cf;
                edx = r;
                }
                // 00e68917: jne 0xe68901
                if !zf { pc = 0x00e68901; continue; }
                pc = 0x00e68919;
                continue;
            }
            0x00e68919 => {
                // 00e68919: cmp ebx, edi
                { let (a, b) = (ebx, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6891b: jbe 0xe689b0
                if (cf || zf) { pc = 0x00e689b0; continue; }
                pc = 0x00e68921;
                continue;
            }
            0x00e68921 => {
                // 00e68921: lea esi, [esp + 0x4c]
                esi = esp.wrapping_add(0x4c);
                pc = 0x00e68925;
                continue;
            }
            0x00e68925 => {
                // 00e68925: lea ecx, [ebx - 1]
                ecx = ebx.wrapping_add(0xffffffff);
                // 00e68928: lea eax, [esi - 8]
                eax = esi.wrapping_add(0xfffffff8);
                // 00e6892b: mov dword ptr [eax], edi
                pg.set_u32(m, eax, edi);
                // 00e6892d: cmp edi, ecx
                { let (a, b) = (edi, ecx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e6892f: jne 0xe68940
                if !zf { pc = 0x00e68940; continue; }
                pc = 0x00e68931;
                continue;
            }
            0x00e68931 => {
                // 00e68931: mov ecx, dword ptr [esp + 0x10]
                ecx = { ld154 = pg.u32(m, esp.wrapping_add(0x10)); ld154 };
                // 00e68935: mov edx, dword ptr [esp + 0x14]
                edx = { ld155 = pg.u32(m, esp.wrapping_add(0x14)); ld155 };
                // 00e68939: mov dword ptr [esi - 4], ecx
                pg.set_u32(m, esi.wrapping_add(0xfffffffc), ecx);
                // 00e6893c: mov dword ptr [esi], edx
                pg.set_u32(m, esi, edx);
                // 00e6893e: jmp 0xe68958
                pc = 0x00e68958;
                continue;
            }
            0x00e68940 => {
                // 00e68940: mov edx, dword ptr [esp + 0x10]
                edx = { ld156 = pg.u32(m, esp.wrapping_add(0x10)); ld156 };
                // 00e68944: mov ecx, dword ptr [esp + 0x30]
                ecx = { ld157 = pg.u32(m, esp.wrapping_add(0x30)); ld157 };
                // 00e68948: sub dword ptr [esp + 0x14], ecx
                { let (a, b) = ({ ld158 = pg.u32(m, esp.wrapping_add(0x14)); ld158 }, ecx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                let r = a.wrapping_sub(b) & 0xffffffffu32;
                pg.set_u32(m, esp.wrapping_add(0x14), r);
                }
                // 00e6894c: mov dword ptr [esi - 4], edx
                pg.set_u32(m, esi.wrapping_add(0xfffffffc), edx);
                // 00e6894f: lea edx, [edx + ecx*8]
                edx = edx.wrapping_add(ecx.wrapping_mul(8));
                // 00e68952: mov dword ptr [esi], ecx
                pg.set_u32(m, esi, ecx);
                // 00e68954: mov dword ptr [esp + 0x10], edx
                pg.set_u32(m, esp.wrapping_add(0x10), edx);
                pc = 0x00e68958;
                continue;
            }
            0x00e68958 => {
                // 00e68958: cmp byte ptr [esp + 0x1f], 0
                { let (a, b) = ({ ld159 = pg.u8(m, esp.wrapping_add(0x1f)) as u32; ld159 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 00e6895d: mov cl, byte ptr [esp + 0x27]
                ecx = (ecx & !(0xffu32 << 0)) | ((({ ld160 = pg.u8(m, esp.wrapping_add(0x27)) as u32; ld160 }) & 0xff) << 0);
                // 00e68961: mov byte ptr [esi + 4], cl
                pg.set_u8(m, esi.wrapping_add(0x4), (((ecx >> 0) & 0xff)) as u8);
                // 00e68964: je 0xe68991
                if zf { pc = 0x00e68991; continue; }
                pc = 0x00e68966;
                continue;
            }
            0x00e68966 => {
                // 00e68966: mov edx, dword ptr [0x18d0b28]
                edx = { ld161 = scheduler(m, pg); ld161 };
                // 00e6896c: mov eax, dword ptr [edx + 0x8c]
                eax = { ld162 = pg.u32(m, edx.wrapping_add(0x8c)); ld162 };
                // 00e68972: lea ecx, [eax + eax*2 + 0x30]
                ecx = eax.wrapping_add(eax.wrapping_mul(2)).wrapping_add(0x30);
                // 00e68976: lea ecx, [edx + ecx*4]
                ecx = edx.wrapping_add(ecx.wrapping_mul(4));
                // 00e68979: inc eax
                { let a = eax; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                eax = r;
                }
                // 00e6897a: mov dword ptr [edx + 0x8c], eax
                pg.set_u32(m, edx.wrapping_add(0x8c), eax);
                // 00e68980: lea edx, [esi - 8]
                edx = esi.wrapping_add(0xfffffff8);
                // 00e68983: mov dword ptr [ecx], ebp
                pg.set_u32(m, ecx, ebp);
                // 00e68985: mov dword ptr [ecx + 4], 0x10b7440
                pg.set_u32(m, ecx.wrapping_add(0x4), 0x10b7440u32);
                // 00e6898c: mov dword ptr [ecx + 8], edx
                pg.set_u32(m, ecx.wrapping_add(0x8), edx);
                // 00e6898f: jmp 0xe689a4
                pc = 0x00e689a4;
                continue;
            }
            0x00e68991 => {
                // 00e68991: mov edx, dword ptr [ebp]
                edx = { ld163 = pg.u32(m, ebp); ld163 };
                // 00e68994: mov edx, dword ptr [edx + 0x3c]
                edx = { ld164 = pg.u32(m, edx.wrapping_add(0x3c)); ld164 };
                // 00e68997: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e68998: mov eax, dword ptr [esp + 0xcc]
                eax = { ld165 = pg.u32(m, esp.wrapping_add(0xcc)); ld165 };
                // 00e6899f: push eax
                { let v = eax; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e689a0: mov ecx, ebp
                ecx = ebp;
                // 00e689a2: call edx
                let target: u32 = edx;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e689a4);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                pc = 0x00e689a4;
                continue;
            }
            0x00e689a4 => {
                // 00e689a4: inc edi
                { let a = edi; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edi = r;
                }
                // 00e689a5: add esi, 0x10
                { let (a, b) = (esi, 0x10u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esi = r;
                }
                // 00e689a8: cmp edi, ebx
                { let (a, b) = (edi, ebx);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e689aa: jb 0xe68925
                if cf { pc = 0x00e68925; continue; }
                pc = 0x00e689b0;
                continue;
            }
            0x00e689b0 => {
                // 00e689b0: cmp byte ptr [esp + 0x1f], 0
                { let (a, b) = ({ ld166 = pg.u8(m, esp.wrapping_add(0x1f)) as u32; ld166 }, 0x0u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 00e689b5: je 0xe689c4
                if zf { pc = 0x00e689c4; continue; }
                pc = 0x00e689b7;
                continue;
            }
            0x00e689b7 => {
                // 00e689b7: mov ecx, dword ptr [0x18d0b28]
                ecx = { ld167 = scheduler(m, pg); ld167 };
                // 00e689bd: push 3
                { let v = 0x3u32; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e689bf: call 0xdbe560
                let target: u32 = 0x00dbe560u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e689c4);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                pc = 0x00e689c4;
                continue;
            }
            0x00e689c4 => {
                // 00e689c4: mov ecx, dword ptr [0x187663c]
                ecx = { ld168 = pg.u32(m, 0x0187663cu32); ld168 };
                // 00e689ca: mov eax, dword ptr [ecx]
                eax = { ld169 = pg.u32(m, ecx); ld169 };
                // 00e689cc: mov edx, dword ptr [esp + 0x2c]
                edx = { ld170 = pg.u32(m, esp.wrapping_add(0x2c)); ld170 };
                // 00e689d0: mov eax, dword ptr [eax + 0x24]
                eax = { ld171 = pg.u32(m, eax.wrapping_add(0x24)); ld171 };
                // 00e689d3: push edx
                { let v = edx; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }
                // 00e689d4: call eax
                let target: u32 = eax;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e689d6);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e689d6: mov esi, dword ptr [esp + 0x28]
                esi = { ld172 = pg.u32(m, esp.wrapping_add(0x28)); ld172 };
                pc = 0x00e689da;
                continue;
            }
            0x00e689da => {
                // 00e689da: xor edi, edi
                { let (a, b) = (edi, edi);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                edi = r;
                }
                // 00e689dc: mov bl, 1
                ebx = (ebx & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
                // 00e689de: cmp dword ptr [esi + 0xd0], edi
                { let (a, b) = ({ ld173 = pg.u32(m, esi.wrapping_add(0xd0)); ld173 }, edi);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e689e4: jbe 0xe68a12
                if (cf || zf) { pc = 0x00e68a12; continue; }
                pc = 0x00e689e6;
                continue;
            }
            0x00e689e6 => {
                // 00e689e6: lea ebp, [esi + 0x30]
                ebp = esi.wrapping_add(0x30);
                // 00e689e9: lea esp, [esp]
                esp = esp;
                pc = 0x00e689f0;
                continue;
            }
            0x00e689f0 => {
                // 00e689f0: mov ecx, dword ptr [ebp]
                ecx = { ld174 = pg.u32(m, ebp); ld174 };
                // 00e689f3: call 0x111d650
                let target: u32 = 0x0111d650u32;
                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x00e689f8);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                call_ext(m, c, target);
                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);
                xmm = c.xmm.map(|v| v.map(f32::from_bits));
                // 00e689f8: test bl, bl
                { let (a, b) = (((ebx >> 0) & 0xff), ((ebx >> 0) & 0xff));
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffu32, 8);
                }
                // 00e689fa: je 0xe68a04
                if zf { pc = 0x00e68a04; continue; }
                pc = 0x00e689fc;
                continue;
            }
            0x00e689fc => {
                // 00e689fc: test eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a & b) & 0xffffffffu32, 32);
                }
                // 00e689fe: jne 0xe68a04
                if !zf { pc = 0x00e68a04; continue; }
                pc = 0x00e68a00;
                continue;
            }
            0x00e68a00 => {
                // 00e68a00: mov bl, 1
                ebx = (ebx & !(0xffu32 << 0)) | (((0x1u32) & 0xff) << 0);
                // 00e68a02: jmp 0xe68a06
                pc = 0x00e68a06;
                continue;
            }
            0x00e68a04 => {
                // 00e68a04: xor bl, bl
                { let (a, b) = (((ebx >> 0) & 0xff), ((ebx >> 0) & 0xff));
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffu32, 8);
                let r = (a ^ b) & 0xffu32;
                ebx = (ebx & !(0xffu32 << 0)) | (((r) & 0xff) << 0);
                }
                pc = 0x00e68a06;
                continue;
            }
            0x00e68a06 => {
                // 00e68a06: inc edi
                { let a = edi; let old_cf = cf;
                (cf, zf, sf, of_, pf) = rt::add_flags(a, 1, 32);
                let r = a.wrapping_add(1) & 0xffffffffu32;
                cf = old_cf;
                edi = r;
                }
                // 00e68a07: add ebp, 4
                { let (a, b) = (ebp, 0x4u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                ebp = r;
                }
                // 00e68a0a: cmp edi, dword ptr [esi + 0xd0]
                { let (a, b) = (edi, { ld175 = pg.u32(m, esi.wrapping_add(0xd0)); ld175 });
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 32);
                }
                // 00e68a10: jb 0xe689f0
                if cf { pc = 0x00e689f0; continue; }
                pc = 0x00e68a12;
                continue;
            }
            0x00e68a12 => {
                // 00e68a12: pop edi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); edi = v; }
                // 00e68a13: pop esi
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); esi = v; }
                // 00e68a14: xor eax, eax
                { let (a, b) = (eax, eax);
                (cf, zf, sf, of_, pf) = rt::logic_flags((a ^ b) & 0xffffffffu32, 32);
                let r = (a ^ b) & 0xffffffffu32;
                eax = r;
                }
                // 00e68a16: cmp bl, 1
                { let (a, b) = (((ebx >> 0) & 0xff), 0x1u32);
                (cf, zf, sf, of_, pf) = rt::sub_flags(a, b, 8);
                }
                // 00e68a19: pop ebp
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebp = v; }
                // 00e68a1a: setne al
                eax = (eax & !(0xffu32 << 0)) | ((((!zf) as u32) & 0xff) << 0);
                // 00e68a1d: pop ebx
                { let v = pg.u32(m, esp); esp = esp.wrapping_add(4); ebx = v; }
                // 00e68a1e: add esp, 0xb4
                { let (a, b) = (esp, 0xb4u32);
                (cf, zf, sf, of_, pf) = rt::add_flags(a, b, 32);
                let r = a.wrapping_add(b) & 0xffffffffu32;
                esp = r;
                }
                // 00e68a24: ret 0xc
                esp = esp.wrapping_add(4 + 12);
                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);
                c.xmm = xmm.map(|v| v.map(f32::to_bits));
                return;
            }
            _ => unreachable!("pc {pc:#x}"),
        }
    }
}

/// 0x0111E180, `pool->sort_pairs(scratch)` (thiscall, `ret 4`): the kind-2
/// sort job of the sPrimitive pools. Sort the pool's current list (a count
/// and an array of 8-byte records in its descriptor `[pool + 4]`, at
/// `desc + depth * 16 - 4 / - 8`) by the unsigned second dword, largest
/// first, stable, with a bottom-up merge sort of doubling run widths
/// through `scratch` (merge pass 0x0111D490). EAX returns the count.
///
/// The merge pass copies each pair of runs into `scratch` (same indices) and
/// merges back; the last pass spans the whole list, so `scratch` ends up
/// holding the list as it was before that pass.
// Translated from 0x0111E180 and 0x0111D490 (DDDA.exe 2017-12-13 Steam).
pub fn sprimitive_sort(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let pool = c.ecx;
    let scratch = pg.u32(m, entry.wrapping_add(4));
    c.esp = entry.wrapping_add(8);
    let desc = pg.u32(m, pool.wrapping_add(4));
    let row = desc.wrapping_add(pg.u32(m, desc.wrapping_add(0x64)).wrapping_mul(16));
    let count = pg.u32(m, row.wrapping_sub(4));
    let array = if count != 0 { pg.u32(m, row.wrapping_sub(8)) } else { 0 };
    c.eax = count;
    let n = count as i32;
    if n <= 1 {
        return;
    }
    let n = n as usize;
    let mut list: Vec<[u32; 2]> = (0..n as u32)
        .map(|i| {
            let at = array.wrapping_add(8 * i);
            [pg.u32(m, at), pg.u32(m, at.wrapping_add(4))]
        })
        .collect();
    let mut before_last = Vec::new();
    let mut run = vec![[0u32; 2]; n];
    let mut width = 1usize;
    while width < n {
        before_last.clone_from(&list);
        let mut lo = 0usize;
        while lo + width < n {
            let hi = (lo + 2 * width).min(n);
            let mid = lo + width;
            run[lo..hi].copy_from_slice(&list[lo..hi]);
            let (mut left, mut right, mut out) = (lo, mid, lo);
            while left < mid && right < hi {
                // the right record goes first only when its key is larger
                if run[right][1] <= run[left][1] {
                    list[out] = run[left];
                    left += 1;
                } else {
                    list[out] = run[right];
                    right += 1;
                }
                out += 1;
            }
            let rest = if left < mid { &run[left..mid] } else { &run[right..hi] };
            list[out..out + rest.len()].copy_from_slice(rest);
            lo += 2 * width;
        }
        width *= 2;
    }
    for (i, record) in list.iter().enumerate() {
        let at = array.wrapping_add(8 * i as u32);
        pg.set_u32(m, at, record[0]);
        pg.set_u32(m, at.wrapping_add(4), record[1]);
    }
    for (i, record) in before_last.iter().enumerate() {
        let at = scratch.wrapping_add(8 * i as u32);
        pg.set_u32(m, at, record[0]);
        pg.set_u32(m, at.wrapping_add(4), record[1]);
    }
}

/// 0x01192260, `owner->push_snapshot() -> error` (thiscall): save the two
/// words at `+0x74` / `+0x78` as a 16-byte record from `+4` on the owner's
/// stack (depth `+0x64`, at most six) and count it. Returns 9 when full.
// Translated from 0x01192260 (DDDA.exe 2017-12-13 Steam).
pub fn owner_push_snapshot(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    c.eax = push_snapshot(m, pg, c.ecx);
    c.esp = entry.wrapping_add(4);
}

fn push_snapshot(m: &mut dyn Mem, pg: Pages, owner: u32) -> u32 {
    let depth = pg.u32(m, owner.wrapping_add(0x64));
    if depth >= 6 {
        return 9;
    }
    let record = owner.wrapping_add(depth.wrapping_mul(16)).wrapping_add(4);
    let first = pg.u32(m, owner.wrapping_add(0x74));
    pg.set_u32(m, record, first);
    let second = pg.u32(m, owner.wrapping_add(0x78));
    pg.set_u32(m, record.wrapping_add(4), second);
    pg.set_u32(m, record.wrapping_add(8), 0);
    pg.set_u8(m, record.wrapping_add(0xc), 0);
    let again = pg.u32(m, owner.wrapping_add(0x64)).wrapping_add(1);
    pg.set_u32(m, owner.wrapping_add(0x64), again);
    0
}

/// 0x01192290, `owner->pop_snapshot() -> error` (thiscall): restore the
/// top record into `+0x74` / `+0x78` and subtract its third word from
/// `+0x80`. Returns 10 when empty.
// Translated from 0x01192290 (DDDA.exe 2017-12-13 Steam).
pub fn owner_pop_snapshot(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    c.eax = pop_snapshot(m, pg, c.ecx);
    c.esp = entry.wrapping_add(4);
}

fn pop_snapshot(m: &mut dyn Mem, pg: Pages, owner: u32) -> u32 {
    let depth = pg.u32(m, owner.wrapping_add(0x64));
    if depth == 0 {
        return 0xa;
    }
    let depth = depth - 1;
    pg.set_u32(m, owner.wrapping_add(0x64), depth);
    let record = owner.wrapping_add(depth.wrapping_mul(16)).wrapping_add(4);
    let first = pg.u32(m, record);
    pg.set_u32(m, owner.wrapping_add(0x74), first);
    let second = pg.u32(m, record.wrapping_add(4));
    pg.set_u32(m, owner.wrapping_add(0x78), second);
    let amount = pg.u32(m, record.wrapping_add(8));
    let total = pg.u32(m, owner.wrapping_add(0x80)).wrapping_sub(amount);
    pg.set_u32(m, owner.wrapping_add(0x80), total);
    0
}

/// 0x0111D610, `pool->push_state(value) -> error` (thiscall, `ret 4`): open
/// a nested list: snapshot the owner (`[pool + 4]`, 0x01192260, its error is
/// ignored) and push a 12-byte record (the value byte, two zero words) from
/// `pool + 0xC` on the pool's stack (depth `+0x54`, at most six). Returns 9
/// when the pool's stack is full.
// Translated from 0x0111D610 (DDDA.exe 2017-12-13 Steam).
pub fn pool_push_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let pool = c.ecx;
    let depth = pg.u32(m, pool.wrapping_add(0x54));
    c.eax = if depth >= 6 {
        9
    } else {
        let owner = pg.u32(m, pool.wrapping_add(4));
        push_snapshot(m, pg, owner);
        let value = pg.u8(m, entry.wrapping_add(4));
        let slot = pool.wrapping_add(depth.wrapping_mul(12).wrapping_add(12));
        pg.set_u32(m, slot.wrapping_add(4), 0);
        pg.set_u32(m, slot.wrapping_add(8), 0);
        pg.set_u8(m, slot, value);
        let again = pg.u32(m, pool.wrapping_add(0x54)).wrapping_add(1);
        pg.set_u32(m, pool.wrapping_add(0x54), again);
        0
    };
    c.esp = entry.wrapping_add(8);
}

/// 0x0111D650, `pool->pop_state() -> error` (thiscall): close the nested
/// list: restore the owner's snapshot (0x01192290, error ignored) and drop
/// the pool's top record. Returns 10 when the pool's stack is empty.
// Translated from 0x0111D650 (DDDA.exe 2017-12-13 Steam).
pub fn pool_pop_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let pool = c.ecx;
    c.eax = if pg.u32(m, pool.wrapping_add(0x54)) == 0 {
        0xa
    } else {
        let owner = pg.u32(m, pool.wrapping_add(4));
        pop_snapshot(m, pg, owner);
        let again = pg.u32(m, pool.wrapping_add(0x54)).wrapping_sub(1);
        pg.set_u32(m, pool.wrapping_add(0x54), again);
        0
    };
    c.esp = entry.wrapping_add(4);
}

/// 0x010B7440, the kind-3 draw job target (thiscall, tail thunk): jump to
/// the unit's virtual +0x3C with the caller's arguments and return address
/// untouched (`mov eax, [ecx]; jmp [eax + 0x3c]`).
// Translated from 0x010B7440 (DDDA.exe 2017-12-13 Steam).
pub fn draw_job_thunk(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let target = pg.u32(m, pg.u32(m, c.ecx).wrapping_add(0x3c));
    call_ext(m, c, target);
}

/// 0x0107FF00, `sPrimitive::begin(a0, a1, a2, a3)` (cdecl, caller pops):
/// forward the four stack arguments to virtual +0x2C of the singleton at
/// `0x018D1848`; the known vf11 is called directly.
// Translated from 0x0107FF00 (DDDA.exe 2017-12-13 Steam).
pub fn dispatch_begin(m: &mut dyn Mem, c: &mut Cpu) {
    dispatch(m, c, 4, 0x2c, 0x00e6_1010, 0x0107_ff21, sprimitive_vf11)
}

/// 0x0107FF30, `sPrimitive::end(a0, a1, a2)` (cdecl, caller pops): forward
/// the three stack arguments to virtual +0x30 of the singleton; the known
/// vf12 is called directly.
// Translated from 0x0107FF30 (DDDA.exe 2017-12-13 Steam).
pub fn dispatch_end(m: &mut dyn Mem, c: &mut Cpu) {
    dispatch(m, c, 3, 0x30, 0x00e6_8390, 0x0107_ff4c, sprimitive_vf12)
}

fn dispatch(m: &mut dyn Mem, c: &mut Cpu, count: u32, slot: u32, known: u32, back: u32, native: fn(&mut dyn Mem, &mut Cpu)) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let singleton = pg.u32(m, 0x018d_1848);
    let target = pg.u32(m, pg.u32(m, singleton).wrapping_add(slot));
    // push the arguments again, last first (they keep their order)
    let mut esp = entry;
    for k in (0..count).rev() {
        let v = pg.u32(m, entry.wrapping_add(4 + 4 * k));
        esp = esp.wrapping_sub(4);
        pg.set_u32(m, esp, v);
    }
    esp = esp.wrapping_sub(4);
    pg.set_u32(m, esp, back);
    c.esp = esp;
    c.ecx = singleton;
    if target == known {
        native(m, c);
    } else {
        call_ext(m, c, target);
    }
    // the callee popped the arguments; RET
    c.esp = entry.wrapping_add(4);
}
