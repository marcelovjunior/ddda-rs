//! `cThinkMgr` (DTI 0x01996740, vtable 0x01568a20): one enemy's think
//! state and the table interpreter.
//!
//! uEm0100 keeps it at unit +0x6150 (vf 0x2f4, 0x008c7770). Field comments
//! give the offset inside `cThinkMgr`. The reset is 0x008b73f0 (with the
//! timer reset 0x008b76e0 and counter reset 0x008b7c70).

use super::table::{Act, Tables, FLT_MAX_BITS};
use crate::Error;

/// Target slots (`cThinkMgrTargetData`, 0x90 bytes from +0x80).
pub const SLOTS: usize = 23;
/// Timers (16 bytes from +0xfac).
pub const TIMERS: usize = 32;
/// Counters (12 bytes from +0x11ac).
pub const COUNTERS: usize = 8;
/// Call-stack depth (+0xeb0 count, 12-byte entries from +0xeb4).
const CALLS: usize = 16;

/// What the enemy supplies to the think: the callbacks the uEm installs in
/// `cThinkMgr` (uEm0100 at 0x008cef02..0x008cef16) and the game's rand.
pub trait Host {
    /// +0x4c, a row/condition check with id `id` and argument `arg`
    /// (uEm0100: 0x008ea7a0, which sends ids below 1000 to 0x008ea7e0 and
    /// the rest to uEm 0x008ab100).
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, Error>;
    /// +0x50, a row commit with id `id` (uEm0100: 0x008ac270 -> 0x008abfa0
    /// for ids from 1000). A nonzero result is stored at +0x1240.
    fn set(&mut self, id: u32, arg: u32) -> Result<u32, Error>;
    /// A setter may replace target data while this table is executing.
    /// uEm set1003 (0x008ab5b0) writes slot7 through 0x008b72a0/80d0.
    /// Existing hosts keep their ordinary setter behavior by default.
    fn set_with_manager(&mut self, id: u32, arg: u32, manager: &mut ThinkMgr) -> Result<u32, Error> {
        let _ = manager;
        self.set(id, arg)
    }
    /// +0x5c, called before a row is tried while target selection is
    /// explicit (+0xe70) and no measurement is pinned (+0xe88 & 0x17);
    /// uEm0100's 0x00aada50 makes the slot's object the unit's attack
    /// target (unit +0x2b98) and returns false only when that object is not
    /// a `uObjModel`. False makes the interpreter try the same row again.
    fn row_target(&mut self, slot: usize) -> bool {
        let _ = slot;
        true
    }
    /// The game's rand (0x00d00690), a full 32-bit value.
    fn rand(&mut self) -> u32;
}

/// `cThinkMgrTargetData`: what the think knows about one target.
///
/// Slots 0..=4 of uEm0100 are filled each think by 0x008d2930 through
/// 0x00aadcf0 -> [`Slot::measure`] (0x008b6de0): slot 0 the unit's target
/// (grab target +0x5c40, else +0x60c4, else the sensor's pick 0x00ab52e0,
/// else player 0), slot 1 the escape point (unit +0x60e0, written by the
/// FSM's SetEscapePos through uEm0100 vf186 0x008c75d0), slot 2 the point
/// at unit +0x5f40, slot 3 the object at +0x6140, slot 4 the object at
/// +0x5fb4. Slots 9..=22 are re-measured from their objects at the start of
/// every think (0x008b68e0). The host fills them; the think only reads
/// them and its own commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// +0x70 bit 0.
    pub valid: bool,
    /// +0x10: the thinker's position at measurement.
    pub self_pos: [f32; 3],
    /// +0x20: the target's position.
    pub pos: [f32; 3],
    /// +0x30: passed in by the filler (xmm1 of 0x008b6de0).
    pub radius: f32,
    /// +0x34: 3D distance.
    pub dist3: f32,
    /// +0x38: horizontal (XZ) distance; the rows' distance range.
    pub dist: f32,
    /// +0x3c: target y minus own y; the rows' height range.
    pub height: f32,
    /// +0x40: bearing in degrees, -180..180, relative to the thinker's yaw;
    /// the rows' angle range.
    pub angle: f32,
    /// +0x48 / +0x50: flag masks tested by conditions 0x54..0x5b.
    pub mask48: u32,
    pub mask50: u32,
    /// +0x60: copied to the think's +0xe80 (row flag 2 tests its bit 0).
    pub x60: u32,
    /// +0x64: the target object (0 for a point).
    pub obj: u32,
    /// +0x68 and +0x70 bit1: category mask and whether commands0x40..42
    /// may select this slot by category (0x008bc3b0).
    pub category: u32,
    pub categorized: bool,
    /// +0x74 / +0x78: kind and id stamped when a command selects the slot
    /// (+0xe60 / +0xe64 counters).
    pub kind: u32,
    pub id: u32,
    /// +0x7c / +0x80: reset to 1 by 0x008b6d30; +0x80 set to 1 at the start
    /// of each think.
    pub x7c: u32,
    pub x80: u32,
}

impl Default for Slot {
    /// 0x008b6d30's cleared slot (positions at the far point 0x018cfe80
    /// are left 0 here; the think never reads them).
    fn default() -> Self {
        Slot {
            valid: false,
            self_pos: [0.0; 3],
            pos: [0.0; 3],
            radius: 0.0,
            dist3: 0.0,
            dist: 0.0,
            height: 0.0,
            angle: 0.0,
            mask48: 0,
            mask50: 0,
            x60: 0,
            obj: 0,
            category: 0,
            categorized: false,
            kind: 0,
            id: 0,
            x7c: 1,
            x80: 1,
        }
    }
}

impl Slot {
    /// 0x008b6de0: measures a target at `pos` from `self_pos` with yaw
    /// `yaw` (radians). The bearing is `atan2(dx, dz) - yaw` wrapped to
    /// [-pi, pi) and turned into degrees (2*pi 0x0161c274, 1/(2*pi)
    /// 0x0161c548, 180/pi 0x0161c208).
    // Translated from 0x008b6de0 (DDDA.exe 2017-12-13 Steam).
    pub fn measure(self_pos: [f32; 3], yaw: f32, pos: [f32; 3], radius: f32, obj: u32) -> Slot {
        let dx = pos[0] - self_pos[0];
        let dy = pos[1] - self_pos[1];
        let dz = pos[2] - self_pos[2];
        let two_pi = 6.283_185_5_f32;
        let a = (f64::from(dx).atan2(f64::from(dz)) as f32) - yaw;
        let mut w = a - ((a * 0.159_154_94) as i32 as f32) * two_pi;
        if a < 0.0 {
            w += two_pi;
        }
        if w >= std::f32::consts::PI {
            w -= two_pi;
        }
        Slot {
            valid: true,
            self_pos,
            pos,
            radius,
            dist3: (dy * dy + dx * dx + dz * dz).sqrt(),
            dist: (dz * dz + dx * dx).sqrt(),
            height: dy,
            angle: w * 57.295_776,
            obj,
            kind: 0,
            id: 0,
            ..Slot::default()
        }
    }
}

/// One timer (+0xfac + 16*i): two ids, a time and flags (bit 0 counts,
/// bit 1 counts up instead of down). Ticked by 0x008b7930.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timer {
    pub id_a: i32,
    pub id_b: i32,
    pub time: f32,
    pub flags: u32,
}

impl Default for Timer {
    fn default() -> Self {
        Timer {
            id_a: -1,
            id_b: -1,
            time: 0.0,
            flags: 1,
        }
    }
}

/// One counter (+0x11ac + 12*i), decremented once per think that picks a
/// result (0x008b7dc0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counter {
    pub id_a: i32,
    pub id_b: i32,
    pub count: i32,
}

impl Default for Counter {
    fn default() -> Self {
        Counter {
            id_a: -1,
            id_b: -1,
            count: 0,
        }
    }
}

/// A position in a table: record address of the table, byte offset, and
/// the record counter (+0x64). The `{table, offset, index}` triple of
/// 0x008b7e80 and the stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub table: u32,
    pub off: u32,
    pub index: u32,
}

/// The think result of one row (0x008b8bb0).
const NONE: i32 = -1;
const JUMP: i32 = -2;
const RETURN: i32 = -3;
const JUMP_RESET: i32 = -4;
const CONTINUE: i32 = -5;
/// A row result that switches table without an action; the think returns
/// -1 for it (0x008b8b26).
pub const SWITCH: i32 = -6;

#[derive(Debug, Clone, PartialEq)]
pub struct ThinkMgr {
    /// +0x04..+0x23: 32 condition flags the unit writes (uEm0100: in
    /// 0x008d1280 and 0x008d2930, e.g. +0x615d = flag 9 = unit +0x607c ==
    /// 1). Row masks +0x10 / +0x14 require them set / clear (0x008bbd30).
    pub flags: u32,
    /// +0x24..+0x40: the chosen row's +0x8c, +0x90, +0x94, +0x98, +0x9c,
    /// +0x80, +0x84, +0x88.
    pub out: [u32; 8],
    /// +0x44 / +0x48: range scale and whether it applies. The minimum side
    /// uses min(scale, 1).
    pub scale: f32,
    pub scale_on: bool,
    /// +0x60..+0x63: four small counters (0..15) for rows and conditions.
    pub bytes: [u8; 4],
    /// +0x64: records stepped in the current table.
    pub index: u32,
    pub slots: [Slot; SLOTS],
    /// +0xd8b: the first eight slots ordered by 3D distance, valid first.
    /// Rebuilt before each think by 0x008b71a0.
    pub distance_order: [u8; 8],
    /// +0xde4 / +0xe44: selected-slot history and its length.
    pub history: [u32; SLOTS],
    pub history_len: u32,
    /// +0xe40: the current target slot.
    pub cur: u32,
    /// +0xe4c / +0xe50: target kind / id.
    pub kind: u32,
    pub kind_id: u32,
    /// +0xe58 / +0xe5c: cycle direction and mode (command 0x6a).
    pub cycle_up: bool,
    pub cycle_mode: u32,
    /// +0xe60 / +0xe64: kind and id stamped on selected slots.
    pub stamp_kind: u32,
    pub stamp_id: u32,
    /// +0xe70: explicit target selection (commands 7 / 8); when off, every
    /// row looks at the first valid slot of 0..=7.
    pub explicit: bool,
    /// +0xe74 / +0xe78 / +0xe7c / +0xe80: the current target's distance,
    /// angle, height and +0x60 as the rows see them.
    pub dist: f32,
    pub angle: f32,
    pub height: f32,
    pub x80: u32,
    /// +0xe84: compared with row +0x84 under row flag 0x10 (cleared every
    /// think).
    pub e84: f32,
    /// +0xe88: bits 0..4 pin the measurements above (cleared every think).
    pub pin: u32,
    /// +0xe8c / +0xe90 / +0xe94: random block in progress, its roll
    /// (0..100) and the weight sum so far.
    pub rnd_on: bool,
    pub rnd_roll: i32,
    pub rnd_acc: i32,
    /// +0xe98: the table being run; +0xe9c the default table reloaded each
    /// think (0x008b80a0) when +0x120c is set and +0xea4 is clear; +0xea0
    /// the base table (0x008b7e40 restores it after forced actions).
    pub table: u32,
    pub default_table: u32,
    pub base_table: u32,
    /// +0xea4 / +0xea5: row flag 0x4000 / row +0x0c bit 0x10 of the last
    /// row taken.
    pub lock: bool,
    pub ea5: bool,
    /// +0xea6 / +0xea8: row flag 0x8000 delay: the think returns -1 until
    /// the delay (row +0x80, ticked by 0x008b7930) runs out.
    pub delay_on: bool,
    pub delay: f32,
    /// +0xeb0 / +0xeb4: call stack.
    pub calls: [Pos; CALLS],
    pub call_len: u32,
    /// +0xf74: pop the call stack at the next row (row flag 0x80000).
    pub pop_call: bool,
    /// +0xf75 / +0xf76: take a return from the ring after this row (row
    /// flags 0x100000 / 0x200000).
    pub ret1: bool,
    pub ret2: bool,
    /// +0xf78 / +0xf7c: return ring of four.
    pub ring_at: u32,
    pub ring: [Pos; 4],
    pub timers: [Timer; TIMERS],
    pub counters: [Counter; COUNTERS],
    /// +0x120c: reload the default table each think (command 3).
    pub auto_default: bool,
    /// +0x120d: resume next think where this one stopped (row flag 8).
    pub resume: bool,
    /// +0x120e: this think resumed.
    pub resumed: bool,
    /// +0x120f: row flag 0x4000000 of the last row taken.
    pub f120f: bool,
    /// +0x1210 / +0x1211: row flag 0x100 of the last result row (commands
    /// 0x7c / 0x7d) and the gate for rows with flag 0x400.
    pub f1210: bool,
    pub f1211: bool,
    /// +0x1214 / +0x1218 / +0x1220 / +0x1224: resume table, the root it
    /// belongs to, offset and counter; +0x121c the root of this think.
    pub resume_at: Pos,
    pub resume_root: u32,
    pub root: u32,
    /// +0x1228..+0x1238: the resume state saved at the start of a think.
    saved: (Pos, u32, bool),
    /// +0x1240: last nonzero result of the +0x50 callback.
    pub set_result: u32,
    /// +0x1244: last think result (rows with +0x0c bit 31, or every row
    /// when +0x1248 is set, may not repeat it).
    pub last: i32,
    pub no_repeat: bool,
    /// +0x124c: persistent state bits; +0x1250: per-think bits (cleared at
    /// the start of every think).
    pub state: u32,
    pub think_bits: u32,
    /// Not in the game: the rows taken by the last think (row address and
    /// row result), for hosts and tests.
    pub trace: Vec<(u32, i32)>,
}

impl Default for ThinkMgr {
    fn default() -> Self {
        Self::new()
    }
}

fn is_max(bits: u32) -> bool {
    bits == FLT_MAX_BITS
}

/// `(short)(int)x` as the angle tests do it.
fn s16(x: f32) -> i16 {
    (x as i32) as i16
}

/// The +-360 fold the angle tests apply after adding 360.
// Translated from 0x008b8bb0 (angle test) (DDDA.exe 2017-12-13 Steam).
fn fold(x: i16) -> i16 {
    let mut v = x;
    if v > 360 {
        v = s16(f32::from(v) - 360.0);
    }
    if v < -360 {
        v = s16(f32::from(v) + 360.0);
    }
    v
}

/// The angle range test shared by the row filter (0x008b8e50) and
/// condition 0x69: false rejects.
// Translated from 0x008b8bb0 / 0x008b9500 case 0x69 (DDDA.exe 2017-12-13 Steam).
fn angle_ok(angle: f32, lo: f32, hi: f32) -> bool {
    let (s9, s12, s6);
    if lo <= hi {
        s9 = s16(angle);
        s12 = s16(lo);
        s6 = s16(hi);
    } else {
        s9 = fold(s16(angle + 360.0));
        s12 = fold(s16(lo + 360.0));
        s6 = fold(s16(hi + 360.0));
    }
    if s6 < s12 {
        !(s9 <= s12 && s6 <= s9)
    } else {
        !(s9 < s12 || s6 < s9)
    }
}

/// 0x008b7ff0: one 16-bit byte rule (value in bits 0..3; 0x1000 equal,
/// 0x4000 not equal, 0x800 below, 0x400 above). Passes when no test is
/// enabled or any enabled test holds.
// Translated from 0x008b7ff0 (DDDA.exe 2017-12-13 Steam).
fn byte_rule_ok(rule: u32, byte: u8) -> bool {
    let v = (rule & 0xf) as u8;
    let mut any = false;
    let mut hit = false;
    if rule & 0x1000 != 0 {
        any = true;
        hit |= v == byte;
    }
    if rule & 0x4000 != 0 {
        any = true;
        hit |= v != byte;
    }
    if rule & 0x800 != 0 {
        any = true;
        hit |= byte < v;
    }
    if rule & 0x400 != 0 {
        if byte > v {
            return true;
        }
        return hit;
    }
    !any || hit
}

/// 0x008b8070: a byte rule's add (0x200) and subtract (0x100), clamped to
/// 0..15.
// Translated from 0x008b8070 (DDDA.exe 2017-12-13 Steam).
fn byte_rule_apply(rule: u32, byte: u8) -> u8 {
    let d = ((rule >> 4) & 0xf) as u8;
    let mut b = byte;
    if rule & 0x200 != 0 {
        b = b.wrapping_add(d);
        if (b as i8) > 15 {
            b = 15;
        }
    }
    if rule & 0x100 != 0 {
        b = b.wrapping_sub(d);
        if (b as i8) < 0 {
            b = 0;
        }
    }
    b
}

/// Unsigned 32-bit to float as the timer commands convert their argument.
fn u2f(v: u32) -> f32 {
    v as f32
}

impl ThinkMgr {
    /// 0x008b73f0 (+0x008b76e0, 0x008b7c70).
    // Translated from 0x008b73f0, 0x008b76e0, 0x008b7c70 (DDDA.exe 2017-12-13 Steam).
    pub fn new() -> Self {
        ThinkMgr {
            flags: 0,
            out: [0; 8],
            scale: 1.0,
            scale_on: false,
            bytes: [0; 4],
            index: 0,
            slots: [Slot::default(); SLOTS],
            distance_order: [0, 1, 2, 3, 4, 5, 6, 7],
            history: [0; SLOTS],
            history_len: 0,
            cur: 0,
            kind: 0,
            kind_id: 0,
            cycle_up: false,
            cycle_mode: 0,
            stamp_kind: 0,
            stamp_id: 0,
            explicit: true,
            dist: 0.0,
            angle: 0.0,
            height: 0.0,
            x80: 0,
            e84: 0.0,
            pin: 0,
            rnd_on: false,
            rnd_roll: 0,
            rnd_acc: 0,
            table: 0,
            default_table: 0,
            base_table: 0,
            lock: false,
            ea5: false,
            delay_on: false,
            delay: 0.0,
            calls: [Pos::default(); CALLS],
            call_len: 0,
            pop_call: false,
            ret1: false,
            ret2: false,
            ring_at: 0,
            ring: [Pos::default(); 4],
            timers: [Timer::default(); TIMERS],
            counters: [Counter::default(); COUNTERS],
            auto_default: true,
            resume: false,
            resumed: false,
            f120f: false,
            f1210: false,
            f1211: true,
            resume_at: Pos::default(),
            resume_root: 0,
            root: 0,
            saved: (Pos::default(), 0, false),
            set_result: 0,
            last: -1,
            no_repeat: false,
            state: 0,
            think_bits: 0,
            trace: Vec::new(),
        }
    }

    /// CtrlThinkTbl's table switch (0x005a9240): a new table restarts the
    /// think there; it becomes the default and base table either way.
    // Translated from 0x005a9240 (DDDA.exe 2017-12-13 Steam).
    pub fn request_table(&mut self, table: u32) {
        if table != self.table {
            self.table = table;
            self.root = table;
        }
        self.base_table = table;
        self.default_table = table;
    }

    /// 0x008b7e40: back to the base table (the uEm driver calls it when it
    /// forces an action, 0x008d2406).
    // Translated from 0x008b7e40 (DDDA.exe 2017-12-13 Steam).
    pub fn reset_to_base(&mut self) -> bool {
        self.resume = false;
        self.lock = false;
        self.ea5 = false;
        if self.base_table == 0 {
            return false;
        }
        self.default_table = self.base_table;
        self.table = self.base_table;
        self.root = self.base_table;
        true
    }

    /// 0x008b80a0: reload the default table before a think.
    // Translated from 0x008b80a0 (DDDA.exe 2017-12-13 Steam).
    pub fn reload_default(&mut self) {
        if self.auto_default && !self.lock && self.default_table != 0 {
            self.table = self.default_table;
            self.root = self.default_table;
        }
    }

    /// 0x008b7930: advances timers and the row delay by `delta` (the unit's
    /// frame delta, uEm +0x14; uEm0100 calls it from vf08 0x008cf880).
    // Translated from 0x008b7930 (DDDA.exe 2017-12-13 Steam).
    pub fn tick(&mut self, delta: f32) {
        let mut d = delta;
        for t in &mut self.timers {
            if t.id_a == -1 && t.id_b == -1 {
                t.time = -1.0;
                continue;
            }
            let f = t.flags;
            if f & 1 != 0 {
                if f & 2 == 0 {
                    t.time -= d;
                } else {
                    if d > f32::MAX {
                        d = 3e6;
                    }
                    if t.time < 3e6 {
                        t.time += d;
                    }
                }
            }
            if t.time < 0.0 {
                t.id_a = -1;
                t.id_b = -1;
                t.flags = f & !6 | 1;
                t.time = -1.0;
            }
        }
        if self.delay > 0.0 {
            self.delay -= d;
        } else {
            self.delay = 0.0;
        }
    }

    fn slot(&self, i: u32) -> Option<&Slot> {
        self.slots.get(i as usize).filter(|s| s.valid)
    }

    /// 0x008b8b50.
    // Translated from 0x008b8b50 (DDDA.exe 2017-12-13 Steam).
    fn restore(&mut self) {
        self.lock = false;
        self.ea5 = false;
        if self.resume_root != self.root {
            self.resume_root = self.saved.1;
            self.resume_at = self.saved.0;
            self.resume = self.saved.2;
        }
    }

    /// 0x008b7e80: the position after the row at `at`; inside a random
    /// block (or with `skip`) it skips to the block's end.
    // Translated from 0x008b7e80 (DDDA.exe 2017-12-13 Steam).
    fn next(&mut self, tables: &Tables, at: Pos, skip: bool) -> Result<Pos, Error> {
        let row = tables.get(at.table + at.off)?;
        let mut out = Pos {
            table: at.table,
            off: at.off + row.step(),
            index: at.index + 1,
        };
        if (!self.rnd_on && !skip) || row.flags() & 0x4000_0000 != 0 {
            self.rnd_on = false;
            return Ok(out);
        }
        loop {
            let r = tables.get(out.table + out.off)?;
            if r.kind() == 0 {
                if self.resume && r.flags() & 0x8000_0000 != 0 {
                    return Ok(out);
                }
                if r.flags() & 1 != 0 {
                    return Ok(out);
                }
                if r.flags() & 0x4000_0000 != 0 {
                    self.rnd_on = false;
                    out.off += r.step();
                    out.index += 1;
                    return Ok(out);
                }
            }
            out.index += 1;
            if r.step() == 0 {
                return Err(Error::Data(format!(
                    "think record {:#010x} has kind {}",
                    r.va,
                    r.kind()
                )));
            }
            out.off += r.step();
        }
    }

    /// 0x008b75e0.
    // Translated from 0x008b75e0 (DDDA.exe 2017-12-13 Steam).
    fn push_ret(&mut self, p: Pos) {
        self.ring[self.ring_at as usize] = p;
        self.ring_at += 1;
        if self.ring_at > 3 {
            self.ring_at = 0;
        }
    }

    /// 0x008b7620: a return, when the last row asked for one.
    // Translated from 0x008b7620 (DDDA.exe 2017-12-13 Steam).
    fn pop_ret(&mut self) -> Option<Pos> {
        let take = |m: &mut ThinkMgr, zero: bool| -> Option<Pos> {
            m.ring_at = if m.ring_at == 0 { 3 } else { m.ring_at - 1 };
            let mut p = m.ring[m.ring_at as usize];
            if p.table != 0 {
                if zero {
                    p.off = 0;
                    p.index = 0;
                }
                return Some(p);
            }
            None
        };
        if self.ret1 {
            self.ret1 = false;
            if let Some(p) = take(self, true) {
                return Some(p);
            }
        }
        if self.ret2 {
            self.ret2 = false;
            return take(self, false);
        }
        None
    }

    /// 0x008b7f80.
    // Translated from 0x008b7f80 (DDDA.exe 2017-12-13 Steam).
    fn pop_call_stack(&mut self) -> Option<Pos> {
        if self.call_len == 0 {
            return None;
        }
        self.call_len -= 1;
        let p = self.calls[self.call_len as usize];
        self.calls[self.call_len as usize] = Pos::default();
        Some(p)
    }

    /// 0x008b7dc0.
    // Translated from 0x008b7dc0 (DDDA.exe 2017-12-13 Steam).
    fn tick_counters(&mut self) {
        for c in &mut self.counters {
            if c.id_a == -1 && c.id_b == -1 {
                c.count = -1;
            } else {
                c.count -= 1;
                if c.count < 0 {
                    c.id_a = -1;
                    c.id_b = -1;
                    c.count = -1;
                }
            }
        }
    }

    /// 0x008bc410: selects slot `n` if it is valid.
    // Translated from 0x008bc410 (with 0x008bc3f0) (DDDA.exe 2017-12-13 Steam).
    fn select(&mut self, n: u32) -> bool {
        if (n as usize) < SLOTS && self.slots[n as usize].valid {
            self.cur = n;
            self.history[self.history_len as usize] = n;
            self.history_len += 1;
            if self.history_len > 22 {
                self.history_len = 0;
            }
            let s = &mut self.slots[n as usize];
            s.kind = self.stamp_kind;
            s.id = self.stamp_id;
            self.stamp_id = self.stamp_id.wrapping_add(1);
            return true;
        }
        false
    }

    /// 0x008b7af0: sets or refreshes a timer. `restart` false keeps a
    /// running timer's time and only changes its direction bits.
    // Translated from 0x008b7af0 (DDDA.exe 2017-12-13 Steam).
    #[allow(clippy::too_many_arguments)]
    fn set_timer(
        &mut self,
        id_a: i32,
        time: f32,
        id_b: i32,
        count: bool,
        up: bool,
        b2: bool,
        restart: bool,
    ) -> bool {
        if (id_a == -1 && id_b == -1) || id_b == 0 {
            return false;
        }
        let mut free = None;
        let mut found = None;
        for (i, t) in self.timers.iter().enumerate() {
            if t.id_a == -1 && t.id_b == -1 {
                free = Some(i);
            } else if (t.id_b == id_b && id_b != -1) || (t.id_a == id_a && id_b == -1) {
                found = Some(i);
                break;
            }
        }
        let Some(i) = found.or(free) else { return false };
        let dir = (u32::from(b2) << 1 | u32::from(up)) << 1;
        let t = &mut self.timers[i];
        if !restart && found.is_some() {
            t.flags = dir | t.flags & !6;
            return true;
        }
        t.id_a = id_a;
        t.id_b = id_b;
        t.flags = dir | u32::from(count) | t.flags & !7;
        t.time = time;
        true
    }

    /// 0x008b7c30: a timer's time by `id_b`, -1 (0x0161c20c) when none.
    // Translated from 0x008b7c30 (DDDA.exe 2017-12-13 Steam).
    fn timer_time(&self, id_b: i32) -> f32 {
        if id_b == -1 || id_b == 0 {
            return -1.0;
        }
        self.timers
            .iter()
            .find(|t| t.id_b == id_b)
            .map_or(-1.0, |t| t.time)
    }

    /// 0x008bbd30: row masks against the 32 flags.
    // Translated from 0x008bbd30 (DDDA.exe 2017-12-13 Steam).
    fn flags_ok(&self, need: u32, forbid: u32) -> bool {
        need & !self.flags == 0 && forbid & self.flags == 0
    }

    /// 0x008bbc60 / 0x008bbb90: a row check triple; true when the op is
    /// neither a callback check nor a VM condition (no test).
    // Translated from 0x008bbc60, 0x008bbb90 (DDDA.exe 2017-12-13 Steam).
    fn triple_check(&mut self, host: &mut dyn Host, row: &Act, i: usize) -> Result<bool, Error> {
        let (op, a, b) = row.triple(i);
        if op & 0xffff == 1 {
            let want = match op >> 16 {
                0 => true,
                1 => false,
                _ => return Ok(true),
            };
            return Ok(host.check(a, b)? == want);
        }
        if op & 0xff == 3 {
            let code = op >> 20;
            let a = if code == 0x31 || code == 0x35 {
                row.word(0x30)
            } else {
                a
            };
            return self.cond(host, row, code, 0, a, b);
        }
        Ok(true)
    }

    /// 0x008bbcd0 / 0x008bbbe0: a row commit triple.
    // Translated from 0x008bbcd0 (DDDA.exe 2017-12-13 Steam).
    fn triple_commit(&mut self, host: &mut dyn Host, row: &Act, i: usize) -> Result<(), Error> {
        let (op, a, b) = row.triple(i);
        if op & 0xffff == 2 && op & 0xffff_0000 == 0 {
            let r = host.set_with_manager(a, b, self)?;
            if r != 0 {
                self.set_result = r;
            }
        }
        Ok(())
    }

    // Translated from 0x008bbbe0 (DDDA.exe 2017-12-13 Steam).
    fn triple_command(
        &mut self,
        host: &mut dyn Host,
        row: &Act,
        i: usize,
        bd: &mut bool,
    ) -> Result<(), Error> {
        let (op, a, b) = row.triple(i);
        if op & 0xff == 3 {
            let code = (op >> 8) & 0xfff;
            let a = if matches!(code, 0x32 | 0x36 | 0x44 | 0x4e) {
                row.word(0x30)
            } else {
                a
            };
            self.command(host, row, code, 0, a, b, bd, false)?;
        }
        Ok(())
    }

    /// 0x008b9500: condition `code` of record `rec` with arguments `a`
    /// (+0x38) and `b` (+0x40).
    // Translated from 0x008b9500 (DDDA.exe 2017-12-13 Steam).
    #[allow(clippy::too_many_arguments)]
    fn cond(
        &mut self,
        host: &mut dyn Host,
        rec: &Act,
        code: u32,
        p: u32,
        a: u32,
        b: u32,
    ) -> Result<bool, Error> {
        if p != 0 {
            return Ok(true);
        }
        let byte = |k: usize| u32::from(self.bytes[k]);
        let (ai, bf) = (a as i32, u2f(b));
        Ok(match code {
            0x0a..=0x21 => {
                let k = ((code - 0x0a) / 6) as usize;
                let v = byte(k);
                match (code - 0x0a) % 6 {
                    0 => v == b,
                    1 => v != b,
                    2 => v < b,
                    3 => v > b,
                    4 => v <= b,
                    _ => v >= b,
                }
            }
            0x22 => self.state & b == b,
            0x23 => self.state & b == 0,
            0x24 => self.think_bits & b == b,
            0x25 => self.think_bits & b == 0,
            0x27 => self.resume,
            0x2f => ai == -1 || ai == 0 || !self.timers.iter().any(|t| t.id_b == ai && t.time >= 0.0),
            0x31 => ai == -1 || !self.timers.iter().any(|t| t.id_a == ai && t.time >= 0.0),
            0x33 => ai == -1 || ai == 0 || !self.counters.iter().any(|c| c.id_b == ai && c.count > -1),
            0x35 => ai == -1 || !self.counters.iter().any(|c| c.id_a == ai && c.count > -1),
            0x47 => self.slot(b).is_some(),
            0x48 => self.slot(b).is_none(),
            0x50 => self.timer_time(ai) >= bf,
            0x51 => self.timer_time(ai) <= bf,
            0x52 => self.timer_time(ai) < bf,
            0x53 => self.timer_time(ai) > bf,
            0x54..=0x5b => {
                let Some(s) = self.slot(self.cur) else {
                    return Ok(false);
                };
                let m = a | b;
                let v = if matches!(code, 0x54 | 0x55 | 0x58 | 0x59) {
                    s.mask48
                } else {
                    s.mask50
                };
                match code {
                    0x54 | 0x56 => v & m == m,
                    0x55 | 0x57 => v & m == 0,
                    0x58 | 0x5a => v & m != 0,
                    _ => v & m == 0 || v & m != m,
                }
            }
            0x69 => self.cond_range(rec),
            0x70 | 0x71 => self.kind == a && self.kind_id == b,
            0x72 => self.kind == a && b <= self.kind_id,
            0x73 => self.kind == a && self.kind_id <= b,
            0x74 => self.kind == a && b < self.kind_id,
            0x75 => self.kind == a && self.kind_id < b,
            0x76 => self.kind == a,
            0x77 => self.flags_ok(rec.word(0x10), rec.word(0x14)),
            0x78 => {
                let (op, ta, tb) = (rec.word(0x38), rec.word(0x3c), rec.word(0x40));
                if op & 0xffff == 1 {
                    let want = match op >> 16 {
                        0 => true,
                        1 => false,
                        _ => return Ok(true),
                    };
                    host.check(ta, tb)? == want
                } else if op & 0xff == 3 {
                    let c = op >> 20;
                    let ta = if c == 0x31 || c == 0x35 {
                        rec.word(0x30)
                    } else {
                        ta
                    };
                    self.cond(host, rec, c, 0, ta, tb)?
                } else {
                    true
                }
            }
            0x7e => self.f1210,
            0x7f => !self.f1210,
            0x37..=0x3e => {
                return Err(Error::Unimplemented(format!(
                    "think condition {code:#x} (0x008bc480 / 0x008bc4b0 / 0x008bc500) at {:#010x}",
                    rec.va
                )))
            }
            _ => true,
        })
    }

    /// Condition 0x69: the record's own distance / angle / height ranges
    /// against the current target slot.
    // Translated from 0x008b9500 case 0x69 (DDDA.exe 2017-12-13 Steam).
    fn cond_range(&self, rec: &Act) -> bool {
        let Some(s) = self.slots.get(self.cur as usize) else {
            return true;
        };
        if !s.valid {
            return false;
        }
        let sc = |v: f32, clamp: bool| {
            if self.scale_on {
                v * if clamp { self.scale.min(1.0) } else { self.scale }
            } else {
                v
            }
        };
        if !is_max(rec.word(0x18)) && s.dist < sc(rec.float(0x18), true) {
            return false;
        }
        if !is_max(rec.word(0x1c)) && sc(rec.float(0x1c), false) <= s.dist {
            return false;
        }
        let (lo, hi) = (rec.word(0x20), rec.word(0x24));
        if is_max(lo) != is_max(hi) {
            return false;
        }
        if !is_max(lo) && !angle_ok(s.angle, rec.float(0x20), rec.float(0x24)) {
            return false;
        }
        let (lo, hi) = (rec.word(0x28), rec.word(0x2c));
        let h = s.height;
        if !is_max(lo) && !is_max(hi) {
            let (l, u) = (sc(rec.float(0x28), false), sc(rec.float(0x2c), false));
            if u < l {
                return l < h || h < u;
            }
        }
        if !is_max(lo) && h < sc(rec.float(0x28), true) {
            return false;
        }
        is_max(hi) || h <= sc(rec.float(0x2c), false)
    }

    /// 0x008ba2a0: command `code` of record `rec`. `interp` marks calls
    /// from the interpreter, which update +0x1250 from the record's +0x68.
    // Translated from 0x008ba2a0 (DDDA.exe 2017-12-13 Steam).
    #[allow(clippy::too_many_arguments)]
    fn command(
        &mut self,
        host: &mut dyn Host,
        rec: &Act,
        code: u32,
        p: u32,
        a: u32,
        b: u32,
        bd: &mut bool,
        interp: bool,
    ) -> Result<(), Error> {
        if p != 0 {
            return Ok(());
        }
        let ai = a as i32;
        let ok = match code {
            1 => {
                self.default_table = rec.table();
                true
            }
            2 => {
                self.base_table = rec.table();
                true
            }
            3 => {
                self.auto_default = b as u8 != 0;
                true
            }
            4 | 5 | 0x3f => {
                if !*bd {
                    // 0x008bc3f0 -> 0x008b6a80 on +0x70: the history restarts
                    // once per chain.
                    *bd = true;
                    self.history = [SLOTS as u32; SLOTS];
                    self.history_len = 0;
                }
                self.select(b)
            }
            // Translated from 0x008ba2a0 case0x42 / 0x008bc3b0
            // (DDDA.exe 2017-12-13 Steam). Unlike 0x41, uses the distance
            // order at +0xd8b, not the priority order at +0xd74.
            0x42 => {
                if !*bd {
                    *bd = true;
                    self.history = [SLOTS as u32; SLOTS];
                    self.history_len = 0;
                }
                let slot = self.distance_order.iter().copied().find(|&index| {
                    let slot = &self.slots[usize::from(index)];
                    slot.valid && slot.categorized && slot.category & b != 0
                });
                slot.is_some_and(|index| self.select(u32::from(index)))
            }
            7 => {
                self.explicit = false;
                true
            }
            8 => {
                self.explicit = true;
                true
            }
            0x28 => {
                self.state |= b;
                true
            }
            0x29 => {
                self.state &= !b;
                true
            }
            0x2a => {
                self.think_bits |= b;
                true
            }
            0x2b => {
                self.think_bits &= !b;
                true
            }
            0x2c..=0x2e => {
                if let Some(k) = (a < 4).then_some(a as usize) {
                    let v = self.bytes[k];
                    self.bytes[k] = match code {
                        0x2c => b.min(15) as u8,
                        0x2d => (v.wrapping_add(b.min(15) as u8)).min(15),
                        _ => (i32::from(v) - b as i32).clamp(0, 15) as u8,
                    };
                }
                true
            }
            0x30 => self.set_timer(-1, u2f(b), ai, true, false, false, true),
            0x43 => self.set_timer(-1, u2f(b), ai, false, false, false, true),
            0x49 => self.set_timer(-1, u2f(b), ai, true, true, false, true),
            0x4a => self.set_timer(-1, u2f(b), ai, true, true, false, false),
            0x4b => self.set_timer(-1, u2f(b), ai, true, false, true, true),
            0x4c => self.set_timer(-1, u2f(b), ai, true, false, true, false),
            0x32 | 0x44 => {
                if ai == -1 {
                    false
                } else {
                    let mut free = None;
                    let mut found = None;
                    for (i, t) in self.timers.iter().enumerate() {
                        if t.id_a == -1 && t.id_b == -1 {
                            free = Some(i);
                        } else if t.id_a == ai {
                            found = Some(i);
                            break;
                        }
                    }
                    match found.or(free) {
                        Some(i) => {
                            let t = &mut self.timers[i];
                            t.id_a = ai;
                            t.time = u2f(b);
                            t.id_b = -1;
                            t.flags = if code == 0x32 {
                                t.flags & !6 | 1
                            } else {
                                t.flags & !7
                            };
                            true
                        }
                        None => false,
                    }
                }
            }
            0x36 => {
                if ai == -1 {
                    false
                } else {
                    let mut free = None;
                    let mut found = None;
                    for (i, c) in self.counters.iter().enumerate() {
                        if c.id_a == -1 && c.id_b == -1 {
                            free = Some(i);
                        } else if c.id_a == ai {
                            found = Some(i);
                            break;
                        }
                    }
                    match found.or(free) {
                        Some(i) => {
                            self.counters[i] = Counter {
                                id_a: ai,
                                id_b: -1,
                                count: b as i32,
                            };
                            true
                        }
                        None => false,
                    }
                }
            }
            0x45 => {
                self.no_repeat = true;
                true
            }
            0x46 => {
                self.no_repeat = false;
                true
            }
            0x4d => {
                if ai != -1 {
                    for t in self.timers.iter_mut().filter(|t| t.id_b == ai) {
                        t.id_a = -1;
                        t.id_b = -1;
                        t.time = -1.0;
                        t.flags = t.flags & !6 | 1;
                    }
                }
                true
            }
            0x62 => {
                if let Some(s) = self.slots.get_mut(a as usize) {
                    *s = Slot::default();
                }
                true
            }
            0x65 | 0x66 => self.sort_history(code, a),
            0x67 => {
                self.stamp_kind = a;
                self.stamp_id = 0;
                return self.tail(rec, false, interp);
            }
            0x68 => return self.tail(rec, false, interp),
            0x6a => self.cycle(),
            0x6b | 0x6d => match self.slot(self.cur).copied() {
                Some(s) if s.kind == self.kind => match self.find_kind(self.kind, self.kind_id) {
                    Some(i) => {
                        self.cur = i;
                        false
                    }
                    None => false,
                },
                Some(s) => {
                    self.kind = s.kind;
                    self.kind_id = s.id;
                    false
                }
                None => false,
            },
            0x6c => match self.find_kind(a, b) {
                Some(i) => {
                    self.kind = a;
                    self.kind_id = b;
                    self.cur = i;
                    false
                }
                None => false,
            },
            0x6e => {
                self.kind = 0;
                self.kind_id = 0;
                true
            }
            0x6f => {
                self.cycle_mode = a;
                true
            }
            0x79 => {
                let (op, ta, tb) = (rec.word(0x38), rec.word(0x3c), rec.word(0x40));
                if op & 0xffff == 2 && op & 0xffff_0000 == 0 {
                    let r = host.set_with_manager(ta, tb, self)?;
                    if r != 0 {
                        self.set_result = r;
                    }
                }
                if op & 0xff == 3 {
                    let c = (op >> 8) & 0xfff;
                    let ta = if matches!(c, 0x32 | 0x36 | 0x44 | 0x4e) {
                        rec.word(0x30)
                    } else {
                        ta
                    };
                    let mut local = false;
                    self.command(host, rec, c, 0, ta, tb, &mut local, false)?;
                }
                return self.tail(rec, false, interp);
            }
            0x7c => {
                self.f1210 = true;
                true
            }
            0x7d => {
                self.f1210 = false;
                true
            }
            0x40..=0x42 | 0x4e | 0x4f | 0x5c..=0x61 | 0x63 | 0x64 | 0x7a | 0x7b => {
                return Err(Error::Unimplemented(format!(
                    "think command {code:#x} at {:#010x}",
                    rec.va
                )));
            }
            _ => return Ok(()),
        };
        self.tail(rec, ok, interp)
    }

    /// The end of 0x008ba2a0 for interpreter calls.
    // Translated from 0x008ba2a0 (tail) (DDDA.exe 2017-12-13 Steam).
    fn tail(&mut self, rec: &Act, ok: bool, interp: bool) -> Result<(), Error> {
        if interp {
            if ok && (rec.word(0x68) != 0 || rec.word(0x6c) != 0) {
                self.think_bits |= rec.word(0x68);
            } else {
                self.think_bits &= !rec.word(0x68);
            }
        }
        Ok(())
    }

    /// The slot with kind `k` and id `id` among the valid slots.
    // Translated from 0x008ba2a0 cases 0x6b..0x6d (DDDA.exe 2017-12-13 Steam).
    fn find_kind(&self, k: u32, id: u32) -> Option<u32> {
        self.slots
            .iter()
            .position(|s| s.valid && s.kind == k && s.id == id)
            .map(|i| i as u32)
    }

    /// Commands 0x65 / 0x66: sort the selection history by 3D distance
    /// (valid first), then pick its first entry (0x66: the first of kind
    /// `a`).
    // Translated from 0x008ba2a0 cases 0x65 / 0x66 (DDDA.exe 2017-12-13 Steam).
    fn sort_history(&mut self, code: u32, a: u32) -> bool {
        let n = self.history_len as usize;
        if n == 0 || n >= SLOTS {
            return false;
        }
        let at = |m: &ThinkMgr, i: usize| m.slots.get(m.history[i] as usize).copied();
        for pass in 0..n.saturating_sub(1) {
            let mut j = n - 1;
            while pass < j {
                if let (Some(x), Some(y)) = (at(self, j - 1), at(self, j)) {
                    if (y.dist3 < x.dist3 || !x.valid) && y.valid {
                        self.history.swap(j - 1, j);
                    }
                }
                j -= 1;
            }
        }
        let mut pick = 0;
        if code == 0x66 {
            if let Some(i) = (0..n).find(|&i| at(self, i).is_some_and(|s| s.valid && s.kind == a)) {
                pick = i;
            }
        }
        self.cur = self.history[pick];
        false
    }

    /// Command 0x6a: step through the slots of the current kind.
    // Translated from 0x008ba2a0 case 0x6a (DDDA.exe 2017-12-13 Steam).
    fn cycle(&mut self) -> bool {
        if self.kind == 0 {
            return false;
        }
        let list: Vec<u32> = (0..SLOTS as u32)
            .filter(|&i| self.slots[i as usize].valid && self.slots[i as usize].kind == self.kind)
            .collect();
        let Some(mut at) = list
            .iter()
            .position(|&i| self.slots[i as usize].id == self.kind_id)
        else {
            return false;
        };
        let n = list.len();
        if self.cycle_mode == 1 {
            if at == 0 {
                self.cycle_up = true;
            } else if at == n - 1 {
                self.cycle_up = false;
            }
        }
        if self.cycle_up {
            at += 1;
            if at >= n {
                at = if self.cycle_mode == 2 { 0 } else { n - 1 };
            }
        } else if at != 0 {
            at -= 1;
        } else if self.cycle_mode == 2 {
            at = n - 1;
        }
        let i = list[at];
        self.cur = i;
        self.kind_id = self.slots[i as usize].id;
        false
    }

    /// 0x008bc2f0: the chain of kind-1 records from `rec`, an AND; true for
    /// a row.
    // Translated from 0x008bc2f0 (DDDA.exe 2017-12-13 Steam).
    fn chain(&mut self, host: &mut dyn Host, tables: &Tables, va: u32) -> Result<bool, Error> {
        let mut va = va;
        let mut r = tables.get(va)?;
        if r.kind() != 1 {
            return Ok(true);
        }
        let mut c = true;
        loop {
            if r.code() == 9 && r.param() == 0 {
                return Ok(c);
            }
            c = self.cond(host, r, r.code(), r.param(), r.a(), r.b())?;
            if c && (r.word(0x68) != 0 || r.word(0x6c) != 0) {
                self.think_bits |= r.word(0x68);
            } else {
                self.think_bits &= !r.word(0x68);
                if !c {
                    return Ok(false);
                }
            }
            va += r.step();
            r = tables.get(va)?;
            if r.kind() != 1 {
                return Ok(c);
            }
        }
    }

    /// 0x008b8bb0: tries a row against the current target. Returns the
    /// row's result (an action id, -6, or [`NONE`]/[`JUMP`]/...).
    // Translated from 0x008b8bb0 (DDDA.exe 2017-12-13 Steam).
    fn row(&mut self, host: &mut dyn Host, row: &Act) -> Result<i32, Error> {
        let fl = row.flags();
        if fl & 0x400 != 0 && !self.f1211 {
            return Ok(NONE);
        }
        if fl & 0x8000_0000 != 0 && !self.resume {
            return Ok(NONE);
        }
        let mut pass = true;
        if fl & 0x40_0000 != 0 && !self.rnd_on {
            self.rnd_on = true;
            self.rnd_acc = 0;
            self.rnd_roll = (u2f(host.rand()) * 2.328_306_4e-10 * 100.0) as i32;
        }
        if self.rnd_on {
            let acc = self.rnd_acc as f32;
            let w = row.float(0x88);
            let roll = self.rnd_roll as f32;
            if roll < acc || w + acc <= roll {
                pass = false;
            }
            self.rnd_acc = (w + acc) as i32;
        }
        if fl & 0x4000_0000 != 0 && self.rnd_on {
            self.rnd_on = false;
            self.rnd_roll = 0;
            self.rnd_acc = 0;
        }
        if !pass {
            return Ok(NONE);
        }
        if (row.param() & 0x8000_0000 != 0 || self.no_repeat) && self.last != -1 && row.action() == self.last
        {
            return Ok(NONE);
        }
        if fl & 2 != 0 && self.x80 & 1 == 0 {
            return Ok(NONE);
        }
        if fl & 0x10 != 0 {
            let r = row.float(0x84);
            let e = self.e84;
            let strict = if fl & 0x20 != 0 {
                e > r
            } else {
                fl & 0x40 != 0 && r > e
            };
            if !strict && !(fl & 0x80 != 0 && e == r) {
                return Ok(NONE);
            }
        }
        let min_scale = if self.scale_on { self.scale.min(1.0) } else { 1.0 };
        let max_scale = if self.scale_on { self.scale } else { 1.0 };
        if !is_max(row.word(0x18)) && self.dist < min_scale * row.float(0x18) {
            return Ok(NONE);
        }
        if !is_max(row.word(0x1c)) && max_scale * row.float(0x1c) <= self.dist {
            return Ok(NONE);
        }
        if !is_max(row.word(0x20))
            && !is_max(row.word(0x24))
            && !angle_ok(self.angle, row.float(0x20), row.float(0x24))
        {
            return Ok(NONE);
        }
        let (hl, hh) = (row.word(0x28), row.word(0x2c));
        let mut heights_done = false;
        if !is_max(hl) && !is_max(hh) {
            let (l, u) = (row.float(0x28) * max_scale, row.float(0x2c) * max_scale);
            if u < l {
                if self.height <= l && u <= self.height {
                    return Ok(NONE);
                }
                heights_done = true;
            }
        }
        if !heights_done {
            if !is_max(hl) && self.height < row.float(0x28) * min_scale {
                return Ok(NONE);
            }
            if !is_max(hh) && row.float(0x2c) * max_scale < self.height {
                return Ok(NONE);
            }
        }
        if !self.flags_ok(row.word(0x10), row.word(0x14)) {
            return Ok(NONE);
        }
        // cThinkMgr vf+0x14 (0x00fbc0a0) is always true.
        for i in 0..4 {
            if !self.triple_check(host, row, i)? {
                return Ok(NONE);
            }
        }
        let m50 = row.mask1250();
        let need = (m50 >> 32) as u32;
        if need & 0xffff & !self.think_bits != 0 || (need >> 16) & self.think_bits != 0 {
            return Ok(NONE);
        }
        let m4c = row.mask124c();
        let need = (m4c >> 32) as u32;
        if need & 0xffff & !self.state != 0 || (need >> 16) & self.state != 0 {
            return Ok(NONE);
        }
        let rules = row.byte_rules();
        for (k, &r) in rules.iter().enumerate() {
            if !byte_rule_ok(r, self.bytes[k]) {
                return Ok(NONE);
            }
        }
        // Commit.
        for (k, &r) in rules.iter().enumerate() {
            if r & 0x2000 != 0 {
                self.bytes[k] = ((r >> 4) & 0xf) as u8;
            }
        }
        for (k, &r) in rules.iter().enumerate() {
            self.bytes[k] = byte_rule_apply(r, self.bytes[k]);
        }
        let set = m4c as u32;
        self.state = !(set >> 16) & self.state | set & 0xffff;
        let set = m50 as u32;
        self.think_bits = !(set >> 16) & self.think_bits | set & 0xffff;
        if fl & 0x8000 != 0 {
            self.delay_on = true;
            self.delay = row.float(0x80);
        }
        let action = row.action();
        let mut jumped = false;
        if row.table() != 0 {
            self.table = row.table();
            jumped = true;
        }
        if fl & 0x200 != 0 {
            self.table = self.base_table;
            jumped = true;
        }
        self.out = [
            row.word(0x8c),
            row.word(0x90),
            row.word(0x94),
            row.word(0x98),
            row.word(0x9c),
            row.word(0x80),
            row.word(0x84),
            row.word(0x88),
        ];
        self.pop_call = fl & 0x8_0000 != 0;
        self.ret1 = fl & 0x10_0000 != 0;
        self.ret2 = fl & 0x20_0000 != 0;
        for i in 0..4 {
            self.triple_commit(host, row, i)?;
        }
        let mut bd = false;
        for i in 0..4 {
            self.triple_command(host, row, i, &mut bd)?;
        }
        self.f120f = fl & 0x400_0000 != 0;
        if jumped {
            if action == -1 {
                return Ok(JUMP);
            }
            if action == -2 {
                return Ok(JUMP_RESET);
            }
        }
        self.lock = fl & 0x4000 != 0;
        self.ea5 = row.param() & 0x10 != 0;
        Ok(match action {
            -2 => RETURN,
            -1 => CONTINUE,
            a => a,
        })
    }

    /// 0x008b81a0: one think. Returns the table's result: an action id for
    /// the unit, or -1 for none. Runs from the default/resume position;
    /// call [`ThinkMgr::reload_default`] first as the uEm driver does.
    // Translated from 0x008b81a0 (DDDA.exe 2017-12-13 Steam).
    pub fn think(&mut self, tables: &Tables, host: &mut dyn Host) -> Result<i32, Error> {
        self.trace.clear();
        for t in &mut self.timers {
            t.flags |= 1;
        }
        // Translated from 0x008b71a0's second ordering pass
        // (DDDA.exe 2017-12-13 Steam). Strict comparisons keep distance ties
        // in slot order, and put valid slots before invalid ones.
        self.distance_order = [0, 1, 2, 3, 4, 5, 6, 7];
        for first in 0..7 {
            for right in (first + 1..8).rev() {
                let left_slot = self.slots[usize::from(self.distance_order[right - 1])];
                let right_slot = self.slots[usize::from(self.distance_order[right])];
                if right_slot.valid && (!left_slot.valid || right_slot.dist3 < left_slot.dist3) {
                    self.distance_order.swap(right - 1, right);
                }
            }
        }
        for s in &mut self.slots {
            s.x80 = 1;
        }
        self.stamp_kind = 0;
        self.stamp_id = 0;
        self.pin &= !0x1f;
        self.think_bits = 0;
        self.f120f = false;
        if self.delay_on {
            if self.delay > 0.0 {
                return Ok(-1);
            }
            self.delay_on = false;
            self.delay = 0.0;
        }
        self.saved = (self.resume_at, self.resume_root, self.resume);
        self.resumed = self.resume_root == self.root && self.resume;
        self.e84 = 0.0;
        self.rnd_on = false;
        self.rnd_acc = 0;
        self.rnd_roll = 0;
        if !self.resumed {
            self.call_len = 0;
            self.calls = [Pos::default(); CALLS];
            self.pop_call = false;
        }
        let mut result = NONE;
        let mut off = if self.resumed {
            self.table = self.resume_at.table;
            self.resume_at.off
        } else {
            0
        };
        self.index = 0;
        let root = if off == 0 {
            self.root
        } else {
            self.index = self.resume_at.index;
            self.resume_root
        };
        if self.table == 0 {
            self.restore();
            return Ok(-1);
        }
        let mut loops = 0u32;
        let mut in_chain = false;
        let mut bd = false;
        let mut guard = 0u32;
        loop {
            guard += 1;
            if guard > 100_000 {
                return Err(Error::Data(format!(
                    "think did not finish in table {:#010x}",
                    self.table
                )));
            }
            let va = self.table + off;
            if !in_chain && !self.chain(host, tables, va)? {
                // Skip the failed chain and its terminator (0x008b8416).
                let mut r = tables.get(self.table + off)?;
                while r.kind() == 1 {
                    let term = r.code() == 9 && r.param() == 0;
                    self.index += 1;
                    off += r.step();
                    if term {
                        break;
                    }
                    r = tables.get(self.table + off)?;
                }
                continue;
            }
            in_chain = true;
            let rec = tables.get(va)?.clone();
            if rec.kind() == 1 {
                if rec.code() == 9 && rec.param() == 0 {
                    in_chain = false;
                } else {
                    self.command(
                        host,
                        &rec,
                        rec.code(),
                        rec.param(),
                        rec.a(),
                        rec.b(),
                        &mut bd,
                        true,
                    )?;
                }
                self.index += 1;
                off += self.step_of(&rec)?;
                continue;
            }
            self.stamp_kind = 0;
            self.stamp_id = 0;
            in_chain = false;
            bd = false;
            if self.pop_call {
                let p = self.pop_call_stack();
                self.pop_call = false;
                if let Some(p) = p {
                    self.index = p.index;
                    off = p.off;
                    self.table = p.table;
                    continue;
                }
            }
            let fl = rec.flags();
            if fl & 1 != 0 {
                if fl & 0x8000_0000 != 0 && !self.resume {
                    self.index += 1;
                    off += self.step_of(&rec)?;
                    continue;
                }
                if let Some(p) = self.pop_call_stack() {
                    self.index = p.index;
                    off = p.off;
                    self.table = p.table;
                    continue;
                }
                self.out = [0; 8];
                self.resume = false;
                self.restore();
                return Ok(-1);
            }
            let row_table = self.table;
            let here = Pos {
                table: row_table,
                off,
                index: self.index,
            };
            if !self.explicit {
                if let Some(i) = (0..8).find(|&i| self.slots[i].valid) {
                    self.cur = i as u32;
                }
            }
            if let Some(s) = self.slot(self.cur).copied() {
                if self.explicit && self.pin & 0x17 == 0 && !host.row_target(self.cur as usize) {
                    continue;
                }
                if self.pin & 0x10 == 0 {
                    self.x80 = s.x60;
                }
                if self.pin & 1 == 0 {
                    self.dist = s.dist;
                }
                if self.pin & 2 == 0 {
                    self.angle = s.angle;
                }
                if self.pin & 4 == 0 {
                    self.height = s.height;
                }
                // 0x008bc550 / 0x008b6f20 keep per-category slot lists that
                // no ported code reads.
                if s.kind != 0 {
                    self.kind = s.kind;
                    self.kind_id = s.id;
                }
                result = self.row(host, &rec)?;
                if result != NONE {
                    self.trace.push((rec.va, result));
                }
            }
            if !self.resume && fl & 0x8000_0000 != 0 && result == NONE {
                if fl & 0x40_0000 != 0 && fl & 0x4000_0000 == 0 {
                    let p = self.next(tables, here, true)?;
                    self.table = p.table;
                    self.index = p.index;
                    off = p.off;
                    continue;
                }
                self.index += 1;
                off += self.step_of(&rec)?;
                continue;
            }
            match result {
                CONTINUE => {
                    result = NONE;
                    match self.pop_ret() {
                        Some(p) => {
                            self.index = p.index;
                            off = p.off;
                            self.table = p.table;
                        }
                        None => {
                            let p = self.next(tables, here, false)?;
                            self.index = p.index;
                            off = p.off;
                        }
                    }
                    self.resume = fl & 8 != 0;
                }
                RETURN => {
                    result = NONE;
                    self.resume = false;
                    match self.pop_ret() {
                        Some(p) => {
                            self.index = p.index;
                            off = p.off;
                            self.table = p.table;
                            self.rnd_on = false;
                        }
                        None => {
                            let p = self.next(tables, here, false)?;
                            self.index = p.index;
                            off = p.off;
                        }
                    }
                }
                JUMP | JUMP_RESET => {
                    if result == JUMP_RESET {
                        self.resume = false;
                    }
                    let p = self.next(tables, here, false)?;
                    if fl & 0x2000 != 0 {
                        let n = self.call_len as usize;
                        self.calls[n] = p;
                        self.push_ret(p);
                        self.call_len += 1;
                        if self.call_len > 15 {
                            self.call_len = 15;
                        }
                    } else {
                        self.push_ret(p);
                    }
                    self.index = 0;
                    off = 0;
                    loops += 1;
                    self.rnd_on = false;
                    if loops > 16 {
                        self.restore();
                        return Ok(-1);
                    }
                    result = NONE;
                }
                NONE => {
                    self.index += 1;
                    off += self.step_of(&rec)?;
                }
                _ => {
                    let mut at_off = off;
                    if self.table != row_table {
                        let p = self.next(tables, here, false)?;
                        self.push_ret(p);
                    }
                    let mut popped = false;
                    if let Some(p) = self.pop_ret() {
                        self.index = p.index;
                        self.table = p.table;
                        at_off = p.off;
                        popped = true;
                    }
                    if fl & 8 != 0 {
                        self.resume = true;
                        if popped {
                            self.resume_root = root;
                            self.resume_at = Pos {
                                table: self.table,
                                off: at_off,
                                index: self.index,
                            };
                        } else if self.table != row_table {
                            self.resume_root = root;
                            self.resume_at = Pos {
                                table: self.table,
                                off: 0,
                                index: 0,
                            };
                        } else {
                            let p = self.next(
                                tables,
                                Pos {
                                    table: row_table,
                                    off: at_off,
                                    index: self.index,
                                },
                                false,
                            )?;
                            self.resume_root = root;
                            self.resume_at = p;
                        }
                    } else {
                        self.resume = false;
                    }
                    self.f1210 = fl & 0x100 != 0;
                    self.tick_counters();
                    self.last = result;
                    return Ok(if result == SWITCH { -1 } else { result });
                }
            }
        }
    }

    fn step_of(&self, r: &Act) -> Result<u32, Error> {
        match r.step() {
            0 => Err(Error::Data(format!(
                "think record {:#010x} has kind {}",
                r.va,
                r.kind()
            ))),
            s => Ok(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thinktbl::table::RECORD_SIZE;

    struct Fixed {
        roll: u32,
    }

    impl Host for Fixed {
        fn check(&mut self, id: u32, _arg: u32) -> Result<bool, Error> {
            Ok(id == 1)
        }
        fn set(&mut self, _id: u32, _arg: u32) -> Result<u32, Error> {
            Ok(0)
        }
        fn rand(&mut self) -> u32 {
            // The row filter turns rand into percent as rand * 2^-32 * 100.
            ((f64::from(self.roll) + 0.5) / 100.0 * 4_294_967_296.0) as u32
        }
    }

    /// A row with no limits.
    fn row(va: u32, flags: u32, action: i32) -> Act {
        let mut w = [0u32; 40];
        w[2] = flags;
        for x in &mut w[6..12] {
            *x = FLT_MAX_BITS;
        }
        w[12] = action as u32;
        Act { va, w }
    }

    fn end(va: u32) -> Act {
        row(va, 1, -1)
    }

    fn with(mut a: Act, off: u32, v: u32) -> Act {
        a.w[(off / 4) as usize] = v;
        a
    }

    fn mgr(table: u32, slot: Slot) -> ThinkMgr {
        let mut m = ThinkMgr::new();
        m.request_table(table);
        m.slots[0] = slot;
        m.explicit = false;
        m
    }

    fn at(dist: f32, angle: f32) -> Slot {
        Slot {
            valid: true,
            dist,
            angle,
            ..Slot::default()
        }
    }

    const T: u32 = 0x1000;

    #[test]
    fn world_setters_can_install_a_target_before_the_next_command() {
        struct SlotSetter;
        impl Host for SlotSetter {
            fn check(&mut self, _: u32, _: u32) -> Result<bool, Error> {
                Ok(true)
            }
            fn set(&mut self, _: u32, _: u32) -> Result<u32, Error> {
                panic!("setter must receive the executing manager")
            }
            fn set_with_manager(&mut self, id: u32, arg: u32, mgr: &mut ThinkMgr) -> Result<u32, Error> {
                assert_eq!((id, arg), (1003, 100));
                mgr.slots[7] = Slot::measure([0.0; 3], 0.0, [0.0, 0.0, 500.0], 0.0, 0);
                Ok(9)
            }
            fn rand(&mut self) -> u32 {
                0
            }
        }
        let mut record = row(T, 0, 85);
        record.w[14..17].copy_from_slice(&[2, 1003, 100]);
        for command in [false, true] {
            let mut manager = mgr(T, Slot::default());
            if command {
                manager
                    .command(&mut SlotSetter, &record, 0x79, 0, 0, 0, &mut false, false)
                    .unwrap();
            } else {
                manager.triple_commit(&mut SlotSetter, &record, 0).unwrap();
            }
            assert_eq!(manager.set_result, 9);
            assert!(manager.slots[7].valid);
            manager
                .command(&mut Fixed { roll: 0 }, &record, 4, 0, 0, 7, &mut false, false)
                .unwrap();
            assert_eq!(manager.cur, 7);
            assert_eq!(manager.slots[manager.cur as usize].dist, 500.0);
        }
    }

    #[test]
    fn category_selection_uses_valid_distance_order_and_matching_mask() {
        let tables = Tables::from_records(vec![T], [end(T)]);
        let mut manager = mgr(T, Slot::default());
        for (index, distance, category, categorized) in [
            (0, 0.0, 4, true),
            (1, 200.0, 2, true),
            (2, 100.0, 2, true),
            (3, 50.0, 2, false),
            (4, 100.0, 2, true),
        ] {
            manager.slots[index] = Slot {
                valid: true, dist3: distance, category, categorized,
                ..Slot::default()
            };
        }
        manager.slots[5] = Slot { category: 2, categorized: true, ..Slot::default() };
        manager.think(&tables, &mut Fixed { roll: 0 }).unwrap();
        let mut reset_history = false;
        manager.command(&mut Fixed { roll: 0 }, &end(T), 0x42, 0, 0, 2, &mut reset_history, false).unwrap();
        // Ignores nearer uncategorized and wrong-category slots, as well as
        // invalid slot5. Equal-distance slot4 keeps its original order.
        assert_eq!(manager.cur, 2);
        assert_eq!(manager.history_len, 1);
        assert_eq!(manager.history[0], 2);
        manager.command(&mut Fixed { roll: 0 }, &end(T), 0x42, 0, 0, 0, &mut reset_history, false).unwrap();
        assert_eq!((manager.cur, manager.history_len), (2, 1));
        manager.command(&mut Fixed { roll: 0 }, &end(T), 0x42, 0, 0, 4, &mut reset_history, false).unwrap();
        assert_eq!((manager.cur, manager.history_len), (0, 2));
    }

    #[test]
    fn distance_min_inclusive_max_exclusive() {
        let r = with(with(row(T, 0, 7), 0x18, 100f32.to_bits()), 0x1c, 300f32.to_bits());
        let t = Tables::from_records(vec![T], [r, end(T + RECORD_SIZE)]);
        for (d, want) in [(99.9, -1), (100.0, 7), (299.9, 7), (300.0, -1)] {
            let mut m = mgr(T, at(d, 0.0));
            assert_eq!(m.think(&t, &mut Fixed { roll: 0 }).unwrap(), want, "dist {d}");
        }
    }

    #[test]
    fn angle_range_wraps_behind() {
        // 45..-45 is the back arc (rows like 0x017e8eb8 use it).
        let r = with(
            with(row(T, 0, 7), 0x20, 45f32.to_bits()),
            0x24,
            (-45f32).to_bits(),
        );
        let t = Tables::from_records(vec![T], [r, end(T + RECORD_SIZE)]);
        for (a, want) in [
            (0.0, -1),
            (44.0, -1),
            (45.0, 7),
            (90.0, 7),
            (180.0, 7),
            (-90.0, 7),
            (-45.0, 7),
            (-44.0, -1),
        ] {
            let mut m = mgr(T, at(10.0, a));
            assert_eq!(m.think(&t, &mut Fixed { roll: 0 }).unwrap(), want, "angle {a}");
        }
    }

    #[test]
    fn random_block_weights() {
        let w = |a: Act, p: f32| with(a, 0x88, p.to_bits());
        let recs = [
            w(row(T, 0x40_0000, 1), 25.0),
            w(row(T + 0xa0, 0, 2), 25.0),
            w(row(T + 0x140, 0x4000_0000, 3), 50.0),
            end(T + 0x1e0),
        ];
        let t = Tables::from_records(vec![T], recs);
        for (roll, want) in [(0, 1), (24, 1), (25, 2), (49, 2), (50, 3), (99, 3)] {
            let mut m = mgr(T, at(10.0, 0.0));
            assert_eq!(m.think(&t, &mut Fixed { roll }).unwrap(), want, "roll {roll}");
        }
    }

    #[test]
    fn call_returns_to_the_next_row() {
        const SUB: u32 = 0x2000;
        let recs = [
            with(row(T, 0x2000, -1), 0x34, SUB),
            row(T + 0xa0, 0, 9),
            end(T + 0x140),
            with(row(SUB, 0, 5), 0x18, 500f32.to_bits()),
            end(SUB + 0xa0),
        ];
        let t = Tables::from_records(vec![T], recs);
        let mut m = mgr(T, at(10.0, 0.0));
        assert_eq!(m.think(&t, &mut Fixed { roll: 0 }).unwrap(), 9);
        let mut m = mgr(T, at(600.0, 0.0));
        assert_eq!(m.think(&t, &mut Fixed { roll: 0 }).unwrap(), 5);
    }

    #[test]
    fn timer_cooldown_holds_until_it_runs_out() {
        // Row 0: when timer 5 is not running, pick 7 and start it at 60
        // (op 0x2f03003: condition 0x2f, command 0x30).
        let r0 = with(with(with(row(T, 0, 7), 0x38, 0x02f0_3003), 0x3c, 5), 0x40, 60);
        let t = Tables::from_records(vec![T], [r0, row(T + 0xa0, 0, 8), end(T + 0x140)]);
        let mut m = mgr(T, at(10.0, 0.0));
        let h = &mut Fixed { roll: 0 };
        assert_eq!(m.think(&t, h).unwrap(), 7);
        assert_eq!(m.think(&t, h).unwrap(), 8);
        m.tick(60.0);
        assert_eq!(m.think(&t, h).unwrap(), 8, "time 0 still counts as running");
        m.tick(0.5);
        assert_eq!(m.think(&t, h).unwrap(), 7);
    }

    #[test]
    fn callback_checks_and_negation() {
        // Op 1: host check `a` must be true; op 0x10001: must be false.
        let r0 = with(with(row(T, 0, 7), 0x38, 0x1_0001), 0x3c, 1);
        let r1 = with(with(row(T + 0xa0, 0, 8), 0x38, 1), 0x3c, 1);
        let t = Tables::from_records(vec![T], [r0, r1, end(T + 0x140)]);
        let mut m = mgr(T, at(10.0, 0.0));
        assert_eq!(m.think(&t, &mut Fixed { roll: 0 }).unwrap(), 8);
    }

    #[test]
    fn switch_resumes_in_the_new_table_next_think() {
        const NEXT: u32 = 0x3000;
        let recs = [
            with(row(T, 0x4008, SWITCH), 0x34, NEXT),
            end(T + 0xa0),
            row(NEXT, 0, 4),
            end(NEXT + 0xa0),
        ];
        let t = Tables::from_records(vec![T], recs);
        let mut m = mgr(T, at(10.0, 0.0));
        let h = &mut Fixed { roll: 0 };
        m.reload_default();
        assert_eq!(m.think(&t, h).unwrap(), -1);
        m.reload_default();
        assert_eq!(m.think(&t, h).unwrap(), 4);
    }

    #[test]
    fn measure_bearing() {
        let s = Slot::measure([0.0; 3], 0.0, [0.0, 50.0, 100.0], 0.0, 0);
        assert_eq!((s.dist, s.height, s.angle), (100.0, 50.0, 0.0));
        let s = Slot::measure([0.0; 3], 0.0, [100.0, 0.0, 0.0], 0.0, 0);
        assert!((s.angle - 90.0).abs() < 1e-3);
        let s = Slot::measure([0.0; 3], 0.0, [-100.0, 0.0, 0.0], 0.0, 0);
        assert!((s.angle + 90.0).abs() < 1e-3);
        let s = Slot::measure([0.0; 3], 0.0, [0.0, 0.0, -100.0], 0.0, 0);
        assert!(
            (s.angle + 180.0).abs() < 1e-3,
            "behind wraps to -180: {}",
            s.angle
        );
        // Facing +x (yaw pi/2), a target at +x is ahead.
        let s = Slot::measure([0.0; 3], std::f32::consts::FRAC_PI_2, [100.0, 0.0, 0.0], 0.0, 0);
        assert!(s.angle.abs() < 1e-3);
    }
}
