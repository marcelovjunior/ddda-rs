//! Bounded DDDA PC weather readers. See docs/WEATHER.md for executable evidence.
//! No rendering, process access, global clock or player state.

use std::fmt;

pub mod clock;
pub mod lighting;
pub mod post;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| Error("size overflow".into()))?;
        let b = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| Error(format!("truncated at {}: need {n} bytes", self.pos)))?;
        self.pos = end;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn float(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() {
            return Err(Error(format!("non-finite float at {}", self.pos - 4)));
        }
        Ok(v)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut a = [0.; N];
        for x in &mut a {
            *x = self.float()?;
        }
        Ok(a)
    }
    fn header(bytes: &'a [u8], magic: &[u8; 4], version: u32) -> Result<Self> {
        let mut r = Self { bytes, pos: 0 };
        if r.take(4)? != magic {
            return Err(Error("bad magic".into()));
        }
        if r.u32()? != version {
            return Err(Error("unsupported version".into()));
        }
        Ok(r)
    }
    fn finish(&self) -> Result<()> {
        if self.pos != self.bytes.len() {
            return Err(Error(format!("{} trailing bytes", self.bytes.len() - self.pos)));
        }
        Ok(())
    }
    fn path(&mut self) -> Result<Vec<u8>> {
        let n = self.bytes[self.pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error("unterminated texture path".into()))?;
        if n > 258 {
            return Err(Error("texture path exceeds loader buffer".into()));
        }
        let path = self.take(n)?.to_vec();
        self.take(1)?;
        Ok(path)
    }
    fn exact_records(&self, count: usize, stride: usize) -> Result<()> {
        let n = count
            .checked_mul(stride)
            .ok_or_else(|| Error("record size overflow".into()))?;
        if self.bytes.len() - self.pos != n {
            return Err(Error(format!(
                "record bounds: {count} x {stride}, {} remaining",
                self.bytes.len() - self.pos
            )));
        }
        Ok(())
    }
}

/// File time keys can include the explicit 24:00 endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeKey {
    pub hour: u32,
    pub minute: u32,
}
impl TimeKey {
    pub fn new(hour: u32, minute: u32) -> Result<Self> {
        if hour > 24 || minute > 59 || (hour == 24 && minute != 0) {
            return Err(Error(format!("invalid time {hour}:{minute}")));
        }
        Ok(Self { hour, minute })
    }
    pub fn minutes(self) -> u32 {
        self.hour * 60 + self.minute
    }
    fn read(r: &mut Reader<'_>) -> Result<Self> {
        Self::new(r.u32()?, r.u32()?)
    }
}

/// Order in the WEP file; symbolic correction ids from the reference grammar.
pub const CORRECTION_TYPES: [u32; 6] = [1, 4, 6, 7, 8, 9];
/// Reflected list names in sWeatherManager::vf03 at 0x004bca80.
pub const CORRECTION_NAMES: [&str; 6] = [
    "EffectColor",
    "WaterEffectColor",
    "AddEffectColor",
    "AddNEffectColor",
    "SubEffectColor",
    "DarkSkyEffectColor",
];

/// Consumer correction ids 2/3 alias list 0; id 5 aliases list 1.
pub fn correction_list(correction_type: u32) -> Option<usize> {
    // Translated from table 0x01875570 used by 0x005d3360
    // (DDDA.exe 2017-12-13 Steam); zero is a disabled/sentinel type.
    match correction_type {
        1..=3 => Some(0),
        4..=5 => Some(1),
        6..=9 => Some(correction_type as usize - 4),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectValues {
    pub color: [u8; 4],
    pub color_blend: f32,
    pub intensity: f32,
    pub intensity_blend: f32,
    pub env_map_power_scale: f32,
    pub shadow_color: [u8; 4],
    pub shadow_color_blend: f32,
    pub shadow_intensity: f32,
    pub shadow_intensity_blend: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectKey {
    pub time: TimeKey,
    pub values: EffectValues,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherEffect {
    /// Original order is retained, including backwards keys in shipped data.
    /// Sampling scans this order just as the executable does; it never sorts.
    pub lists: [Vec<EffectKey>; 6],
}
impl WeatherEffect {
    /// Layout traced from 0x00cea2b0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::header(bytes, b"wep\0", 1)?;
        let mut counts = [0usize; 6];
        let mut total = 0usize;
        for c in &mut counts {
            *c = r.u32()? as usize;
            total = total
                .checked_add(*c)
                .ok_or_else(|| Error("count overflow".into()))?;
        }
        r.exact_records(total, 44)?;
        let mut lists: [Vec<EffectKey>; 6] = std::array::from_fn(|_| Vec::new());
        for (list, count) in lists.iter_mut().zip(counts) {
            for _ in 0..count {
                let time = TimeKey::read(&mut r)?;
                let values = EffectValues {
                    color: r.take(4)?.try_into().unwrap(),
                    color_blend: r.float()?,
                    intensity: r.float()?,
                    intensity_blend: r.float()?,
                    env_map_power_scale: r.float()?,
                    shadow_color: r.take(4)?.try_into().unwrap(),
                    shadow_color_blend: r.float()?,
                    shadow_intensity: r.float()?,
                    shadow_intensity_blend: r.float()?,
                };
                list.push(EffectKey { time, values });
            }
        }
        r.finish()?;
        Ok(Self { lists })
    }

    /// Sample one list at integer clock minutes. Empty lists return None;
    /// callers retain their own defaults. 24:00 copies the first row, as the
    /// manager does, rather than extrapolating the last row through midnight.
    pub fn sample(&self, list: usize, minute_of_day: u32) -> Option<EffectValues> {
        let rows = self.lists.get(list)?;
        let first = rows.first()?;
        let minute = minute_of_day % 1440;
        // Translated from 0x004c19f0 and 0x004c4330 (DDDA.exe 2017-12-13 Steam).
        let upper = rows.iter().position(|r| minute < r.time.minutes());
        match upper {
            Some(0) => Some(first.values),
            Some(i) => {
                let lo = rows[i - 1];
                let hi = rows[i];
                Some(
                    lo.values
                        .lerp(hi.values, fraction(minute, lo.time.minutes(), hi.time.minutes())),
                )
            }
            None => {
                let lo = rows.last()?;
                Some(
                    lo.values
                        .lerp(first.values, fraction(minute, lo.time.minutes(), 1440)),
                )
            }
        }
    }
}

fn fraction(value: u32, lo: u32, hi: u32) -> f32 {
    if hi == lo {
        0.
    } else {
        (value - lo) as f32 / (hi - lo) as f32
    }
}
fn scalar(a: f32, b: f32, t: f32) -> f32 {
    a * (1. - t) + b * t
}
fn rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    // Translated from 0x004c4330 (DDDA.exe 2017-12-13 Steam).
    let weight = (t * 256.) as i32;
    std::array::from_fn(|i| (a[i] as i32 + (((b[i] as i32 - a[i] as i32) * weight) >> 8)) as u8)
}
impl EffectValues {
    fn lerp(self, other: Self, t: f32) -> Self {
        // Translated from 0x004c4330 (DDDA.exe 2017-12-13 Steam).
        Self {
            color: rgba(self.color, other.color, t),
            color_blend: scalar(self.color_blend, other.color_blend, t),
            intensity: scalar(self.intensity, other.intensity, t),
            intensity_blend: scalar(self.intensity_blend, other.intensity_blend, t),
            env_map_power_scale: scalar(self.env_map_power_scale, other.env_map_power_scale, t),
            shadow_color: rgba(self.shadow_color, other.shadow_color, t),
            shadow_color_blend: scalar(self.shadow_color_blend, other.shadow_color_blend, t),
            shadow_intensity: scalar(self.shadow_intensity, other.shadow_intensity, t),
            shadow_intensity_blend: scalar(self.shadow_intensity_blend, other.shadow_intensity_blend, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogKey {
    pub time: TimeKey,
    pub density: f32,
    /// Stored, but 0x00cead20 does not sample or copy this member.
    pub exponent_density: f32,
    pub start: f32,
    pub end: f32,
    pub color: [f32; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogValues {
    pub density: f32,
    pub start: f32,
    pub end: f32,
    pub color: [f32; 3],
}
impl From<FogKey> for FogValues {
    fn from(k: FogKey) -> Self {
        Self {
            density: k.density,
            start: k.start,
            end: k.end,
            color: k.color,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherFog {
    pub keys: Vec<FogKey>,
}
impl WeatherFog {
    /// Layout traced from 0x00ceaa80 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::header(bytes, b"wfp\0", 1)?;
        let count = r.u32()? as usize;
        r.exact_records(count, 36)?;
        let mut keys = Vec::new();
        for _ in 0..count {
            let time = TimeKey::read(&mut r)?;
            keys.push(FogKey {
                time,
                density: r.float()?,
                exponent_density: r.float()?,
                start: r.float()?,
                end: r.float()?,
                color: r.floats()?,
            });
        }
        r.finish()?;
        Ok(Self { keys })
    }
    /// Unlike WEP, fog clamps at both ends and does not append a 24:00 key.
    pub fn sample(&self, minute_of_day: u32) -> Option<FogValues> {
        // Translated from 0x00cead20 (DDDA.exe 2017-12-13 Steam).
        let minute = minute_of_day % 1440;
        let upper = self.keys.iter().position(|k| minute < k.time.minutes());
        match upper {
            Some(0) => self.keys.first().copied().map(Into::into),
            None => self.keys.last().copied().map(Into::into),
            Some(i) => {
                let a = self.keys[i - 1];
                let b = self.keys[i];
                let t = fraction(minute, a.time.minutes(), b.time.minutes());
                Some(FogValues {
                    density: scalar(a.density, b.density, t),
                    start: scalar(a.start, b.start, t),
                    end: scalar(a.end, b.end, t),
                    color: std::array::from_fn(|i| (b.color[i] - a.color[i]) * t + a.color[i]),
                })
            }
        }
    }
}

/// Full SKY v6 record, retaining fields under runtime offsets where semantics
/// have not been independently established. Paths are raw bytes, not lossy UTF-8.
#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub vector_60: [f32; 4],
    pub scalar_90: f32,
    pub vector_80: [f32; 4],
    pub vector_70: [f32; 4],
    pub scalar_94: f32,
    pub word_98: u32,
    pub word_9c: u32,
    pub scalars_a0_bc: [f32; 8],
    pub scalars_c4_cc: [f32; 3],
    pub sun_texture: Vec<u8>,
    pub matrix_d0: [f32; 16],
    pub scalars_110_120: [f32; 5],
    pub vector_140: [f32; 4],
    pub scalar_130: f32,
    pub scalars_124_12c: [f32; 3],
    pub scalar_150: f32,
    pub moon_texture: Vec<u8>,
}
impl Sky {
    /// Layout traced from 0x00e2c350 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::header(bytes, b"SKY ", 6)?;
        let sky = Self {
            vector_60: r.floats()?,
            scalar_90: r.float()?,
            vector_80: r.floats()?,
            vector_70: r.floats()?,
            scalar_94: r.float()?,
            word_98: r.u32()?,
            word_9c: r.u32()?,
            scalars_a0_bc: r.floats()?,
            scalars_c4_cc: r.floats()?,
            sun_texture: r.path()?,
            matrix_d0: r.floats()?,
            scalars_110_120: r.floats()?,
            vector_140: r.floats()?,
            scalar_130: r.float()?,
            scalars_124_12c: r.floats()?,
            scalar_150: r.float()?,
            moon_texture: r.path()?,
        };
        r.finish()?;
        Ok(sky)
    }
}
