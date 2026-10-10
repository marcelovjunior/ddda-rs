//! Installed opening BreathWide collision-unit fields. This is a reader,
//! not an emitter: trajectory, binding, overrides and damage remain separate.

use super::attacks::GeneratorParams;

/// Convert actual type11 samples into ordinary collision entries. The caller
/// supplies the native primary collision resource and its slot1 fallback;
/// this adapter never invents an emitter position or replaces it with a cone.
// Translated from 0x00A56D10's collector and 0x0076F9C0/0x0076FAB0
// (DDDA.exe 2017-12-13 Steam).
pub fn collision_attacks(primary: &ddcombat::ocl::ObjCollision,
    fallback: Option<&ddcombat::ocl::ObjCollision>, owner: u32, samples: &[[f32; 4]])
    -> Result<Vec<ddcombat::volumes::AttackVolume>, String> {
    use ddcombat::volumes::{AttackFlags, AttackVolume, Side, Volume};
    // The collector reads group/attack numbers from SeqIndex slot1, but
    // passes slot0 as the entry's sequence metadata to 0x0076F9C0.
    let (group_no, attack_no) = primary.seq_index.get(1)
        .map_or((0, 0), |s| (s.group_no, s.attack_no));
    let sequence = primary.seq_index.first().ok_or("breath sequence metadata absent")?;
    let group = primary.groups.get(group_no as usize).ok_or("breath collision group absent")?;
    let attack = primary.attacks.get(attack_no as usize).and_then(|a| a.as_ref())
        .or_else(|| fallback.and_then(|f| f.attacks.get(attack_no as usize)).and_then(|a| a.as_ref()))
        .ok_or("breath attack parameter absent")?;
    if !group.is_attack { return Err("unsupported non-attack breath group".into()); }
    samples.iter().take(15).map(|sample| {
        if sample.iter().any(|v| !v.is_finite()) || sample[3] < 0.0 {
            return Err("invalid live collision particle sample".into());
        }
        Ok(AttackVolume { side: Side { owner, kind: group.kind },
            // A56D10 explicitly writes world center+radius and entry type2.
            // The particle radius already contains its native effect scale.
            volume: Volume::Sphere { c: [sample[0], sample[1], sample[2]], r: sample[3] },
            row: ddcombat::damage::AttackRow::from_attack(attack),
            flags: AttackFlags::from_attack(attack),
            reset_time: attack.value("mResetTime").map_or(0.0, |v| v.as_f32()),
            key: ((sequence.no << 8) | group.no) << 16,
            relation: sequence.relation })
    }).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationRange {
    pub base: i16,
    pub variation: u16,
}

/// Effect-owned integer updates and the fractional collision-sample clock.
/// Generator clocks advance only once per returned integer update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectClock {
    pub fraction: f32,
    first_update: bool,
    initial_steps: u16,
}

impl EffectClock {
    // Translated from 0x00FDF5B0 (DDDA.exe 2017-12-13 Steam).
    pub fn new(initial_steps: u16) -> Self {
        Self { fraction: 1.0, first_update: true, initial_steps }
    }

    /// `rate` is the actual effect+0xFC value for this native update.
    /// Birth uses +0x112 instead; it does not consume the rate yet.
    // Translated from 0x00E6BCA0, reached by uEffect::vf09 0x00E70710
    // (DDDA.exe 2017-12-13 Steam).
    pub fn advance(&mut self, rate: f32) -> Result<u16, String> {
        if !rate.is_finite() || rate < 0.0 || !self.fraction.is_finite() {
            return Err("invalid native effect clock".into());
        }
        let value = if self.first_update { self.fraction } else { self.fraction + rate };
        if !(0.0..32768.0).contains(&value) {
            return Err("unsupported native effect update count".into());
        }
        let whole = value.trunc() as u16;
        let steps = if self.first_update { whole.wrapping_add(self.initial_steps).wrapping_sub(1) }
            else { whole };
        self.first_update = false;
        self.fraction = value - value.trunc();
        Ok(steps)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectQuality {
    pub tier: u8,
    pub effect_kind: u8,
    /// Low nibble at effect+0x11A, consumed by duration reduction.
    pub duration_mode: u8,
    /// High nibble at effect+0x119. This is separate from duration mode.
    pub state: u8,
}

/// Select the actual provider quality criterion. Global criteria and quality
/// are supplied by the effect manager, never inferred from this resource.
/// `requested_quality` is the create context's +0x24 bits8..11.
// Translated from 0x00CB6CF0's criterion packing, 0x004298B0's LOD walk,
// and 0x0042D750's +0x118 packing (DDDA.exe 2017-12-13 Steam).
pub fn select_effect_quality(element: &mteffect::Element, requested_quality: u8,
    manager_quality: u32, global_criteria: [u32; 3], distance_squared: f32)
    -> Result<Option<EffectQuality>, String> {
    if requested_quality > 15 || !distance_squared.is_finite() || distance_squared < 0.0 {
        return Err("invalid effect quality context".into());
    }
    let requested = if requested_quality == 3 { manager_quality }
        else { u32::from(requested_quality).min(manager_quality) };
    let tier = if requested < 3 { requested as usize } else { 0 };
    for index in (0..=tier).rev() {
        let criterion = if element.parameters[38] != 0 { global_criteria[index] } else {
            let p = &element.parameters[45 + index * 8..53 + index * 8];
            (p[0] & 0x3FF) | ((p[1] & 0xFF) << 12) | ((p[2] & 7) << 23)
                | ((p[3] & 3) << 26) | ((p[4] & 3) << 10)
                | (u32::from(p[5] != 0) << 20) | (u32::from(p[6] != 0) << 21)
                | (u32::from(p[7] != 0) << 22)
        };
        let threshold = (criterion & 0x3FF) as f32 * 100.0;
        // Native equality falls through to the next lower tier. Tier0 has
        // no distance gate, including when its threshold is zero.
        if index != 0 && threshold * threshold <= distance_squared { continue; }
        let effect_kind = ((criterion >> 23) & 7) as u8;
        if effect_kind == 4 { return Ok(None); }
        return Ok(Some(EffectQuality { tier: index as u8, effect_kind,
            duration_mode: ((criterion >> 10) & 3) as u8,
            state: ((criterion >> 26) & 3) as u8 }));
    }
    unreachable!("native tier0 criterion always terminates the walk")
}

/// Duration reduction selected by the actual parent effect +0x11A low nibble.
/// Caller must supply that state; this reader does not choose a quality mode.
// Translated from 0x010FA8D0's call into 0x010F9CB0
// (DDDA.exe 2017-12-13 Steam).
pub fn adjust_durations_for_owner(durations: [u16; 3], owner_flags: u16) -> [u16; 3] {
    let mode = owner_flags & 0xF;
    if mode == 2 { return durations; }
    let shift = if mode == 0 { 2 } else { 1 };
    durations.map(|duration| if duration == 0 { 0 } else { (duration >> shift).max(1) })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathSample {
    pub position: [f32; 3],
    /// Native 0x400 result. Lifetime4's separate end policy decides retirement.
    pub at_end: bool,
}

/// Sampled native path fields; scale/orientation and RNG belong to the caller.
#[derive(Debug, Clone, Copy)]
pub struct PathDynamics {
    pub segment_length: f32,
    pub damping: f32,
    pub spring: f32,
    pub taper_points: u8,
    pub force: [f32; 3],
}

/// Unconstrained type4 path state. Collision-constrained and angular paths
/// have separate native routines and must not be routed through this one.
#[derive(Debug, Clone)]
pub struct ParticlePath {
    pub points: Vec<[f32; 3]>,
    pub velocities: Vec<[f32; 3]>,
    pub total_length: f32,
}

impl ParticlePath {
    /// Supported zero-angular, unconstrained path's emitter update. The
    /// caller supplies its real current-minus-prior emitter displacement.
    // Translated from 0x010E6720's anchor assignment and final relative
    // coordinate conversion (DDDA.exe 2017-12-13 Steam).
    pub fn advance_emitter(&mut self, dynamics: PathDynamics, displacement: [f32; 3]) -> Result<(), String> {
        if displacement.iter().any(|v| !v.is_finite()) || self.points.len() < 2 {
            return Err("invalid collision emitter displacement".into());
        }
        validate_dynamics(dynamics)?;
        if self.points.len() > u8::MAX as usize || self.points.len() != self.velocities.len()
            || self.points.iter().chain(self.velocities.iter()).flatten().any(|v| !v.is_finite()) {
            return Err("invalid native collision path state".into());
        }
        self.points[0] = displacement;
        self.advance(dynamics)?;
        for point in &mut self.points {
            for axis in 0..3 { point[axis] -= displacement[axis]; }
        }
        Ok(())
    }

    /// Construct from the actual transformed direction and sampled force.
    /// Zero relaxation iterations are not represented by this adapter.
    // Translated from 0x012A1330 (DDDA.exe 2017-12-13 Steam).
    pub fn construct(point_count: u8, direction: [f32; 3], dynamics: PathDynamics,
        iterations: u16) -> Result<Self, String> {
        if point_count < 2 || iterations == 0 || direction.iter().any(|v| !v.is_finite()) {
            return Err("unsupported collision path construction input".into());
        }
        validate_dynamics(dynamics)?;
        let length = vector_length(direction);
        let unit = if length >= f32::EPSILON { direction.map(|v| v / length) } else { direction };
        let delta = unit.map(|v| v * dynamics.segment_length);
        let mut position = [0.0; 3];
        let mut points = Vec::with_capacity(point_count as usize);
        for _ in 0..point_count {
            points.push(position);
            for axis in 0..3 { position[axis] += delta[axis]; }
        }
        let mut path = Self { points, velocities: vec![[0.0; 3]; point_count as usize], total_length: 0.0 };
        for _ in 0..iterations { path.advance(dynamics)?; }
        Ok(path)
    }

    /// One native update, without a guessed delta-time multiplier. The outer
    /// 0x010E6720 adapter must apply the actual emitter displacement first.
    // Translated from 0x0129AF50 (DDDA.exe 2017-12-13 Steam).
    pub fn advance(&mut self, dynamics: PathDynamics) -> Result<(), String> {
        validate_dynamics(dynamics)?;
        if self.points.len() < 2 || self.points.len() > u8::MAX as usize
            || self.points.len() != self.velocities.len()
            || self.points.iter().chain(self.velocities.iter()).flatten().any(|v| !v.is_finite()) {
            return Err("invalid native collision path state".into());
        }
        let taper = u32::from(dynamics.taper_points);
        let inverse_taper_squared = if taper == 0 { 0.0 } else { 1.0 / (taper * taper) as f32 };
        let mut total = 0.0;
        for index in 1..self.points.len() {
            let squared_index = (index * index) as f32;
            let weight = squared_index * inverse_taper_squared;
            let damping = if index < taper as usize {
                dynamics.damping * squared_index * inverse_taper_squared + (1.0 - weight)
            } else { dynamics.damping };
            for axis in 0..3 {
                self.velocities[index][axis] = dynamics.force[axis] + self.velocities[index][axis] * damping;
            }
            let predecessor = self.points[index - 1];
            let delta = std::array::from_fn(|axis| predecessor[axis] - self.points[index][axis]);
            let length = vector_length(delta);
            if dynamics.segment_length < length {
                let correction = ((length - dynamics.segment_length) / length) * dynamics.spring;
                for axis in 0..3 { self.velocities[index][axis] += correction * delta[axis]; }
            }
            for axis in 0..3 { self.points[index][axis] += self.velocities[index][axis]; }
            let delta = std::array::from_fn(|axis| predecessor[axis] - self.points[index][axis]);
            let length = vector_length(delta);
            if length > 1e-8 {
                let correction = (length - dynamics.segment_length) / length;
                for axis in 0..3 {
                    self.points[index][axis] += correction * delta[axis];
                    self.velocities[index][axis] += delta[axis] * correction * dynamics.spring;
                }
            }
            total += vector_length(std::array::from_fn(|axis| predecessor[axis] - self.points[index][axis]));
        }
        self.total_length = total;
        Ok(())
    }
}

fn vector_length(v: [f32; 3]) -> f32 { (v[1] * v[1] + v[0] * v[0] + v[2] * v[2]).sqrt() }

fn validate_dynamics(d: PathDynamics) -> Result<(), String> {
    if [d.segment_length, d.damping, d.spring].iter().chain(d.force.iter()).any(|v| !v.is_finite())
        || d.segment_length < 0.0 {
        return Err("invalid native collision path dynamics".into());
    }
    Ok(())
}

/// Sample the actual constructed type4 path, before the separate spawn-offset
/// transform. This helper cannot construct a path from guessed fire geometry.
// Translated from 0x010E2EB0's path walk (DDDA.exe 2017-12-13 Steam).
pub fn sample_path(points: &[[f32; 3]], total_length: f32, distance: f32,
    emitter_position: [f32; 3]) -> Result<PathSample, String> {
    if points.len() < 2 || !total_length.is_finite() || total_length < 0.0
        || !distance.is_finite() || points.iter().flatten().chain(emitter_position.iter()).any(|v| !v.is_finite()) {
        return Err("invalid native collision path input".into());
    }
    if total_length <= distance {
        let last = points.last().unwrap();
        return Ok(PathSample { position: std::array::from_fn(|i| last[i] + emitter_position[i]), at_end: true });
    }
    let mut cumulative = 0.0;
    let mut index = 0;
    let mut delta = [0.0; 3];
    for segment in points.windows(2) {
        delta = std::array::from_fn(|i| segment[1][i] - segment[0][i]);
        let before = cumulative;
        cumulative += delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        if before < cumulative && distance < cumulative {
            let fraction = (distance - before) / (cumulative - before);
            for value in &mut delta { *value *= fraction; }
            break;
        }
        index += 1;
    }
    // Keep the source's unclamped walk/fraction; the caller supplies its real
    // runtime length field, which normally equals the constructed segment sum.
    Ok(PathSample { position: std::array::from_fn(|i| points[index][i] + emitter_position[i] + delta[i]), at_end: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_birth_skips_rate_then_preserves_fractional_native_updates() {
        let mut clock = EffectClock::new(0);
        assert_eq!(clock.advance(7.0).unwrap(), 0);
        assert_eq!(clock.fraction, 0.0);
        assert_eq!(clock.advance(0.25).unwrap(), 0);
        assert_eq!(clock.fraction, 0.25);
        assert_eq!(clock.advance(0.75).unwrap(), 1);
        assert_eq!(clock.fraction, 0.0);
        assert_eq!(clock.advance(2.5).unwrap(), 2);
        assert_eq!(clock.fraction, 0.5);
        assert_eq!(clock.advance(0.5).unwrap(), 1);
        let mut preadvanced = EffectClock::new(4);
        assert_eq!(preadvanced.advance(0.5).unwrap(), 4);
        assert_eq!(preadvanced.fraction, 0.0);
        assert_eq!(preadvanced.advance(0.5).unwrap(), 0);
        let before = preadvanced;
        assert!(preadvanced.advance(f32::NAN).is_err());
        assert_eq!(preadvanced, before);
    }

    #[test]
    fn provider_quality_uses_global_owner_mode_and_strict_distance_boundary() {
        let mut element = mteffect::Element { lists: std::array::from_fn(|_| String::new()),
            effect_2d: String::new(), parameters: [0; 69] };
        // Synthetic criteria deliberately give state and duration distinct
        // values to catch the native stack-argument/packed-byte confusion.
        for (index, distance) in [0, 4, 2].into_iter().enumerate() {
            element.parameters[45 + index * 8..53 + index * 8]
                .copy_from_slice(&[distance, 0, 1, 3, 0, 0, 0, 0]);
        }
        let globals = [2 << 10, (1 << 26) | (1 << 10) | 4,
            (2 << 26) | (2 << 10) | 2];
        let select = |e: &mteffect::Element, distance| select_effect_quality(e, 3, 2,
            globals, distance).unwrap().unwrap();
        assert_eq!(select(&element, 0.0), EffectQuality { tier: 2, effect_kind: 1,
            state: 3, duration_mode: 0 });
        element.parameters[38] = 1;
        let close = select(&element, 199.0 * 199.0);
        assert_eq!(close, EffectQuality { tier: 2, effect_kind: 0, state: 2, duration_mode: 2 });
        assert_eq!(adjust_durations_for_owner([7, 11, 23], close.duration_mode.into()), [7, 11, 23]);
        let boundary = select(&element, 200.0 * 200.0);
        assert_eq!((boundary.tier, boundary.duration_mode, boundary.state), (1, 1, 1));
        assert_eq!(select(&element, 400.0 * 400.0).tier, 0);
        assert_eq!(select_effect_quality(&element, 2, 1, globals, 0.0).unwrap().unwrap().tier, 1);
        assert_eq!(select_effect_quality(&element, 3, 9, globals, 0.0).unwrap().unwrap().tier, 0);
        assert!(select_effect_quality(&element, 3, 2, [4 << 23; 3], 0.0).unwrap().is_none());
        assert!(select_effect_quality(&element, 3, 2, globals, f32::NAN).is_err());
    }

    #[test]
    fn native_path_spring_projection_preserves_velocity_and_handles_compression() {
        let dynamics = PathDynamics { segment_length: 1.0, damping: 1.0, spring: 0.5,
            taper_points: 0, force: [0.0; 3] };
        let mut path = ParticlePath { points: vec![[0.0; 3], [2.0, 0.0, 0.0]],
            velocities: vec![[0.0; 3]; 2], total_length: 0.0 };
        path.advance(dynamics).unwrap();
        assert_eq!(path.points[1], [1.0, 0.0, 0.0]);
        assert_eq!(path.velocities[1], [-0.75, 0.0, 0.0]);
        assert_eq!(path.total_length, 1.0);
        path.advance(dynamics).unwrap();
        assert_eq!(path.points[1], [1.0, 0.0, 0.0]);
        assert_eq!(path.velocities[1], [-0.375, 0.0, 0.0]);
    }

    #[test]
    fn native_path_taper_and_degenerate_segment_keep_source_boundaries() {
        let dynamics = PathDynamics { segment_length: 1.0, damping: 0.0, spring: 0.5,
            taper_points: 2, force: [0.0; 3] };
        let mut path = ParticlePath { points: vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            velocities: vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], total_length: 0.0 };
        path.advance(dynamics).unwrap();
        assert_eq!(path.velocities[1], [0.375, 0.0, 0.0]);
        assert_eq!(path.velocities[2], [0.0; 3]);
        assert_eq!(path.total_length, 2.0);
        let mut degenerate = ParticlePath { points: vec![[0.0; 3]; 2],
            velocities: vec![[0.0; 3]; 2], total_length: 99.0 };
        degenerate.advance(dynamics).unwrap();
        assert_eq!(degenerate.points, vec![[0.0; 3]; 2]);
        assert_eq!(degenerate.total_length, 0.0);
    }

    #[test]
    fn native_path_constructor_normalizes_direction_and_relaxes_actual_force() {
        let dynamics = PathDynamics { segment_length: 2.0, damping: 0.0, spring: 0.1,
            taper_points: 0, force: [0.0, 0.0, 1.0] };
        let path = ParticlePath::construct(3, [0.0, 0.0, 4.0], dynamics, 2).unwrap();
        assert_eq!(path.points, vec![[0.0; 3], [0.0, 0.0, 2.0], [0.0, 0.0, 4.0]]);
        assert_eq!(path.total_length, 4.0);
        assert!(path.velocities[1][2] > 0.0);
        assert!(ParticlePath::construct(3, [0.0; 3], dynamics, 0).is_err());
        assert!(ParticlePath::construct(3, [f32::NAN; 3], dynamics, 1).is_err());
    }

    #[test]
    fn native_emitter_motion_bends_path_then_restores_relative_anchor() {
        let dynamics = PathDynamics { segment_length: 2.0, damping: 0.0, spring: 0.1,
            taper_points: 0, force: [0.0; 3] };
        let mut path = ParticlePath::construct(3, [0.0, 0.0, 1.0], dynamics, 1).unwrap();
        path.advance_emitter(dynamics, [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(path.points[0], [0.0; 3]);
        assert!(path.points[1][0] < -0.8, "path must retain prior trailing positions");
        assert!(path.points[1][2] > 1.7);
        assert!((path.total_length - 4.0).abs() < 1e-6);
        assert!((vector_length(path.points[1]) - 2.0).abs() < 1e-6);
        assert_ne!(path.velocities[1], [0.0; 3]);
    }

    #[test]
    fn path_walk_uses_segment_lengths_and_native_end_flag_without_time_clamping() {
        let path = [[0.0; 3], [2.0, 0.0, 0.0], [2.0, 4.0, 0.0]];
        let sample = |distance| sample_path(&path, 6.0, distance, [10.0, 20.0, 30.0]).unwrap();
        assert_eq!(sample(3.0), PathSample { position: [12.0, 21.0, 30.0], at_end: false });
        assert_eq!(sample(2.0), PathSample { position: [12.0, 20.0, 30.0], at_end: false });
        assert_eq!(sample(-1.0), PathSample { position: [9.0, 20.0, 30.0], at_end: false });
        assert_eq!(sample(6.0), PathSample { position: [12.0, 24.0, 30.0], at_end: true });
        assert_eq!(sample(8.0), sample(6.0));
        let duplicate = [[0.0; 3], [0.0; 3], [0.0, 2.0, 0.0]];
        assert_eq!(sample_path(&duplicate, 2.0, 1.0, [0.0; 3]).unwrap().position, [0.0, 1.0, 0.0]);
        assert!(sample_path(&[], 0.0, 0.0, [0.0; 3]).is_err());
        assert!(sample_path(&path, 6.0, f32::NAN, [0.0; 3]).is_err());
    }

    #[test]
    fn owner_duration_reduction_preserves_zero_and_positive_minimum() {
        assert_eq!(adjust_durations_for_owner([0, 1, 23], 0), [0, 1, 5]);
        assert_eq!(adjust_durations_for_owner([0, 1, 23], 1), [0, 1, 11]);
        assert_eq!(adjust_durations_for_owner([0, 1, 23], 2), [0, 1, 23]);
        assert_eq!(adjust_durations_for_owner([u16::MAX, 7, 2], 0xFFF2), [u16::MAX, 7, 2]);
        assert_eq!(adjust_durations_for_owner([u16::MAX, 7, 2], 0xFFF0), [16383, 1, 1]);
    }

    fn synthetic_list() -> mteffect::EffectList {
        let mut payload = vec![0; 0x1B0];
        let put = |bytes: &mut [u8], at, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(&mut payload, 0x10 + 8, 2);
        put(&mut payload, 0x10 + 0xC, 5);
        put(&mut payload, 0x10 + 0x30, 0xABCD_0066);
        put(&mut payload, 0x10 + 0x78, 0x1234_5603);
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
        // These packed generator selectors are unrelated to actor bone IDs.
        assert_eq!(unit.generator_axis_selectors, 0x66);
        assert_eq!(unit.emission_shape, 3);
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
    /// Generator+0x30's two axis-selector nibbles; not an actor joint ID.
    /// 0x010EC540 copies them into runtime+0x108 bits8..15.
    pub generator_axis_selectors: u8,
    /// Generator+0x78 low byte, copied to runtime+0x106 for 0x010F37F0.
    pub emission_shape: u8,
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
            capacity: half(generator, 0xC)?, generator_axis_selectors: word(generator, 0x30)? as u8,
            emission_shape: word(generator, 0x78)? as u8,
            radius: [float(particle, 0x40)?, float(particle, 0x44)?, float(particle, 0x48)?],
            fade: [a?, b?, c?], hold_curve: (curve_word >> 1) & 0x7FFF,
            hold_limit: half(movement, 0xE)?, speed: pair(life, 0x28)?,
            acceleration: pair(life, 0x48)?, initial_distance: pair(life, 0x70)?, path_points })
    }

    /// Native raw duration sampling in 0x010FA8D0, before the owner reduction
    /// exposed above. Curved hold durations require their native context first.
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
