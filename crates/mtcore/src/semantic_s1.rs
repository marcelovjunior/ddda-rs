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
