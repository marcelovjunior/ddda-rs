//! Installed opening BreathWide collision-unit fields. This is a reader,
//! not an emitter: trajectory, binding, overrides and damage remain separate.

use super::attacks::GeneratorParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationRange {
    pub base: i16,
    pub variation: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_list() -> mteffect::EffectList {
        let mut payload = vec![0; 0x1B0];
        let put = |bytes: &mut [u8], at, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(&mut payload, 0x10 + 8, 2);
        put(&mut payload, 0x10 + 0xC, 5);
        put(&mut payload, 0xD0 + 0x40, 2.0_f32.to_bits());
        put(&mut payload, 0xD0 + 0x44, 1.0_f32.to_bits());
        put(&mut payload, 0xD0 + 0x48, 0.5_f32.to_bits());
        put(&mut payload, 0x120, 1 << 16); // fade-in 0 plus variation1.
        put(&mut payload, 0x124, (3 << 16) | 4); // hold4 plus variation3.
        payload[0x130 + 0x78] = 4;
        mteffect::EffectList { id: 0, auxiliary_count: 0, flags: 0,
            units: vec![mteffect::Unit { generator_kind: 0, particle_kind: 11,
                third_kind: 0x12, life_kind: 4, offsets: [0x10, 0xD0, 0x120, 0x130] }],
            resources: vec![], payload }
    }

    #[test]
    fn raw_duration_sampler_consumes_three_counters_and_inclusive_variations() {
        let unit = CollisionUnit::read("synthetic".into(), &synthetic_list()).unwrap();
        assert_eq!(unit.radius, [2.0, 1.0, 0.5]);
        assert_eq!((unit.capacity, unit.path_points), (5, 4));
        let mut counter = 0xFFE;
        let mut indices = Vec::new();
        let durations = unit.sample_raw_durations(&mut counter, |index| {
            indices.push(index); 42
        }).unwrap();
        assert_eq!(durations, [0, 6, 0]);
        assert_eq!(indices, [0xFFF, 0]);
        assert_eq!(counter, 0x1001);
        let mut curved = unit.clone();
        curved.hold_curve = 1;
        assert!(curved.sample_raw_durations(&mut counter, |_| panic!("unsupported curve must not consume RNG")).is_err());
        assert_eq!(counter, 0x1001);
    }

    #[test]
    fn collision_reader_rejects_missing_variant_inheritance_and_bad_fields() {
        let mut list = synthetic_list();
        list.units[0].third_kind = 2;
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err());
        list.units[0].third_kind = 0x12;
        list.units[0].offsets[0] = 0;
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err());
        list.units[0].offsets[0] = 0x10;
        list.payload[0xD0 + 0x40..0xD0 + 0x44].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err());
        let mut list = synthetic_list();
        list.payload.truncate(0x130 + 0x78);
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollisionUnit {
    pub path: String,
    pub generator: GeneratorParams,
    pub capacity: u16,
    pub binding_joint: i32,
    /// Default particle fields +0x40/44/48, before runtime override +0x128.
    pub radius: [f32; 3],
    pub fade: [DurationRange; 3],
    pub hold_curve: u16,
    pub hold_limit: u16,
    /// Default type4 fields; variation is multiplied by native float RNG.
    pub speed: [f32; 2],
    pub acceleration: [f32; 2],
    pub initial_distance: [f32; 2],
    pub path_points: u8,
}

impl CollisionUnit {
    /// 0x00A55BC0 opening mode1 selects EPV index4, element2 for collision.
    /// The list slot is explicit: native LOD selection is not translated.
    pub fn load(vfs: &ddvfs::Vfs, list_slot: usize) -> Result<Self, String> {
        let bytes = vfs.read("effect/epv/em/em58/p_em5800", mtarc::type_hash("rEffectProvider"))?;
        let provider = mteffect::Provider::parse(&bytes)?;
        let path = provider.element(4, 2).and_then(|e| e.lists.get(list_slot))
            .filter(|p| !p.is_empty()).ok_or("opening collision effect list slot absent")?;
        let bytes = vfs.read(path, mtarc::type_hash("rEffectList"))?;
        Self::read(path.clone(), &mteffect::EffectList::parse(&bytes)?)
    }

    // Field interpretation from 0x010FA3E0, 0x010D6FA0,
    // 0x012AC1E0, 0x010FA8D0 and 0x010EDC90
    // (DDDA.exe 2017-12-13 Steam). Offsets are decoded EFL payload offsets,
    // never process addresses. Other generator/life/movement kinds reject.
    pub fn read(path: String, list: &mteffect::EffectList) -> Result<Self, String> {
        let [unit] = list.units.as_slice() else { return Err("collision effect needs exactly one unit".into()); };
        if (unit.generator_kind, unit.particle_kind, unit.third_kind, unit.life_kind) != (0, 11, 0x12, 4) {
            return Err("unsupported opening collision generator/particle/movement/life kinds".into());
        }
        let [generator, particle, movement, life] = unit.offsets.map(|v| v as usize);
        if unit.offsets.contains(&0) { return Err("collision unit inherits an unported payload".into()); }
        let bytes = &list.payload;
        let half = |base: usize, offset: usize| -> Result<u16, String> {
            let at = base.checked_add(offset).ok_or("collision field offset overflow")?;
            let end = at.checked_add(2).ok_or("collision field end overflow")?;
            Ok(u16::from_le_bytes(bytes.get(at..end).ok_or("truncated collision unit field")?.try_into().unwrap()))
        };
        let word = |base: usize, offset: usize| -> Result<u32, String> {
            let at = base.checked_add(offset).ok_or("collision field offset overflow")?;
            let end = at.checked_add(4).ok_or("collision field end overflow")?;
            Ok(u32::from_le_bytes(bytes.get(at..end).ok_or("truncated collision unit field")?.try_into().unwrap()))
        };
        let float = |base, offset| -> Result<f32, String> {
            let value = f32::from_bits(word(base, offset)?);
            if !value.is_finite() { return Err("nonfinite collision unit scalar".into()); }
            Ok(value)
        };
        let pair = |base, offset| -> Result<[f32; 2], String> {
            Ok([float(base, offset)?, float(base, offset + 4)?])
        };
        let fade = [0, 4, 8].map(|offset| -> Result<DurationRange, String> {
            Ok(DurationRange { base: half(movement, offset)? as i16, variation: half(movement, offset + 2)? })
        });
        let [a, b, c] = fade;
        let curve_word = half(movement, 0xC)?;
        if curve_word & 1 != 0 { return Err("unported movement duration flags".into()); }
        let point_at = life.checked_add(0x78).ok_or("collision path offset overflow")?;
        let path_points = *bytes.get(point_at).ok_or("truncated collision path count")?;
        if path_points < 2 { return Err("collision path has fewer than two points".into()); }
        Ok(Self { path, generator: GeneratorParams { flags: word(generator, 8)?,
                restart_delay: half(generator, 0xAC)? as i16, restart_variation: half(generator, 0xAE)? },
            capacity: half(generator, 0xC)?, binding_joint: word(generator, 0x30)? as i32,
            radius: [float(particle, 0x40)?, float(particle, 0x44)?, float(particle, 0x48)?],
            fade: [a?, b?, c?], hold_curve: (curve_word >> 1) & 0x7FFF,
            hold_limit: half(movement, 0xE)?, speed: pair(life, 0x28)?,
            acceleration: pair(life, 0x48)?, initial_distance: pair(life, 0x70)?, path_points })
    }

    /// Native raw duration sampling in 0x010FA8D0, before 0x010F9CB0's
    /// owner-specific duration adjustment. Do not substitute this for that
    /// adapter. Curved hold durations require their native context first.
    pub fn sample_raw_durations(&self, counter: &mut u32,
        mut random_word: impl FnMut(usize) -> u32) -> Result<[u16; 3], String> {
        if self.hold_curve != 0 { return Err("curved hold duration needs native context".into()); }
        Ok(self.fade.map(|range| {
            *counter = counter.wrapping_add(1);
            let extra = if range.variation == 0 { 0 } else {
                random_word((*counter & 0xFFF) as usize) % (u32::from(range.variation) + 1)
            };
            (range.base as u16).wrapping_add(extra as u16)
        }))
    }
}
