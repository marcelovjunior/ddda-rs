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
