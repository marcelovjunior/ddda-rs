//! Enemy recognition, `sRecognition::vf06` (0x00487B00; docs/FRAME_RATE.md
//! 4.1: dt-approximate, a frame-rate bug).
//!
//! **Observable-equivalent model, not wired; the high-frame-rate mode's
//! baseline.** The lifted body stays in the program: a strict port would
//! have to replay dead state (scratch registers carried from one phase
//! into the callouts of the next). This model is verified against
//! recordings of the original on its observable behaviour instead
//! (`crates/mtframe/tests/models.rs`): every byte of memory outside its
//! own stack frame, the order, targets and arguments of its calls, and
//! what it does with their results. It does not reproduce register
//! values nobody reads.
//!
//! Every `interval` base frames (the check interval, "frames at 30 fps"),
//! the manager gathers the candidates (three unit lists filtered by class,
//! and the player manager's three slots), drops the ones not alive and
//! engaged, keeps a `cEnemyInfo` per candidate and advances its timers:
//! drawn time while visible, hidden time while not. An enemy is
//! recognised after `recognise_after` seconds drawn and forgotten after
//! `forget_after` seconds hidden.
//!
//! The bug: the timers gain only **this frame's** step (`mDeltaTime /
//! mFps`) although the body runs once per interval. At 30 fps with
//! interval 1 it runs every frame and is exact; at F fps it runs every
//! ceil(F / 30) frames and the timers run at 30 / F of real time.

use crate::smain::Main;
use ddnative_base::mem::Mem;
use mtcore::array::{self, Array};
use mtcore::dti::{self, Dti};
use mtmath::scalar::{x87_copy, F32};

/// sRecognition's fields.
pub const TIMER: u32 = 0x20;
pub const INTERVAL: u32 = 0x24;
pub const NEXT_ID: u32 = 0x28;
pub const RECOGNISE_AFTER: u32 = 0x2C;
pub const FORGET_AFTER: u32 = 0x30;
/// The `cEnemyInfo` records: an MtArray at +0x34.
pub const INFOS: u32 = 0x34;

/// cEnemyInfo (0x50 bytes).
pub const INFO_SIZE: u32 = 0x50;
pub const INFO_VTABLE: u32 = 0x0156_0244;
pub const INFO_DTI: u32 = 0x0198_B568;
/// The third argument of the allocation (the DTI's +0x1C).
pub const INFO_TAG: u32 = 0x0198_B584;
pub const ID: u32 = 0x4;
pub const RECOGNISED: u32 = 0x8;
pub const DRAWN_NOW: u32 = 0x9;
pub const SEEN: u32 = 0xA;
pub const TARGET: u32 = 0xC;
pub const DRAWN: u32 = 0x10;
pub const HIDDEN: u32 = 0x14;
pub const SPECIES_VALUE: u32 = 0x40;

/// The three unit lists (manager pointer; class whose descriptor's name
/// pointer the candidates are matched against, by `isKindOf`). Each
/// manager holds a count at +0x24 and the units at +0x30.
pub const LISTS: [(u32, u32); 3] = [
    (0x018F_A4B0, 0x019A_1134),
    (0x018F_A4C8, 0x019A_3DB8),
    (0x018F_A4E0, 0x019A_3DE0),
];
/// The player manager: three unit slots at +0x9A0.
pub const PLAYERS: u32 = 0x018F_A4EC;
pub const PLAYER_SLOTS: [u32; 3] = [0x9A0, 0x9A4, 0x9A8];
/// The manager whose list (count +0xBE284, units +0xBE290) is always
/// recognised.
pub const ALWAYS: u32 = 0x018F_A4BC;

/// The local array's vtable (MtArray).
const ARRAY_VTABLE: u32 = array::VTABLE;

/// The calls the update makes (each through the original's convention).
pub trait Calls {
    /// The unit's virtual +0x10: its class descriptor.
    fn class_of(&mut self, m: &mut dyn Mem, unit: u32) -> u32;
    /// An allocator's virtual +0x20(size, align).
    fn allocate(&mut self, m: &mut dyn Mem, allocator: u32, size: u32, align: u32) -> u32;
    /// An allocator's virtual +0x1C(size, align, tag).
    fn allocate_tagged(&mut self, m: &mut dyn Mem, allocator: u32, size: u32, align: u32, tag: u32) -> u32;
    /// An allocator's virtual +0x24(block).
    fn free(&mut self, m: &mut dyn Mem, allocator: u32, block: u32);
    /// The object's deleting destructor (virtual slot 0, argument 1).
    fn delete(&mut self, m: &mut dyn Mem, object: u32);
    /// 0x005A02F0: the unit is alive and engaged (target in eax).
    fn alive(&mut self, m: &mut dyn Mem, unit: u32) -> bool;
    /// 0x008BC6E0: the species record of a unit (its bytes +0x2C, +0x2D),
    /// or 0.
    fn species(&mut self, m: &mut dyn Mem, unit: u32, a: u8, b: u8) -> u32;
    /// 0x00488440: the record's unit is visible (line of sight).
    fn visible(&mut self, m: &mut dyn Mem, info: u32) -> bool;
}

fn f(m: &dyn Mem, a: u32) -> F32 {
    F32::from_bits(m.u32(a))
}

fn array_allocator(m: &dyn Mem) -> u32 {
    dti::allocator_of(m, Dti(array::DTI)).0
}

/// Inline growth of a pointer array: a new block of `slots` pointers from
/// MtArray's allocator, zeroed, the first `count` copied, the old block
/// freed (also when it is null). The old block and the count are read
/// where the original reads them (after the allocation).
fn grow(
    m: &mut dyn Mem,
    calls: &mut impl Calls,
    slots: u32,
    old: impl Fn(&dyn Mem) -> u32,
    count: impl Fn(&dyn Mem) -> u32,
) -> u32 {
    let allocator = array_allocator(m);
    let size = slots.wrapping_mul(4);
    let new = calls.allocate(m, allocator, size, 0x10);
    for k in 0..size {
        m.set_u8(new.wrapping_add(k), 0);
    }
    let (from, n) = (old(m), count(m).wrapping_mul(4));
    for k in 0..n {
        let b = m.u8(from.wrapping_add(k));
        m.set_u8(new.wrapping_add(k), b);
    }
    let allocator = array_allocator(m);
    let from = old(m);
    calls.free(m, allocator, from);
    new
}

/// Whether a class descriptor or one of its parents has the name pointer
/// `name`.
fn kind_of(m: &dyn Mem, dti: u32, name: u32) -> bool {
    let mut d = dti;
    if m.u32(d.wrapping_add(4)) == name {
        return true;
    }
    loop {
        d = m.u32(d.wrapping_add(0x10));
        if d == 0 {
            return false;
        }
        if m.u32(d.wrapping_add(4)) == name {
            return true;
        }
    }
}

/// The candidates as the original keeps them: the array object (on the
/// stack, at `found`) holds the count; its capacity and storage are kept
/// aside and written back where the original writes them.
struct Found {
    array: Array,
    capacity: u32,
    data: u32,
}

impl Found {
    fn count(&self, m: &dyn Mem) -> u32 {
        self.array.count(m)
    }
    fn set_count(&self, m: &mut dyn Mem, n: u32) {
        self.array.set_count(m, n)
    }
}

/// One frame of the recognition manager. `found` is room for a local
/// MtArray (0x14 bytes, the original's stack frame).
// Model of 0x00487B00 (DDDA.exe 2017-12-13 Steam), sRecognition::vf06.
pub fn update(m: &mut dyn Mem, this: u32, found: u32, calls: &mut impl Calls) {
    let at = |o: u32| this.wrapping_add(o);
    let infos = Array(at(INFOS));
    let a = Array(found);
    a.set_vtable(m, ARRAY_VTABLE);
    a.set_count(m, 0);
    a.set_capacity(m, 0);
    m.set_u8(found.wrapping_add(0xC), 0);
    a.set_data(m, 0);
    let mut found = Found { array: a, capacity: 0, data: 0 };
    let main = Main::get(m);
    let gate = |m: &mut dyn Mem| {
        if main.halted(m) {
            return false;
        }
        let t = F32::from_bits(main.delta_time_bits(m)) + f(m, at(TIMER));
        m.set_u32(at(TIMER), t.to_bits());
        !(f(m, at(INTERVAL)).0 > t.0)
    };
    if !gate(m) {
        // the empty local array's destructor: no calls
        array::destroy(m, a, |_, _| {}, |_, _| {});
        return;
    }
    m.set_u32(at(TIMER), 0);
    let mut i = 0;
    while i < infos.count(m) {
        m.set_u8(infos.get(m, i).wrapping_add(SEEN), 0);
        i += 1;
    }

    // 1. candidates: the three unit lists, filtered by class
    for (list, &(manager, class)) in LISTS.iter().enumerate() {
        let n = m.u32(m.u32(manager).wrapping_add(0x24));
        for i in 0..n {
            let mgr = m.u32(manager);
            let count = m.u32(mgr.wrapping_add(0x24));
            if count == 0 || i >= count {
                continue;
            }
            let unit = m.u32(m.u32(mgr.wrapping_add(0x30)).wrapping_add(4 * i));
            if unit == 0 {
                continue;
            }
            let dti = calls.class_of(m, unit);
            if !kind_of(m, dti, m.u32(class)) {
                continue;
            }
            let count = found.count(m);
            if count >= found.capacity {
                if list == 0 {
                    // the first list grows the array through MtArray::reserve
                    found.capacity = found.capacity.wrapping_add(16);
                    let calls_cell = std::cell::RefCell::new(&mut *calls);
                    array::reserve(
                        m,
                        a,
                        found.capacity,
                        |m, size, align| {
                            let allocator = array_allocator(m);
                            calls_cell.borrow_mut().allocate(m, allocator, size, align)
                        },
                        |m, block, value, size| (0..size).for_each(|k| m.set_u8(block.wrapping_add(k), value as u8)),
                        |m, block, data, size| {
                            (0..size).for_each(|k| {
                                let b = m.u8(data.wrapping_add(k));
                                m.set_u8(block.wrapping_add(k), b)
                            })
                        },
                        |m, block| {
                            let allocator = array_allocator(m);
                            calls_cell.borrow_mut().free(m, allocator, block)
                        },
                    );
                    found.data = a.data(m);
                    found.capacity = a.capacity(m);
                } else {
                    // the others grow it inline
                    let data = found.data;
                    let new = grow(m, calls, found.capacity.wrapping_add(16), |_| data, |m| a.count(m));
                    found.capacity = found.capacity.wrapping_add(16);
                    found.data = new;
                    a.set_data(m, new);
                }
            }
            let count = found.count(m);
            m.set_u32(found.data.wrapping_add(4 * count), unit);
            found.set_count(m, count.wrapping_add(1));
        }
    }
    // the player manager's three slots
    let mut count = found.count(m);
    for slot in PLAYER_SLOTS {
        let unit = m.u32(m.u32(PLAYERS).wrapping_add(slot));
        if unit == 0 {
            continue;
        }
        if count >= found.capacity {
            let data = found.data;
            let new = grow(m, calls, found.capacity.wrapping_add(16), |_| data, |m| a.count(m));
            found.capacity = found.capacity.wrapping_add(16);
            found.data = new;
        }
        m.set_u32(found.data.wrapping_add(4 * count), unit);
        count = count.wrapping_add(1);
        found.set_count(m, count);
    }
    a.set_data(m, found.data);

    // 2. drop the candidates that are not alive and engaged
    if count != 0 {
        let (mut j, mut k) = (1u32, 0u32);
        loop {
            let unit = m.u32(found.data.wrapping_add(4 * k));
            let kind = m.u32(unit.wrapping_add(4)) & 7;
            let engaged = (kind == 1 || kind == 2)
                && (m.u32(unit.wrapping_add(0x23A4)) as i32) > 0
                && {
                    let state = m.u32(m.u32(unit.wrapping_add(0x23B0)));
                    state != 0 && m.u32(state.wrapping_add(0x44)) == 3
                };
            if engaged {
                k += 1;
                j += 1;
            } else {
                if m.u8(found.array.0.wrapping_add(0xC)) != 0 && unit != 0 {
                    calls.delete(m, unit);
                    count = found.count(m);
                }
                for x in j..count {
                    let v = m.u32(found.data.wrapping_add(4 * x));
                    m.set_u32(found.data.wrapping_add(4 * (x - 1)), v);
                }
                count = count.wrapping_sub(1);
                found.set_count(m, count);
            }
            if j.wrapping_sub(1) >= count {
                break;
            }
        }
    }

    // 3. a record per candidate: mark the known ones seen, add the others
    let mut i = 0;
    while i < found.count(m) {
        let unit = m.u32(found.data.wrapping_add(4 * i));
        let records = infos.data(m);
        let known = (0..infos.count(m)).map(|k| m.u32(records.wrapping_add(4 * k))).find(|&r| m.u32(r.wrapping_add(TARGET)) == unit);
        if let Some(info) = known {
            m.set_u8(info.wrapping_add(SEEN), 1);
        } else {
            let allocator = dti::allocator_of(m, Dti(INFO_DTI)).0;
            let tag = m.u32(INFO_TAG);
            let info = calls.allocate_tagged(m, allocator, INFO_SIZE, 0x10, tag);
            if info != 0 {
                let id = m.u32(at(NEXT_ID));
                m.set_u32(info, INFO_VTABLE);
                for o in [0x2C, 0x3C] {
                    m.set_u32(info.wrapping_add(o), 0);
                }
                m.set_u32(info.wrapping_add(ID), id);
                m.set_u8(info.wrapping_add(DRAWN_NOW), 0);
                m.set_u8(info.wrapping_add(SEEN), 1);
                m.set_u8(info.wrapping_add(RECOGNISED), 0);
                m.set_u32(info.wrapping_add(TARGET), unit);
                for o in [DRAWN, HIDDEN, SPECIES_VALUE] {
                    m.set_u32(info.wrapping_add(o), 0);
                }
            }
            m.set_u32(at(NEXT_ID), m.u32(at(NEXT_ID)).wrapping_add(1));
            let (count, capacity) = (infos.count(m), infos.capacity(m));
            if count < capacity {
                m.set_u32(infos.data(m).wrapping_add(4 * count), info);
                infos.set_count(m, count.wrapping_add(1));
            } else {
                let slots = capacity.wrapping_add(16);
                let new = grow(m, calls, slots, |m| infos.data(m), |m| infos.count(m));
                let count = infos.count(m);
                infos.set_data(m, new);
                infos.set_capacity(m, slots);
                m.set_u32(new.wrapping_add(4 * count), info);
                infos.set_count(m, count.wrapping_add(1));
            }
        }
        i += 1;
    }

    // 4. drop the records whose unit is not alive or no longer a candidate
    if infos.count(m) != 0 {
        let (mut j, mut k) = (1u32, 0u32);
        loop {
            let info = infos.get(m, k);
            let unit = m.u32(info.wrapping_add(TARGET));
            let kind = m.u32(unit.wrapping_add(4)) & 7;
            let keep = (kind == 1 || kind == 2) && calls.alive(m, unit) && m.u8(info.wrapping_add(SEEN)) != 0;
            if keep {
                k += 1;
                j += 1;
            } else {
                if m.u8(infos.0.wrapping_add(0xC)) != 0 {
                    let object = if kind == 1 || kind == 2 { infos.get(m, k) } else { info };
                    if object != 0 {
                        calls.delete(m, object);
                    }
                }
                let mut x = j;
                while x < infos.count(m) {
                    let v = infos.get(m, x);
                    m.set_u32(infos.data(m).wrapping_add(4 * (x - 1)), v);
                    x += 1;
                }
                let n = infos.count(m).wrapping_sub(1);
                infos.set_count(m, n);
            }
            if j.wrapping_sub(1) >= infos.count(m) {
                break;
            }
        }
    }

    // 5. the timers (the frame-rate bug: this frame's step, once per
    // interval)
    let mut i = 0;
    while i < infos.count(m) {
        let info = infos.get(m, i);
        let unit = m.u32(info.wrapping_add(TARGET));
        let (sa, sb) = (m.u8(unit.wrapping_add(0x2C)), m.u8(unit.wrapping_add(0x2D)));
        let record = calls.species(m, unit, sa, sb);
        if record != 0 {
            let v = x87_copy(f32::from_bits(m.u32(record.wrapping_add(8))));
            m.set_u32(info.wrapping_add(SPECIES_VALUE), v.to_bits());
        }
        let visible = calls.visible(m, info);
        let main = Main::get(m);
        let step = F32::from_bits(main.delta_time_bits(m)) / F32(main.fps(m));
        let hidden = if visible {
            m.set_u8(info.wrapping_add(DRAWN_NOW), 1);
            let drawn = step + f(m, info.wrapping_add(DRAWN));
            m.set_u32(info.wrapping_add(DRAWN), drawn.to_bits());
            F32::ZERO
        } else {
            m.set_u8(info.wrapping_add(DRAWN_NOW), 0);
            m.set_u32(info.wrapping_add(DRAWN), 0);
            step + f(m, info.wrapping_add(HIDDEN))
        };
        m.set_u32(info.wrapping_add(HIDDEN), hidden.to_bits());
        if m.u8(info.wrapping_add(RECOGNISED)) != 0 {
            if hidden.0 > f(m, at(FORGET_AFTER)).0 {
                m.set_u8(info.wrapping_add(RECOGNISED), 0);
            }
        } else {
            let unit = m.u32(info.wrapping_add(TARGET));
            if calls.alive(m, unit) && f(m, info.wrapping_add(DRAWN)).0 > f(m, at(RECOGNISE_AFTER)).0 {
                m.set_u8(info.wrapping_add(RECOGNISED), 1);
            }
        }
        // units on this list are always recognised
        let always = m.u32(ALWAYS);
        let n = m.u32(always.wrapping_add(0xB_E284));
        if n != 0 {
            let unit = m.u32(info.wrapping_add(TARGET));
            let list = m.u32(always.wrapping_add(0xB_E290));
            if let Some(k) = (0..n).find(|&k| m.u32(list.wrapping_add(4 * k)) == unit) {
                if (k as i32) >= 0 {
                    m.set_u8(info.wrapping_add(RECOGNISED), 1);
                }
            }
        }
        i += 1;
    }

    // 6. the local array's storage
    let count = found.count(m);
    let data = a.data(m);
    if m.u8(found.array.0.wrapping_add(0xC)) != 0 {
        for k in 0..count {
            let unit = m.u32(data.wrapping_add(4 * k));
            if unit != 0 {
                calls.delete(m, unit);
            }
        }
    }
    if data != 0 {
        let allocator = array_allocator(m);
        calls.free(m, allocator, data);
    }
}
