//! Semantic ports (L184, subagent s1): native rewrites of per-unit update
//! functions that keep the original's ABI-visible result (callee-saved
//! registers, ESP, every guest memory write at or above the returned stack,
//! and every guest call with its arguments) instead of replaying its
//! instructions. See `semantic.rs` for the conventions.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// x87 `fld dword` / `fstp dword` round trip of a float's bits: identical
/// except that a signalling NaN comes back quiet (bit 22 set).
fn through_x87(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

fn float(pg: Pages, m: &dyn Mem, address: u32) -> f32 {
    f32::from_bits(pg.u32(m, address))
}

fn set_float(pg: Pages, m: &mut dyn Mem, address: u32, v: f32) {
    pg.set_u32(m, address, v.to_bits());
}

// Translated from 0x004241f0 (DDDA.exe 2017-12-13 Steam).
/// 0x004241F0 (register convention, `EAX` = record): initialise the 0x68-byte
/// effect request the timer update below hands to 0x00429680. Only the bits
/// of the flags word at +0x24 it keeps are read from the stale record.
fn init_effect_request(pg: Pages, m: &mut dyn Mem, r: u32) {
    let at = |o: u32| r.wrapping_add(o);
    pg.set_u32(m, at(0xc), 0);
    pg.set_u32(m, at(0x1c), 0);
    pg.set_u32(m, at(0x3c), 0);
    let v = pg.u32(m, 0x018c_fe80);
    pg.set_u32(m, at(0), v);
    let v = pg.u32(m, 0x018c_fe84);
    pg.set_u32(m, at(4), v);
    let v = pg.u32(m, 0x018c_fe88);
    pg.set_u32(m, at(8), v);
    pg.set_u32(m, at(0xc), 0);
    let v = pg.u32(m, 0x018c_fe80);
    pg.set_u32(m, at(0x10), v);
    let v = pg.u32(m, 0x018c_fe84);
    pg.set_u32(m, at(0x14), v);
    let v = pg.u32(m, 0x018c_fe88);
    pg.set_u32(m, at(0x18), v);
    pg.set_u32(m, at(0x1c), 0);
    let flags = pg.u32(m, at(0x24));
    pg.set_u32(m, at(0x20), u32::MAX);
    pg.set_u32(m, at(0x24), (flags & 0xffff_f346) | 0x346);
    let v = pg.u32(m, 0x0162_9510);
    pg.set_u32(m, at(0x30), v);
    let v = pg.u32(m, 0x0162_9514);
    pg.set_u32(m, at(0x34), v);
    let v = pg.u32(m, 0x0162_9518);
    pg.set_u32(m, at(0x38), v);
    pg.set_u32(m, at(0x3c), 0);
    let flags = pg.u32(m, at(0x24));
    pg.set_u32(m, at(0x24), flags & 0x8000_0fff);
    pg.set_u32(m, at(0x40), u32::MAX);
    pg.set_u32(m, at(0x44), 0);
    pg.set_u32(m, at(0x48), 0x17);
    pg.set_u32(m, at(0x4c), 0);
    pg.set_u32(m, at(0x50), 0);
    pg.set_u32(m, at(0x54), u32::MAX);
    pg.set_u32(m, at(0x58), 0);
    pg.set_u32(m, at(0x5c), 0);
    pg.set_u32(m, at(0x60), 0xffff);
    pg.set_u32(m, at(0x64), 0xff);
}

// Translated from 0x005d1af0 (DDDA.exe 2017-12-13 Steam), FUN_005D1AF0.
/// 0x005D1AF0, `update(timers, unit)` (stdcall, `ret 8`): per-frame update
/// of the periodic effect timers of one unit.
///
/// `timers` holds `[timers + 8]` timer records of 0x70 bytes at
/// `[timers + 4]`. `unit` has up to 8 (`[unit + 0x374]`, capped) state slots
/// of 0x140 bytes from `unit + 0x380`, each with a current time at +0x4c, a
/// threshold at +0x58 and a type (u16) at +0x4. A timer record applies to
/// slot `s` when its slot field (+8) is 0xff or `s`; per slot it keeps a
/// fired flag (u32 at `0x2c + 4 s`) and the time last seen (f32 at
/// `0x4c + 4 s`); it has a start delay +0xc, period +0x14, end time +0x10
/// and next-fire time +0x6c. Slots in `[lo, lo + extent)` named by
/// `unit + 0x1fcc` (when its bytes +8 and +9 are set; otherwise lo = 0xff
/// and no range) are skipped. A record of another type resets its flag and
/// last time (-1). Otherwise the time reaching the start delay arms the
/// flag and the next-fire time, and a timer whose next-fire time has come
/// (advancing by the period, honouring the end time) fires: the flag is set,
/// a 0x68-byte effect request is built in the original's stack frame
/// ([`init_effect_request`] plus the record's fields) and handed to
/// 0x00429680 `(DAT_018D2818, record.+0x20, request)` with ECX/EDX =
/// record.+0x24/+0x28. The slot's time is then stored as the last time seen.
pub fn update_effect_timers(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let unit = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    if unit != 0 {
        let count = pg.u32(m, pg.u32(m, entry.wrapping_add(4)).wrapping_add(8));
        if count != 0 {
            let slots = pg.u32(m, unit.wrapping_add(0x374)).min(8);
            if slots != 0 {
                let range = unit.wrapping_add(0x1fcc);
                let (mut lo, mut extent) = (0xffu32, 0u32);
                if range != 0 && pg.u8(m, range.wrapping_add(8)) != 0 && pg.u8(m, range.wrapping_add(9)) != 0 {
                    lo = pg.u32(m, range.wrapping_add(0x20));
                    extent = pg.u32(m, range.wrapping_add(0x24));
                }
                // the original's frame after its prologue: its locals start here
                let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x94 + 12);
                let request = frame.wrapping_add(0x30);
                for index in 0..slots {
                    let slot = unit.wrapping_add(0x380).wrapping_add(index.wrapping_mul(0x140));
                    let skipped = extent != 0 && index >= lo && index < lo.wrapping_add(extent);
                    if slot == 0 || skipped {
                        continue;
                    }
                    let offset = 0x2c + 4 * index;
                    let mut record_offset = 0u32;
                    for _ in 0..count {
                        let record = pg.u32(m, pg.u32(m, entry.wrapping_add(4)).wrapping_add(4)).wrapping_add(record_offset);
                        record_offset = record_offset.wrapping_add(0x70);
                        if record == 0 {
                            continue;
                        }
                        let at = |o: u32| record.wrapping_add(o);
                        let (flag, last) = (at(offset), at(offset + 0x20));
                        let applies = pg.u32(m, at(8));
                        if applies != 0xff && applies != index {
                            continue;
                        }
                        let now = |m: &dyn Mem| float(pg, m, slot.wrapping_add(0x4c));
                        if pg.u16(m, slot.wrapping_add(4)) as u32 != pg.u32(m, at(4)) {
                            pg.set_u32(m, flag, 0);
                            let minus_one = pg.u32(m, 0x0161_c20c);
                            pg.set_u32(m, last, minus_one);
                            continue;
                        }
                        // time went backwards past the start delay: restart
                        if float(pg, m, last) > now(m)
                            && float(pg, m, at(0xc)) >= float(pg, m, slot.wrapping_add(0x58))
                            && pg.u8(m, at(0x1c)) == 0
                        {
                            pg.set_u32(m, flag, 0);
                            let v = float(pg, m, at(0x14)) + float(pg, m, at(0xc));
                            set_float(pg, m, at(0x6c), v);
                        }
                        // not yet fired and the start delay reached: arm
                        let mut armed = false;
                        if pg.u32(m, flag) == 0 {
                            let delay = float(pg, m, at(0xc));
                            if now(m) >= delay {
                                armed = true;
                                let v = float(pg, m, at(0x14)) + delay;
                                set_float(pg, m, at(0x6c), v);
                            }
                        }
                        // periodic firing
                        let period = float(pg, m, at(0x14));
                        let mut fire = false;
                        if period > 0.0 {
                            let end = float(pg, m, at(0x10));
                            if end == 0.0 || end > float(pg, m, at(0x6c)) {
                                let next = float(pg, m, at(0x6c));
                                if now(m) >= next {
                                    set_float(pg, m, at(0x6c), next + period);
                                    fire = true;
                                }
                            }
                        }
                        if fire || armed {
                            pg.set_u32(m, flag, 1);
                            init_effect_request(pg, m, request);
                            pg.set_u32(m, request.wrapping_add(0x4c), unit);
                            let kind = pg.u8(m, at(0x19));
                            let mut flags = if kind == 0 { 8 } else { 0 };
                            if pg.u8(m, at(0x1a)) == 0 {
                                flags |= 0x10;
                            }
                            if pg.u8(m, at(0x1b)) != 0 {
                                flags |= 0x20;
                            }
                            let id = pg.u32(m, at(4));
                            let delay = pg.u32(m, at(0xc));
                            pg.set_u32(m, request.wrapping_add(0x60), id);
                            pg.set_u32(m, request.wrapping_add(0x44), flags | 0x100);
                            let mode = (((kind & 1) << 1) | (pg.u8(m, at(0x18)) & 1)) as u32;
                            let word = pg.u32(m, request.wrapping_add(0x24));
                            pg.set_u32(m, request.wrapping_add(0x24), (mode << 29) | ((word | 0x1000_0000) & 0x9fff_ffff));
                            pg.set_u32(m, request.wrapping_add(0x58), delay);
                            let end = pg.u32(m, at(0x10));
                            pg.set_u32(m, request.wrapping_add(0x64), index);
                            pg.set_u32(m, request.wrapping_add(0x5c), end);
                            let (effect, ecx, edx) = (pg.u32(m, at(0x20)), pg.u32(m, at(0x24)), pg.u32(m, at(0x28)));
                            let sink = pg.u32(m, 0x018d_2818);
                            (c.ebx, c.ebp, c.esi, c.edi) = (offset, entry.wrapping_sub(4), record, unit);
                            c.edx = edx;
                            call_from(m, c, pg, frame, ecx, &[sink, effect, request], 0x005d_1d46, |m, c| {
                                ddnative_base::x86::call_ext(m, c, 0x0042_9680)
                            });
                        }
                        // fld / fstp: the slot's time becomes the last time seen
                        let t = through_x87(pg.u32(m, slot.wrapping_add(0x4c)));
                        pg.set_u32(m, last, t);
                    }
                }
            }
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(12);
}

/// Calls the virtual method at vtable slot 0x3c of the runtime's tuning-value
/// object (`[[0x018FA4BC] + 0xB8754]`) with `index` and returns its x87
/// result as a float; 0.0 when the runtime has no such object. `frame` is
/// the original's stack pointer at its call sites.
fn tuning_value(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, index: u32, back: u32) -> f32 {
    let runtime = pg.u32(m, 0x018f_a4bc);
    let object = pg.u32(m, runtime.wrapping_add(0xb8754));
    if object == 0 {
        return 0.0;
    }
    let method = pg.u32(m, pg.u32(m, object).wrapping_add(0x3c));
    call_from(m, c, pg, frame, object, &[index], back, |m, c| ddnative_base::x86::call_ext(m, c, method));
    // fstp dword: the value leaves the x87 stack as a float
    let bits = c.st.top_f32().unwrap_or(0);
    c.st.pop();
    f32::from_bits(bits)
}

// Translated from 0x0084c0a0 (DDDA.exe 2017-12-13 Steam), uCharacterBase::vf92.
/// 0x0084C0A0, `character->update_frame()` (thiscall, plain `ret`): the
/// per-frame update of a character (virtual 92 of uCharacterBase).
///
/// 1. Frame time. The delta time `this[0x14]` is scaled for this frame: 0
///    while the freeze flag (`this[0x26A0] & 0x800`) is set and
///    `this[0x2864] >= 1.0` (which also sets bit 0 of `this[0x1DE0]` once,
///    tracked by the byte `this[0x286D]`; leaving the freeze clears it), and
///    times `this[0x27B0]` under flag 0x1000 (unless the player-flag byte
///    `this[0x206D] & 1` is set with `this[0x2DD4] == 0xE4`).
/// 2. Speed blend `this[0x29A8]` (default 1.0). With any of
///    `this[0x2368] & 0x80001C`, the blend between `this[0x29A4]` and
///    `this[0x29A0]` (or 0.95) follows the speed `this[0x29B4]` against two
///    tuning values (indexes 10 and 11 of the runtime's tuning object,
///    [`tuning_value`]) and 25.0; the state at `this + 0x2A90` is asked
///    through 0x00423B80 when `this[0x2B30]` is set. The player flag forces
///    the blend to 1.0 when 0x00B809D0 (`this->has_action(0x70)`) says so.
/// 3. The scaled time (blend times scale, times the one-shot factor
///    `this[0x2DB0]` armed by `this[0x2DAE]`) is stored in `this[0x2EEC]`
///    and as `this[0x14]` while the base model update 0x00886210 runs, then
///    the original delta time is put back. Then 0x0081F280 (when
///    `this[0x2E74]` is set) and 0x0080B450 (`this + 0x2E40`) run, and the
///    two action-slot bits `this[0x494] & 0x80/0x100` call 0x004219A0 with
///    the slot ids at `this[0x484] + 0xE/0x10`.
/// 4. The timer `this[0x2EA8]` counts down by the delta time (calling
///    0x004215E0 with `this[0x2EAC]`) under byte flags 0x206C/0x206E, and
///    for slots 9 and 10 newly set in `this[0x494]` (against `this[0x4A4]`)
///    the vector `this[0xE0..0xF0]` is handed to 0x00CADE50 for the slot's
///    id, inside the global instance's lock (EnterCriticalSection through
///    the import at 0x0139D160, leave at 0x0139D164).
pub fn update_character_frame(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let at = |o: u32| this.wrapping_add(o);
    // the original's frame after its prologue and three pushes; its calls are made here
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x34 + 12);
    let vector = frame.wrapping_add(0x20);
    c.ebp = entry.wrapping_sub(4);
    c.edi = this;
    let one = float(pg, m, 0x0153_19b8);

    // 1. frame time
    let dt = float(pg, m, at(0x14));
    let mut scaled = dt;
    if pg.u32(m, at(0x26a0)) & 0x800 != 0 && float(pg, m, at(0x2864)) >= one {
        scaled = 0.0;
        if pg.u8(m, at(0x286d)) == 0 {
            let v = pg.u32(m, at(0x1de0)) | 1;
            pg.set_u32(m, at(0x1de0), v);
        }
        pg.set_u8(m, at(0x286d), 1);
    } else {
        if pg.u8(m, at(0x286d)) != 0 {
            let v = pg.u32(m, at(0x1de0)) & 0xffff_fffe;
            pg.set_u32(m, at(0x1de0), v);
        }
        pg.set_u8(m, at(0x286d), 0);
    }
    if pg.u32(m, at(0x26a0)) & 0x1000 != 0 && !(pg.u8(m, at(0x206d)) & 1 != 0 && pg.u32(m, at(0x2dd4)) == 0xe4) {
        scaled = float(pg, m, at(0x27b0)) * scaled;
    }

    // 2. speed blend
    set_float(pg, m, at(0x29a8), one);
    if pg.u32(m, at(0x2368)) & 0x80_001c != 0 {
        let speed = float(pg, m, at(0x29b4));
        let mut plain = pg.u32(m, at(0x2b30)) == 0;
        if !plain {
            c.esi = at(0x2a90);
            c.eax = 0;
            call_from(m, c, pg, frame, 0, &[], 0x0084_c1b2, |m, c| ddnative_base::x86::call_ext(m, c, 0x0042_3b80));
            plain = c.eax & 0x0c00_2000 == 0;
            if !plain {
                let low = tuning_value(m, c, pg, frame, 10, 0x0084_c1da);
                let t = if speed >= low {
                    let high = tuning_value(m, c, pg, frame, 11, 0x0084_c213);
                    let t = speed / high;
                    if t > one { one } else { t }
                } else {
                    0.0
                };
                let (rest, target) = (float(pg, m, at(0x29a4)), float(pg, m, at(0x29a0)));
                set_float(pg, m, at(0x29a8), (target - rest) * (one - t) + rest);
            }
        }
        if plain {
            let start = float(pg, m, 0x014f_a314);
            if speed >= start {
                let high = tuning_value(m, c, pg, frame, 11, 0x0084_c2bb);
                let t = (speed - start) / (high - start);
                let t = if t > one { one } else { t };
                let rest = float(pg, m, at(0x29a4));
                // the original blends towards 0.95 in double precision
                let target = f64::from_bits(((pg.u32(m, 0x0161_d594) as u64) << 32) | pg.u32(m, 0x0161_d590) as u64);
                let blend = (target - rest as f64) * ((one - t) as f64) + rest as f64;
                set_float(pg, m, at(0x29a8), blend as f32);
            }
        }
    }
    if pg.u8(m, at(0x206d)) & 1 != 0 {
        c.edx = 0x70;
        c.eax = 0;
        call_from(m, c, pg, frame, this, &[], 0x0084_c343, |m, c| ddnative_base::x86::call_ext(m, c, 0x00b8_09d0));
        if c.eax & 0xff != 0 {
            set_float(pg, m, at(0x29a8), one);
        }
    }

    // 3. scaled time, base model update
    let one_shot = pg.u8(m, at(0x2dae)) != 0;
    let mut time = float(pg, m, at(0x29a8)) * scaled;
    if one_shot {
        time *= float(pg, m, at(0x2db0));
    }
    pg.set_u8(m, at(0x2dae), 0);
    set_float(pg, m, at(0x2db0), one);
    set_float(pg, m, at(0x2eec), time);
    set_float(pg, m, at(0x14), time);
    call_from(m, c, pg, frame, this, &[], 0x0084_c391, |m, c| ddnative_base::x86::call_ext(m, c, 0x0088_6210));
    let attachment = pg.u32(m, at(0x2e74));
    set_float(pg, m, at(0x14), dt);
    if attachment != 0 {
        c.eax = attachment;
        call_from(m, c, pg, frame, this, &[], 0x0084_c3ab, |m, c| ddnative_base::x86::call_ext(m, c, 0x0081_f280));
    }
    c.eax = at(0x2e40);
    call_from(m, c, pg, frame, this, &[], 0x0084_c3b6, |m, c| ddnative_base::x86::call_ext(m, c, 0x0080_b450));
    let (mut bit, mut word) = (0x80u32, 0xeu32);
    loop {
        if pg.u32(m, at(0x494)) & bit != 0 {
            let table = pg.u32(m, at(0x484));
            let id = if table == 0 { 0 } else { pg.u16(m, word.wrapping_add(table)) as u32 };
            let instance = pg.u32(m, 0x018d_0b6c);
            c.ebx = bit;
            c.esi = word;
            c.eax = pg.u32(m, at(0x210c));
            call_from(m, c, pg, frame, id, &[instance, id, this, this], 0x0084_c3f2, |m, c| {
                ddnative_base::x86::call_ext(m, c, 0x0042_19a0)
            });
        }
        word += 2;
        bit = bit.rotate_left(1);
        if word > 0x10 {
            break;
        }
    }
    c.ebx = bit;
    c.esi = word;

    // 4. countdown timer, per-slot vectors
    if pg.u8(m, at(0x206c)) & 0x80 != 0 || pg.u8(m, at(0x206e)) & 0x10 != 0 {
        let timer = float(pg, m, at(0x2ea8));
        if timer > 0.0 {
            let id = pg.u32(m, at(0x2eac));
            let v = timer - float(pg, m, at(0x14));
            let instance = pg.u32(m, 0x018d_0b6c);
            set_float(pg, m, at(0x2ea8), v);
            c.eax = instance;
            call_from(m, c, pg, frame, 0, &[id, this, 0], 0x0084_c43f, |m, c| ddnative_base::x86::call_ext(m, c, 0x0042_15e0));
        } else {
            set_float(pg, m, at(0x2ea8), 0.0);
        }
    }
    let flags = pg.u8(m, at(0x206d));
    if flags & 2 != 0 || flags & 1 == 0 {
        for slot in 9u32..=10 {
            let now = pg.u32(m, at(0x494));
            let before = pg.u32(m, at(0x4a4));
            if (((before ^ now) & now) >> (slot & 31)) & 1 == 0 {
                continue;
            }
            // the slot's vector, copied to the frame (the callee reads it)
            for k in 0..4 {
                let v = pg.u32(m, at(0xe0 + 4 * k));
                pg.set_u32(m, vector.wrapping_add(4 * k), v);
            }
            let table = pg.u32(m, at(0x484));
            let id = if table == 0 { 0 } else { pg.u16(m, table.wrapping_add(slot * 2)) as u32 };
            let instance = pg.u32(m, 0x018d_0b6c);
            c.ebx = instance;
            let locked = |m: &dyn Mem| pg.u8(m, instance.wrapping_add(0x1c)) != 0 || pg.u8(m, 0x018d_0bb0) != 0;
            if locked(m) {
                let enter = pg.u32(m, 0x0139_d160);
                call_from(m, c, pg, frame, 0, &[instance.wrapping_add(4)], 0x0084_c4fe, |m, c| {
                    ddnative_base::x86::call_ext(m, c, enter)
                });
            }
            if pg.u32(m, instance.wrapping_add(0xce0)) != 0 {
                let handlers = pg.u32(m, instance.wrapping_add(0xcf8));
                if handlers != 0 {
                    let list = pg.u32(m, handlers.wrapping_add(0x70));
                    c.eax = pg.u32(m, list.wrapping_add(id.wrapping_mul(4)));
                    call_from(m, c, pg, frame, 0, &[slot.wrapping_sub(7), vector], 0x0084_c52e, |m, c| {
                        ddnative_base::x86::call_ext(m, c, 0x00ca_de50)
                    });
                }
            }
            if locked(m) {
                let leave = pg.u32(m, 0x0139_d164);
                call_from(m, c, pg, frame, 0, &[instance.wrapping_add(4)], 0x0084_c547, |m, c| {
                    ddnative_base::x86::call_ext(m, c, leave)
                });
            }
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}
