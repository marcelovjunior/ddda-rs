//! L184 (s4): semantic port of 0x00B66B30, the player actor's per-frame
//! camera/aiming pass. See `semantic.rs` for the pattern: ABI-level
//! equivalence (EAX unused, callee-saved registers, ESP, every guest write)
//! and every call to other guest code kept as a guest call.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

/// What the x87 `FLD m32` / `FSTP m32` pair does to a float's bits: a
/// signalling NaN comes back quiet, everything else is unchanged.
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// The (carry, zero) flags `COMISS a, b` sets; unordered sets both.
fn comiss(a: f32, b: f32) -> (bool, bool) {
    if a.is_nan() || b.is_nan() {
        (true, true)
    } else {
        (a < b, a == b)
    }
}

/// `JBE` after a `COMISS`: taken when the first operand is below or equal
/// to the second, or either is NaN.
fn below_or_equal(a: f32, b: f32) -> bool {
    let (carry, zero) = comiss(a, b);
    carry || zero
}

/// `JC` after a `COMISS`: the first operand is below the second or NaN.
fn below(a: f32, b: f32) -> bool {
    comiss(a, b).0
}

/// Registers the original has loaded at a guest call (the callees take
/// inputs in EAX, EDX, ESI and EDI as well as ECX and the stack).
#[derive(Clone, Copy, Default)]
struct Regs {
    eax: u32,
    ecx: u32,
    edx: u32,
    esi: u32,
    edi: u32,
}

/// Guest call from `frame` with `regs`, `args` pushed (last first) and the
/// original's return site `back`; returns EAX.
#[allow(clippy::too_many_arguments)]
fn guest_call(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, target: u32, regs: Regs, args: &[u32], back: u32) -> u32 {
    call_from(m, c, pg, frame, regs.ecx, args, back, |m, c| {
        (c.eax, c.edx, c.esi, c.edi) = (regs.eax, regs.edx, regs.esi, regs.edi);
        call_ext(m, c, target);
    })
}

/// Constants and tables the function reads from the exe's data.
mod exe {
    /// Written to both camera sub-objects as their upper limit (+0x58), and
    /// to their lower limit (+0x5C) in mode 1.
    pub const LIMIT_A: u32 = 0x0153_19b8;
    pub const LIMIT_B: u32 = 0x014e_ae5c;
    /// Rate the offset at +0x46B4 sweeps toward its target (times frame time).
    pub const SWEEP_RATE: u32 = 0x014f_7270;
    pub const SIGN_MASK: u32 = 0x0161_c280;
    pub const PROBE_ARG_A: u32 = 0x0161_c308;
    pub const PROBE_ARG_B: u32 = 0x0150_a088;
    pub const MARGIN: u32 = 0x014e_a574;
    pub const FLOOR_LIMIT: u32 = 0x0152_1690;
    pub const FLOOR_STEP: u32 = 0x0152_0bac;
    pub const NO_FLOOR: u32 = 0x0153_1ae8;
    /// Pointer to the global game state.
    pub const GAME_STATE: u32 = 0x018f_a4bc;
}

// Translated from 0x00b66b30 (DDDA.exe 2017-12-13 Steam).
/// 0x00B66B30, `player->post_update()` (thiscall, void; vf97 under the vf09
/// update path 0x00B66110; the class and the meaning of several fields are
/// unresolved, so their names below are guesses): the per-frame
/// camera/aiming pass of the player actor (`this`, EBX in the original).
///
/// 1. With state flags (+0x273C) bit 0 set it asks the action state
///    (0x00B66320, 0x00B661F0), runs the aim timer (+0x49EC) down by the
///    frame time (+0x14) clamped at 0 (zero while flags & 0xC), and asks the
///    request handler 0x00B71B90 (then 0x00B72360, vtable +0x210 and
///    0x00B72D30 as it agrees).
/// 2. It copies the position (+0x40..+0x48) to +0x4D10..+0x4D18, clears some
///    selection fields, remembers the five button bytes (+0x40E1, +0x41B1,
///    +0x4281, +0x4351, +0x4421) and reads four floats from the input source
///    at +0x3348 (vtable +0x3C, keys 0xDD..0xE0, floats returned in ST0).
/// 3. Unless +0x3CF2 is set it refreshes the model (0x01054860, the
///    uModel::vf28) and reads three more input floats to +0x4984..+0x498C.
/// 4. A look mode 0, 1 or 2 follows from the action state (+0x2DD4), the
///    flags and three exe tables (0x00B677AC/B4, 0x00B678A8/B4,
///    0x00B67908/10). For the camera pair at +0x2E74 (sub-objects +0xC and
///    +0x10, with limits +0x58/+0x5C and an active byte +0x61) the mode sets
///    the limits and sweeps +0x46B4 toward its target; modes 1 and 2 probe
///    two anchor points (0x007B0580) and hand them to the pair (0x00E4B700).
/// 5. It runs the vtable hooks (+0x50, +0x2E0, +0x70), steps the pair object
///    (0x0081F2D0, 0x0081F8F0, 0x0081FC60, 0x0081FD10), computes the floor
///    offset at +0x29F0 and finishes with 0x00837980 / 0x00B67930, the byte
///    at +0x359 of the object at +0x3DEC and the float at +0x2510.
///
/// Every local of the original frame is kept in guest memory at the
/// original's address (`sp` below), since the guest callees read them.
pub fn player_post_update(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: push ebp; mov ebp, esp; and esp, -16; sub esp,
    // 0x74; push ebx; push esi; push edi. Its locals are addressed from `sp`.
    let sp = (entry.wrapping_sub(4) & !15).wrapping_sub(0x74 + 12);
    c.ebx = this;
    c.ebp = entry.wrapping_sub(4);
    let at = |offset: u32| this.wrapping_add(offset);

    macro_rules! r8 { ($addr:expr) => { pg.u8(m, $addr) }; }
    macro_rules! r16 { ($addr:expr) => { pg.u16(m, $addr) }; }
    macro_rules! r32 { ($addr:expr) => { pg.u32(m, $addr) }; }
    macro_rules! rf { ($addr:expr) => { f32::from_bits(pg.u32(m, $addr)) }; }
    macro_rules! w8 { ($addr:expr, $v:expr) => { pg.set_u8(m, $addr, $v) }; }
    macro_rules! w32 { ($addr:expr, $v:expr) => { pg.set_u32(m, $addr, $v) }; }
    macro_rules! wf { ($addr:expr, $v:expr) => { pg.set_u32(m, $addr, f32::to_bits($v)) }; }
    // `FSTP m32` of the float a guest callee returned in ST0
    macro_rules! store_st0 { ($addr:expr) => { c.st.real_memory::<0x18d9, 4, true>(m, $addr) }; }
    // a guest call from the original's frame
    macro_rules! gcall {
        ($target:expr, $regs:expr, [$($arg:expr),*], $back:expr) => {{
            let target: u32 = $target;
            guest_call(m, c, pg, sp, target, $regs, &[$($arg),*], $back)
        }};
    }
    // a call of vtable slot `slot` of `object`, with ECX = `object`
    macro_rules! vcall {
        ($object:expr, $slot:expr, [$($arg:expr),*], $back:expr) => {{
            let object: u32 = $object;
            let method = r32!(r32!(object).wrapping_add($slot));
            gcall!(method, Regs { ecx: object, ..Regs::default() }, [$($arg),*], $back)
        }};
    }
    let on_this = Regs { ecx: this, ..Regs::default() };
    // an input float of the source at +0x3348 (vtable +0x3C), left in ST0
    macro_rules! input_float {
        ($key:expr, $back:expr) => {{
            let source = r32!(at(0x3348));
            vcall!(source, 0x3c, [$key], $back)
        }};
    }
    // sub-objects of the camera pair at +0x2E74 (reloaded every time, as the original does)
    macro_rules! first_camera { () => { r32!(r32!(at(0x2e74)).wrapping_add(0xc)) }; }
    macro_rules! second_camera { () => { r32!(r32!(at(0x2e74)).wrapping_add(0x10)) }; }
    macro_rules! set_active { ($v:expr) => {{
        let first = first_camera!();
        w8!(first.wrapping_add(0x61), $v);
        let second = second_camera!();
        w8!(second.wrapping_add(0x61), $v);
    }}; }

    gcall!(0x0084_c800, on_this, [], 0x00b6_6b43);
    let flags = r32!(at(0x273c));
    w32!(sp + 0x28, flags);
    'queue: {
        if flags & 1 == 0 {
            break 'queue;
        }
        let state = r32!(at(0x2dd4));
        let mut action = gcall!(0x00b6_6320, Regs { edx: this, ..Regs::default() }, [state], 0x00b6_6b66);
        let held = gcall!(0x00b6_61f0, Regs { eax: this, ..Regs::default() }, [], 0x00b6_6b6f) & 0xff;
        w8!(sp + 0x15, held as u8);
        if r8!(at(0x206d)) & 2 == 0 {
            action = 0;
        }
        let timer = rf!(at(0x49ec)) - rf!(at(0x14));
        wf!(at(0x49ec), timer);
        if !below(0.0, timer) {
            wf!(at(0x49ec), 0.0);
        }
        if r8!(sp + 0x28) & 0xc != 0 {
            wf!(at(0x49ec), 0.0);
        }
        let agreed = gcall!(0x00b7_1b90, Regs { esi: this, ..Regs::default() }, [(action != 0) as u32], 0x00b6_6bbf) & 0xff;
        if agreed == 0 {
            break 'queue;
        }
        w8!(sp + 0x13, 0);
        if r8!(sp + 0x15) != 0 || action != 0 {
            let r = gcall!(0x00b7_2360, Regs { esi: this, ..Regs::default() }, [], 0x00b6_6bd8) & 0xff;
            w8!(sp + 0x13, r as u8);
        }
        if action == 1 {
            let (carry, zero) = comiss(rf!(at(0x49ec)), 0.0);
            if (!carry && !zero) || r8!(sp + 0x13) == 0 {
                break 'queue;
            }
            vcall!(this, 0x210, [0x38], 0x00b6_6c06);
        }
        if r8!(sp + 0x13) != 0 {
            gcall!(0x00b7_2d30, Regs { eax: this, ..Regs::default() }, [], 0x00b6_6c14);
        }
    }

    // 0x00B66C17: the position copy goes through x87 (signalling NaNs come back quiet)
    let v = through_x87(r32!(at(0x40)));
    w32!(at(0x4d10), v);
    let v = through_x87(r32!(at(0x44)));
    w32!(at(0x4d14), v);
    let v = through_x87(r32!(at(0x48)));
    w32!(at(0x4d1c), 0);
    w32!(at(0x4d18), v);
    // the five button bytes are kept at sp+0x13..0x17 and put back at the end
    let buttons = [(0x41b1u32, 0x17u32), (0x40e1, 0x16), (0x4421, 0x15), (0x4281, 0x13), (0x4351, 0x14)];
    let values = buttons.map(|(field, _)| r8!(at(field)));
    w32!(at(0x3d58), 0);
    w32!(at(0x3d60), 0);
    w32!(at(0x3c70), 0);
    w32!(at(0x3d5c), 0x2a);
    for ((_, local), value) in buttons.iter().zip(values) {
        w8!(sp + local, value);
    }
    input_float!(0xdd, 0x00b6_6ca0);
    store_st0!(sp + 0x2c);
    input_float!(0xde, 0x00b6_6cb6);
    store_st0!(sp + 0x24);
    input_float!(0xdf, 0x00b6_6ccc);
    store_st0!(sp + 0x20);
    input_float!(0xe0, 0x00b6_6ce2);
    store_st0!(sp + 0x1c);

    if r8!(at(0x3cf2)) == 0 {
        w8!(at(0x47d1), 0);
        w8!(at(0x4861), 0);
        w8!(at(0x48f1), 0);
        gcall!(0x0085_08c0, Regs { esi: this, ..Regs::default() }, [], 0x00b6_6d0f);
        gcall!(0x0105_4860, on_this, [1], 0x00b6_6d18);
        input_float!(0xcb, 0x00b6_6d2a);
        store_st0!(at(0x4984));
        input_float!(0xcc, 0x00b6_6d42);
        store_st0!(at(0x4988));
        input_float!(0xcd, 0x00b6_6d5a);
        store_st0!(at(0x498c));
        if r32!(at(0x4924)) != 0 {
            let block = at(0x4760);
            if r8!(block.wrapping_add(0x1c0)) != 0 {
                gcall!(0x0078_a730, Regs { esi: block, ..Regs::default() }, [], 0x00b6_6d7f);
            } else {
                gcall!(0x0078_b4c0, Regs { eax: block, ..Regs::default() }, [], 0x00b6_6d86);
            }
        }
    }

    // 0x00B66D86: the look mode (the original's ESI): 0, 1 or 2
    let mut mode: u32 = 1;
    let flags = r32!(at(0x273c));
    if flags & 1 == 0 {
        let state = r32!(at(0x2dd4)) as i32;
        if !(0xaa..=0xab).contains(&state) {
            mode = 0;
        }
    }
    let state = r32!(at(0x2dd4));
    if state.wrapping_sub(0x0206_0000) <= 2 && r8!(at(0x206d)) & 2 != 0 && r16!(at(0x384)) != 0x905 {
        mode = 0;
    }
    if flags & 0x30 != 0 {
        let index = state.wrapping_sub(0x11);
        if index > 0xf0 {
            mode = 0;
        } else {
            let class = r8!(0x00b6_77b4 + index) as u32;
            if r32!(0x00b6_77ac + class * 4) == 0x00b6_6df3 {
                mode = 0;
            }
        }
    }
    let signed = state as i32;
    if signed > 0x65 {
        if signed > 0xe8 {
            if state == 0x0101_0020 {
                mode = 0;
            }
        } else if signed >= 0xe7 || signed == 0x7a || (signed > 0x7f && signed <= 0x82) {
            mode = 0;
        }
    } else if signed == 0x65 {
        if r8!(at(0x3d51)) != 0 {
            mode = 0;
        }
    } else {
        let index = state.wrapping_sub(1);
        if index <= 0x52 {
            let class = r8!(0x00b6_78b4 + index) as u32;
            match r32!(0x00b6_78a8 + class * 4) {
                0x00b6_6e12 => {
                    if r8!(at(0x3d51)) != 0 {
                        mode = 0;
                    }
                }
                0x00b6_6e45 => mode = 0,
                _ => {}
            }
        }
    }
    let index = state.wrapping_sub(0x94);
    if index <= 0x18 {
        let class = r8!(0x00b6_7910 + index) as u32;
        if r32!(0x00b6_7908 + class * 4) == 0x00b6_6e60 {
            mode = 2;
        }
    }
    if state == 2 || state == 5 {
        input_float!(0xe1, 0x00b6_6e83);
        store_st0!(sp + 0x1c);
    }

    // 0x00B66E87: the camera pair
    let pair = r32!(at(0x2e74));
    let first = r32!(pair.wrapping_add(0xc));
    'pair: {
        if first == 0 || r32!(pair.wrapping_add(0x10)) == 0 {
            break 'pair;
        }
        if r8!(at(0x206d)) & 6 == 0 || r8!(at(0x3cf1)) != 0 {
            w8!(first.wrapping_add(0x61), 0);
            let second = second_camera!();
            w8!(second.wrapping_add(0x61), 0);
            break 'pair;
        }
        // The offset at +0x46B4 sweeps toward zero by (frame time * rate);
        // the same code ends modes 0 and 2 (their tails differ).
        macro_rules! sweep_to_zero { () => {{
            let offset = rf!(at(0x46b4));
            if below_or_equal(0.0, offset) {
                if !below_or_equal(offset, 0.0) {
                    let step = rf!(at(0x14)) * rf!(exe::SWEEP_RATE);
                    let next = offset - step;
                    wf!(at(0x46b4), next);
                    if !below(0.0, next) {
                        wf!(at(0x46b4), 0.0);
                    }
                }
            } else {
                let step = rf!(at(0x14)) * rf!(exe::SWEEP_RATE);
                let next = step + offset;
                wf!(at(0x46b4), next);
                if !below(next, 0.0) {
                    wf!(at(0x46b4), 0.0);
                }
            }
        }}; }
        // the anchor probe: 0x00B85F70 then 0x007B0580(out = sp+0x50, point);
        // returns whether the probe hit. 0x007B0580 is cdecl (ADD ESP, 0xc).
        macro_rules! probe { ($point:expr, $scale_back:expr, $back:expr) => {{
            let scale = gcall!(0x00b8_5f70, on_this, [], $scale_back);
            let arg_a = r32!(exe::PROBE_ARG_A);
            let arg_b = r32!(exe::PROBE_ARG_B);
            let hit = guest_call(m, c, pg, sp, 0x007b_0580, Regs { esi: $point, edi: sp + 0x50, ..Regs::default() }, &[arg_b, arg_a, scale], $back) & 0xff;
            c.esp = sp;
            hit != 0
        }}; }
        // hand the two anchor points (vectors at sp+0x30 and sp+0x40) to the pair
        macro_rules! hand_to_pair { ($first_point:expr, $second_point:expr, $back_a:expr, $back_b:expr) => {{
            let first = first_camera!();
            gcall!(0x00e4_b700, Regs { ecx: first, ..Regs::default() }, [$first_point], $back_a);
            let second = second_camera!();
            gcall!(0x00e4_b700, Regs { ecx: second, ..Regs::default() }, [$second_point], $back_b);
        }}; }
        match mode {
            0 => {
                let upper = rf!(exe::LIMIT_A);
                wf!(first.wrapping_add(0x5c), 0.0);
                wf!(first.wrapping_add(0x58), upper);
                let second = second_camera!();
                wf!(second.wrapping_add(0x5c), 0.0);
                wf!(second.wrapping_add(0x58), upper);
                sweep_to_zero!();
                set_active!(0);
            }
            1 => {
                let lower = rf!(exe::LIMIT_A);
                let upper = rf!(exe::LIMIT_B);
                wf!(first.wrapping_add(0x5c), lower);
                wf!(first.wrapping_add(0x58), upper);
                let second = second_camera!();
                wf!(second.wrapping_add(0x5c), lower);
                wf!(second.wrapping_add(0x58), upper);
                set_active!(0);
                w8!(at(0x4281), 0);
                w8!(at(0x4351), 0);
                w8!(at(0x4681), 0);
                gcall!(0x0085_0440, Regs { esi: this, ..Regs::default() }, [], 0x00b6_71af);
                gcall!(0x0105_4860, on_this, [1], 0x00b6_71b8);
                w32!(sp + 0x3c, 0);
                w32!(sp + 0x4c, 0);
                let lookup = gcall!(0x0042_1ed0, on_this, [0x10], 0x00b6_71d0);
                let (x0, y0, z0) = (r32!(lookup + 0x40), r32!(lookup + 0x44), r32!(lookup + 0x48));
                w32!(sp + 0x30, x0);
                w32!(sp + 0x64, y0);
                w32!(sp + 0x34, y0);
                w32!(sp + 0x38, z0);
                w32!(sp + 0x3c, 0);
                let lookup = gcall!(0x0042_1ed0, on_this, [0x14], 0x00b6_7209);
                let (x1, y1, z1) = (r32!(lookup + 0x40), r32!(lookup + 0x44), r32!(lookup + 0x48));
                w32!(sp + 0x40, x1);
                w32!(sp + 0x4c, 0);
                let height = r32!(at(0x44));
                w32!(sp + 0x74, y1);
                w32!(sp + 0x44, y1);
                w32!(sp + 0x48, z1);
                w32!(sp + 0x28, height);
                w32!(sp + 0x18, height);
                // Clamp each anchor's height to the probed ground plus the
                // window below/above it. `point` is the anchor vector, `target_y`
                // the saved target height and `keep` where the probed height is kept.
                macro_rules! clamp_anchor { ($point:expr, $target_y:expr, $keep:expr) => {{
                    let point: u32 = $point;
                    let mut ground_z = rf!(sp + 0x58);
                    let target_y = rf!($target_y);
                    let ground_x = rf!(sp + 0x50);
                    let ground_y = rf!(sp + 0x54);
                    let rise = target_y - rf!(at(0xe4));
                    wf!(point + 8, ground_z);
                    ground_z = 0.0;
                    let rising = !below_or_equal(rise, ground_z);
                    wf!(point, ground_x);
                    let mut y = ground_y;
                    wf!(point + 4, y);
                    wf!(point + 0xc, ground_z);
                    if rising {
                        y += rise;
                        wf!(point + 4, y);
                    }
                    let floor = target_y - rf!(sp + 0x20);
                    if !below_or_equal(floor, y) {
                        y = floor;
                        wf!(point + 4, y);
                    }
                    let ceiling = target_y + rf!(sp + 0x24);
                    if !below_or_equal(y, ceiling) {
                        wf!(point + 4, ceiling);
                    }
                    wf!($keep, ground_y);
                }}; }
                if probe!(sp + 0x30, 0x00b6_7251, 0x00b6_727d) {
                    clamp_anchor!(sp + 0x30, sp + 0x64, sp + 0x28);
                }
                if probe!(sp + 0x40, 0x00b6_7309, 0x00b6_7335) {
                    clamp_anchor!(sp + 0x40, sp + 0x74, sp + 0x18);
                }
                hand_to_pair!(sp + 0x30, sp + 0x40, 0x00b6_73cd, 0x00b6_73e0);
                // the offset's target from the height window [sp+0x18, sp+0x28]
                let height = rf!(at(0x44));
                let upper = rf!(sp + 0x28);
                let lower = rf!(sp + 0x18);
                w8!(at(0x4681), 1);
                let mut gap = 0.0f32;
                let (carry, zero) = comiss(height, upper);
                let mut outside = !carry && !zero;
                if !outside {
                    outside = !below_or_equal(height, lower);
                }
                if outside {
                    if below_or_equal(lower, upper) {
                        gap = lower - height;
                    } else {
                        gap = upper - height;
                    }
                }
                let bound = f32::from_bits(r32!(sp + 0x2c) ^ r32!(exe::SIGN_MASK));
                if !below(bound, gap) {
                    gap = bound;
                }
                let offset = rf!(at(0x46b4));
                if !below_or_equal(gap, offset) {
                    let next = rf!(at(0x14)) * rf!(sp + 0x1c) + offset;
                    wf!(at(0x46b4), next);
                    if !below(next, gap) {
                        wf!(at(0x46b4), gap);
                    }
                } else if !below_or_equal(offset, gap) {
                    let next = offset - rf!(at(0x14)) * rf!(sp + 0x1c);
                    wf!(at(0x46b4), next);
                    if !below(gap, next) {
                        wf!(at(0x46b4), gap);
                    }
                }
                set_active!(1);
            }
            _ => {
                let limit = rf!(exe::LIMIT_A);
                wf!(first.wrapping_add(0x5c), limit);
                wf!(first.wrapping_add(0x58), limit);
                let second = second_camera!();
                wf!(second.wrapping_add(0x5c), limit);
                wf!(second.wrapping_add(0x58), limit);
                set_active!(0);
                w8!(at(0x4281), 0);
                w8!(at(0x4351), 0);
                gcall!(0x0085_0440, Regs { esi: this, ..Regs::default() }, [], 0x00b6_6f2b);
                gcall!(0x0105_4860, on_this, [1], 0x00b6_6f34);
                w32!(sp + 0x4c, 0);
                w32!(sp + 0x3c, 0);
                let lookup = gcall!(0x0042_1ed0, on_this, [0x10], 0x00b6_6f4c);
                let (x0, y0, z0) = (r32!(lookup + 0x40), r32!(lookup + 0x44), r32!(lookup + 0x48));
                w32!(sp + 0x40, x0);
                w32!(sp + 0x44, y0);
                w32!(sp + 0x48, z0);
                w32!(sp + 0x4c, 0);
                let lookup = gcall!(0x0042_1ed0, on_this, [0x14], 0x00b6_6f7f);
                let (x1, y1, z1) = (r32!(lookup + 0x40), r32!(lookup + 0x44), r32!(lookup + 0x48));
                w32!(sp + 0x30, x1);
                w32!(sp + 0x34, y1);
                w32!(sp + 0x38, z1);
                w32!(sp + 0x3c, 0);
                // pull each anchor up to the probed ground (minus a margin)
                for (point, scale_back, back) in [(sp + 0x40, 0x00b6_6fb0u32, 0x00b6_6fdcu32), (sp + 0x30, 0x00b6_7032, 0x00b6_705e)] {
                    if probe!(point, scale_back, back) {
                        let margin = rf!(exe::MARGIN);
                        let anchor_y = rf!(point + 4);
                        let ground_y = rf!(sp + 0x54);
                        if !below(ground_y, anchor_y - margin) {
                            let out_x = rf!(sp + 0x50);
                            let moved = ground_y + margin;
                            let out_z = rf!(sp + 0x58);
                            wf!(point + 4, moved);
                            wf!(point, out_x);
                            wf!(point + 8, out_z);
                            wf!(point + 0xc, 0.0);
                        }
                    }
                }
                hand_to_pair!(sp + 0x40, sp + 0x30, 0x00b6_70c0, 0x00b6_70d3);
                sweep_to_zero!();
                set_active!(1);
            }
        }
    }

    // 0x00B6756B
    let pair = r32!(at(0x2e74));
    if r8!(pair.wrapping_add(0x7d)) != 0 {
        let (a16, a17, a13, a14, a15) = (r8!(sp + 0x16), r8!(sp + 0x17), r8!(sp + 0x13), r8!(sp + 0x14), r8!(sp + 0x15));
        w8!(at(0x40e1), a16);
        w8!(at(0x41b1), a17);
        w8!(at(0x4281), a13);
        w8!(at(0x4351), a14);
        w8!(at(0x4421), a15);
    }
    vcall!(this, 0x50, [], 0x00b6_75b6);
    vcall!(this, 0x2e0, [], 0x00b6_75c2);
    let arg = r32!(at(0x20d4));
    vcall!(this, 0x70, [arg], 0x00b6_75d2);
    let object = r32!(at(0x2e74));
    if object != 0 {
        gcall!(0x0081_f2d0, Regs { esi: object, ..Regs::default() }, [], 0x00b6_75e1);
    }
    let object = r32!(at(0x2e74));
    if object != 0 {
        let held = r8!(at(0x4051));
        w8!(at(0x4051), 0);
        let reads_buttons = r8!(object.wrapping_add(0x7d)) != 0;
        w8!(sp + 0x17, held);
        if reads_buttons {
            for field in [0x40e1u32, 0x41b1, 0x4281, 0x4351, 0x4421] {
                w8!(at(field), 0);
            }
        }
        w8!(at(0x5729), 1);
        gcall!(0x0081_f8f0, Regs::default(), [object], 0x00b6_7636);
        let object = r32!(at(0x2e74));
        w8!(at(0x5729), 0);
        gcall!(0x0081_fc60, Regs { eax: object, ..Regs::default() }, [], 0x00b6_7648);
        let object = r32!(at(0x2e74));
        if r8!(object.wrapping_add(0x7d)) != 0 {
            let (a13, a14, a15) = (r8!(sp + 0x13), r8!(sp + 0x14), r8!(sp + 0x15));
            w8!(at(0x40e1), a13);
            w8!(at(0x4281), a13);
            w8!(at(0x41b1), a14);
            w8!(at(0x4351), a14);
            w8!(at(0x4421), a15);
        }
        let steady = gcall!(0x00b8_36a0, on_this, [], 0x00b6_7686) & 0xff;
        if steady != 0 {
            let handler = r32!(at(0x3d6c));
            if handler != 0 {
                vcall!(handler, 0x54, [], 0x00b6_769b);
            }
        }
        let held = r8!(sp + 0x17);
        w8!(at(0x4051), held);
    }
    if r8!(at(0x1ee4)) != 0 {
        let object = r32!(at(0x2e74));
        if object != 0 {
            gcall!(0x0081_fd10, Regs { edi: object, ..Regs::default() }, [1, 1], 0x00b6_76c1);
        }
    }
    // the floor offset
    let table = r32!(at(0x368));
    let index = r8!(table) as u32;
    let mut floor = None;
    if index != 0xff {
        let record = index.wrapping_mul(160).wrapping_add(r32!(at(0x364)));
        if record != 0 {
            let rise = rf!(record.wrapping_add(0x44)) - rf!(at(0xe4));
            if !below(rise, rf!(exe::FLOOR_LIMIT)) {
                floor = Some(rise - rf!(exe::FLOOR_STEP));
            }
        }
    }
    let floor = floor.unwrap_or_else(|| rf!(exe::NO_FLOOR));
    let state = r32!(exe::GAME_STATE);
    wf!(at(0x29f0), floor);
    let frozen = r32!(state.wrapping_add(0xb8844)) & 0x0040_0000 != 0;
    'tail: {
        let player_bits = |m: &dyn Mem| pg.u8(m, at(0x206d)) & 6 != 0;
        if !frozen {
            if !player_bits(m) {
                break 'tail;
            }
            let object = r32!(at(0x3dec));
            gcall!(0x0083_7980, Regs::default(), [at(0x2698), object], 0x00b6_7742);
        }
        if !player_bits(m) {
            break 'tail;
        }
        gcall!(0x00b6_7930, Regs { eax: this, ..Regs::default() }, [], 0x00b6_7752);
        if !player_bits(m) {
            break 'tail;
        }
        let object = r32!(at(0x3dec));
        if object != 0 {
            let handler = r32!(at(0x3d6c));
            let value = if handler == 0 {
                0
            } else if r8!(handler.wrapping_add(0x28)) == 1 && r8!(handler.wrapping_add(0x29)) == 1 {
                0
            } else {
                1
            };
            w8!(object.wrapping_add(0x359), value);
        }
        if !player_bits(m) {
            break 'tail;
        }
        let state = r32!(exe::GAME_STATE);
        let v = through_x87(r32!(state.wrapping_add(0x58)));
        w32!(at(0x2510), v);
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}
