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
pub fn init_effect_request(pg: Pages, m: &mut dyn Mem, r: u32) {
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
                                crate::semantic::call_guest(m, c, 0x0042_9680)
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
    call_from(m, c, pg, frame, object, &[index], back, |m, c| crate::semantic::call_guest(m, c, method));
    // fstp dword [frame + 0x1c]: the value leaves the x87 stack as a float
    c.st.real_memory::<0x18d9, 4, true>(m, frame.wrapping_add(0x1c));
    let bits = pg.u32(m, frame.wrapping_add(0x1c));
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
            call_from(m, c, pg, frame, 0, &[], 0x0084_c1b2, |m, c| crate::semantic::call_guest(m, c, 0x0042_3b80));
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
        call_from(m, c, pg, frame, this, &[], 0x0084_c343, |m, c| crate::semantic::call_guest(m, c, 0x00b8_09d0));
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
    call_from(m, c, pg, frame, this, &[], 0x0084_c391, |m, c| crate::semantic::call_guest(m, c, 0x0088_6210));
    let attachment = pg.u32(m, at(0x2e74));
    set_float(pg, m, at(0x14), dt);
    if attachment != 0 {
        c.eax = attachment;
        call_from(m, c, pg, frame, this, &[], 0x0084_c3ab, |m, c| crate::semantic::call_guest(m, c, 0x0081_f280));
    }
    c.eax = at(0x2e40);
    call_from(m, c, pg, frame, this, &[], 0x0084_c3b6, |m, c| crate::semantic::call_guest(m, c, 0x0080_b450));
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
                crate::semantic::call_guest(m, c, 0x0042_19a0)
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
            call_from(m, c, pg, frame, 0, &[id, this, 0], 0x0084_c43f, |m, c| crate::semantic::call_guest(m, c, 0x0042_15e0));
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
                    crate::semantic::call_guest(m, c, enter)
                });
            }
            if pg.u32(m, instance.wrapping_add(0xce0)) != 0 {
                let handlers = pg.u32(m, instance.wrapping_add(0xcf8));
                if handlers != 0 {
                    let list = pg.u32(m, handlers.wrapping_add(0x70));
                    c.eax = pg.u32(m, list.wrapping_add(id.wrapping_mul(4)));
                    call_from(m, c, pg, frame, 0, &[slot.wrapping_sub(7), vector], 0x0084_c52e, |m, c| {
                        crate::semantic::call_guest(m, c, 0x00ca_de50)
                    });
                }
            }
            if locked(m) {
                let leave = pg.u32(m, 0x0139_d164);
                call_from(m, c, pg, frame, 0, &[instance.wrapping_add(4)], 0x0084_c547, |m, c| {
                    crate::semantic::call_guest(m, c, leave)
                });
            }
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x0076f2d0 (DDDA.exe 2017-12-13 Steam).
/// 0x0076F2D0, `refresh_slots(actor)` (stdcall, `ret 4`): for an actor whose
/// controller object (`actor[0xCD0]`) derives from the class `[0x018D2D10]`
/// (found by walking the parent links, +0x10, of the DTI node the object's
/// virtual method 4 returns) either hand the actor to 0x0076F430 (the
/// controller is also an uActorModel, DTI 0x018D2108) or, when the byte
/// `actor[0xD99]` is set, refresh the actor's 8 equipment slots
/// (`actor[0xC3C + 4 s]`, each with a model id at +0x70).
///
/// The slot ids are 16 groups of u16 values at `actor[(0x1C + 0x10 g + k) * 4]`
/// (g = 0..7, k = 0..15), used when bit g of `actor[0xD3C]` is set and the
/// presence byte at `actor + 0x72 + 0x40 g + 4 k` is non-zero. An id divided
/// by 1000 names the equipment model; the first slot holding that model, if
/// its flag byte (+0x50) bit 0 is set and 0x004C9B80 `(actor, slot)` accepts
/// it, is then updated through 0x0076F660 `(actor, slot)`.
pub fn refresh_actor_slots(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let actor = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    // the original's frame after its prologue (sub esp,0x10 and four pushes)
    let frame = entry.wrapping_sub(0x20);
    c.ebp = actor;
    let controller = pg.u32(m, actor.wrapping_add(0xcd0));
    if controller == 0 {
        return finish(c);
    }
    let method = pg.u32(m, pg.u32(m, controller).wrapping_add(0x10));
    let mut node = call_from(m, c, pg, frame, controller, &[], 0x0076_f2f0, |m, c| crate::semantic::call_guest(m, c, method));
    let class = pg.u32(m, 0x018d_2d10);
    while pg.u32(m, node.wrapping_add(4)) != class {
        node = pg.u32(m, node.wrapping_add(0x10));
        if node == 0 {
            return finish(c);
        }
    }
    let controller = pg.u32(m, actor.wrapping_add(0xcd0));
    let is_model = call_from(m, c, pg, frame, controller, &[0x018d_2108], 0x0076_f32c, |m, c| {
        crate::semantic::call_guest(m, c, 0x0041_9210)
    });
    if is_model != 0 {
        call_from(m, c, pg, frame, 0, &[actor], 0x0076_f336, |m, c| crate::semantic::call_guest(m, c, 0x0076_f430));
        return finish(c);
    }
    if pg.u8(m, actor.wrapping_add(0xd99)) == 0 {
        return finish(c);
    }
    let mut groups = pg.u32(m, actor.wrapping_add(0xd3c));
    for group in 0..8u32 {
        if groups & 1 != 0 {
            for k in 0..16u32 {
                let present = actor.wrapping_add(0x72 + 0x40 * group + 4 * k);
                if pg.u8(m, present) == 0 {
                    continue;
                }
                let id = pg.u16(m, actor.wrapping_add((0x1c + 0x10 * group + k) * 4)) as u32;
                let model = id / 1000;
                c.ebx = k;
                for slot in 0..8u32 {
                    let equipment = pg.u32(m, actor.wrapping_add(0xc3c + 4 * slot));
                    c.esi = slot;
                    c.edi = equipment;
                    if equipment == 0 || pg.u32(m, equipment.wrapping_add(0x70)) != model {
                        continue;
                    }
                    let equipment = pg.u32(m, actor.wrapping_add(0xc3c + 4 * slot));
                    if equipment != 0 && pg.u8(m, equipment.wrapping_add(0x50)) & 1 != 0 {
                        c.eax = actor;
                        let ok = call_from(m, c, pg, frame, 0, &[slot], 0x0076_f3e4, |m, c| {
                            crate::semantic::call_guest(m, c, 0x004c_9b80)
                        });
                        if ok != 0 {
                            call_from(m, c, pg, frame, 0, &[actor, slot], 0x0076_f3ef, |m, c| {
                                crate::semantic::call_guest(m, c, 0x0076_f660)
                            });
                        }
                    }
                    break;
                }
            }
        }
        groups >>= 1;
    }
    finish(c)
}

// ---------------------------------------------------------------------------
// 0x00C88380: nDraw::DDMaterialStd::vf12, the standard material's state sync
// ---------------------------------------------------------------------------

/// The draw state ("render context") a material writes its bindings into.
/// Every binding slot is two words: its value at `slot` and a tag at
/// `slot + 4`. With the record flag (`byte [ctx + 0x5c] & 0x80`) set, every
/// change first pushes the old (value, tag) pair onto the undo journal at
/// `ctx + 0x30C7C` (12-byte entries: code, value, tag; count at `ctx +
/// 0x6C`). `ctx + 0x154` collects dirty bits: 1 = values, 2 = tags.
struct RenderContext {
    pg: Pages,
    ctx: u32,
}

impl RenderContext {
    fn word(&self, m: &dyn Mem, offset: u32) -> u32 {
        self.pg.u32(m, self.ctx.wrapping_add(offset))
    }

    fn set_word(&self, m: &mut dyn Mem, offset: u32, v: u32) {
        self.pg.set_u32(m, self.ctx.wrapping_add(offset), v);
    }

    /// `ctx[0x154] |= bits`.
    fn mark_dirty(&self, m: &mut dyn Mem, bits: u32) {
        let v = self.word(m, 0x154) | bits;
        self.set_word(m, 0x154, v);
    }

    /// Records the slot's current (value, tag) in the undo journal under
    /// `code`, when recording is on.
    fn journal(&self, m: &mut dyn Mem, code: u32, slot: u32) {
        if self.pg.u8(m, self.ctx.wrapping_add(0x5c)) & 0x80 == 0 {
            return;
        }
        let count = self.word(m, 0x6c);
        let entry = 0x30c7c_u32.wrapping_add(count.wrapping_mul(12));
        self.set_word(m, entry, code);
        let (value, tag) = (self.word(m, slot), self.word(m, slot + 4));
        self.set_word(m, entry + 4, value);
        self.set_word(m, entry + 8, tag);
        let count = self.word(m, 0x6c);
        self.set_word(m, 0x6c, count.wrapping_add(1));
    }

    /// A binding that follows a source value: when `slot` differs from
    /// `[[ctx + 0x20] + source]`, journal it, take the value, set its tag and
    /// mark both dirty bits.
    fn follow(&self, m: &mut dyn Mem, slot: u32, source: u32, journal_code: u32, tag: u32) {
        let shared = self.word(m, 0x20);
        let current = self.word(m, slot);
        if current == self.pg.u32(m, shared.wrapping_add(source)) {
            return;
        }
        self.journal(m, journal_code, slot);
        let shared = self.word(m, 0x20);
        let v = self.pg.u32(m, shared.wrapping_add(source));
        self.set_word(m, slot, v);
        self.set_word(m, slot + 4, tag);
        self.mark_dirty(m, 3);
    }

    /// Releases a binding: when its value is set, journal it, clear value
    /// and tag (tag change marks 2) and mark 1.
    fn release(&self, m: &mut dyn Mem, slot: u32, journal_code: u32) {
        if self.word(m, slot) == 0 {
            return;
        }
        self.journal(m, journal_code, slot);
        self.set_word(m, slot, 0);
        if self.word(m, slot + 4) != 0 {
            self.set_word(m, slot + 4, 0);
            self.mark_dirty(m, 2);
        }
        self.mark_dirty(m, 1);
    }

    /// Binds texture `texture` (or none) to a sampler slot when it differs
    /// from the slot's: journal, stamp the texture with `frame + 1` (+4), set
    /// the slot value and the tag `[texture + 0xC] | kind`.
    fn bind_texture(&self, m: &mut dyn Mem, slot: u32, journal_code: u32, texture: u32, frame: u32, kind: u32) {
        if self.word(m, slot) == texture {
            return;
        }
        self.journal(m, journal_code, slot);
        let mut tag = 0;
        if texture != 0 {
            self.pg.set_u32(m, texture.wrapping_add(4), frame.wrapping_add(1));
            self.set_word(m, slot, texture);
            tag = self.pg.u32(m, texture.wrapping_add(0xc)) | kind;
        } else {
            self.set_word(m, slot, 0);
        }
        if self.word(m, slot + 4) != tag {
            self.set_word(m, slot + 4, tag);
            self.mark_dirty(m, 2);
        }
        self.mark_dirty(m, 1);
    }

    /// Re-copies the constant block `[[ctx + 0x18] + holder]` (u16 word count
    /// at +0x18) held in `ctx[field]` into fresh frame memory from the
    /// context's bump allocator (`ctx[0x24]`, limit `ctx[0x28]`, grown by
    /// 0x00FED890 `ctx->grow(bytes)`), and marks dirty bit 1. Returns the new
    /// copy. `memcpy` (0x012EFEA0) is done natively.
    fn recopy_block(&self, m: &mut dyn Mem, c: &mut Cpu, frame: u32, holder: u32, field: u32, grow_back: u32, from_block: bool) -> u32 {
        let pg = self.pg;
        let block = pg.u32(m, self.word(m, 0x18).wrapping_add(holder));
        let bytes = ((pg.u16(m, block.wrapping_add(0x18)) as u32) * 4 + 0xf) & 0xffff_fff0;
        if self.word(m, 0x24).wrapping_add(bytes) > self.word(m, 0x28) {
            c.ebx = bytes;
            call_from(m, c, pg, frame, self.ctx, &[bytes], grow_back, |m, c| crate::semantic::call_guest(m, c, 0x00fe_d890));
        }
        let copy = self.word(m, 0x24);
        self.set_word(m, 0x24, copy.wrapping_add(bytes));
        let size = if from_block && copy == 0 { 0 } else { (pg.u16(m, block.wrapping_add(0x18)) as u32) * 4 };
        let source = if size == 0 { 0 } else if from_block { pg.u32(m, block.wrapping_add(0x24)) } else { self.word(m, field) };
        // the CRT memcpy behaves like memmove: read everything before writing
        let words: Vec<u32> = (0..size).step_by(4).map(|k| pg.u32(m, source.wrapping_add(k))).collect();
        for (k, v) in words.into_iter().enumerate() {
            pg.set_u32(m, copy.wrapping_add(4 * k as u32), v);
        }
        self.set_word(m, field, copy);
        self.mark_dirty(m, 1);
        copy
    }
}

// Translated from 0x00c88380 (DDDA.exe 2017-12-13 Steam), nDraw::DDMaterialStd::vf12.
/// 0x00C88380, `material->sync(ctx, arg) -> result` (thiscall, `ret 8`): the
/// standard material's per-draw state sync into the draw context `ctx`
/// (`DDMaterialStd::vf12`). By the material's mode (`this[0xA4]`):
/// - mode 2: follows the shared state's values (`[ctx + 0x20] + 0x27B8..`,
///   seven bindings at `ctx + 0x29B0`) and releases the 16 slots at
///   `ctx + 0x1B90`; `arg` becomes 0x14;
/// - always: the mode-dependent binding at `ctx + 0x2EB0`, the bindings at
///   `ctx + 0x2BB8`, `0x2DA8`, `0x2D78` and the releases at `0x1C50`,
///   `0x1CA8`;
/// then runs the base class sync 0x01145BF0 `(this, ctx, arg)` and, by mode:
/// - not 0: binds the five textures `this[0xD0..0xE4]` to the sampler slots
///   at `ctx + 0x2068` (stamped with the frame counter `[0x018D2CE8]`); when
///   the material has not the flag `0x75A5334B` it sets the two parameters
///   0x54408001 = 0.0 and 0x54409001 = 1.0; and re-copies the constant block
///   `0xC2C` into frame memory, whose float at +0x6C is set by the flag
///   `0x64C43356` (mode 3 also clears +0x48);
/// - 0: releases the five sampler slots;
/// then re-copies the constant blocks 0xC08, 0x7B8 and 0xC1C.
/// Returns the base class result.
pub fn sync_standard_material(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let mut arg = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after its five pushes
    let frame = entry.wrapping_sub(20);
    let rc = RenderContext { pg, ctx };
    (c.ebx, c.ebp, c.esi, c.edi) = (this, 1, ctx, 0);
    let mode = |m: &dyn Mem| pg.u32(m, this.wrapping_add(0xa4));

    if mode(m) == 2 {
        for i in 0..7 {
            rc.follow(m, 0x29b0 + 8 * i, 0x27b8 + 8 * i, 0x4f7 + i, 0x4f7 + i);
        }
        for j in 0..16 {
            rc.release(m, 0x1b90 + 8 * j, 0x333 + j);
        }
        arg = 0x14;
    }
    if mode(m) == 1 {
        rc.follow(m, 0x2eb0, 0x2cb8, 0x597, 0x597);
    } else {
        rc.follow(m, 0x2eb0, 0x2cc8, 0x597, 0x950a_6599);
    }
    rc.follow(m, 0x2bb8, 0x29c0, 0x538, 0x538);
    rc.release(m, 0x1c50, 0x34b);
    rc.follow(m, 0x2da8, 0x2bb0, 0x576, 0x576);
    rc.follow(m, 0x2d78, 0x2b80, 0x570, 0x570);
    rc.release(m, 0x1ca8, 0x356);

    c.ebx = this;
    let result = call_from(m, c, pg, frame, this, &[ctx, arg], 0x00c8_94ed, |m, c| crate::semantic::call_guest(m, c, 0x0114_5bf0));
    let query = |m: &mut dyn Mem, c: &mut Cpu, id: u32, back: u32| {
        call_from(m, c, pg, frame, this, &[id], back, |m, c| crate::semantic::call_guest(m, c, 0x0106_6af0))
    };
    if mode(m) != 0 {
        const SAMPLER_KINDS: [u32; 5] = [0xe700_7000, 0x83b0_f000, 0x1295_9000, 0x93d3_a000, 0x999e_7000];
        let frame_counter = pg.u32(m, 0x018d_2ce8);
        for (i, kind) in SAMPLER_KINDS.into_iter().enumerate() {
            let texture = pg.u32(m, this.wrapping_add(0xd0 + 4 * i as u32));
            rc.bind_texture(m, 0x2068 + 8 * i as u32, 0x3ce + i as u32, texture, frame_counter, kind);
        }
        if query(m, c, 0x75a5_334b, 0x00c8_989b) == 0 {
            for (parameter, value_back) in [(0x5440_8001u32, 0x00c8_98b4u32), (0x5440_9001, 0x00c8_98ce)] {
                let value = if parameter == 0x5440_8001 { 0 } else { pg.u32(m, 0x0153_19b8) };
                call_from(m, c, pg, frame, this, &[parameter, value], value_back, |m, c| {
                    crate::semantic::call_guest(m, c, 0x0106_71f0)
                });
            }
        }
        let copy = rc.recopy_block(m, c, frame, 0xc2c, 0x1a50, 0x00c8_98fb, false);
        if copy != 0 {
            let on = query(m, c, 0x64c4_3356, 0x00c8_9944) != 0;
            let weight = if on { pg.u32(m, 0x0153_19b8) } else { 0 };
            pg.set_u32(m, copy.wrapping_add(0x6c), weight);
            if mode(m) == 3 {
                pg.set_u32(m, copy.wrapping_add(0x48), 0);
            }
        }
    } else {
        for i in 0..5 {
            rc.release(m, 0x2068 + 8 * i, 0x3ce + i);
        }
    }
    rc.recopy_block(m, c, frame, 0xc08, 0x1a08, 0x00c8_9c74, false);
    rc.recopy_block(m, c, frame, 0x7b8, 0x1168, 0x00c8_9ccb, false);
    rc.recopy_block(m, c, frame, 0xc1c, 0x1a30, 0x00c8_9d22, false);
    c.eax = result;
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(12);
}

// Translated from 0x00c25ea0 (DDDA.exe 2017-12-13 Steam), uOmSwingBaseInstancing::vf11.
/// 0x00C25EA0, `instancing->update(ctx)` (thiscall, `ret 4`): the instanced
/// object's per-draw state update (`uOmSwingBaseInstancing::vf11`). `this`
/// embeds a sub-object at `this + 0x470` (called `part` here) whose virtual
/// methods do the work; `ctx` is the draw context.
///
/// 1. When the object is active (`ctx[0x16A] & 0x208`, the culling record
///    `P = ctx[0x1370] & ~0xF` has `P[5] != 0`, and the byte `this[0x15F0]`
///    is set): with `P[4] != 0` it builds six clip planes from `P`'s four
///    floats (`p0..p3`: `(c, p3 - p0)`, `(-c, p0 + p3)`, and likewise for
///    `p1` and `p2`, with the plane normals `c` from the constants at
///    0x01629530/40/50 and their sign-flipped copies) in frame memory,
///    clears the cache with 0x00FED1E0, copies the constant block
///    `[[ctx + 0x18] + 0x554]` and the planes into the context's bump
///    allocator (`ctx[0x24]`, grown by 0x00FED890), points `ctx[0xCA0]` at
///    it and calls the part's method 0x34 `(ctx)`. With `P[4] == 0` it only
///    remembers (flag A) that there were no planes.
/// 2. Calls the part's method 0x2C `(ctx, ((this[0x108] << 19) >> 28), flag A)`.
///    If it accepts: copies `this[0x108] >> 25 & 0xF` and `this[0x100]` into
///    the part (+0x1C, +0x20), calls 0x00FA5860 `(ctx)` when `this[0x449] == 1`,
///    then methods 0x3C, and (planes built) 0x38 with the clip-binding
///    restore below, then 0x30. If it declines: only 0x38 and the restore,
///    when planes were built.
/// The restore puts the clip binding `ctx[0xCA0]` back to its previous
/// value (or `[ctx[0x20] + 0xAA8]` when there was none), journalled as
/// entry 0x155, when it changed.
pub fn update_instancing(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after its prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x74 + 12);
    let part = this.wrapping_add(0x470);
    let rc = RenderContext { pg, ctx };
    (c.ebp, c.esi, c.edi) = (entry.wrapping_sub(4), ctx, part);
    let culling = pg.u32(m, ctx.wrapping_add(0x1370)) & 0xffff_fff0;
    let active = pg.u16(m, ctx.wrapping_add(0x16a)) & 0x208 != 0
        && pg.u32(m, culling.wrapping_add(0x14)) != 0
        && pg.u8(m, this.wrapping_add(0x15f0)) != 0;
    let previous = pg.u32(m, ctx.wrapping_add(0xca0)) & 0xffff_fff0;
    pg.set_u8(m, frame.wrapping_add(0x14), 0);
    let mut built = false;
    // virtual call on the part: `part->method(args)`
    let part_call = |m: &mut dyn Mem, c: &mut Cpu, slot: u32, args: &[u32], back: u32| {
        let method = pg.u32(m, pg.u32(m, part).wrapping_add(slot));
        call_from(m, c, pg, frame, part, args, back, |m, c| crate::semantic::call_guest(m, c, method));
    };
    if active {
        if pg.u32(m, culling.wrapping_add(0x10)) == 0 {
            pg.set_u8(m, frame.wrapping_add(0x14), 1);
        } else {
            c.ebx = previous;
            call_from(m, c, pg, frame, ctx, &[], 0x00c2_5f0f, |m, c| crate::semantic::call_guest(m, c, 0x00fe_d1e0));
            let f = |m: &dyn Mem, o: u32| float(pg, m, culling.wrapping_add(o));
            let (p3, p0) = (f(m, 0xc), f(m, 0));
            let block = pg.u32(m, pg.u32(m, ctx.wrapping_add(0x18)).wrapping_add(0x554));
            let normals0 = [pg.u32(m, 0x0162_9530), pg.u32(m, 0x0162_9534), pg.u32(m, 0x0162_9538)];
            let top = pg.u32(m, ctx.wrapping_add(0x24));
            let p1 = f(m, 4);
            let p2 = f(m, 8);
            let sign = pg.u32(m, 0x0161_c280);
            let normals1 = [pg.u32(m, 0x0162_9540), pg.u32(m, 0x0162_9544), pg.u32(m, 0x0162_9548)];
            let size = ((pg.u16(m, block.wrapping_add(0x18)) as u32) * 4 + 0xf) & 0xffff_fff0;
            let normals2 = [pg.u32(m, 0x0162_9550), pg.u32(m, 0x0162_9554), pg.u32(m, 0x0162_9558)];
            // six planes: (normal, offset), (-normal, offset)
            let flip = |n: [u32; 3]| [n[0] ^ sign, n[1] ^ sign, n[2] ^ sign];
            let planes: [[u32; 4]; 6] = [
                [normals0[0], normals0[1], normals0[2], (p3 - p0).to_bits()],
                [flip(normals0)[0], flip(normals0)[1], flip(normals0)[2], (p0 + p3).to_bits()],
                [normals1[0], normals1[1], normals1[2], (p3 - p1).to_bits()],
                [flip(normals1)[0], flip(normals1)[1], flip(normals1)[2], (p1 + p3).to_bits()],
                [normals2[0], normals2[1], normals2[2], (p3 - p2).to_bits()],
                [flip(normals2)[0], flip(normals2)[1], flip(normals2)[2], (p2 + p3).to_bits()],
            ];
            c.edi = size;
            if top.wrapping_add(size) > pg.u32(m, ctx.wrapping_add(0x28)) {
                call_from(m, c, pg, frame, ctx, &[size], 0x00c2_606b, |m, c| crate::semantic::call_guest(m, c, 0x00fe_d890));
            }
            let buffer = pg.u32(m, ctx.wrapping_add(0x24));
            pg.set_u32(m, ctx.wrapping_add(0x24), buffer.wrapping_add(size));
            pg.set_u32(m, ctx.wrapping_add(0xca0), buffer);
            rc.mark_dirty(m, 1);
            if buffer != 0 {
                // memcpy(buffer, block->source, block->words * 4), memmove semantics
                let bytes = (pg.u16(m, block.wrapping_add(0x18)) as u32) * 4;
                let source = pg.u32(m, block.wrapping_add(0x24));
                let words: Vec<u32> = (0..bytes).step_by(4).map(|k| pg.u32(m, source.wrapping_add(k))).collect();
                for (k, v) in words.into_iter().enumerate() {
                    pg.set_u32(m, buffer.wrapping_add(4 * k as u32), v);
                }
            }
            for (k, plane) in planes.iter().enumerate() {
                for (j, v) in plane.iter().enumerate() {
                    pg.set_u32(m, buffer.wrapping_add(16 * k as u32 + 4 * j as u32), *v);
                }
            }
            c.ebx = buffer;
            part_call(m, c, 0x34, &[ctx], 0x00c2_60e5);
            built = true;
        }
    }
    // the flag A dword the original pushes includes three stale stack bytes
    let flag_a = pg.u32(m, frame.wrapping_add(0x14));
    let field = ((pg.u32(m, this.wrapping_add(0x108)) << 19) as i32 >> 28) as u32;
    let mut base = previous;
    c.ebx = previous;
    let accepted = {
        let method = pg.u32(m, pg.u32(m, part).wrapping_add(0x2c));
        call_from(m, c, pg, frame, part, &[ctx, field, flag_a], 0x00c2_611f, |m, c| crate::semantic::call_guest(m, c, method));
        c.eax & 0xff != 0
    };
    // after planes are built: the clip binding goes back to `base`
    let restore = |m: &mut dyn Mem, c: &mut Cpu, base: &mut u32| {
        part_call(m, c, 0x38, &[ctx], if accepted { 0x00c2_6171 } else { 0x00c2_624a });
        if *base == 0 {
            *base = pg.u32(m, pg.u32(m, ctx.wrapping_add(0x20)).wrapping_add(0xaa8));
        }
        let current = pg.u32(m, ctx.wrapping_add(0xca0)) & 0xffff_fff0;
        if current != *base || pg.u32(m, ctx.wrapping_add(0xca4)) != 0 {
            rc.journal(m, 0x155, 0xca0);
            let different = pg.u32(m, ctx.wrapping_add(0xca4)) != 0;
            pg.set_u32(m, ctx.wrapping_add(0xca0), *base);
            if different {
                pg.set_u32(m, ctx.wrapping_add(0xca4), 0);
                rc.mark_dirty(m, 2);
            }
            rc.mark_dirty(m, 1);
        }
    };
    if accepted {
        let (mode, flags) = (pg.u32(m, this.wrapping_add(0x108)), pg.u32(m, this.wrapping_add(0x100)));
        pg.set_u32(m, part.wrapping_add(0x1c), (mode >> 25) & 0xf);
        pg.set_u32(m, part.wrapping_add(0x20), flags);
        if pg.u8(m, this.wrapping_add(0x449)) == 1 {
            c.ecx = this;
            call_from(m, c, pg, frame, this, &[ctx], 0x00c2_6152, |m, c| crate::semantic::call_guest(m, c, 0x00fa_5860));
        }
        part_call(m, c, 0x3c, &[ctx], 0x00c2_615c);
        if built {
            restore(m, c, &mut base);
        }
        part_call(m, c, 0x30, &[ctx], 0x00c2_622c);
    } else if built {
        restore(m, c, &mut base);
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(8);
}

// Translated from 0x00e6d070 (DDDA.exe 2017-12-13 Steam).
/// 0x00E6D070, `owner->count_active_children()` (fastcall, ECX = owner,
/// plain `ret`): counts the nodes of the owner's child list (head
/// `owner[0x1F0]`, next link at node +8) whose flag byte +0xC has bit 0 set
/// and whose virtual method 0x44 `is_active()` returns non-zero, into the
/// u16 `owner[0x1E2]`.
pub fn count_active_children(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let owner = c.ecx;
    let saved = (c.ebx, c.esi, c.edi);
    // the original's frame after its three pushes
    let frame = entry.wrapping_sub(12);
    let counter = owner.wrapping_add(0x1e2);
    let mut node = pg.u32(m, owner.wrapping_add(0x1f0));
    pg.set_u16(m, counter, 0);
    (c.ebx, c.edi) = (1, owner);
    while node != 0 {
        if pg.u8(m, node.wrapping_add(0xc)) & 1 != 0 {
            let method = pg.u32(m, pg.u32(m, node).wrapping_add(0x44));
            c.esi = node;
            call_from(m, c, pg, frame, node, &[], 0x00e6_d09e, |m, c| crate::semantic::call_guest(m, c, method));
            if c.eax & 0xff != 0 {
                let v = pg.u16(m, counter).wrapping_add(1);
                pg.set_u16(m, counter, v);
            }
        }
        node = pg.u32(m, node.wrapping_add(8));
    }
    (c.ebx, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x007781a0 (DDDA.exe 2017-12-13 Steam).
/// 0x007781A0, `object->tick()` (register convention: EAX = object, plain
/// `ret`): the per-frame timer update of a 0x1E4-byte gauge object. The time
/// step is `dt` = `[[obj + 0x1B4] + 0x14]` (or 1.0 without that owner).
/// - two gauges grow by rate times `dt` and stop at their limit: `obj[0x10]`
///   (rate +0x18, limit +0x14) and `obj[0x1C]` (rate +0x24, limit +0x20);
/// - two hold timers (flag bytes +0x34 / +0x40, elapsed +0x38 / +0x44,
///   duration +0x3C / +0x48) reset their gauge to its limit and stop when
///   the duration is exceeded;
/// - the step index `obj[0x1C0]` follows the owner's mode (`[owner + 0xD14]`)
///   and `obj[0x1BC]`, advances by `obj[0x1E0]` and wraps (to 0 or 4);
/// - cooldowns `obj[0x1C4]`, `obj[0x1C8]` count down to 0;
/// - the 20 timers `obj[0x114 + 4 i]` count up to `obj[0x1D0]`; the matching
///   charge `obj[0x70 + 4 i]` drains by `dt` times `obj[0x1D8]` once a timer
///   passed that limit, `obj[0x1D4]` once past `obj[0x1CC]`, never below 0;
/// - `obj[0x1E0]` is cleared.
pub fn tick_gauges(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let obj = c.eax;
    let at = |o: u32| obj.wrapping_add(o);
    let one = float(pg, m, 0x0153_19b8);
    let owner = pg.u32(m, at(0x1b4));
    let dt = if owner != 0 { float(pg, m, owner.wrapping_add(0x14)) } else { one };
    // a gauge grows by rate * dt up to its limit
    let limit_a = float(pg, m, at(0x14));
    let a = float(pg, m, at(0x18)) * dt + float(pg, m, at(0x10));
    set_float(pg, m, at(0x10), if a >= limit_a { limit_a } else { a });
    let limit_b = float(pg, m, at(0x20));
    let b = float(pg, m, at(0x24)) * dt + float(pg, m, at(0x1c));
    set_float(pg, m, at(0x1c), if b >= limit_b { limit_b } else { b });
    // hold timers
    if pg.u8(m, at(0x34)) == 1 {
        let t = float(pg, m, at(0x38)) + dt;
        set_float(pg, m, at(0x38), t);
        if t > float(pg, m, at(0x3c)) {
            set_float(pg, m, at(0x10), limit_a);
            set_float(pg, m, at(0x38), 0.0);
            pg.set_u8(m, at(0x34), 0);
        }
    }
    if pg.u8(m, at(0x40)) == 1 {
        let t = float(pg, m, at(0x44)) + dt;
        set_float(pg, m, at(0x44), t);
        if t > float(pg, m, at(0x48)) {
            set_float(pg, m, at(0x1c), limit_b);
            set_float(pg, m, at(0x44), 0.0);
            pg.set_u8(m, at(0x40), 0);
        }
    }
    // step index
    let source = pg.u32(m, at(0x1b8));
    if source != 0 {
        match pg.u32(m, source.wrapping_add(0xd14)) {
            0 => {
                let v = pg.u32(m, at(0x1bc));
                pg.set_u32(m, at(0x1c0), v);
            }
            1 => match pg.u32(m, at(0x1bc)) {
                1 => pg.set_u32(m, at(0x1c0), 0),
                2 => pg.set_u32(m, at(0x1c0), 1),
                3 => pg.set_u32(m, at(0x1c0), 2),
                _ => {}
            },
            _ => {}
        }
        let advance = pg.u32(m, at(0x1e0));
        let step = pg.u32(m, at(0x1c0)).wrapping_add(advance);
        pg.set_u32(m, at(0x1c0), step);
        if step > 4 {
            pg.set_u32(m, at(0x1c0), if (advance as i32) < 0 { 0 } else { 4 });
        }
    }
    // cooldowns
    for offset in [0x1c4, 0x1c8] {
        let v = float(pg, m, at(offset)) - dt;
        set_float(pg, m, at(offset), if 0.0 > v { 0.0 } else { v });
    }
    // the 20 timers and their charges
    let limit = float(pg, m, at(0x1d0));
    for i in 0..20u32 {
        let timer = at(0x114 + 4 * i);
        let t = dt + float(pg, m, timer);
        let drain = if t > limit {
            set_float(pg, m, timer, limit);
            float(pg, m, at(0x1d8))
        } else {
            set_float(pg, m, timer, t);
            if t > float(pg, m, at(0x1cc)) { float(pg, m, at(0x1d4)) } else { 0.0 }
        };
        let charge = at(0x70 + 4 * i);
        let v = float(pg, m, charge) - drain * dt;
        set_float(pg, m, charge, if 0.0 > v { 0.0 } else { v });
    }
    pg.set_u32(m, at(0x1e0), 0);
    c.esp = entry.wrapping_add(4);
}

// Translated from 0x00de50b0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DE50B0, `this->get_triangle(out0, out1, out2)` (thiscall, `ret 0xc`):
/// copies three 16-byte-aligned vertices out of `this[0x3c]` (an array of
/// 16-byte vectors) as 4-float vectors `(x, y, z, 0.0)`; their indices are
/// the u16 values at `this[0x38] + 0xC / 0xE / 0x10`. The original copies
/// the floats through the x87 stack, so a signalling NaN comes out quiet.
pub fn load_triangle_vertices(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let header = pg.u32(m, this.wrapping_add(0x38));
    let vertices = pg.u32(m, this.wrapping_add(0x3c));
    for k in 0..3u32 {
        let out = pg.u32(m, entry.wrapping_add(4 + 4 * k));
        let index = pg.u16(m, header.wrapping_add(0xc + 2 * k)) as u32;
        let from = vertices.wrapping_add(index.wrapping_mul(16));
        for j in 0..3u32 {
            let v = through_x87(pg.u32(m, from.wrapping_add(4 * j)));
            if j == 2 {
                pg.set_u32(m, out.wrapping_add(12), 0);
            }
            pg.set_u32(m, out.wrapping_add(4 * j), v);
        }
    }
    c.esp = entry.wrapping_add(16);
}

/// Bit-exact `a*b + c*d`-style helper: the original adds left to right, so
/// every sum below is written in the original's association order.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn dot4(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32, g: f32, h: f32) -> f32 {
    a * b + c * d + e * f + g * h
}

/// The 4x4 float matrix of an object (row-major): its 16 floats from `base`.
fn read_matrix(pg: Pages, m: &dyn Mem, base: u32) -> [[f32; 4]; 4] {
    let mut v = [[0.0f32; 4]; 4];
    for (i, row) in v.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = float(pg, m, base.wrapping_add(16 * i as u32 + 4 * j as u32));
        }
    }
    v
}

fn write_matrix(pg: Pages, m: &mut dyn Mem, base: u32, v: &[[f32; 4]; 4]) {
    for (i, row) in v.iter().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            set_float(pg, m, base.wrapping_add(16 * i as u32 + 4 * j as u32), *cell);
        }
    }
}

// Translated from 0x00ea42e0 (DDDA.exe 2017-12-13 Steam).
/// 0x00EA42E0, `node->cull_bounds(culler, owner, planes) -> planes`
/// (thiscall, `ret 0xC`): tests the node's bounding volume against a
/// frustum culler and returns the culler's plane mask (`0x3F` = nothing left
/// to test, returned at once). `culler` is the callee object (ECX of the
/// culler methods), `owner[0x2C]` the shape data, the node's flags
/// `this[0x108]`, its world matrix the 16 floats at `this + 0xB0`
/// (rows = x axis, y axis, z axis, translation).
///
/// Shape kind (flags & 0x3F == 3: shape data transformed by the matrix):
/// - a sphere (`((flags >> 1) | flags) & 0x40`): centre = shape (x, y, z)
///   (`shape[0x10..0x18]`) transformed by the matrix, radius =
///   `sqrt(largest squared axis length) * shape[0x1C]`; 0x00FF0800
///   `culler.test_sphere(&centre_radius, planes)`; if some planes remain,
///   the shape's own matrix (0x00433B00 `build(shape + 0x40)`) is combined
///   with the node's and handed to 0x00FF0AD0 `culler.test_oriented(matrix,
///   planes)`;
/// - a box (`flags & 0x100`): 0x005B19C0 fills the box corners from
///   `shape + 0x20`, which are shifted by the translation, tested with
///   0x00FF0D20 `culler.test_box(corners, planes)` and, when some planes
///   remain, as above (the product's association order differs);
/// - else the plain box `shape + 0x20` through 0x00FF0D20 and the oriented
///   box `shape + 0x40` through 0x00FF0AD0.
/// For any other kind the same sphere and box tests run without the
/// transform step: only 0x00FF0800, the shifted-corner 0x00FF0D20, or the
/// plain 0x00FF0D20.
pub fn cull_bounds(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let node = c.ecx;
    let culler = pg.u32(m, entry.wrapping_add(4));
    let owner = pg.u32(m, entry.wrapping_add(8));
    let planes = pg.u32(m, entry.wrapping_add(12));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let finish = |c: &mut Cpu, result: u32| {
        c.eax = result;
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(16);
    };
    if planes == 0x3f {
        return finish(c, 0x3f);
    }
    // the original's frame after its prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xd4 + 12);
    let matrix_at = frame.wrapping_add(0x30);
    let sphere_at = frame.wrapping_add(0xc0);
    let corners_at = frame.wrapping_add(0x90);
    let flags = pg.u32(m, node.wrapping_add(0x108));
    let n = |o: u32| node.wrapping_add(o);
    let transformed = flags & 0x3f == 3;
    let sphere = ((flags >> 1) | flags) & 0x40 != 0;
    // calls on the culler: thiscall with ECX = culler
    let culler_call = |m: &mut dyn Mem, c: &mut Cpu, target: u32, args: &[u32], back: u32| -> u32 {
        call_from(m, c, pg, frame, culler, args, back, |m, c| crate::semantic::call_guest(m, c, target))
    };
    if sphere {
        let shape = pg.u32(m, owner.wrapping_add(0x2c));
        let a = read_matrix(pg, m, n(0xb0));
        // squared axis lengths and the largest of them
        let sq = |r: &[f32; 4]| ((r[0] * r[0] + r[1] * r[1]) + r[2] * r[2]) + r[3] * r[3];
        let (n0, n1, n2) = (sq(&a[0]), sq(&a[1]), sq(&a[2]));
        let largest = if n0 > n1 {
            if n0 > n2 { n0 } else { n2 }
        } else if n1 > n2 {
            n1
        } else {
            n2
        };
        let (x, y, z) = (float(pg, m, shape.wrapping_add(0x10)), float(pg, m, shape.wrapping_add(0x14)), float(pg, m, shape.wrapping_add(0x18)));
        // centre = ((y * row1 + x * row0) + z * row2) + row3, per lane
        let lane = |k: usize| ((y * a[1][k] + x * a[0][k]) + z * a[2][k]) + a[3][k];
        let radius = largest.sqrt() * float(pg, m, shape.wrapping_add(0x1c));
        for (k, v) in [lane(0), lane(1), lane(2), radius].into_iter().enumerate() {
            set_float(pg, m, sphere_at.wrapping_add(4 * k as u32), v);
        }
        let result = culler_call(m, c, 0x00ff_0800, &[sphere_at, planes], if transformed { 0x00ea_4497 } else { 0x00ea_5218 });
        if !transformed || (result as i32) <= 0 || result == 0x3f {
            return finish(c, result);
        }
        // the shape's own matrix, then the combined matrix
        call_from(m, c, pg, frame, matrix_at, &[shape.wrapping_add(0x40)], 0x00ea_44b7, |m, c| crate::semantic::call_guest(m, c, 0x0043_3b00));
        let mm = read_matrix(pg, m, matrix_at);
        let (b0, b4, b8, bc) = (a[0][0], a[0][1], a[0][2], a[0][3]);
        let (c0, c4, c8, cc) = (a[1][0], a[1][1], a[1][2], a[1][3]);
        let (d0, d4, d8, dc) = (a[2][0], a[2][1], a[2][2], a[2][3]);
        let (e0, e4, e8, ec) = (a[3][0], a[3][1], a[3][2], a[3][3]);
        let [[m00, m01, m02, m03], [m10, m11, m12, m13], [m20, m21, m22, m23], [m30, m31, m32, m33]] = mm;
        // partial products the original computes first (from the old matrix)
        let (f13, f14, f15, f7) = (b4 * m00, b8 * m00, m01 * c8, m01 * cc);
        let (f16, f17, f18, f19) = (bc * m00, m02 * dc, m10 * b4, m10 * b8);
        let (f9, f20, f10, f21, f22) = (m11 * c8, m10 * bc, m11 * cc, m12 * dc, m20 * b4);
        let p22 = dot4(m21, c8, m20, b8, m22, d8, m23, e8);
        let p23 = dot4(m21, cc, m20, bc, m22, dc, m23, ec);
        let p31 = dot4(m31, c4, m30, b4, m32, d4, m33, e4);
        let p32 = dot4(m31, c8, m30, b8, m32, d8, m33, e8);
        let p33 = dot4(m31, cc, m30, bc, m32, dc, m33, ec);
        let product = [
            [
                dot4(c0, m01, b0, m00, d0, m02, e0, m03),
                m01 * c4 + f13 + m02 * d4 + m03 * e4,
                f14 + f15 + m02 * d8 + m03 * e8,
                f7 + f16 + f17 + m03 * ec,
            ],
            [
                dot4(m11, c0, b0, m10, m12, d0, e0, m13),
                m11 * c4 + f18 + m12 * d4 + m13 * e4,
                f9 + f19 + m12 * d8 + m13 * e8,
                f10 + f20 + f21 + m13 * ec,
            ],
            [
                dot4(m21, c0, b0, m20, m22, d0, e0, m23),
                m21 * c4 + f22 + m22 * d4 + m23 * e4,
                p22,
                p23,
            ],
            [dot4(m31, c0, b0, m30, m32, d0, e0, m33), p31, p32, p33],
        ];
        write_matrix(pg, m, matrix_at, &product);
        let result = culler_call(m, c, 0x00ff_0ad0, &[matrix_at, result], 0x00ea_49c3);
        return finish(c, result);
    }
    let shape = pg.u32(m, owner.wrapping_add(0x2c));
    if flags & 0x100 != 0 {
        // box corners shifted by the node's translation
        call_from(m, c, pg, frame, corners_at, &[shape.wrapping_add(0x20)], if transformed { 0x00ea_4a0a } else { 0x00ea_5254 }, |m, c| {
            crate::semantic::call_guest(m, c, 0x005b_19c0)
        });
        let (ex, ey, ez) = (float(pg, m, n(0xe0)), float(pg, m, n(0xe4)), float(pg, m, n(0xe8)));
        for (offset, shift) in [(0x00, ex), (0x04, ey), (0x08, ez), (0x10, ex), (0x14, ey), (0x18, ez)] {
            let v = float(pg, m, corners_at.wrapping_add(offset)) + shift;
            set_float(pg, m, corners_at.wrapping_add(offset), v);
        }
        let result = culler_call(m, c, 0x00ff_0d20, &[corners_at, planes], if transformed { 0x00ea_4ab9 } else { 0x00ea_5301 });
        if !transformed || (result as i32) <= 0 || result == 0x3f {
            return finish(c, result);
        }
        call_from(m, c, pg, frame, matrix_at, &[shape.wrapping_add(0x40)], 0x00ea_4ae0, |m, c| crate::semantic::call_guest(m, c, 0x0043_3b00));
        let a = read_matrix(pg, m, n(0xb0));
        let mm = read_matrix(pg, m, matrix_at);
        let (b0, b4, b8, bc) = (a[0][0], a[0][1], a[0][2], a[0][3]);
        let (c0, c4, c8, cc) = (a[1][0], a[1][1], a[1][2], a[1][3]);
        let (d0, d4, d8, dc) = (a[2][0], a[2][1], a[2][2], a[2][3]);
        let (e0, e4, e8, ec) = (a[3][0], a[3][1], a[3][2], a[3][3]);
        let [[m00, m01, m02, m03], [m10, m11, m12, m13], [m20, m21, m22, m23], [m30, m31, m32, m33]] = mm;
        let product = [
            [
                dot4(b0, m00, m01, c0, m02, d0, m03, e0),
                dot4(b4, m00, c4, m01, d4, m02, e4, m03),
                dot4(c8, m01, b8, m00, d8, m02, e8, m03),
                dot4(cc, m01, bc, m00, dc, m02, ec, m03),
            ],
            [
                dot4(b0, m10, m11, c0, m12, d0, m13, e0),
                dot4(c4, m11, b4, m10, d4, m12, e4, m13),
                dot4(c8, m11, b8, m10, d8, m12, e8, m13),
                dot4(cc, m11, bc, m10, dc, m12, ec, m13),
            ],
            [
                dot4(b0, m20, m21, c0, m22, d0, m23, e0),
                dot4(c4, m21, b4, m20, d4, m22, e4, m23),
                dot4(c8, m21, b8, m20, d8, m22, e8, m23),
                cc * m21 + bc * m20 + dc * m22 + ec * m23,
            ],
            [
                dot4(b0, m30, m31, c0, m32, d0, m33, e0),
                dot4(c4, m31, b4, m30, d4, m32, e4, m33),
                c8 * m31 + b8 * m30 + d8 * m32 + e8 * m33,
                cc * m31 + bc * m30 + dc * m32 + ec * m33,
            ],
        ];
        write_matrix(pg, m, matrix_at, &product);
        let result = culler_call(m, c, 0x00ff_0ad0, &[matrix_at, result], 0x00ea_5052);
        return finish(c, result);
    }
    // plain box, and for the transformed kind the oriented box after it
    let result = culler_call(m, c, 0x00ff_0d20, &[shape.wrapping_add(0x20), planes], if transformed { 0x00ea_5074 } else { 0x00ea_5314 });
    if !transformed || (result as i32) <= 0 || result == 0x3f {
        return finish(c, result);
    }
    let result = culler_call(m, c, 0x00ff_0ad0, &[shape.wrapping_add(0x40), result], 0x00ea_5091);
    finish(c, result)
}

// Translated from 0x00f23260 (DDDA.exe 2017-12-13 Steam).
/// 0x00F23260, `object->draw_attachment(ctx)` (thiscall, `ret 4`): for an
/// object of kind 1 or 2 (`this[4] & 7`) with an attached effect node
/// `this[0x1E0]` that has data (`node[0xF0]`) and a weight `node[0x158]`
/// different from the constant at 0x01561738, asks the node for its slot
/// (0x00EA1EE0 `node->slot(ctx)`, negative = none), prepares it
/// (0x00EA1F40, the virtual method 0x64, 0x00EA1FD0) and draws it with
/// 0x00F1FD30 `this->draw(ctx, node[0xF0], node[0xF8], &scratch, slot,
/// prepared)`. Then it updates the draw context: the binding at
/// `ctx + 0x2E08` follows `shared[0x2C10]` and the slot at `ctx + 0x1D90` is
/// released; and when bit 30 of `node[0x108]` is set the binding at
/// `ctx + 0x2EB0` follows `shared[0x2CC8]` (tag 0x950A6599) and the constant
/// block `[[ctx + 0x18] + 0x89C]` is re-copied into frame memory
/// (`ctx[0x1330]`, journalled as 0x227) whose float at +8 is zeroed, with
/// bit 1 of `ctx[0x5C]` set around the journal entry.
pub fn draw_attachment(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let ctx = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    let kind = pg.u32(m, this.wrapping_add(4)) & 7;
    if kind != 2 && kind != 1 {
        return finish(c);
    }
    let node = pg.u32(m, this.wrapping_add(0x1e0));
    if node == 0 || pg.u32(m, node.wrapping_add(0xf0)) == 0 {
        return finish(c);
    }
    // the original's frame after its prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x14 + 12);
    let scratch = frame.wrapping_add(0x10);
    let weight = float(pg, m, node.wrapping_add(0x158));
    if weight == float(pg, m, 0x0156_1738) {
        return finish(c);
    }
    (c.esi, c.edi) = (ctx, this);
    let node_call = |m: &mut dyn Mem, c: &mut Cpu, target: u32, args: &[u32], back: u32| -> u32 {
        let node = pg.u32(m, this.wrapping_add(0x1e0));
        call_from(m, c, pg, frame, node, args, back, |m, c| crate::semantic::call_guest(m, c, target))
    };
    let slot = node_call(m, c, 0x00ea_1ee0, &[ctx], 0x00f2_32bf);
    if (slot as i32) < 0 {
        return finish(c);
    }
    c.ebx = slot;
    node_call(m, c, 0x00ea_1f40, &[scratch], 0x00f2_32d9);
    {
        let node = pg.u32(m, this.wrapping_add(0x1e0));
        let method = pg.u32(m, pg.u32(m, node).wrapping_add(0x64));
        call_from(m, c, pg, frame, node, &[ctx], 0x00f2_32e7, |m, c| crate::semantic::call_guest(m, c, method));
    }
    let prepared = node_call(m, c, 0x00ea_1fd0, &[ctx], 0x00f2_32f3);
    let node = pg.u32(m, this.wrapping_add(0x1e0));
    let (data_b, data_a) = (pg.u32(m, node.wrapping_add(0xf8)), pg.u32(m, node.wrapping_add(0xf0)));
    call_from(m, c, pg, frame, this, &[ctx, data_a, data_b, scratch, slot, prepared], 0x00f2_3316, |m, c| {
        crate::semantic::call_guest(m, c, 0x00f1_fd30)
    });
    // update the draw context
    let rc = RenderContext { pg, ctx };
    rc.follow(m, 0x2e08, 0x2c10, 0x582, 0x582);
    rc.release(m, 0x1d90, 0x373);
    let node = pg.u32(m, this.wrapping_add(0x1e0));
    if (pg.u32(m, node.wrapping_add(0x108)) >> 0x1e) & 1 == 0 {
        return finish(c);
    }
    rc.follow(m, 0x2eb0, 0x2cc8, 0x597, 0x950a_6599);
    let v = rc.word(m, 0x5c) | 2;
    rc.set_word(m, 0x5c, v);
    rc.journal(m, 0x227, 0x1330);
    let copy = rc.recopy_block(m, c, frame, 0x89c, 0x1330, 0x00f2_358d, true);
    pg.set_u32(m, copy.wrapping_add(8), 0);
    let v = rc.word(m, 0x5c) & 0xffff_fffd;
    rc.set_word(m, 0x5c, v);
    finish(c)
}

// Translated from 0x00fedb80 (DDDA.exe 2017-12-13 Steam).
/// 0x00FEDB80, `context->save_recording_state(id, mark)` (thiscall,
/// `ret 8`): begins a nested recording scope. The stack of saved scopes
/// (`context[0x30874]` deep, 0x8144 bytes of saved state each from
/// `context + 0x81F8`, a (mark, command count) pair each from
/// `context + 0x3084C`) gets the current state (`context + 0xB4`, 0x144
/// bytes plus 8 per sort entry `context[0x14]`), the scope counter
/// advances, `context[0x16A]` takes `id` and the cursors
/// `context[0x1D4 / 0x1D8 / 0x1DC / 0x180]` are cleared.
pub fn save_recording_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let at = |o: u32| this.wrapping_add(o);
    let mark = pg.u32(m, entry.wrapping_add(8));
    let depth = pg.u32(m, at(0x30874));
    let slot = at(0x3084c).wrapping_add(depth.wrapping_mul(8));
    pg.set_u32(m, slot, mark);
    let commands = pg.u32(m, at(0x48));
    pg.set_u32(m, slot.wrapping_add(4), commands);
    let bytes = pg.u32(m, at(0x14)).wrapping_mul(8).wrapping_add(0x144);
    let saved = at(0x81f8).wrapping_add(depth.wrapping_mul(0x8144));
    crate::semantic::copy_guest(m, pg, saved, at(0xb4), bytes);
    let id = pg.u16(m, entry.wrapping_add(4));
    let depth = pg.u32(m, at(0x30874));
    pg.set_u32(m, at(0x30874), depth.wrapping_add(1));
    pg.set_u16(m, at(0x16a), id);
    for offset in [0x1d4, 0x1d8, 0x1dc, 0x180] {
        pg.set_u32(m, at(offset), 0);
    }
    c.eax = 0;
    c.esp = entry.wrapping_add(12);
}

// Translated from 0x00fedc00 (DDDA.exe 2017-12-13 Steam).
/// 0x00FEDC00, `context->restore_recording_state()` (fastcall, plain
/// `ret`): ends the nested scope [`save_recording_state`] began. The scope
/// counter drops, the saved state is copied back, the commands recorded in
/// the scope (from the saved command count to `context[0x48]`) are moved to
/// the end of the sort list `context[0x44]` (kept in `context[0x4C]`
/// entries) and a 16-byte reference record for them is appended to the
/// frame arena (`context[0x24]`, grown by 0x00DA7D70 `allocate(0x1000)`), as
/// well as a sort entry for it and an entry in the record table at
/// `context + 0x30878` (count `context[0x30C78]`).
pub fn restore_recording_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let at = |o: u32| this.wrapping_add(o);
    // the original's frame after its four pushes
    let frame = entry.wrapping_sub(16);
    let depth = pg.u32(m, at(0x30874)).wrapping_sub(1);
    pg.set_u32(m, at(0x30874), depth);
    let bytes = pg.u32(m, at(0x14)).wrapping_mul(8).wrapping_add(0x144);
    let scope = at(0x81f8).wrapping_add(depth.wrapping_mul(0x8144));
    crate::semantic::copy_guest(m, pg, at(0xb4), scope, bytes);
    let depth = pg.u32(m, at(0x30874));
    let first = pg.u32(m, at(0x48)).wrapping_sub(pg.u32(m, at(0x30850).wrapping_add(depth.wrapping_mul(8))));
    let list = pg.u32(m, at(0x44));
    let slot = at(0x3084c).wrapping_add(depth.wrapping_mul(8));
    let tail = pg.u32(m, at(0x4c)).wrapping_sub(first);
    let moved_to = list.wrapping_add(tail.wrapping_mul(8));
    pg.set_u32(m, at(0x4c), tail);
    let from = list.wrapping_add(pg.u32(m, slot.wrapping_add(4)).wrapping_mul(8));
    crate::semantic::copy_guest(m, pg, moved_to, from, first.wrapping_mul(8));
    let commands = pg.u32(m, slot.wrapping_add(4));
    pg.set_u32(m, at(0x48), commands);
    if pg.u32(m, at(0x24)).wrapping_add(0x10) > pg.u32(m, at(0x28)) {
        c.ebx = slot;
        c.ebp = moved_to;
        c.edi = first;
        let allocator = pg.u32(m, 0x018d_08f4);
        let chunk = call_from(m, c, pg, frame, allocator, &[0x1000], 0x00fe_dc95, |m, c| crate::semantic::call_guest(m, c, 0x00da_7d70));
        pg.set_u32(m, at(0x24), chunk);
        pg.set_u32(m, at(0x28), if chunk != 0 { chunk.wrapping_add(0x1000) } else { 0 });
    }
    let record = pg.u32(m, at(0x24));
    pg.set_u32(m, at(0x24), record.wrapping_add(0x10));
    let word = (pg.u32(m, record) & 0xfff6_ffff) | 0x6_0000;
    pg.set_u32(m, record, word);
    pg.set_u32(m, record.wrapping_add(8), moved_to);
    let mark = pg.u32(m, slot);
    pg.set_u32(m, record.wrapping_add(4), mark);
    pg.set_u32(m, record.wrapping_add(12), first);
    let word = (pg.u32(m, at(0x6c)) << 20) | (word & 0xf_ffff);
    pg.set_u32(m, record, word);
    let word = ((pg.u32(m, at(0x74)) ^ word) & 0x7fff) ^ word;
    pg.set_u32(m, record, word);
    // the sort entry
    let index = pg.u32(m, at(0x48));
    let entry_at = pg.u32(m, at(0x44)).wrapping_add(index.wrapping_mul(8));
    pg.set_u32(m, at(0x48), index.wrapping_add(1));
    let sequence = pg.u32(m, at(0x74)).wrapping_add(1);
    pg.set_u32(m, at(0x74), sequence);
    let key = (pg.u32(m, entry_at) & 0x7ff_ffff) | (pg.u32(m, at(0x164)) << 27);
    pg.set_u32(m, entry_at, key);
    let sort = pg.u32(m, at(0x178)).wrapping_add(pg.u32(m, at(0x164)) >> 5);
    pg.set_u32(m, entry_at, ((sort ^ key) & 0x7ff_ffff) ^ key);
    pg.set_u32(m, entry_at.wrapping_add(4), record);
    // the record table
    let count = pg.u32(m, at(0x30c78));
    pg.set_u32(m, at(0x30878).wrapping_add(count.wrapping_mul(4)), record);
    let count = pg.u32(m, at(0x30c78)).wrapping_add(1);
    pg.set_u32(m, at(0x30c78), count);
    c.eax = record;
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}
