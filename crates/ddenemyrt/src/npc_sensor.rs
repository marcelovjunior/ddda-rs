//! The ordinary em0100 sensor branch used by the roadside rescue.
//! Other shapes, FOV/LOS tests and non-distance hate contributions remain
//! unsupported. Resource records are read from the player's installation.

use crate::goblin::Target;

#[derive(Debug, Clone)]
pub struct SensorRow {
    pub class: u32, pub types: u32, pub enabled_by: u32,
    pub shape: u32, pub radius: f32, pub fov: f32, pub flags: u32,
    pub joint: i32, pub offset: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Sensor(pub Vec<SensorRow>);

impl Sensor {
    // Translated from rAISensorExt::vf10 0x00CA8D90 / row lookup 0x00CA8D70
    // (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 16 || &bytes[..4] != b"SNR2" || bytes[4..6] != [0x12, 0] {
            return Err("unsupported AISensor header".into());
        }
        let count = u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as usize;
        if bytes.len() < 16 + count * 80 { return Err("truncated AISensor rows".into()); }
        let rows = (0..count).map(|i| {
            let row = &bytes[16 + i * 80..16 + (i + 1) * 80];
            let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
            SensorRow { class: word(0), types: word(2), enabled_by: word(3), shape: word(5),
                radius: f32::from_bits(word(6)), fov: f32::from_bits(word(12)), flags: word(13),
                joint: word(14) as i32, offset: std::array::from_fn(|i| f32::from_bits(word(16 + i))) }
        }).collect();
        Ok(Self(rows))
    }

    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::parse(&crate::body::load::read(vfs, r"AI\Sensor\Enemy\em0100A", "rAISensorExt")?)
    }

    /// Returns an error if an active query row requires an unported branch.
    // Translated from 0x005B0160, 0x005AF1F0 and 0x005AFC50;
    // em0100 query masks 2 / 0x16 from 0x008D2930
    // (DDDA.exe 2017-12-13 Steam).
    pub fn admits(&self, mask: u32, from: [f32; 3], yaw: f32,
        target: Target, subtype: u32) -> Result<bool, String> {
        if subtype > 31 || (0x16 & (1 << subtype)) == 0 { return Ok(false); }
        let mut admitted = false;
        for row in &self.0 {
            if row.enabled_by != 0 && row.enabled_by & mask == 0 { continue; }
            if row.class & 2 == 0 || row.types & 0x16 == 0 || row.types & (1 << subtype) == 0 { continue; }
            if row.shape != 5 || row.joint != -1 || row.flags & 1 != 0 || row.fov < 360.0 {
                return Err("active goblin sensor needs unported shape, joint, LOS or FOV".into());
            }
            let (s, c) = yaw.sin_cos();
            let center = [from[0] + row.offset[0] * c + row.offset[2] * s,
                from[1] + row.offset[1], from[2] - row.offset[0] * s + row.offset[2] * c];
            let d2: f32 = (0..3).map(|i| (center[i] - target.pos[i]).powi(2)).sum();
            admitted |= d2 <= row.radius * row.radius;
        }
        Ok(admitted)
    }
}

/// Native 16-target distance influence entries. Additional damage/augment
/// influences and the special grabbed/cached-target overrides are unported.
#[derive(Debug, Clone, Default)]
pub struct DistanceHate(pub Vec<(u32, f32)>);

impl DistanceHate {
    // Translated from 0x00AB5A50 and 0x00AB52E0 (DDDA.exe 2017-12-13 Steam).
    pub fn select(&mut self, admitted: &[Target], all: &[Target], from: [f32; 3]) -> Option<Target> {
        for target in admitted.iter().filter(|t| t.alive) {
            let distance = (0..3).map(|i| (target.pos[i] - from[i]).powi(2)).sum::<f32>().sqrt();
            // 0x008D2930 supplies 3000 cm to 0x00AB5A50.
            if distance >= 3000.0 { continue; }
            let score = ((distance / 3000.0 - 1.0) * 300.0).abs().min(300.0);
            if score <= 0.0 { continue; }
            if let Some(entry) = self.0.iter_mut().find(|(owner, _)| *owner == target.id) { entry.1 = score; }
            else if self.0.len() < 16 { self.0.push((target.id, score)); }
        }
        let mut best = None;
        let mut maximum = 0.0;
        for &(owner, score) in &self.0 {
            let Some(target) = all.iter().find(|t| t.id == owner && t.alive) else { continue };
            if score > maximum { maximum = score; best = Some(*target); }
        }
        best
    }

    pub fn retire(&mut self, owner: u32) { self.0.retain(|(id, _)| *id != owner); }
}
