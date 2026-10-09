//! rScheduler ("SDL", version 19): the timelines that drive events,
//! stage effects and lights.
//!
//! Layout from the loader `rScheduler::vf10` 0x00db8e50 and the evaluator
//! (`uScheduler` 0x00ddfd50, key evaluation 0x00ddee70), DDDA.exe
//! 2017-12-13 Steam. Little-endian; offsets are file offsets.
//!
//! ```text
//! header (0x18)
//!   +0x00 "SDL\0"            checked by the loader
//!   +0x04 u16 version 0x13   checked by the loader
//!   +0x06 u16 track count    (vf15 0x00db8d30)
//!   +0x08 u32                0xee4828d3 in every file; never read
//!   +0x0c u32 bits 0-23 frame count (vf16 0x00db8d40), bit 24 integer frames (vf17 0x00db8d60)
//!   +0x10 u32 cut track      0 = none; see `Scheduler::cut_track`
//!   +0x14 u32 string table offset
//! track (0x18 each, from 0x18)
//!   +0x00 u8  kind           see `Kind`
//!   +0x01 u8  MtDTI property type of the bound property
//!   +0x02 u16 key count
//!   +0x04 u32 kind 2: move line of the new unit (0x00db58b0); kinds 5..14: parent track
//!   +0x08 u32 name (string table offset; kind 4 has none)
//!   +0x0c u32 kinds 2/3: JAMCRC type hash of the unit class (0x00cf55b0)
//!   +0x10 u32 kinds 6..14: offset of the keys (u32: bits 0-23 frame, 24-31 mode)
//!   +0x14 u32 kinds 6..14: offset of the values
//! ```

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn err<T>(s: impl Into<String>) -> Result<T> {
    Err(Error(s.into()))
}

/// What a track does (byte +0x00).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 1: the root.
    Root,
    /// 2: spawn a unit of the class; it dies with the scheduler
    /// (`uScheduler::vf15` 0x00ddecb0).
    Unit,
    /// 3: bind an existing system singleton (e.g. sCameraExt,
    /// sWeatherManager) found through sMain's class references.
    Bind,
    /// 4: folder, no runtime action.
    Folder,
    /// 5: a member object (property) of the parent.
    Member,
    Int,
    Vec4,
    Float,
    Bool,
    /// 10: reference to the object bound by another track (index, 0 = null).
    Ref,
    /// 11: resource {type hash, path}, loaded with the scheduler.
    Resource,
    String,
    /// 13: event call (property type 0x18, e.g. `Finish`).
    Event,
    Matrix,
}

impl Kind {
    fn from_u8(k: u8) -> Option<Kind> {
        Some(match k {
            1 => Kind::Root,
            2 => Kind::Unit,
            3 => Kind::Bind,
            4 => Kind::Folder,
            5 => Kind::Member,
            6 => Kind::Int,
            7 => Kind::Vec4,
            8 => Kind::Float,
            9 => Kind::Bool,
            10 => Kind::Ref,
            11 => Kind::Resource,
            12 => Kind::String,
            13 => Kind::Event,
            14 => Kind::Matrix,
            _ => return None,
        })
    }
    fn value_size(self) -> usize {
        match self {
            Kind::Int | Kind::Float | Kind::Ref | Kind::Resource | Kind::String | Kind::Event => 4,
            Kind::Vec4 => 16,
            Kind::Bool => 1,
            Kind::Matrix => 64,
            _ => 0,
        }
    }
    /// Kinds that carry keys.
    pub fn has_keys(self) -> bool {
        self.value_size() != 0
    }
}

/// How a key applies (0x00ddee70).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 0: hold the value.
    Set,
    /// 1 and 4: count up: value + (frame - key frame).
    Count(u8),
    /// 2: fire once when the playhead crosses the key (prev <= key <= cur).
    Trigger,
    /// 3: linear to the next key.
    Linear,
    /// 5: Catmull-Rom for vec4/float; ints treat it as linear.
    Curve,
    Other(u8),
}

impl Mode {
    fn from_u8(m: u8) -> Mode {
        match m {
            0 => Mode::Set,
            1 | 4 => Mode::Count(m),
            2 => Mode::Trigger,
            3 => Mode::Linear,
            5 => Mode::Curve,
            _ => Mode::Other(m),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Vec4([f32; 4]),
    Float(f32),
    Bool(bool),
    Ref(u32),
    /// `None` when the offset is 0.
    Resource(Option<(u32, String)>),
    String(Option<String>),
    Event(u32),
    Matrix([f32; 16]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub frame: u32,
    pub mode: Mode,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub kind: Kind,
    pub property_type: u8,
    /// Move line (kind 2) or parent track index (kinds 5..14).
    pub link: u32,
    pub name: String,
    pub type_hash: u32,
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scheduler {
    pub unknown_08: u32,
    pub frames: u32,
    pub integer_frames: bool,
    /// Bits 25..31 of +0x0c, preserved.
    pub frame_flags_high: u8,
    /// Track whose keys are camera cuts. With `uScheduler`+0x43 set (the
    /// default) the playhead snaps to floor(frame) when floor(frame)+1 is a
    /// key of this track, so cuts are never interpolated (0x00ddeb00).
    pub cut_track: u32,
    pub tracks: Vec<Track>,
}

fn u8_at(b: &[u8], o: usize) -> Result<u8> {
    b.get(o).copied().ok_or_else(|| Error(format!("read past end at {o:#x}")))
}
fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    let s = b.get(o..o + 2).ok_or_else(|| Error(format!("read past end at {o:#x}")))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    let s = b.get(o..o + 4).ok_or_else(|| Error(format!("read past end at {o:#x}")))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn f32_at(b: &[u8], o: usize) -> Result<f32> {
    Ok(f32::from_bits(u32_at(b, o)?))
}
fn cstr(b: &[u8], o: usize) -> Result<String> {
    let s = b.get(o..).ok_or_else(|| Error(format!("string past end at {o:#x}")))?;
    let n = s.iter().position(|&c| c == 0).ok_or_else(|| Error(format!("unterminated string at {o:#x}")))?;
    // Names and paths are ASCII in the install; map bytes 1:1 to keep any
    // other byte recoverable.
    Ok(s[..n].iter().map(|&c| c as char).collect())
}

impl Scheduler {
    pub fn parse(b: &[u8]) -> Result<Scheduler> {
        if b.len() < 0x18 {
            return err("short header");
        }
        if &b[0..4] != b"SDL\0" {
            return err("bad magic");
        }
        let version = u16_at(b, 4)?;
        if version != 0x13 {
            return err(format!("version {version:#x}, expected 0x13"));
        }
        let count = u16_at(b, 6)? as usize;
        let unknown_08 = u32_at(b, 8)?;
        let fr = u32_at(b, 0xc)?;
        let cut_track = u32_at(b, 0x10)?;
        let strtab = u32_at(b, 0x14)? as usize;
        if 0x18 + 0x18 * count > b.len() || strtab > b.len() {
            return err("track table or string table out of range");
        }
        let mut tracks = Vec::with_capacity(count);
        for i in 0..count {
            let o = 0x18 + 0x18 * i;
            let raw_kind = u8_at(b, o)?;
            let kind = Kind::from_u8(raw_kind).ok_or_else(|| Error(format!("track {i}: kind {raw_kind}")))?;
            let property_type = u8_at(b, o + 1)?;
            let n = u16_at(b, o + 2)? as usize;
            let link = u32_at(b, o + 4)?;
            let name_off = u32_at(b, o + 8)? as usize;
            let type_hash = u32_at(b, o + 0xc)?;
            let ko = u32_at(b, o + 0x10)? as usize;
            let vo = u32_at(b, o + 0x14)? as usize;
            let name = if kind == Kind::Folder { String::new() } else { cstr(b, strtab + name_off)? };
            let mut keys = Vec::new();
            if kind.has_keys() {
                let vs = kind.value_size();
                if ko + 4 * n > b.len() || vo + vs * n > b.len() {
                    return err(format!("track {i}: keys out of range"));
                }
                for k in 0..n {
                    let key = u32_at(b, ko + 4 * k)?;
                    let v = vo + vs * k;
                    let string_at = |off: u32| -> Result<Option<String>> {
                        if off == 0 {
                            Ok(None)
                        } else {
                            cstr(b, strtab + off as usize).map(Some)
                        }
                    };
                    let value = match kind {
                        Kind::Int => Value::Int(u32_at(b, v)? as i32),
                        Kind::Vec4 => {
                            let mut a = [0f32; 4];
                            for (j, x) in a.iter_mut().enumerate() {
                                *x = f32_at(b, v + 4 * j)?;
                            }
                            Value::Vec4(a)
                        }
                        Kind::Float => Value::Float(f32_at(b, v)?),
                        Kind::Bool => Value::Bool(u8_at(b, v)? & 1 != 0),
                        Kind::Ref => Value::Ref(u32_at(b, v)?),
                        Kind::Resource => {
                            let off = u32_at(b, v)?;
                            Value::Resource(if off == 0 {
                                None
                            } else {
                                let p = strtab + off as usize;
                                Some((u32_at(b, p)?, cstr(b, p + 4)?))
                            })
                        }
                        Kind::String => Value::String(string_at(u32_at(b, v)?)?),
                        Kind::Event => Value::Event(u32_at(b, v)?),
                        Kind::Matrix => {
                            let mut a = [0f32; 16];
                            for (j, x) in a.iter_mut().enumerate() {
                                *x = f32_at(b, v + 4 * j)?;
                            }
                            Value::Matrix(a)
                        }
                        _ => unreachable!(),
                    };
                    keys.push(Key { frame: key & 0xff_ffff, mode: Mode::from_u8((key >> 24) as u8), value });
                }
                if keys.windows(2).any(|w| w[0].frame > w[1].frame) {
                    return err(format!("track {i}: keys not sorted"));
                }
            }
            tracks.push(Track { kind, property_type, link, name, type_hash, keys });
        }
        Ok(Scheduler {
            unknown_08,
            frames: fr & 0xff_ffff,
            integer_frames: fr >> 24 & 1 != 0,
            frame_flags_high: (fr >> 25) as u8,
            cut_track,
            tracks,
        })
    }

    /// Checks the structure the runtime relies on: parents precede their
    /// children and are units, binds or members; references point at a
    /// track. Measured to hold for the whole install.
    pub fn validate(&self) -> Result<()> {
        for (i, t) in self.tracks.iter().enumerate() {
            if matches!(t.kind, Kind::Root | Kind::Unit | Kind::Bind | Kind::Folder) {
                continue;
            }
            let p = t.link as usize;
            match self.tracks.get(p) {
                Some(pt) if p < i && matches!(pt.kind, Kind::Unit | Kind::Bind | Kind::Member) => {}
                _ => return err(format!("track {i}: bad parent {p}")),
            }
            for k in &t.keys {
                if let Value::Ref(r) = k.value {
                    if r as usize >= self.tracks.len() {
                        return err(format!("track {i}: reference {r} out of range"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The key in effect at `frame`: the last key whose frame is <= it
    /// (0x00ddee70 searches the same way from a cached index).
    pub fn key_at(track: &Track, frame: u32) -> Option<&Key> {
        let i = track.keys.partition_point(|k| k.frame <= frame);
        i.checked_sub(1).map(|i| &track.keys[i])
    }

    /// Path of parent names from the unit or bind down to `index`, e.g.
    /// `uGUISubtitlesExt2.IndexName`.
    pub fn path(&self, index: usize) -> String {
        let mut parts = vec![];
        let mut i = index;
        loop {
            let t = &self.tracks[i];
            parts.push(t.name.clone());
            if matches!(t.kind, Kind::Root | Kind::Unit | Kind::Bind | Kind::Folder) || t.link as usize >= i {
                break;
            }
            i = t.link as usize;
        }
        parts.reverse();
        parts.join(".")
    }
}

/// Playback state of one `uScheduler` unit: frame +0x34, previous frame
/// +0x38, speed +0x3c, paused +0x40, loop +0x41, cut snapping +0x43.
/// Defaults from the constructor 0x00dde6c0: frame 0, previous -1, speed
/// 1, paused, looping, snapping on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub frame: f32,
    pub prev: f32,
    pub speed: f32,
    pub paused: bool,
    pub looping: bool,
    pub snap_cuts: bool,
}

impl Default for Clock {
    // Translated from 0x00dde6c0 (DDDA.exe 2017-12-13 Steam).
    fn default() -> Clock {
        Clock { frame: 0.0, prev: -1.0, speed: 1.0, paused: true, looping: true, snap_cuts: true }
    }
}

/// Where the tracks are evaluated in one update (0x00ddfd50).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    /// The frame the tracks see (floored for integer-frame files, snapped
    /// below a cut).
    pub frame: f32,
    /// The previous update's frame (floored likewise, never snapped).
    pub prev: f32,
    /// `frame` rounded to an integer: selects the key in force. The x87
    /// conversion uses the FPU's rounding mode, *assumed* the default
    /// round-to-nearest-even.
    pub index_frame: u32,
}

impl Clock {
    /// One update of the unit (`uScheduler::vf08` 0x00de02a0): the tracks
    /// are evaluated at the current frame when it differs from the previous
    /// update's (returned as `Some`), then the frame advances by
    /// `speed * delta` unless paused and wraps (looping) or clamps at the
    /// frame count.
    // Translated from 0x00de02a0 and 0x00ddfd50 (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, s: &Scheduler, delta: f32) -> Option<Playhead> {
        let changed = self.frame != self.prev;
        let head = changed.then(|| s.playhead(self.frame, self.prev, self.snap_cuts));
        self.prev = self.frame;
        if !self.paused {
            self.frame += self.speed * delta;
        }
        let end = s.frames as f32;
        if self.speed >= 0.0 || self.frame >= 0.0 {
            if self.frame >= end {
                self.frame = if self.looping { 0.0 } else { end };
            }
        } else {
            self.frame = if self.looping { end } else { 0.0 };
        }
        head
    }
}

/// x87 `fistp` in the default rounding mode (nearest, ties to even).
fn round_even(x: f32) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i64
    } else {
        r as i64
    }
}

impl Scheduler {
    /// The playhead of an update at `frame` after `prev` (0x00ddfd50).
    // Translated from 0x00ddfd50 (DDDA.exe 2017-12-13 Steam).
    pub fn playhead(&self, frame: f32, prev: f32, snap_cuts: bool) -> Playhead {
        let (mut f, mut p) = (frame, prev);
        if self.integer_frames {
            f = f.floor();
            p = p.floor();
        }
        if snap_cuts {
            f = self.snap_to_cut(f);
        }
        Playhead { frame: f, prev: p, index_frame: round_even(f).clamp(0, u32::MAX as i64) as u32 }
    }

    /// With a cut track, a frame whose next integer frame is a cut key is
    /// floored, so the frame before a cut is never interpolated into it.
    // Translated from 0x00ddeb00 (DDDA.exe 2017-12-13 Steam).
    pub fn snap_to_cut(&self, frame: f32) -> f32 {
        let Some(t) = (self.cut_track != 0).then(|| self.tracks.get(self.cut_track as usize)).flatten() else {
            return frame;
        };
        let floor = frame.floor() as i64;
        for k in &t.keys {
            let kf = k.frame as i64;
            if kf == floor + 1 {
                return floor as f32;
            }
            if floor + 1 < kf {
                break;
            }
        }
        frame
    }

    /// The value track `index` applies at the playhead, or `None` when it
    /// applies nothing (no keys; a trigger key the playhead did not cross;
    /// an int key of a mode the evaluator has no case for). Before its
    /// first key a track applies its first key.
    // Translated from 0x00ddee70 (DDDA.exe 2017-12-13 Steam).
    pub fn eval(&self, index: usize, at: &Playhead) -> Option<Value> {
        let t = self.tracks.get(index)?;
        let keys = &t.keys;
        if keys.is_empty() {
            return None;
        }
        let i = keys.partition_point(|k| k.frame <= at.index_frame).saturating_sub(1);
        let k = &keys[i];
        let kf = k.frame as f32;
        if k.mode == Mode::Trigger && !(at.prev <= kf && kf <= at.frame) {
            return None;
        }
        let next = keys.get(i + 1);
        let span = |n: &Key| (at.frame - kf) / (n.frame - k.frame) as f32;
        let raw_mode = match k.mode {
            Mode::Set => 0,
            Mode::Count(m) => m,
            Mode::Trigger => 2,
            Mode::Linear => 3,
            Mode::Curve => 5,
            Mode::Other(m) => m,
        };
        Some(match (&k.value, raw_mode) {
            (Value::Int(v), 0 | 2) => Value::Int(*v),
            (Value::Int(v), 1 | 4) => Value::Int(v.wrapping_sub(k.frame as i32).wrapping_add(at.index_frame as i32)),
            (Value::Int(v), 3 | 5) => match next.map(|n| (n, &n.value)) {
                Some((n, Value::Int(w))) => {
                    let s = span(n);
                    Value::Int(((1.0 - s) * *v as f32 + *w as f32 * s) as i32)
                }
                _ => Value::Int(*v),
            },
            (Value::Int(_), _) => return None,
            (Value::Float(v), 1 | 4) => Value::Float(at.frame - kf + v),
            (Value::Float(v), 3) => match next.map(|n| (n, &n.value)) {
                Some((n, Value::Float(w))) => {
                    let s = span(n);
                    Value::Float((1.0 - s) * v + w * s)
                }
                _ => Value::Float(*v),
            },
            (Value::Float(v), 5) => match next {
                Some(n) => {
                    let s = span(n);
                    let f = |j: usize| match keys[j].value {
                        Value::Float(x) => x,
                        _ => 0.0,
                    };
                    let (p1, p2) = (*v, f(i + 1));
                    let p0 = f(i.saturating_sub(1));
                    let p3 = f(if i + 2 < keys.len() { i + 2 } else { i + 1 });
                    let m0 = (p2 - p0) * 0.5;
                    let m1 = (p3 - p1) * 0.5;
                    Value::Float(
                        (p1 * 2.0 - p2 * 2.0 + m0 + m1) * s * s * s
                            + (p2 * 3.0 - p1 * 3.0 - m0 * 2.0 - m1) * s * s
                            + m0 * s
                            + p1,
                    )
                }
                None => Value::Float(*v),
            },
            (Value::Vec4(v), 1 | 4) => Value::Vec4(v.map(|x| x + (at.frame - kf))),
            (Value::Vec4(v), 3) => match next.map(|n| (n, &n.value)) {
                Some((n, Value::Vec4(w))) => {
                    let s = span(n);
                    Value::Vec4(std::array::from_fn(|c| (1.0 - s) * v[c] + w[c] * s))
                }
                _ => Value::Vec4(*v),
            },
            (Value::Vec4(v), 5) => match next {
                Some(n) => {
                    let s = span(n);
                    let f = |j: usize| match keys[j].value {
                        Value::Vec4(x) => x,
                        _ => [0.0; 4],
                    };
                    let (p1, p2) = (*v, f(i + 1));
                    let p0 = f(i.saturating_sub(1));
                    let p3 = f(if i + 2 < keys.len() { i + 2 } else { i + 1 });
                    let (s2, s3) = (s * s, s * s * s);
                    let h10 = s3 - 2.0 * s2 + s;
                    let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                    let h01 = 3.0 * s2 - 2.0 * s3;
                    let h11 = s3 - s2;
                    Value::Vec4(std::array::from_fn(|c| {
                        h10 * ((p2[c] - p0[c]) * 0.5) + h00 * p1[c] + h01 * p2[c] + h11 * (p3[c] - p1[c]) * 0.5
                    }))
                }
                None => Value::Vec4(*v),
            },
            (Value::Matrix(v), 3) => match next.map(|n| (n, &n.value)) {
                Some((n, Value::Matrix(w))) => {
                    let s = span(n);
                    Value::Matrix(std::array::from_fn(|c| v[c] * (1.0 - s) + w[c] * s))
                }
                _ => Value::Matrix(*v),
            },
            (v, _) => v.clone(),
        })
    }

    /// Track indices by their `path()` (several tracks can share a path,
    /// e.g. two `mAdjustParamList` members).
    pub fn paths(&self) -> std::collections::HashMap<String, Vec<usize>> {
        let mut m: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
        for i in 0..self.tracks.len() {
            m.entry(self.path(i)).or_default().push(i);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    /// Root, one unit, one float track with two keys.
    fn sample() -> Vec<u8> {
        let mut b = vec![];
        b.extend_from_slice(b"SDL\0");
        b.extend_from_slice(&0x13u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&le(0xee48_28d3));
        b.extend_from_slice(&le(100));
        b.extend_from_slice(&le(0));
        let strtab = 0x18 + 3 * 0x18 + 8 + 8;
        b.extend_from_slice(&le(strtab as u32));
        // root
        b.extend_from_slice(&[1, 0]);
        b.extend_from_slice(&0u16.to_le_bytes());
        for v in [0, 0, 0, 0, 0] {
            b.extend_from_slice(&le(v));
        }
        // unit "U" on line 21
        b.extend_from_slice(&[2, 0]);
        b.extend_from_slice(&0u16.to_le_bytes());
        for v in [21, 5, 0x1234, 0, 0] {
            b.extend_from_slice(&le(v));
        }
        // float "f", parent 1, keys at 0x60, values at 0x68
        b.extend_from_slice(&[8, 0xc]);
        b.extend_from_slice(&2u16.to_le_bytes());
        for v in [1, 7, 0, 0x60, 0x68] {
            b.extend_from_slice(&le(v));
        }
        b.extend_from_slice(&le(0));
        b.extend_from_slice(&le(0x0300_000a));
        b.extend_from_slice(&1.0f32.to_le_bytes());
        b.extend_from_slice(&2.0f32.to_le_bytes());
        // string table: 5 bytes pad, "U\0" at 5, "f\0" at 7
        b.extend_from_slice(b"Root\0U\0f\0");
        b
    }

    #[test]
    fn parses_tracks_keys_and_names() {
        let s = Scheduler::parse(&sample()).unwrap();
        assert_eq!(s.frames, 100);
        assert_eq!(s.tracks.len(), 3);
        assert_eq!(s.tracks[1].kind, Kind::Unit);
        assert_eq!(s.tracks[1].link, 21);
        assert_eq!(s.tracks[1].name, "U");
        assert_eq!(s.tracks[2].keys[1].frame, 10);
        assert_eq!(s.tracks[2].keys[1].mode, Mode::Linear);
        assert_eq!(s.tracks[2].keys[1].value, Value::Float(2.0));
        assert_eq!(s.path(2), "U.f");
        s.validate().unwrap();
        assert_eq!(Scheduler::key_at(&s.tracks[2], 9).unwrap().frame, 0);
        assert_eq!(Scheduler::key_at(&s.tracks[2], 10).unwrap().frame, 10);
    }

    fn track(kind: Kind, keys: Vec<(u32, Mode, Value)>) -> Track {
        Track {
            kind,
            property_type: 0,
            link: 1,
            name: String::new(),
            type_hash: 0,
            keys: keys.into_iter().map(|(frame, mode, value)| Key { frame, mode, value }).collect(),
        }
    }

    fn sched(tracks: Vec<Track>, cut_track: u32) -> Scheduler {
        Scheduler { unknown_08: 0, frames: 20, integer_frames: false, frame_flags_high: 0, cut_track, tracks }
    }

    fn at(f: f32, p: f32) -> Playhead {
        Playhead { frame: f, prev: p, index_frame: round_even(f) as u32 }
    }

    #[test]
    fn key_modes() {
        let s = sched(
            vec![
                track(Kind::Root, vec![]),
                // count: value + frames since the key
                track(Kind::Float, vec![(0, Mode::Count(1), Value::Float(0.0)), (10, Mode::Count(1), Value::Float(5.0))]),
                // trigger: only when crossed
                track(Kind::Int, vec![(4, Mode::Trigger, Value::Int(7))]),
                // linear float, Catmull-Rom vec4
                track(Kind::Float, vec![(0, Mode::Linear, Value::Float(0.0)), (10, Mode::Set, Value::Float(10.0))]),
                track(
                    Kind::Vec4,
                    vec![
                        (0, Mode::Curve, Value::Vec4([0.0; 4])),
                        (10, Mode::Curve, Value::Vec4([1.0; 4])),
                        (20, Mode::Curve, Value::Vec4([2.0; 4])),
                    ],
                ),
                // first key after the playhead still applies
                track(Kind::String, vec![(30, Mode::Set, Value::String(Some("x".into())))]),
                // int count uses the rounded frame
                track(Kind::Int, vec![(2, Mode::Count(4), Value::Int(100))]),
            ],
            0,
        );
        assert_eq!(s.eval(1, &at(3.5, 2.5)), Some(Value::Float(3.5)));
        assert_eq!(s.eval(1, &at(12.0, 11.0)), Some(Value::Float(7.0)));
        assert_eq!(s.eval(2, &at(3.0, 2.0)), None);
        assert_eq!(s.eval(2, &at(4.0, 3.0)), Some(Value::Int(7)));
        assert_eq!(s.eval(2, &at(6.0, 5.0)), None);
        assert_eq!(s.eval(3, &at(2.5, 1.5)), Some(Value::Float(2.5)));
        // Catmull-Rom with the end tangents clamped to the last key:
        // p0 0, p1 1, p2 2, p3 2 at s = 0.5 gives 1.5625.
        match s.eval(4, &at(15.0, 14.0)) {
            Some(Value::Vec4(v)) => assert!((v[0] - 1.5625).abs() < 1e-6, "{v:?}"),
            v => panic!("{v:?}"),
        }
        assert_eq!(s.eval(5, &at(0.0, -1.0)), Some(Value::String(Some("x".into()))));
        // 4.5 rounds to 4 (ties to even): 100 - 2 + 4
        assert_eq!(s.eval(6, &at(4.5, 3.5)), Some(Value::Int(102)));
    }

    #[test]
    fn clock_and_cut_snapping() {
        let s = sched(
            vec![track(Kind::Root, vec![]), track(Kind::Int, vec![(0, Mode::Set, Value::Int(0)), (10, Mode::Set, Value::Int(1))])],
            1,
        );
        // 9.5 is floored to 9 because frame 10 is a cut.
        assert_eq!(s.snap_to_cut(9.5), 9.0);
        assert_eq!(s.snap_to_cut(8.5), 8.5);
        let mut c = Clock { paused: false, looping: false, ..Clock::default() };
        // First update evaluates frame 0 (previous -1), then one per frame.
        let heads: Vec<Option<f32>> = (0..25).map(|_| c.step(&s, 1.0).map(|p| p.frame)).collect();
        assert_eq!(heads[0], Some(0.0));
        assert_eq!(heads[20], Some(20.0));
        // Clamped at the frame count: nothing changes any more.
        assert_eq!(heads[21], None);
        assert_eq!(c.frame, 20.0);
        // Looping wraps to 0.
        let mut l = Clock { paused: false, frame: 19.5, prev: 19.0, ..Clock::default() };
        l.step(&s, 1.0);
        assert_eq!(l.frame, 0.0);
        // Paused: evaluated once if the frame moved, then never.
        let mut p = Clock::default();
        assert!(p.step(&s, 1.0).is_some());
        assert!(p.step(&s, 1.0).is_none());
    }

    #[test]
    fn rejects_bad_header_and_truncation() {
        let b = sample();
        let mut bad = b.clone();
        bad[4] = 0x12;
        assert!(Scheduler::parse(&bad).is_err());
        for n in [0, 4, 0x17, 0x30, 0x60] {
            assert!(Scheduler::parse(&b[..n]).is_err(), "truncated at {n}");
        }
    }
}
