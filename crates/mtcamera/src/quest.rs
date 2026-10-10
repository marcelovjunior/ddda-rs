//! Authored stage event cameras. Stage loader 0x00501020 requests
//! `rCameraParamList scr/stNNN/etc/stNNN` into stage+0x964; loader
//! 0x007d01b0 preserves indexed entries (DDDA.exe 2017-12-13 Steam).
//! This reader supports cCamExParamAnim, not actor-relative AnimTar,
//! templates or LCM motion cameras. All keys remain installation data.

use crate::{Error, Result};
use mtxfs::{Object, Value, Xfs};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub roll_degrees: f32,
    /// Native sequence FOV is vertical at the reference 16:9 viewport.
    pub fov_degrees: f32,
}

impl Pose {
    pub fn mix(self, other: Self, weight: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * weight;
        Self {
            eye: std::array::from_fn(|i| mix(self.eye[i], other.eye[i])),
            target: std::array::from_fn(|i| mix(self.target[i], other.target[i])),
            roll_degrees: mix(self.roll_degrees, other.roll_degrees),
            fov_degrees: mix(self.fov_degrees, other.fov_degrees),
        }
    }
    fn channels(self) -> [f32; 8] {
        [
            self.eye[0],
            self.eye[1],
            self.eye[2],
            self.target[0],
            self.target[1],
            self.target[2],
            self.roll_degrees,
            self.fov_degrees,
        ]
    }
    fn from_channels(v: [f32; 8]) -> Self {
        Self {
            eye: [v[0], v[1], v[2]],
            target: [v[3], v[4], v[5]],
            roll_degrees: v[6],
            fov_degrees: v[7],
        }
    }
    /// Type 3 camera interpolation: shortest yaw/pitch arcs, linear target
    /// and distance. Translated from 0x007e3c70, 0x007d6150, 0x007de430.
    /// Type 2 instead mixes eye and target independently in world space.
    pub fn interpolate(self, other: Self, weight: f32, kind: u32) -> Option<Self> {
        if kind == 2 {
            return Some(self.mix(other, weight));
        }
        if kind != 3 {
            return None;
        }
        let polar = |p: Self| {
            let d: [f32; 3] = std::array::from_fn(|i| p.eye[i] - p.target[i]);
            let length = d.iter().map(|v| v * v).sum::<f32>().sqrt();
            (
                d[0].atan2(d[2]),
                if length > f32::EPSILON {
                    (d[1] / length).clamp(-1.0, 1.0).asin()
                } else {
                    0.0
                },
                length,
            )
        };
        let (ay, ap, ad) = polar(self);
        let (by, bp, bd) = polar(other);
        let angle = |a: f32, b: f32| {
            a + ((b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI)
                * weight
        };
        let yaw = angle(ay, by);
        let pitch = angle(ap, bp);
        let length = ad + (bd - ad) * weight;
        let mut result = self.mix(other, weight);
        let offset = [
            yaw.sin() * pitch.cos() * length,
            pitch.sin() * length,
            yaw.cos() * pitch.cos() * length,
        ];
        result.eye = std::array::from_fn(|i| result.target[i] + offset[i]);
        Some(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub seconds: f32,
    pub kind: i32,
    pub pose: Pose,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blend {
    pub kind: u32,
    pub frame_type: u32,
    pub frames: f32,
    pub curve: [f32; 16],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub keys: Vec<Key>,
    pub camera_target: u32,
    pub option_flags: u32,
    pub start: Blend,
    pub end: Blend,
    pub cant_act_player: bool,
    pub cant_act_flags: u32,
    /// Native range stores start and length, not start and end.
    pub cant_act_frames: [f32; 2],
    pub world_slow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    /// Camera IDs index the authored array, including other kinds/nulls.
    pub animations: Vec<Option<Animation>>,
}

impl List {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_xfs(&mtxfs::parse(bytes).map_err(|e| Error(e.to_string()))?)
    }
    fn from_xfs(x: &Xfs) -> Result<Self> {
        if x.class_of(&x.root).name() != Some("rCameraParamList") {
            return Err(Error("expected rCameraParamList".into()));
        }
        let slots = array(x, &x.root, "mParamList")?;
        let animations = slots
            .iter()
            .map(|slot| match slot {
                Value::Object(Some(o)) if x.class_of(o).name() == Some("cCamExParamAnim") => {
                    let keys = array(x, o, "mCameraInfoList")?
                        .iter()
                        .map(|key| {
                            let Value::Object(Some(key)) = key else {
                                return Err(Error("null camera sequence key".into()));
                            };
                            if x.class_of(key).name() != Some("cCameraSeqInfo") {
                                return Err(Error("unexpected camera key class".into()));
                            }
                            Ok(Key {
                                seconds: scalar(x, key, "mTime")?,
                                kind: integer(x, key, "mType")?,
                                pose: Pose {
                                    eye: vector(x, key, "mCameraPos")?,
                                    target: vector(x, key, "mTargetPos")?,
                                    roll_degrees: scalar(x, key, "mRoll")?,
                                    fov_degrees: scalar(x, key, "mFov")?,
                                },
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    // 0x007b76b0 marks an empty sequence ended immediately.
                    // It has no authored pose to present; preserve its slot
                    // as unsupported rather than rejecting the stage list.
                    if keys.is_empty() {
                        return Ok(None);
                    }
                    if keys.iter().any(|k| {
                        !k.seconds.is_finite()
                            || k.seconds < 0.0
                            || !(0..=2).contains(&k.kind)
                            || k.pose.channels().iter().any(|v| !v.is_finite())
                    }) || keys.windows(2).any(|k| k[0].seconds > k[1].seconds)
                    {
                        return Err(Error("invalid authored camera sequence".into()));
                    }
                    Ok(Some(Animation {
                        keys,
                        camera_target: unsigned(x, o, "mCameraTarget")?,
                        option_flags: unsigned(x, o, "mOptionFlag")?,
                        start: blend(x, o, "Start")?,
                        end: blend(x, o, "End")?,
                        cant_act_player: boolean(x, o, "mIsCantActPlayer")?,
                        cant_act_flags: unsigned(x, o, "mCantActFlag")?,
                        cant_act_frames: floats(x, o, "mPlCantActFrame")?,
                        world_slow: boolean(x, o, "mIsUseWorldSlow")?,
                    }))
                }
                Value::Object(_) => Ok(None),
                _ => Err(Error("camera slot is not an object".into())),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { animations })
    }
}

impl Animation {
    pub fn duration(&self) -> f32 {
        self.keys.last().map_or(0.0, |k| k.seconds)
    }
    pub fn supported_world_camera(&self) -> bool {
        let blend_supported = |blend: &Blend| {
            blend.frames.is_finite()
                && blend.frames >= 0.0
                && (blend.frames <= f32::EPSILON
                    || (blend.frame_type == 0
                        && matches!(blend.kind, 2 | 3)
                        && blend.curve.iter().all(|v| v.is_finite())))
        };
        // 0x007e0d50 locks distinct input domains with flags1/2. This
        // adapter supports their combined flag3 (camera12) and the fully
        // unblocked case (camera149), not either domain on its own.
        let input_supported = self.cant_act_flags == 3 || (self.cant_act_flags == 0 && !self.cant_act_player);
        self.camera_target == 2
            && self.option_flags & !1 == 0
            && !self.world_slow
            && blend_supported(&self.start)
            && blend_supported(&self.end)
            && input_supported
    }
    /// 0x007e0d50: the player input locks apply during the authored range.
    pub fn blocks_actions(&self, frames: f32) -> bool {
        (self.cant_act_player || self.cant_act_flags & 3 != 0)
            && frames >= self.cant_act_frames[0]
            && frames <= self.cant_act_frames[0] + self.cant_act_frames[1]
    }
    /// Translated from 0x007b9f20 / 0x007ba2d0. Eight independent scalar
    /// Hermite channels. Type 1 kills the outgoing tangent; type 2 also
    /// replaces the following key value with the previous value.
    /// Sampling exactly the final time still runs: native ends only after it.
    pub fn sample(&self, seconds: f32) -> (Pose, bool) {
        let running = seconds <= self.duration();
        let seconds = seconds.clamp(0.0, self.duration());
        let hi = self
            .keys
            .iter()
            .position(|k| seconds <= k.seconds)
            .unwrap_or(self.keys.len() - 1);
        let lo = hi.saturating_sub(1);
        let a = self.keys[lo].seconds;
        let b = self.keys[hi].seconds;
        let t = if a == b { 1.0 } else { (seconds - a) / (b - a) };
        let channels = std::array::from_fn(|channel| {
            let left = self.channel(lo, channel);
            let right = self.channel(hi, channel);
            if left.2 == right.1 {
                (1.0 - t) * left.0 + t * right.0
            } else {
                let t2 = t * t;
                let t3 = t2 * t;
                (1.0 - (3.0 * t2 - 2.0 * t3)) * left.0
                    + (3.0 * t2 - 2.0 * t3) * right.0
                    + (t3 - 2.0 * t2 + t) * left.2
                    + (t3 - t2) * right.1
            }
        });
        (Pose::from_channels(channels), running)
    }
    fn channel(&self, index: usize, channel: usize) -> (f32, f32, f32) {
        let previous = index.saturating_sub(1);
        let next = (index + 1).min(self.keys.len() - 1);
        let mut value = self.keys[index].pose.channels()[channel];
        let before = self.keys[previous].pose.channels()[channel];
        let after = self.keys[next].pose.channels()[channel];
        let dt_before = self.keys[index].seconds - self.keys[previous].seconds;
        let dt_after = self.keys[next].seconds - self.keys[index].seconds;
        let (mut incoming, mut outgoing) = if index == 0 {
            (after - value, after - value)
        } else if next == index {
            (value - before, value - before)
        } else {
            let slope = if dt_before + dt_after > f32::EPSILON {
                (after - before) / (dt_before + dt_after)
            } else {
                0.0
            };
            (slope * dt_before, slope * dt_after)
        };
        if index > 0 && dt_before == 0.0 {
            incoming = 0.0;
            outgoing = after - value;
        }
        if next != index && dt_after == 0.0 {
            incoming = value - before;
            outgoing = 0.0;
        }
        if self.keys[index].kind == 1 || self.keys[index].kind == 2 {
            outgoing = 0.0;
        }
        if index > 0 && self.keys[previous].kind == 1 {
            incoming = 0.0;
        }
        if index > 0 && self.keys[previous].kind == 2 {
            incoming = 0.0;
            value = before;
        }
        (value, incoming, outgoing)
    }
}

/// Translated from 0x00d4ec50: the 8-point reflected MtEaseCurve used by
/// camera interpolation 0x007e3c70. Input is a completed fraction.
pub fn ease(curve: &[f32; 16], t: f32) -> f32 {
    if t <= curve[0] {
        return curve[8];
    }
    if t >= 1.0 {
        return (0..8)
            .find(|&i| curve[i] == 1.0)
            .map_or(curve[15], |i| curve[i + 8]);
    }
    let hi = (1..8).find(|&i| t <= curve[i]).unwrap_or(7);
    let lo = hi - 1;
    let width = curve[hi] - curve[lo];
    if width <= 0.0 {
        return curve[hi + 8];
    }
    let a = curve[lo + 8];
    let b = curve[hi + 8];
    let m0 = if lo == 0 {
        b - a
    } else {
        (b - ((1.0 - width / (curve[lo] - curve[lo - 1])) * (a - curve[lo + 7]) + curve[lo + 7])) * 0.5
    };
    let m1 = if curve[hi] == 1.0 {
        b - a
    } else {
        ((width / (curve[hi + 1] - curve[hi])) * (curve[hi + 9] - b) + b - a) * 0.5
    };
    let f = (t - curve[lo]) / width;
    (((a * 2.0 - b * 2.0 + m0 + m1) * f * f * f) + ((b * 3.0 - a * 3.0 - m0 * 2.0 - m1) * f * f) + f * m0 + a)
        .clamp(0.0, 1.0)
}

fn array<'a>(x: &'a Xfs, o: &'a Object, name: &str) -> Result<&'a [Value]> {
    let Some([Value::Object(Some(a))]) = x.get(o, name) else {
        return Err(Error(format!("missing {name}")));
    };
    x.get(a, "mpArray")
        .ok_or_else(|| Error(format!("missing {name}.mpArray")))
}
fn floats<const N: usize>(x: &Xfs, o: &Object, name: &str) -> Result<[f32; N]> {
    match x.get(o, name) {
        Some([Value::F(v)]) if v.len() == N => Ok(std::array::from_fn(|i| v[i] as f32)),
        _ => Err(Error(format!("missing {name} float[{N}]"))),
    }
}
fn scalar(x: &Xfs, o: &Object, name: &str) -> Result<f32> {
    Ok(floats::<1>(x, o, name)?[0])
}
fn vector(x: &Xfs, o: &Object, name: &str) -> Result<[f32; 3]> {
    let v = floats::<4>(x, o, name)?;
    Ok([v[0], v[1], v[2]])
}
fn integer(x: &Xfs, o: &Object, name: &str) -> Result<i32> {
    match x.get(o, name) {
        Some([Value::S(v)]) => i32::try_from(*v).map_err(|_| Error(format!("invalid {name}"))),
        Some([Value::U(v)]) => i32::try_from(*v).map_err(|_| Error(format!("invalid {name}"))),
        _ => Err(Error(format!("missing {name}"))),
    }
}
fn unsigned(x: &Xfs, o: &Object, name: &str) -> Result<u32> {
    match x.get(o, name) {
        Some([Value::U(v)]) => u32::try_from(*v).map_err(|_| Error(format!("invalid {name}"))),
        _ => u32::try_from(integer(x, o, name)?).map_err(|_| Error(format!("negative {name}"))),
    }
}
fn boolean(x: &Xfs, o: &Object, name: &str) -> Result<bool> {
    match x.get(o, name) {
        Some([Value::Bool(v)]) => Ok(*v),
        _ => Err(Error(format!("missing {name}"))),
    }
}
fn blend(x: &Xfs, o: &Object, part: &str) -> Result<Blend> {
    Ok(Blend {
        kind: unsigned(x, o, &format!("m{part}InterType"))?,
        frame_type: unsigned(x, o, &format!("m{part}FrameType"))?,
        frames: scalar(x, o, &format!("m{part}InterFrame"))?,
        curve: floats(x, o, &format!("m{part}InterCurve"))?,
    })
}
