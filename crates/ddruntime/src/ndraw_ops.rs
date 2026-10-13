//! The executor's Clears (type 4) and StretchRects (type 5) on the native
//! path (L182): the ClearCall / CopyCall the D3D9 path would send, sent
//! by the sink in order with the taken draws, so these commands no longer
//! go through the device's apply functions and vtable.
//!
//! What the executor does (mtdraw::executor::{clear, stretch_rect}, the
//! exact port of 0x00DAB480, DDDA.exe 2017-12-13 Steam):
//!
//! - Type 4 applies the targets (+4) and fixed states (+8) of the record's
//!   fallback draw (+0x20, a type-0 DrawPrimitive record) and calls
//!   `Clear(0, NULL, flags, colour, z = +8, stencil = +0xC)` (0x00DABD02),
//!   or, when the colour cannot be cleared exactly, makes the fallback draw
//!   instead (0x00DABD07..0x00DABD8C, the same calls as a type-0 draw). The
//!   fallback draw is decided here as a type-0 draw.
//! - Type 5 applies the targets of `[[+0x28]+4]` and copies (D3DTEXF_NONE)
//!   either a rectangle (+0x10) of that binding's target `+0xC & 0xF` to
//!   (+0x20, +0x24) of the surface `[[+4]+0x44][+8]` (0x00DABF2A), or the
//!   whole of target 3 to the surface `[[+4]+0x48][+8]` between EndScene and
//!   BeginScene (0x00DABF7B; those two only flip the device's scene flag).
//!   A following identical copy is merged by the executor; the sink sees it
//!   as another command and copies the same rectangle again (the same
//!   result: nothing runs between them).
//!
//! As for taken draws (mtdraw::sink), a taken command leaves the device and
//! the executor context alone: the targets and states it would have
//! applied are read from the bindings (`target_part`, the fixed part), on
//! the same assumption the draw decoder makes (the device holds what the
//! last apply of an object set). `DDDRAW_NATIVE=verify` compares every
//! decoded command with the D3D9 path's.

use super::*;
use d3d9cmd::{ClearCall, CopyCall};
use ddnative_base::x86;

/// `DDDRAW_NATIVE_OPS=0`: leave types 4 and 5 to the D3D9 path.
pub(super) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDDRAW_NATIVE_OPS").as_deref() != Ok("0"))
}

/// Float constants in the exe the executor's Clear reads: 1.0, 255.0, 1/255.
const ONE: u32 = 0x0140_6CE0;
const BYTE_MAX: u32 = 0x0161_C298;
const INV_BYTE_MAX: u32 = 0x0161_C29C;

/// A type-4 record's Clear arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClearArgs {
    /// D3DCLEAR_TARGET (1), ZBUFFER (2), STENCIL (4)
    pub flags: u32,
    /// D3DCOLOR (A8R8G8B8)
    pub colour: u32,
    /// the bits of z
    pub z: u32,
    pub stencil: u32,
}

/// The Clear a type-4 record makes, or None when it makes its fallback
/// draw instead. Translated from 0x00DAB480's type 4, 0x00DABAF0..
/// 0x00DABD02 (DDDA.exe 2017-12-13 Steam; mtdraw::executor::clear):
/// each colour float (+0x10 red, green, blue, +0x1C alpha) becomes a byte,
/// `trunc(f * 255)` (SSE) or 255 above 1.0; the colour is exact when every
/// byte * (1/255) (x87) equals its float, and the write mask (+4) covers
/// all of the target's channels (`[binding+0x14] & 7` of them). A record
/// that writes a colour channel (+4 & 0xF) without an exact colour draws.
///
/// The alpha byte is compared before it is rounded to a float (the
/// original's FMULP then FUCOMIP), at the precision of the guest's x87
/// `control` word (the executor's, mtdraw::sink::x87_control): exact at
/// 53 and 64 bits (8 x 24 bits), rounded to single at 24 bits. So at 53
/// bits an alpha of 1.0 (255 * (1/255) is not 1) never clears exactly.
/// A rounding mode other than to nearest is refused (`Refusal::Precision`).
pub(super) fn clear_args(m: &dyn Mem, record: u32, control: u16) -> Result<Option<ClearArgs>, Refusal> {
    let fallback = m.u32(record.wrapping_add(0x20));
    if fallback == 0 {
        return Err(Refusal::NullBinding);
    }
    let binding = m.u32(fallback.wrapping_add(4));
    if binding == 0 {
        return Err(Refusal::NullBinding);
    }
    let (one, byte_max) = (m.u32(ONE), m.u32(BYTE_MAX));
    let inv = f32::from_bits(m.u32(INV_BYTE_MAX)) as f64;
    // COMISS; JBE scales (an unordered value too), else 255.0; CVTTSS2SI
    let byte = |f: u32| {
        let v = if x86::comiss_hw::<true>(f, one).cond("be") { x86::mulss(f, byte_max) } else { byte_max };
        x86::cvttss2si(v) & 0xFF
    };
    let floats: [u32; 4] = std::array::from_fn(|k| m.u32(record.wrapping_add(0x10 + 4 * k as u32)));
    let [r, g, b, a] = floats.map(byte);
    // FILD byte, FMUL 1/255 (exact: 8 x 24 bits), FSTP m32, COMISS: equal
    // and ordered
    let back = |n: u32, f: u32| (n as f64 * inv) as f32 == f32::from_bits(f);
    let product = a as f64 * inv;
    let alpha = f32::from_bits(floats[3]) as f64;
    let alpha_equal = match (control >> 8 & 3, control >> 10 & 3) {
        (0, 0) => (product as f32) as f64 == alpha,
        (2 | 3, 0) => product == alpha,
        _ => return Err(Refusal::Precision),
    };
    let channels = m.u32(binding.wrapping_add(0x14)) & 7;
    let mask = (1u32 << channels).wrapping_sub(1);
    let write = m.u32(record.wrapping_add(4));
    let exact = alpha_equal && back(r, floats[0]) && back(g, floats[1]) && back(b, floats[2]) && mask & write == mask;
    if !exact && write & 0x30 != write {
        return Ok(None);
    }
    let flags = (write & 0xF != 0) as u32 | if write & 0x10 != 0 { 2 } else { 0 } | if write & 0x20 != 0 { 4 } else { 0 };
    // FLD m32 / FSTP m32: the same bits, a signalling NaN quieted
    let mut z = m.u32(record.wrapping_add(8));
    if z & 0x7F80_0000 == 0x7F80_0000 && z & 0x007F_FFFF != 0 {
        z |= 0x0040_0000;
    }
    Ok(Some(ClearArgs { flags, colour: a << 24 | r << 16 | g << 8 | b, z, stencil: m.u32(record.wrapping_add(0xC)) }))
}

/// The ClearCall for `args` into the targets and states of the fallback
/// draw `fallback` (with the D3D lock held, `begin_frame` called).
fn clear_call(g: &G, c: &mut Caches, info: &FrameInfo, fallback: u32, args: ClearArgs) -> Result<ClearCall, Refusal> {
    let [targets, fixed] = [4u32, 8].map(|o| g.u32(fallback.wrapping_add(o)));
    if targets == 0 || fixed == 0 {
        return Err(Refusal::NullBinding);
    }
    let t = {
        let mut map = std::mem::take(&mut c.target_bindings);
        let r = kept_part(&mut map, c.frame, targets, || target_part(g, c, info, targets));
        c.target_bindings = map;
        r?
    };
    let id = identity_of(g, fixed);
    if id.blend.address == 0 || id.depth.address == 0 || id.raster.address == 0 {
        return Err(Refusal::NullBinding);
    }
    let fp = match c.fixed.get(&id) {
        Some(f) => f.clone(),
        None => {
            let f = Arc::new(fixed_part(g, &id));
            c.fixed.insert(id, f.clone());
            f
        }
    };
    // as in draw_call's list: the fixed part's last call, else the device's
    let state = |s: u32| {
        fp.states.iter().rev().find(|e| e.0 == s).or_else(|| c.device.states.iter().find(|e| e.0 == s)).map_or(0, |e| e.1)
    };
    Ok(ClearCall {
        rects: Vec::new(),
        flags: args.flags,
        colour: args.colour,
        z: f32::from_bits(args.z),
        stencil: args.stencil,
        targets: t.targets.clone(),
        depth: t.depth.clone(),
        viewport: t.viewport,
        scissor: (state(174) != 0).then_some(t.scissor),
        srgb: state(194) != 0,
    })
}

/// The surfaces (guest handles) and rectangles of a type-5 record's copy.
/// Translated from 0x00DAB480's type 5, 0x00DABEA5..0x00DABF7B (DDDA.exe
/// 2017-12-13 Steam; mtdraw::executor::stretch_rect).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CopyArgs {
    pub src: u32,
    pub src_rect: [i32; 4],
    pub dst: u32,
    pub dst_rect: [i32; 4],
}

pub(super) fn copy_args(m: &dyn Mem, record: u32) -> Result<CopyArgs, Refusal> {
    let object = m.u32(record.wrapping_add(0x28));
    let binding = if object == 0 { 0 } else { m.u32(object.wrapping_add(4)) };
    let owner = m.u32(record.wrapping_add(4));
    if binding == 0 || owner == 0 {
        return Err(Refusal::NullBinding);
    }
    let index = m.u32(record.wrapping_add(8));
    let which = m.u32(record.wrapping_add(0xC)) & 0xF;
    // the context's target after the apply: the binding's view (a null
    // view leaves an earlier one there: refused)
    let view = m.u32(binding.wrapping_add(4 * which.min(3)));
    if view == 0 {
        return Err(Refusal::NoColourTarget);
    }
    let list = m.u32(owner.wrapping_add(if which < 4 { 0x44 } else { 0x48 }));
    let surface = m.u32(list.wrapping_add(index.wrapping_mul(4)));
    if list == 0 || surface == 0 {
        return Err(Refusal::NullBinding);
    }
    let (src, dst) = (m.u32(view.wrapping_add(0x14)), m.u32(surface.wrapping_add(0x14)));
    if which >= 4 {
        // NULL rectangles: the whole surfaces
        return Ok(CopyArgs { src, src_rect: [0; 4], dst, dst_rect: [0; 4] });
    }
    let r: [u32; 4] = std::array::from_fn(|k| m.u32(record.wrapping_add(0x10 + 4 * k as u32)));
    let (x, y) = (m.u32(record.wrapping_add(0x20)), m.u32(record.wrapping_add(0x24)));
    let dst_rect = [x, y, x.wrapping_add(r[2].wrapping_sub(r[0])), y.wrapping_add(r[3].wrapping_sub(r[1]))];
    Ok(CopyArgs { src, src_rect: r.map(|v| v as i32), dst, dst_rect: dst_rect.map(|v| v as i32) })
}

/// Types 4 and 5 taken, refused, and fallback draws decided as draws.
#[derive(Default)]
pub(super) struct Stats {
    clears: u64,
    copies: u64,
    fallbacks: u64,
    refused: FxMap<Refusal, u64>,
}

impl Stats {
    pub(super) fn report(&self, frames: u64) -> String {
        let per = |n: u64| n as f64 / frames.max(1) as f64;
        let mut out = format!("  clears taken {:.1}, copies taken {:.1}, clear fallback draws {:.1} (decided as draws); refused:",
                              per(self.clears), per(self.copies), per(self.fallbacks));
        for r in Refusal::ALL {
            if let Some(&n) = self.refused.get(&r) {
                out += &format!(" {r:?} {:.1};", per(n));
            }
        }
        out + "\n"
    }
}

impl Native {
    /// Type 4: the Clear, or the fallback draw decided as a type-0 draw.
    pub(super) fn clear(&self, g: &mut Inner, m: &mut dyn Mem, context: u32, record: u32) -> Taken {
        let args = match clear_args(m, record, mtdraw::sink::x87_control()) {
            Ok(Some(a)) => a,
            Ok(None) => {
                g.totals.ops.fallbacks += 1;
                let fallback = m.u32(record.wrapping_add(0x20));
                return self.draw(g, m, context, fallback, 0);
            }
            Err(r) => {
                *g.totals.ops.refused.entry(r).or_default() += 1;
                return Taken::Default;
            }
        };
        let lock = crate::d3d9dev::device_lock();
        let call = clear_call(&G::new(m), &mut g.caches, &g.info, m.u32(record.wrapping_add(0x20)), args);
        drop(lock);
        let call = match call {
            Ok(c) => c,
            Err(r) => {
                *g.totals.ops.refused.entry(r).or_default() += 1;
                return Taken::Default;
            }
        };
        match self.mode {
            Mode::Verify => {
                expect_op(Op::Clear(call), record);
                Taken::Default
            }
            Mode::Census | Mode::Off => Taken::Default,
            _ => {
                g.totals.ops.clears += 1;
                // after what was taken before it
                g.send();
                crate::d3d9draw::backend_unlocked(|b| b.clear(&call));
                Taken::Handled
            }
        }
    }

    /// Type 5: the copy.
    pub(super) fn copy(&self, g: &mut Inner, m: &mut dyn Mem, record: u32) -> Taken {
        let args = copy_args(m, record);
        let lock = crate::d3d9dev::device_lock();
        let call = args.and_then(|a| {
            let (src, dst) = (g.caches.target(a.src), g.caches.target(a.dst));
            match (src, dst) {
                (Some(src), Some(dst)) => Ok((a.src, CopyCall { src, src_rect: a.src_rect, dst, dst_rect: a.dst_rect, filter: 0 })),
                _ => Err(Refusal::Missing),
            }
        });
        let (src, call) = match call {
            Ok(c) => c,
            Err(r) => {
                *g.totals.ops.refused.entry(r).or_default() += 1;
                return Taken::Default;
            }
        };
        match self.mode {
            Mode::Verify => {
                drop(lock);
                expect_op(Op::Copy(call), record);
                Taken::Default
            }
            Mode::Census | Mode::Off => Taken::Default,
            _ => {
                g.totals.ops.copies += 1;
                // the taken draws first, then the source's upload (as the
                // D3D9 StretchRect flushes it), then the copy
                g.send();
                crate::d3d9draw::flush(m, src);
                drop(lock);
                crate::d3d9draw::backend_unlocked(|b| b.copy(&call));
                Taken::Handled
            }
        }
    }
}

// --- verify ------------------------------------------------------------------------

/// A decoded command waiting for the D3D9 path's.
#[derive(Clone, Debug)]
pub(crate) enum Op {
    Clear(ClearCall),
    Copy(CopyCall),
}

thread_local! {
    static EXPECTED_OP: std::cell::RefCell<Option<(Op, u32)>> = const { std::cell::RefCell::new(None) };
}
static VERIFYING_OPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
struct OpStats {
    compared: u64,
    equal: u64,
    /// expected commands the D3D9 path did not make (a merged copy)
    missed: u64,
    printed: u32,
}
static OP_STATS: Mutex<Option<OpStats>> = Mutex::new(None);

fn expect_op(op: Op, record: u32) {
    VERIFYING_OPS.store(true, std::sync::atomic::Ordering::Relaxed);
    EXPECTED_OP.with(|x| {
        if x.borrow_mut().replace((op, record)).is_some() {
            OP_STATS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).missed += 1;
        }
    });
}

/// The D3D9 path's Clear (d3d9draw) or StretchRect (d3d9copy): compared
/// with the expected command in `verify` mode.
pub(crate) fn observe_op(actual: Op) {
    if !VERIFYING_OPS.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some((expected, record)) = EXPECTED_OP.with(|x| x.borrow_mut().take()) else { return };
    let equal = same_op(&expected, &actual);
    let mut g = OP_STATS.lock().unwrap_or_else(|e| e.into_inner());
    let v = g.get_or_insert_with(Default::default);
    v.compared += 1;
    if equal {
        v.equal += 1;
    } else if v.printed < 8 {
        v.printed += 1;
        eprintln!("ndraw native verify: record {record:#x}: {expected:?} vs the D3D9 path's {actual:?}");
    }
}

/// Equal commands (z by its bits).
fn same_op(a: &Op, b: &Op) -> bool {
    match (a, b) {
        (Op::Clear(x), Op::Clear(y)) => {
            x.z.to_bits() == y.z.to_bits() && ClearCall { z: 0.0, ..x.clone() } == ClearCall { z: 0.0, ..y.clone() }
        }
        (Op::Copy(x), Op::Copy(y)) => x == y,
        _ => false,
    }
}

pub(super) fn verify_report() -> String {
    let mut g = OP_STATS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(v) = g.take() else { return "  verify clears/copies: nothing compared\n".into() };
    format!("  verify clears/copies: {} compared, {} equal, {} not made by the D3D9 path\n", v.compared, v.equal, v.missed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// the MSVC runtime's control word: 53-bit precision, to nearest
    const PC53: u16 = 0x027F;
    /// D3D9's without D3DCREATE_FPU_PRESERVE: 24-bit
    const PC24: u16 = 0x007F;

    #[derive(Default)]
    struct TestMem(HashMap<u32, u8>);

    impl Mem for TestMem {
        fn u8(&self, a: u32) -> u8 {
            self.0.get(&a).copied().unwrap_or(0)
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.0.insert(a, v);
        }
    }

    /// A type-4 record at 0x1000 (fallback draw at 0x2000, its target
    /// binding at 0x3000 with `channels`), the exe's constants in place.
    fn clear_record(write: u32, rgba: [f32; 4], channels: u32) -> TestMem {
        let mut m = TestMem::default();
        m.set_u32(ONE, 1f32.to_bits());
        m.set_u32(BYTE_MAX, 255f32.to_bits());
        m.set_u32(INV_BYTE_MAX, (1f32 / 255.0).to_bits());
        m.set_u32(0x1004, write);
        m.set_u32(0x1008, 0.5f32.to_bits());
        m.set_u32(0x100C, 7);
        for (k, f) in rgba.iter().enumerate() {
            m.set_u32(0x1010 + 4 * k as u32, f.to_bits());
        }
        m.set_u32(0x1020, 0x2000);
        m.set_u32(0x2004, 0x3000);
        m.set_u32(0x3014, channels);
        m
    }

    #[test]
    fn exact_colours_clear_with_the_packed_d3dcolor_and_flags() {
        // 0, 1, 51/255 = 0.2 (its float back from the byte), above 1.0
        let m = clear_record(0x3F, [0.0, 1.0, 51.0 * (1.0 / 255.0), 1.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap(), None, "alpha 1.0 at 53 bits");
        let a = clear_args(&m, 0x1000, PC24).unwrap().unwrap();
        assert_eq!(a, ClearArgs { flags: 7, colour: 0xFF00_FF33, z: 0.5f32.to_bits(), stencil: 7 });
        let m = clear_record(0x0F, [2.0, 0.0, 0.0, 0.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap(), None, "2.0 packs to 255, which is 1.0 back");
        let m = clear_record(0x0F, [1.0, 1.0, 1.0, 1.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC24).unwrap().unwrap().colour, 0xFFFF_FFFF);
        let m = clear_record(0x0F, [1.0, 0.0, 51.0 * (1.0 / 255.0), 0.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap().unwrap().colour, 0x00FF_0033);
    }

    #[test]
    fn inexact_colours_and_partial_masks_draw_unless_only_depth_or_stencil_is_cleared() {
        // 0.5 packs to 127, and 127/255 is not 0.5
        let m = clear_record(0x0F, [0.5, 0.0, 0.0, 1.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap(), None);
        // -1.0 * 255 truncates to -255: the byte 1, not -1.0 back (a
        // one-channel target, alpha 0: only red can make it inexact)
        let m = clear_record(0x01, [-1.0, 0.0, 0.0, 0.0], 1);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap(), None);
        // a mask of 3 channels on a 4-channel target
        let m = clear_record(0x07, [0.0, 0.0, 0.0, 1.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap(), None);
        // the same mask on a 3-channel target clears
        let m = clear_record(0x07, [0.0, 0.0, 0.0, 1.0], 3);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap().unwrap().flags, 1);
        // depth and stencil only: the colour does not matter
        let m = clear_record(0x30, [0.5, f32::NAN, 0.0, 0.3], 4);
        let a = clear_args(&m, 0x1000, PC53).unwrap().unwrap();
        assert_eq!(a.flags, 6);
        // a NaN channel is unordered: scaled (to NaN), truncated to 0x80000000
        assert_eq!(a.colour & 0xFF00, 0);
    }

    #[test]
    fn z_keeps_its_bits_but_a_signalling_nan_is_quieted() {
        let mut m = clear_record(0x10, [0.0; 4], 4);
        m.set_u32(0x1008, 0x7F80_0001);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap().unwrap().z, 0x7FC0_0001);
        m.set_u32(0x1008, 0x8000_0000);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap().unwrap().z, 0x8000_0000);
    }

    /// The alpha comparison at the guest's precision: an alpha of 1.0 is
    /// 255 * (1/255) only once rounded to single.
    #[test]
    fn alpha_compares_at_the_guests_x87_precision() {
        assert_ne!(255.0 * (1f32 / 255.0) as f64, 1.0);
        let m = clear_record(0x0F, [0.0, 0.0, 0.0, 1.0], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53), Ok(None));
        assert_eq!(clear_args(&m, 0x1000, 0x037F), Ok(None));
        assert_eq!(clear_args(&m, 0x1000, 0x007F).unwrap().unwrap().colour, 0xFF00_0000);
        // alpha 0 is exact at every precision
        let m = clear_record(0x0F, [0.0; 4], 4);
        assert_eq!(clear_args(&m, 0x1000, PC53).unwrap().unwrap().colour, 0);
        // rounding up, and the reserved precision: refused
        assert_eq!(clear_args(&m, 0x1000, 0x0A7F), Err(Refusal::Precision));
        assert_eq!(clear_args(&m, 0x1000, 0x017F), Err(Refusal::Precision));
    }

    #[test]
    fn null_fallbacks_are_refused() {
        let mut m = clear_record(0x0F, [0.0; 4], 4);
        m.set_u32(0x2004, 0);
        assert_eq!(clear_args(&m, 0x1000, PC53), Err(Refusal::NullBinding));
        m.set_u32(0x1020, 0);
        assert_eq!(clear_args(&m, 0x1000, PC53), Err(Refusal::NullBinding));
    }

    /// A type-5 record at 0x1000 copying target `which` of the binding at
    /// 0x3000 (views 0x4000 + 0x100 k, handles 0xA000 + k) to entry 1 of the
    /// owner's lists (+0x44: 0x5000, +0x48: 0x5100; surfaces 0x6000/0x6100,
    /// handles 0xB000/0xB100).
    fn copy_record(which: u32) -> TestMem {
        let mut m = TestMem::default();
        m.set_u32(0x1004, 0x2000);
        m.set_u32(0x1008, 1);
        m.set_u32(0x100C, 0x30 | which);
        for (k, v) in [10u32, 20, 110, 70].into_iter().enumerate() {
            m.set_u32(0x1010 + 4 * k as u32, v);
        }
        m.set_u32(0x1020, 300);
        m.set_u32(0x1024, 400);
        m.set_u32(0x1028, 0x2800);
        m.set_u32(0x2804, 0x3000);
        for k in 0..4 {
            m.set_u32(0x3000 + 4 * k, 0x4000 + 0x100 * k);
            m.set_u32(0x4000 + 0x100 * k + 0x14, 0xA000 + k);
        }
        m.set_u32(0x2044, 0x5000);
        m.set_u32(0x2048, 0x5100);
        m.set_u32(0x5004, 0x6000);
        m.set_u32(0x5104, 0x6100);
        m.set_u32(0x6014, 0xB000);
        m.set_u32(0x6114, 0xB100);
        m
    }

    #[test]
    fn copies_take_a_rectangle_of_the_indexed_target_or_the_whole_of_target_3() {
        let m = copy_record(2);
        assert_eq!(copy_args(&m, 0x1000).unwrap(),
                   CopyArgs { src: 0xA002, src_rect: [10, 20, 110, 70], dst: 0xB000, dst_rect: [300, 400, 400, 450] });
        let m = copy_record(4);
        assert_eq!(copy_args(&m, 0x1000).unwrap(), CopyArgs { src: 0xA003, src_rect: [0; 4], dst: 0xB100, dst_rect: [0; 4] });
        let mut m = copy_record(1);
        m.set_u32(0x3004, 0);
        assert_eq!(copy_args(&m, 0x1000), Err(Refusal::NoColourTarget));
    }

    #[test]
    fn verify_counts_equal_and_differing_commands() {
        let target = Target { surface: 1, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 };
        let copy = CopyCall { src: target.clone(), src_rect: [0; 4], dst: target, dst_rect: [0; 4], filter: 0 };
        expect_op(Op::Copy(copy.clone()), 1);
        observe_op(Op::Copy(copy.clone()));
        expect_op(Op::Copy(copy.clone()), 2);
        observe_op(Op::Copy(CopyCall { filter: 2, ..copy }));
        let report = verify_report();
        assert!(report.contains("2 compared, 1 equal"), "{report}");
    }
}
