//! Reader for `rMotionList` (`.lmt`): the skeletal animations of Dragon's
//! Dogma: Dark Arisen (version 66).
//!
//! Layout and codecs from Riftstone (`lmt.py`, `lmtcodec.py`,
//! `docs/animation.md`; MIT, see `third_party/riftstone-LICENSE.txt`), which
//! measured them on every file. `ddtool lmt-scan` re-checks them against the
//! install. The loader is `rMotionList::load` at 0x00E9D330; our MtDTI labels
//! put that address at `rMotionList` vtable slot 10, the same slot as
//! `rLayout`'s loader.
//!
//! ```text
//! file    "LMT\0", u16 version 66, u16 count, u32 motion offset[count] (0 = empty)
//! motion  60 bytes: u32 tracks, u32 track count, u32 frame count, s32 loop frame,
//!         f32[4] end position, f32[4] end rotation, u32 flags, u32 events, u32 floats
//! track   36 bytes: u8 codec, u8 usage, u8 bone type, u8 joint id, f32 weight,
//!         u32 buffer size, u32 buffer offset, f32[4] reference, u32 extremes offset
//! curve   12 bytes (at "floats", (flags >> 16) & 0x1F of them): u8 value type,
//!         u8[3], u32 key count, u32 keys offset
//! key     16 bytes: u32 (interpolation | frame << 8), value, f32 tangent in/out
//! ```
//!
//! The curves are what the game reads with `0x010571C0` (curve k of the
//! playing motion, e.g. the sprint stop's speed); `Curve::value` is
//! translated from their evaluator. Riftstone calls them float groups and
//! the type byte a component remap.
//!
//! Tracks address joints by the model's joint id (the first byte of a bone
//! record), not by bone index; 255 is the root.

use std::fmt;

pub mod blend;

pub const MAGIC: [u8; 4] = *b"LMT\0";
pub const VERSION: u16 = 66;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LMT at 0x{:x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

fn err<T>(offset: usize, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        offset,
        message: message.into(),
    })
}

/// What a track drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Usage {
    LocalRotation,
    LocalPosition,
    LocalScale,
    AbsoluteRotation,
    AbsolutePosition,
    /// Seen in the data; meaning unknown.
    Usage5,
}

impl Usage {
    fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::LocalRotation,
            1 => Self::LocalPosition,
            2 => Self::LocalScale,
            3 => Self::AbsoluteRotation,
            4 => Self::AbsolutePosition,
            5 => Self::Usage5,
            _ => return None,
        })
    }

    pub fn is_rotation(self) -> bool {
        matches!(self, Self::LocalRotation | Self::AbsoluteRotation)
    }
}

/// One decoded key: the frame it starts on and its value (x, y, z[, w]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub frame: u32,
    pub value: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub codec: u8,
    pub usage: Usage,
    pub bone_type: u8,
    pub joint: u8,
    pub weight: f32,
    /// The pose when the track has no keys; for DDDA's codec 11-13 tracks
    /// also the source of the two unstored rotation axes.
    pub reference: [f32; 4],
    pub keys: Vec<Key>,
}

impl Track {
    /// The track's value at `frame`: the key's value, linearly
    /// interpolated to the next key (*assumed*: the engine's interpolation,
    /// including whether rotations are slerped, is not traced yet; this
    /// normalises the lerped quaternion).
    pub fn sample(&self, frame: f32) -> [f32; 4] {
        let Some(first) = self.keys.first() else {
            return self.reference;
        };
        if frame <= first.frame as f32 {
            return first.value;
        }
        let i = self.keys.partition_point(|k| k.frame as f32 <= frame);
        let a = self.keys[i - 1];
        let Some(b) = self.keys.get(i) else { return a.value };
        let t = (frame - a.frame as f32) / (b.frame - a.frame) as f32;
        let mut bv = b.value;
        if self.usage.is_rotation() {
            // shortest arc
            let dot: f32 = (0..4).map(|j| a.value[j] * bv[j]).sum();
            if dot < 0.0 {
                bv = bv.map(|x| -x);
            }
        }
        let mut v: [f32; 4] = std::array::from_fn(|j| a.value[j] + (bv[j] - a.value[j]) * t);
        if self.usage.is_rotation() {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if n > 0.0 {
                v = v.map(|x| x / n);
            }
        }
        v
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub frame_count: u32,
    pub loop_frame: i32,
    pub end_position: [f32; 4],
    pub end_rotation: [f32; 4],
    pub flags: u32,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionList {
    /// One entry per slot; `None` for empty slots.
    pub motions: Vec<Option<Motion>>,
    /// Each slot's curves (empty for empty slots), indexed as the game's
    /// `0x010571C0(k)`.
    pub curves: Vec<Vec<Curve>>,
}

/// One key of a motion curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveKey {
    /// 0 hold, 1 linear, 2 Hermite (to the next key).
    pub interp: u8,
    pub frame: u16,
    /// The value's raw bits: f32 for float curves, i32 for integer ones.
    pub value: u32,
    pub tangent_in: f32,
    pub tangent_out: f32,
}

/// A scalar curve over the motion's frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// 0 integer, 1 float (others evaluate to 0).
    pub kind: u8,
    pub keys: Vec<CurveKey>,
}

impl Curve {
    /// The curve at `frame` (the motion layer's frame, +0x4C), as the
    /// game's float view of it (`0x010571C0` returns the slot as f32).
    /// Integer curves are returned converted to f32.
    // Translated from 0x010571E0 (DDDA.exe 2017-12-13 Steam). The original
    // keeps the last key index between frames; searching afresh gives the
    // same value.
    pub fn value(&self, frame: f32) -> f32 {
        let as_f = |k: &CurveKey| match self.kind {
            0 => k.value as i32 as f32,
            1 => f32::from_bits(k.value),
            _ => 0.0,
        };
        let keys = &self.keys;
        let Some(first) = keys.first() else { return 0.0 };
        if keys.len() == 1 || frame < f32::from(first.frame) {
            return as_f(first);
        }
        let last = keys.len() - 1;
        let mut i = 0;
        while i < last && f32::from(keys[i + 1].frame) < frame {
            i += 1;
        }
        if i == last {
            return as_f(&keys[last]);
        }
        let (a, b) = (&keys[i], &keys[i + 1]);
        let s = (frame - f32::from(a.frame)) / f32::from(b.frame.wrapping_sub(a.frame));
        match a.interp {
            0 => as_f(a),
            1 if self.kind == 0 => {
                let (va, vb) = (a.value as i32, b.value as i32);
                (((vb.wrapping_sub(va)) as f32 * s) as i32).wrapping_add(va) as f32
            }
            1 => {
                let (va, vb) = (as_f(a), as_f(b));
                (vb - va) * s + va
            }
            2 => {
                let (va, vb, m0, m1) = (as_f(a), as_f(b), a.tangent_in, a.tangent_out);
                let v = (((2.0 * va + m0 - 2.0 * vb + m1) * s + (-3.0 * va - 2.0 * m0 + 3.0 * vb - m1)) * s
                    + m0)
                    * s
                    + va;
                if self.kind == 0 { v.trunc() } else { v }
            }
            _ => 0.0,
        }
    }
}

/// The curves of the motion header at `at`.
fn parse_curves(d: &[u8], at: usize, flags: u32) -> Result<Vec<Curve>, Error> {
    let n = ((flags >> 16) & 0x1F) as usize;
    let base = u32_at(d, at + 56) as usize;
    if n == 0 {
        return Ok(Vec::new());
    }
    if base + 12 * n > d.len() {
        return err(at + 56, "curves outside the file");
    }
    (0..n)
        .map(|c| {
            let q = base + 12 * c;
            let count = u32_at(d, q + 4) as usize;
            let keys_at = u32_at(d, q + 8) as usize;
            if keys_at + 16 * count > d.len() {
                return err(q + 8, "curve keys outside the file");
            }
            let keys = (0..count)
                .map(|k| {
                    let r = keys_at + 16 * k;
                    let packed = u32_at(d, r);
                    CurveKey {
                        interp: (packed & 0xFF) as u8,
                        frame: ((packed >> 8) & 0xFFFF) as u16,
                        value: u32_at(d, r + 4),
                        tangent_in: f32_at(d, r + 8),
                        tangent_out: f32_at(d, r + 12),
                    }
                })
                .collect();
            Ok(Curve { kind: d[q], keys })
        })
        .collect()
}

fn u16_at(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[i], d[i + 1]])
}
fn u32_at(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}
fn f32_at(d: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}
fn f32x4(d: &[u8], i: usize) -> [f32; 4] {
    std::array::from_fn(|j| f32_at(d, i + 4 * j))
}

/// Bytes per key for each codec.
pub fn key_size(codec: u8) -> Option<usize> {
    Some(match codec {
        1 | 2 => 12,
        3 => 16,
        4 | 6 => 8,
        5 | 7 | 11 | 12 | 13 => 4,
        14 => 6,
        15 => 5,
        _ => return None,
    })
}

fn signed14(v: u64) -> f32 {
    let v = (v & 0x3FFF) as i32;
    let s = if v > 8191 { v - 16383 } else { v };
    s as f32 * 4.0 / 16383.0
}

/// Decodes a track buffer into keys. `extremes` is (scale, offset) for the
/// bilinear codecs.
fn decode(
    codec: u8,
    buf: &[u8],
    extremes: Option<([f32; 4], [f32; 4])>,
    reference: [f32; 4],
) -> Option<Vec<Key>> {
    let size = key_size(codec)?;
    let mut out = Vec::with_capacity(buf.len() / size);
    let mut frame = 0u32;
    let bil = |c: [f32; 4], n: usize| -> Option<[f32; 4]> {
        let (scale, offset) = extremes?;
        Some(std::array::from_fn(|i| {
            if i < n {
                offset[i] + scale[i] * c[i]
            } else {
                0.0
            }
        }))
    };
    for k in buf.chunks_exact(size) {
        let (delta, value) = match codec {
            1 => (1, [f32_at(k, 0), f32_at(k, 4), f32_at(k, 8), 0.0]),
            2 => {
                let (x, y, z) = (f32_at(k, 0), f32_at(k, 4), f32_at(k, 8));
                (1, [x, y, z, (1.0 - x * x - y * y - z * z).max(0.0).sqrt()])
            }
            3 => (u32_at(k, 12), [f32_at(k, 0), f32_at(k, 4), f32_at(k, 8), 0.0]),
            4 => {
                let c = [0, 2, 4, 6].map(|i| f32::from(u16_at(k, i)) / 65535.0);
                (u32::from(u16_at(k, 6)), bil(c, 3)?)
            }
            5 => {
                let c = [k[0], k[1], k[2], 0].map(|b| f32::from(b) / 255.0);
                (u32::from(k[3]), bil(c, 3)?)
            }
            6 => {
                let v = u64::from_le_bytes(k.try_into().unwrap());
                (
                    (v >> 56) as u32,
                    [
                        signed14(v >> 42),
                        signed14(v >> 28),
                        signed14(v >> 14),
                        signed14(v),
                    ],
                )
            }
            7 => {
                let v = u32_at(k, 0);
                let c = [21, 14, 7, 0].map(|s| ((v >> s) & 0x7F) as f32 / 127.0);
                (v >> 28, bil(c, 4)?)
            }
            11..=13 => {
                // DDDA (v66): no extremes; stored axis and w signed like codec 6,
                // the other two axes from the reference.
                let v = u32_at(k, 0);
                let mut q = [
                    reference[0],
                    reference[1],
                    reference[2],
                    signed14(u64::from(v >> 14)),
                ];
                q[usize::from(codec - 11)] = signed14(u64::from(v));
                (v >> 28, q)
            }
            14 => {
                let (w0, w1, w2) = (u16_at(k, 0) as u64, u16_at(k, 2) as u64, u16_at(k, 4) as u64);
                let v = w0 | w1 << 16 | w2 << 32;
                let x = v & 0x7FF;
                let y = ((v >> 11) & 0x1F) << 6 | (w1 & 0x3F);
                let z = ((v >> 22) & 0x3FF) << 1 | (w2 & 1);
                let w = (v >> 33) & 0x7FF;
                (
                    (w2 >> 12) as u32,
                    bil([x, y, z, w].map(|c| c as f32 / 2047.0), 4)?,
                )
            }
            15 => {
                let b: [u32; 5] = std::array::from_fn(|i| u32::from(k[i]));
                let x = b[0] << 1 | (b[1] & 1);
                let y = (b[1] >> 1) << 2 | (b[2] & 3);
                let z = (b[2] >> 2) << 3 | (b[3] & 7);
                let w = (b[3] >> 3) << 4 | (b[4] & 0xF);
                (b[4] >> 4, bil([x, y, z, w].map(|c| c as f32 / 511.0), 4)?)
            }
            _ => return None,
        };
        out.push(Key { frame, value });
        frame += delta;
    }
    Some(out)
}

/// Parses a complete motion list.
pub fn parse(d: &[u8]) -> Result<MotionList, Error> {
    if d.len() < 8 || d[0..4] != MAGIC {
        return err(0, "not a motion list");
    }
    let version = u16_at(d, 4);
    if version != VERSION {
        return err(4, format!("version {version}, the game loads {VERSION}"));
    }
    let count = usize::from(u16_at(d, 6));
    if 8 + 4 * count > d.len() {
        return err(6, "offset table runs past the end");
    }
    let mut motions = Vec::with_capacity(count);
    let mut curves = Vec::with_capacity(count);
    for slot in 0..count {
        let at = u32_at(d, 8 + 4 * slot) as usize;
        if at == 0 {
            motions.push(None);
            curves.push(Vec::new());
            continue;
        }
        if at + 60 > d.len() {
            return err(8 + 4 * slot, format!("motion {slot} header outside the file"));
        }
        let tracks_at = u32_at(d, at) as usize;
        let ntracks = u32_at(d, at + 4) as usize;
        let frame_count = u32_at(d, at + 8);
        if tracks_at + 36 * ntracks > d.len() {
            return err(at, format!("motion {slot}: {ntracks} tracks run past the end"));
        }
        let mut tracks = Vec::with_capacity(ntracks);
        for t in 0..ntracks {
            let q = tracks_at + 36 * t;
            let codec = d[q];
            let usage = Usage::from_byte(d[q + 1]).ok_or_else(|| Error {
                offset: q + 1,
                message: format!("usage {}", d[q + 1]),
            })?;
            let size = u32_at(d, q + 8) as usize;
            let buf_at = u32_at(d, q + 12) as usize;
            let reference = f32x4(d, q + 16);
            let ext_at = u32_at(d, q + 32) as usize;
            let extremes = if ext_at != 0 && !(11..=13).contains(&codec) {
                if ext_at + 32 > d.len() {
                    return err(q + 32, "extremes outside the file");
                }
                Some((f32x4(d, ext_at), f32x4(d, ext_at + 16)))
            } else {
                None
            };
            let keys = if size == 0 {
                Vec::new()
            } else {
                let buf = d.get(buf_at..buf_at + size).ok_or_else(|| Error {
                    offset: q + 12,
                    message: "buffer outside the file".into(),
                })?;
                let sz = key_size(codec).ok_or_else(|| Error {
                    offset: q,
                    message: format!("codec {codec}"),
                })?;
                if buf.len() % sz != 0 {
                    return err(
                        q + 8,
                        format!("codec {codec}: {} bytes is not whole keys", buf.len()),
                    );
                }
                decode(codec, buf, extremes, reference).ok_or_else(|| Error {
                    offset: q,
                    message: format!("codec {codec} needs extremes"),
                })?
            };
            tracks.push(Track {
                codec,
                usage,
                bone_type: d[q + 2],
                joint: d[q + 3],
                weight: f32_at(d, q + 4),
                reference,
                keys,
            });
        }
        let flags = u32_at(d, at + 48);
        curves.push(parse_curves(d, at, flags)?);
        motions.push(Some(Motion {
            frame_count,
            loop_frame: u32_at(d, at + 12) as i32,
            end_position: f32x4(d, at + 16),
            end_rotation: f32x4(d, at + 32),
            flags,
            tracks,
        }));
    }
    Ok(MotionList { motions, curves })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(keys: &[(u8, u16, f32)]) -> Curve {
        Curve {
            kind: 1,
            keys: keys
                .iter()
                .map(|&(interp, frame, v)| CurveKey {
                    interp,
                    frame,
                    value: v.to_bits(),
                    tangent_in: 0.0,
                    tangent_out: 0.0,
                })
                .collect(),
        }
    }

    #[test]
    fn curves_hold_interpolate_and_clamp() {
        // m0000_co slot 21 (the sprint stop) curve 9
        let c = curve(&[(1, 0, 1.0), (1, 5, 0.5), (1, 25, 0.0), (1, 29, 0.0)]);
        assert_eq!(c.value(-1.0), 1.0);
        assert_eq!(c.value(0.0), 1.0);
        assert!((c.value(2.5) - 0.75).abs() < 1e-6);
        assert_eq!(c.value(5.0), 0.5);
        assert!((c.value(15.0) - 0.25).abs() < 1e-6);
        assert_eq!(c.value(40.0), 0.0);
        // hold keys keep their value until the next key
        let h = curve(&[(0, 0, 2.0), (0, 10, 3.0)]);
        assert_eq!(h.value(9.9), 2.0);
        assert_eq!(h.value(10.5), 3.0);
        // Hermite with zero tangents is smoothstep
        let e = curve(&[(2, 0, 0.0), (2, 10, 1.0)]);
        assert!((e.value(5.0) - 0.5).abs() < 1e-6);
        assert!((e.value(2.5) - 0.15625).abs() < 1e-6);
        assert_eq!(curve(&[]).value(3.0), 0.0);
    }

    #[test]
    fn codec6_round_values() {
        // x = 4095 (0.99994), y = 0, z = 16383-4095 (negative), w = 0, delta 3
        let v: u64 = 3 << 56 | 4095 << 42 | (16383 - 4095) << 14;
        let k = decode(6, &v.to_le_bytes(), None, [0.0; 4]).unwrap();
        assert_eq!(k.len(), 1);
        assert!((k[0].value[0] - 4095.0 * 4.0 / 16383.0).abs() < 1e-6);
        assert!((k[0].value[2] + 4095.0 * 4.0 / 16383.0).abs() < 1e-6);
    }

    #[test]
    fn codec11_takes_other_axes_from_reference() {
        let v: u32 = (2 << 28) | 4095; // w = 0
        let k = decode(11, &v.to_le_bytes(), None, [0.0, 0.25, 0.5, 0.0]).unwrap();
        assert_eq!(k[0].value[1], 0.25);
        assert_eq!(k[0].value[2], 0.5);
    }

    #[test]
    fn sample_interpolates_and_holds() {
        let t = Track {
            codec: 3,
            usage: Usage::LocalPosition,
            bone_type: 0,
            joint: 1,
            weight: 1.0,
            reference: [0.0; 4],
            keys: vec![
                Key {
                    frame: 0,
                    value: [0.0; 4],
                },
                Key {
                    frame: 10,
                    value: [10.0, 0.0, 0.0, 0.0],
                },
            ],
        };
        assert_eq!(t.sample(5.0)[0], 5.0);
        assert_eq!(t.sample(20.0)[0], 10.0);
    }
}
