//! The ordinary em0100 sensor branch used by the roadside rescue.
//! Sphere rows, pose-relative FOV/LOS and the ordinary distance influence.
//! Other shapes and hate contributions beyond distance/provoke remain unsupported.

use crate::goblin::Target;

#[derive(Debug, Clone)]
pub struct SensorRow {
    pub class: u32, pub types: u32, pub enabled_by: u32,
    pub shape: u32, pub radius: f32, pub fov: f32, pub flags: u32,
    pub joint: i32, pub offset: [f32; 3],
    pub direction: [f32; 3],
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
                joint: word(14) as i32, offset: std::array::from_fn(|i| f32::from_bits(word(16 + i))),
                direction: std::array::from_fn(|i| f32::from_bits(word(8 + i))) }
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
        struct Clear;
        impl ddsim::Ground for Clear { fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None } }
        // Compatibility entry point has no posed joint or LOS provider.
        if self.0.iter().any(|row| (row.enabled_by == 0 || row.enabled_by & mask != 0)
            && row.class & 2 != 0 && (row.joint >= 0 || row.flags & 1 != 0)) {
            return Err("sensor needs a posed joint or LOS provider".into());
        }
        let pose = crate::body::Posed { model: crate::body::model_matrix(from, yaw), joints: vec![] };
        self.query(mask, u32::MAX, 2, &pose, 1.0, target, subtype, &Clear)
    }

    /// Native row-index mask and class mask are independent of the active
    /// resource group. `pose` is the actor's current coordinate/joint matrices.
    // Translated from 0x005AF1F0, 0x005AFC50, 0x00CA8EC0 and LOS 0x005AF9A0
    // (DDDA.exe 2017-12-13 Steam). Ground.ray supplies scenery only; the
    // original collision layer masks/target proxy radii are not yet modeled.
    pub fn query(&self, mask: u32, rows: u32, class: u32, pose: &crate::body::Posed,
        radius_scale: f32, target: Target, subtype: u32, ground: &dyn ddsim::Ground) -> Result<bool, String> {
        if subtype > 31 || (0x16 & (1 << subtype)) == 0 { return Ok(false); }
        if !target.alive { return Ok(false); }
        let mut admitted = false;
        for (index, row) in self.0.iter().enumerate() {
            if rows & 1u32.rotate_left(index as u32) == 0 { continue; }
            if row.enabled_by != 0 && row.enabled_by & mask == 0 { continue; }
            if row.class & class == 0 || row.types & (1 << subtype) == 0 { continue; }
            if row.shape != 5 || row.direction != [0.0, 0.0, 1.0] || row.flags & !9 != 0 {
                return Err("active goblin sensor needs unported shape, direction or flags".into());
            }
            let mut matrix = if row.joint == -2 { crate::body::model_matrix([0.0; 3], 0.0) }
                else if row.joint == -1 { pose.model }
                else { pose.joints.iter().find(|(joint, _)| i32::from(*joint) == row.joint)
                    .map_or(pose.model, |(_, matrix)| *matrix) };
            // CA8AE0 strips coordinate scale before transforming the row.
            for (index, axis) in matrix.iter_mut().take(3).enumerate() {
                let squared = axis.iter().map(|v| v * v).sum::<f32>();
                if squared <= f32::EPSILON { *axis = [0.0; 4]; axis[index] = 1.0; }
                else { let length = squared.sqrt(); for value in axis { *value /= length; } }
            }
            if row.flags & 8 != 0 {
                let squared = matrix[2][0].powi(2) + matrix[2][2].powi(2);
                let length = squared.sqrt();
                let forward = if squared > f32::EPSILON { [matrix[2][0] / length, 0.0, matrix[2][2] / length] }
                    else { [0.0, 0.0, 1.0] };
                matrix[0] = [forward[2], 0.0, -forward[0], 0.0];
                matrix[1] = [0.0, 1.0, 0.0, 0.0];
                matrix[2] = [forward[0], 0.0, forward[2], 0.0];
            }
            let center = std::array::from_fn::<_, 3, _>(|i| matrix[3][i]
                + row.offset[0] * matrix[0][i] + row.offset[1] * matrix[1][i] + row.offset[2] * matrix[2][i]);
            let d2: f32 = (0..3).map(|i| (center[i] - target.pos[i]).powi(2)).sum();
            let radius = row.radius * radius_scale;
            // Sphere point test 0x00D134F0 squares the signed radius.
            if !(d2 <= radius * radius) { continue; }
            if row.flags & 1 != 0 && (d2 <= f32::EPSILON.powi(2)
                || (d2 >= 0.01 && ground.ray(target.pos, center).is_some())) { continue; }
            if row.fov < 360.0 {
                let dx = target.pos[0] - center[0]; let dz = target.pos[2] - center[2];
                let squared = dx * dx + dz * dz; let length = squared.sqrt();
                let (dx, dz) = if squared > f32::EPSILON { (dx / length, dz / length) } else { (0.0, 1.0) };
                let squared = matrix[2][0].powi(2) + matrix[2][2].powi(2); let length = squared.sqrt();
                let (fx, fz) = if squared > f32::EPSILON { (matrix[2][0] / length, matrix[2][2] / length) } else { (0.0, 1.0) };
                if (dx * fx + dz * fz).clamp(-1.0, 1.0).acos() > row.fov.to_radians() * 0.5 { continue; }
            }
            admitted = true;
        }
        Ok(admitted)
    }
}

/// Native 16-target distance and provoke influence entries. Additional damage/augment
/// influences and the special grabbed/cached-target overrides are unported.
#[derive(Debug, Clone, Default)]
pub struct DistanceHate(pub Vec<(u32, f32)>, Vec<usize>, Vec<Provoke>);

#[derive(Debug, Clone)]
struct Provoke { owner: u32, score: f32, decay: f32 }

/// Ordinary direct-detection branch of the goblin alert update. Ally alert
/// propagation, noises and damage notifications require explicit future inputs.
#[derive(Debug, Clone, Default)]
pub struct Alert {
    pub state: u8,
    pub search_position: [f32; 3],
    pub search_left: f32,
}

impl Alert {
    /// `seen` is class2 in modes0/2, class1 or row-index2 in mode1.
    /// The search point must come from the native downcast, never a timer guess.
    // Translated from 0x008D1280, search timeout update 0x00AA96E0;
    // group switch 0x009BB750 (DDDA.exe 2017-12-13 Steam).
    pub fn update(&mut self, mode: u8, seen: Option<[f32; 3]>, from: [f32; 3], step: f32) {
        if mode == 1 { self.search_left = (self.search_left - step).max(-1.0); }
        self.state = match mode {
            0 => if let Some(position) = seen {
                self.search_position = position;
                self.search_left = 900.0; // 0x44610000 at 0x008D1B81.
                1
            } else { 0 },
            1 => if seen.is_some() { 2 }
                else if self.search_left <= 0.0 || (0..3).map(|i|
                    (from[i] - self.search_position[i]).powi(2)).sum::<f32>() < 48400.0 { 0 }
                else { 1 },
            2 => if seen.is_some() { 2 } else { 0 },
            _ => self.state,
        };
    }

    pub fn group(mode: u8) -> u32 { 1 << mode.min(2) }
}

impl DistanceHate {
    /// Ordinary goblin's message 0x22, with no damage-vector payload.
    /// The caller supplies the installed adjustment amount, already truncated
    /// as B81600 does. This zero-vector branch requires positive installed
    /// param/game[0x14] and [0x16]; nonzero damage vectors remain unsupported.
    /// Distance influence remains independently refreshed.
    // Translated from 0x00AAC5A0/0x00AACA80 and 0x00AB5860
    // (DDDA.exe 2017-12-13 Steam). Evaluation type 5 has cap 900.
    pub fn provoke(&mut self, owner: u32, amount: u32, decay: u32) {
        if !self.0.iter().any(|(id, _)| *id == owner) {
            if self.0.len() == 16 { return; }
            let slot = (0..16).find(|slot| !self.1.contains(slot)).unwrap();
            let index = self.1.binary_search(&slot).unwrap_err();
            self.1.insert(index, slot);
            self.0.insert(index, (owner, 0.0));
        }
        if let Some(entry) = self.2.iter_mut().find(|entry| entry.owner == owner) {
            entry.score = (entry.score + amount as f32).min(900.0);
            entry.decay = decay as f32;
        } else {
            self.2.push(Provoke { owner, score: (amount as f32).min(900.0), decay: decay as f32 });
        }
    }

    /// Sum of the two supported native influence types (AB52E0).
    pub fn score(&self, owner: u32) -> f32 {
        self.0.iter().find(|(id, _)| *id == owner).map_or(0.0, |(_, score)| *score)
            + self.2.iter().find(|entry| entry.owner == owner).map_or(0.0, |entry| entry.score)
    }
    // Translated from 0x00AB5A50 and 0x00AB52E0 (DDDA.exe 2017-12-13 Steam).
    pub fn select(&mut self, admitted: &[Target], all: &[Target], from: [f32; 3]) -> Option<Target> {
        for target in admitted.iter().filter(|t| t.alive) {
            let distance = (0..3).map(|i| (target.pos[i] - from[i]).powi(2)).sum::<f32>().sqrt();
            // 0x008D2930 supplies 3000 cm to 0x00AB5A50.
            if distance >= 3000.0 { continue; }
            let score = ((distance / 3000.0 - 1.0) * 300.0).abs().min(300.0);
            if score <= 0.0 { continue; }
            if let Some(entry) = self.0.iter_mut().find(|(owner, _)| *owner == target.id) { entry.1 = score; }
            else if self.0.len() < 16 {
                // AB5A50 fills the first vacant native target slot. Retiring
                // a slot cannot move later entries ahead of a replacement.
                let slot = (0..16).find(|slot| !self.1.contains(slot)).unwrap();
                let index = self.1.binary_search(&slot).unwrap_err();
                self.1.insert(index, slot);
                self.0.insert(index, (target.id, score));
            }
        }
        let mut best = None;
        let mut maximum = 0.0;
        for &(owner, _) in &self.0 {
            let score = self.score(owner);
            let Some(target) = all.iter().find(|t| t.id == owner && t.alive) else { continue };
            if score > maximum { maximum = score; best = Some(*target); }
        }
        best
    }

    pub fn retire(&mut self, owner: u32) {
        self.2.retain(|entry| entry.owner != owner);
        for index in (0..self.0.len()).rev() {
            if self.0[index].0 == owner { self.0.remove(index); self.1.remove(index); }
        }
    }

    /// The distance contribution stores decay1; decay happens once per vf90,
    /// independently of the actor delta (0x00AB5860 from 0x00AA96E0).
    // Translated from 0x00AB5860 (DDDA.exe 2017-12-13 Steam).
    pub fn tick(&mut self) {
        for (_, score) in &mut self.0 { *score = (*score - 1.0).max(0.0); }
        for entry in &mut self.2 { entry.score -= entry.decay; }
        self.2.retain(|entry| entry.score > 0.0);
        for index in (0..self.0.len()).rev() {
            if self.0[index].1 <= 0.0 && !self.2.iter().any(|entry| entry.owner == self.0[index].0) {
                self.0.remove(index); self.1.remove(index);
            }
        }
    }
}

#[cfg(test)]
mod provoke_tests {
    use super::*;

    #[test]
    fn provoke_is_additive_capped_and_decays_separately_from_distance() {
        let near = Target { id: 10, pos: [0.0; 3], alive: true };
        let far = Target { id: 20, pos: [0.0, 0.0, 2000.0], alive: true };
        let all = [near, far];
        let mut hate = DistanceHate::default();
        assert_eq!(hate.select(&all, &all, [0.0; 3]).unwrap().id, 10);
        // B81600's installed base amount and fixed message decay flag1.
        hate.provoke(20, 500, 1);
        assert_eq!(hate.select(&all, &all, [0.0; 3]).unwrap().id, 20);
        let distance = hate.0.iter().find(|(id, _)| *id == 20).unwrap().1;
        assert_eq!(hate.score(20), distance + 500.0);
        hate.provoke(20, 500, 1);
        assert_eq!(hate.score(20), distance + 900.0);
        hate.tick();
        assert_eq!(hate.score(20), distance - 1.0 + 899.0);
        assert_eq!(hate.select(&all, &all, [0.0; 3]).unwrap().id, 20);
        assert_eq!(hate.score(20), distance + 899.0);
        hate.retire(20);
        assert_eq!(hate.score(20), 0.0);
        assert_eq!(hate.select(&[], &all, [0.0; 3]).unwrap().id, 10);
    }

    #[test]
    fn provoke_preserves_first_native_target_slot_and_does_not_require_sight() {
        let first = Target { id: 11, pos: [0.0; 3], alive: true };
        let second = Target { id: 22, ..first };
        let mut hate = DistanceHate::default();
        hate.provoke(11, 500, 1);
        hate.provoke(22, 500, 1);
        assert_eq!(hate.select(&[], &[second, first], [0.0; 3]).unwrap().id, 11);
        for _ in 0..500 { hate.tick(); }
        assert!(hate.select(&[], &[second, first], [0.0; 3]).is_none());
        assert!(hate.0.is_empty());
    }
}
