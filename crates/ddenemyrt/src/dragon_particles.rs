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

/// The two independently advanced generator counters. Seed selection uses
/// the installed resource's seed list and the actual effect RNG selector;
/// it does not choose a project-wide seed or merge birth and path draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorCounters {
    initial_seed: u16,
    pub birth: u32,
    pub path: u32,
}

/// Values sampled by generator vf07 before the first binding/birth.
/// The auxiliary triplet is not the effect's axis scale used by vf15.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneratorInitialization {
    pub particle_scale: f32,
    pub auxiliary_triplet: [f32; 3],
}

/// Uncurved life4 birth values, before sampling the actual path position.
/// The secondary-effect delay does not suspend the particle's movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Life4Birth {
    pub speed: f32,
    pub acceleration: f32,
    pub distance: f32,
    pub secondary_effect_delay: i16,
}

/// Generator-owned uncurved life4 travel, with the actual two distance buffers.
/// Admission, curves, scene queries, spawn-offset rotation, retirement and
/// secondary dispatch remain with their native owner; this is their travel phase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Life4Travel {
    pub distance: [f32; 2],
    pub speed: f32,
    pub acceleration: f32,
    pub secondary_effect_delay: i16,
}

impl Life4Travel {
    // Translated from 0x010EDE08..0x010EDE22
    // (DDDA.exe 2017-12-13 Steam).
    pub fn from_birth(birth: Life4Birth) -> Self {
        Self { distance: [birth.distance; 2], speed: birth.speed,
            acceleration: birth.acceleration,
            secondary_effect_delay: birth.secondary_effect_delay }
    }

    /// The caller has already admitted this native update and resolved any
    /// speed curve. Actual particle-context +0x0C bits25/24 select previous
    /// and current distance buffers. The coordinate anchor is +0x30, not a
    /// guessed actor/fire position. This returns the pre-spawn-offset sample.
    /// No delta multiplier or speed/distance clamp is present in this phase.
    /// Secondary delay changes only later, after the actual query/end policy.
    // Translated from 0x010F71F2..0x010F7237, 0x010F74EE..0x010F7524
    // and 0x010E2EB0 (DDDA.exe 2017-12-13 Steam).
    pub fn advance_path(&mut self, context_flags: u32, path: &ParticlePath,
        coordinate_anchor: [f32; 3]) -> Result<PathSample, String> {
        let previous = ((context_flags >> 25) & 1) as usize;
        let current = ((context_flags >> 24) & 1) as usize;
        let distance = self.distance[previous] + self.speed;
        let speed = self.speed + self.acceleration;
        if self.distance.iter().chain([self.speed, self.acceleration, distance, speed].iter())
            .any(|value| !value.is_finite()) {
            return Err("nonfinite life4 travel state".into());
        }
        let sample = sample_path(&path.points, path.total_length, distance, coordinate_anchor)?;
        // Rejection is a project validation policy; commit the supported phase
        // only after a valid path sample, rather than leaving a partial state.
        self.distance[current] = distance;
        self.speed = speed;
        Ok(sample)
    }
}

impl GeneratorCounters {
    /// The caller supplies the resource seed entries, including its first
    /// entry when the native count is zero. An absent list is unsupported.
    /// `global_random` must use the effect manager's actual selected stream.
    // Translated from 0x010FA600..0x010FA66E
    // (DDDA.exe 2017-12-13 Steam).
    pub fn select(seeds: &[i32], effect_flags: u32,
        mut global_random: impl FnMut(u32) -> u32) -> Result<Self, String> {
        if seeds.is_empty() || seeds.len() > usize::from(u8::MAX) {
            return Err("generator seed list is absent or exceeds its native byte count".into());
        }
        let selector = effect_flags & 8;
        let index = if seeds.len() < 2 { 0 }
            else { (global_random(selector) % seeds.len() as u32) as usize };
        let selected = seeds[index];
        let initial_seed = if selected < 0 { (global_random(selector) & 0xFFF) as u16 }
            else { selected as u16 };
        Ok(Self { initial_seed, birth: u32::from(initial_seed), path: u32::from(initial_seed) })
    }

    /// Reuse resets both draw positions to the original low-16-bit seed.
    /// Resetting the generator must not consume another global RNG value.
    // Translated from 0x010FA570 and 0x010FA580
    // (DDDA.exe 2017-12-13 Steam).
    pub fn reset(&mut self) {
        self.birth = u32::from(self.initial_seed);
        self.path = u32::from(self.initial_seed);
    }
}

/// Installed per-coordinate resource, separate from every generator payload.
/// Dynamic curve offsets are retained; this reader does not execute curves,
/// choose a global seed, compose transforms or create an effect.
#[derive(Debug, Clone, PartialEq)]
pub struct CoordinateDefinition {
    pub flags: u32,
    pub binding_options: u16,
    pub binding_bits: u16,
    pub scale: [[f32; 2]; 3],
    pub local_offset: [f32; 3],
    /// Copied by0x010FBB20, not consumed as a float by0x010FC1D0.
    /// Encoded signed parent joint; negative values select the effect parent.
    pub offset_tail: u32,
    pub local_quaternion: [f32; 4],
    pub seed: i32,
    pub delay: DurationRange,
    pub curve_offsets: [u16; 4],
}

/// Static local coordinate output before vf43 applies its parent binding.
/// Scale remains separate: multiplying it into this rotation would apply
/// the effect/parent scale twice when the actual coordinate is assembled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoordinateBasis {
    pub rotation: mtlmt::pose::Mat4,
    pub offset: [f32; 3],
    pub scale: [f32; 3],
    pub largest_scale: f32,
}

/// Supported primary-parent vf43 result. Native stores the same matrix in
/// both blocks when its secondary-binding option is absent; scale is separate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoordinateTransform {
    pub primary: mtlmt::pose::Mat4,
    pub secondary: mtlmt::pose::Mat4,
    pub scale: [f32; 3],
    pub largest_scale: f32,
}

/// Generator-owned scaled basis used by native life4 path setup. This is
/// distinct from the coordinate's normalized primary/secondary matrices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneratorTransform {
    pub matrix: mtlmt::pose::Mat4,
    pub axis_scale: [f32; 3],
    pub particle_scale: f32,
}

/// Uncurved zero-angular life4 setup, ready for real emitter displacement.
/// Points remain relative to the coordinate anchor, as in the native buffer.
#[derive(Debug, Clone)]
pub struct BoundPath {
    pub path: ParticlePath,
    pub dynamics: PathDynamics,
}

impl GeneratorTransform {
    /// The ordinary 0x010F1230 caller supplies +Z, transformed through the
    /// generator's scaled basis. Force keeps that unnormalized direction;
    /// only the initial point spacing normalizes it. Particle scalar scales
    /// the authored total length separately from those generator axis scales.
    // Translated from 0x010F1230 -> 0x010E5620 -> 0x012A1330
    // (DDDA.exe 2017-12-13 Steam).
    pub fn bind_path(&self, point_count: u8, parameters: &PathParameters,
        sampled: SampledPath) -> Result<BoundPath, String> {
        if point_count < 2 || parameters.flags != 0 || parameters.curves != [0; 4]
            || sampled.angular_velocity != 0.0 || sampled.length_velocity != 0.0
            || sampled.rotation != [0.0; 3] {
            return Err("unsupported curved, angular or changing-length path binding".into());
        }
        if self.matrix.iter().flatten().chain([self.particle_scale, sampled.length,
            sampled.force, sampled.damping, sampled.spring].iter()).any(|v| !v.is_finite()) {
            return Err("nonfinite native path binding".into());
        }
        let direction = [self.matrix[2][0], self.matrix[2][1], self.matrix[2][2]];
        let dynamics = PathDynamics {
            segment_length: (sampled.length.max(0.0) / f32::from(point_count - 1))
                * self.particle_scale,
            damping: sampled.damping, spring: sampled.spring,
            taper_points: parameters.taper_points,
            force: direction.map(|component| component * sampled.force),
        };
        let path = ParticlePath::construct(point_count, direction, dynamics, parameters.iterations)?;
        Ok(BoundPath { path, dynamics })
    }
}

impl CoordinateTransform {
    /// Caller supplies the actual uEffect +1A0/+1B0 values and generator
    /// +134 sample. `coordinate_scale_mode` is resource flags bits16..19:
    /// 10FBEC0..10FBED5 pack it into coordinate +10C bits20..23.
    /// `world_axes` is the generator's actual +10C bit12; it is not inferred
    /// from a particle's shape or the effect's quality.
    // Translated from 0x010D79F0 (DDDA.exe 2017-12-13 Steam).
    pub fn generator_transform(&self, effect_axis_scale: [f32; 3],
        effect_particle_scale: f32, sampled_generator_scale: f32,
        coordinate_scale_mode: u8, world_axes: bool) -> Result<GeneratorTransform, String> {
        if coordinate_scale_mode > 15
            || effect_axis_scale.iter().chain([effect_particle_scale,
                sampled_generator_scale, self.largest_scale].iter())
                .chain(self.scale.iter()).chain(self.primary.iter().flatten())
                .any(|v| !v.is_finite()) {
            return Err("invalid generator transform input".into());
        }
        let mut particle_scale = effect_particle_scale * sampled_generator_scale;
        let mut axis_scale = effect_axis_scale.map(|axis| axis * particle_scale);
        if coordinate_scale_mode == 0 {
            particle_scale *= self.largest_scale;
            for axis in 0..3 { axis_scale[axis] *= self.scale[axis]; }
        }
        // The native world-axis constants are the identity rows at016294A0.
        let mut matrix = if world_axes { mtlmt::pose::IDENTITY } else { self.primary };
        for axis in 0..3 {
            for value in &mut matrix[axis] { *value *= axis_scale[axis]; }
        }
        // The world-axis branch also retains the actual coordinate anchor.
        matrix[3] = self.primary[3];
        if !particle_scale.is_finite() || matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err("generator transform overflow".into());
        }
        Ok(GeneratorTransform { matrix, axis_scale, particle_scale })
    }
}

impl CoordinateDefinition {
    /// The apparent fourth offset float is a signed parent joint identifier.
    /// A negative identifier selects the effect's unscaled parent matrix.
    // Translated from 0x010FBB20 -> 0x00E73720 -> 0x00E705C0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn parent_joint(&self) -> i32 { self.offset_tail as i32 }

    /// Nonmirrored static basis only. Mirrored quaternion conversion and
    /// vf43 parent/secondary binding remain separate native adapters.
    // Translated from 0x010FC1D0 (DDDA.exe 2017-12-13 Steam).
    pub fn static_basis(&self, sampled_scale: [f32; 3], effect_scale: [f32; 3],
        mirrored: bool) -> Result<CoordinateBasis, String> {
        if mirrored { return Err("mirrored coordinate needs native quaternion conversion".into()); }
        if self.curve_offsets.iter().any(|&offset| offset != 0) {
            return Err("dynamic coordinate basis needs native context".into());
        }
        if self.local_quaternion.iter().chain(self.local_offset.iter())
            .chain(sampled_scale.iter()).chain(effect_scale.iter()).any(|v| !v.is_finite()) {
            return Err("nonfinite static coordinate input".into());
        }
        let scale = std::array::from_fn(|axis| effect_scale[axis] * sampled_scale[axis]);
        if scale.iter().any(|v: &f32| !v.is_finite()) { return Err("static coordinate scale overflow".into()); }
        let mut largest_scale = scale[0];
        if largest_scale <= scale[1] { largest_scale = scale[1]; }
        if scale[2] > largest_scale { largest_scale = scale[2]; }
        // Preserve the raw quaternion and native scalar operation order
        // without normalizing the quaternion.
        let [x, y, z, w] = self.local_quaternion;
        let double_z = z * 2.0;
        let double_y = y * 2.0;
        let double_x = x * 2.0;
        let double_wx = w * double_x;
        let double_xx = x * double_x;
        let double_zz = z * double_z;
        let rotation = [
            [1.0 - (double_zz + y * double_y), x * double_y + w * double_z,
                x * double_z - w * double_y, 0.0],
            [x * double_y - w * double_z, 1.0 - (double_zz + double_xx),
                y * double_z + double_wx, 0.0],
            [x * double_z + w * double_y, y * double_z - double_wx,
                1.0 - (y * double_y + double_xx), 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        if rotation.iter().flatten().any(|v| !v.is_finite()) {
            return Err("static coordinate rotation overflow".into());
        }
        Ok(CoordinateBasis { rotation, offset: self.local_offset, scale, largest_scale })
    }

    /// Primary binding0 / parent-scale mode0 only. The caller supplies the
    /// real vf44-resolved parent: negative parent_joint uses effect+0x120,
    /// NOT the separately scaled final effect+0xB0 provider matrix.
    // Translated from 0x00E73720, writes0x00E6BD70/0x00E6BDE0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn static_parent(&self, basis: CoordinateBasis, parent: &mtlmt::pose::Mat4,
        mirrored: bool) -> Result<CoordinateTransform, String> {
        if mirrored || self.curve_offsets != [0; 4]
            || self.flags >> 12 & 0xF != 0 || self.flags >> 16 & 0xF != 0
            || self.binding_options & 0x10 != 0 || self.binding_bits & 1 != 0 {
            return Err("unsupported static parent binding mode or postprocess".into());
        }
        if parent.iter().flatten().chain(basis.rotation.iter().flatten())
            .chain(basis.offset.iter()).chain(basis.scale.iter()).any(|v| !v.is_finite()) {
            return Err("invalid static parent coordinate".into());
        }
        // Native normalization includes all four row components. Preserve
        // zero/very small rows, and retain original lengths in the scale.
        let normalize = |row: &mut [f32; 4]| -> f32 {
            let length = (row[0] * row[0] + row[1] * row[1]
                + row[2] * row[2] + row[3] * row[3]).sqrt();
            if length >= f32::EPSILON {
                let reciprocal = 1.0 / length;
                for value in row { *value *= reciprocal; }
            }
            length
        };
        let mut normalized_parent = *parent;
        let mut scale = basis.scale;
        for axis in 0..3 { scale[axis] *= normalize(&mut normalized_parent[axis]); }
        if scale.iter().any(|v| !v.is_finite()) {
            return Err("static parent scale overflow".into());
        }
        let mut offset = basis.offset;
        if self.binding_options & 0x20 != 0 {
            for axis in 0..3 { offset[axis] *= scale[axis]; }
        }
        let mut primary = [[0.0; 4]; 4];
        for row in 0..3 {
            for column in 0..4 {
                primary[row][column] = basis.rotation[row][0] * normalized_parent[0][column]
                    + basis.rotation[row][1] * normalized_parent[1][column]
                    + basis.rotation[row][2] * normalized_parent[2][column]
                    + basis.rotation[row][3] * normalized_parent[3][column];
            }
            normalize(&mut primary[row]);
        }
        for column in 0..4 {
            primary[3][column] = normalized_parent[0][column] * offset[0]
                + normalized_parent[1][column] * offset[1]
                + normalized_parent[2][column] * offset[2] + normalized_parent[3][column];
        }
        if primary.iter().flatten().any(|v| !v.is_finite()) {
            return Err("static parent transform overflow".into());
        }
        let mut largest_scale = scale[0];
        if largest_scale <= scale[1] { largest_scale = scale[1]; }
        if scale[2] > largest_scale { largest_scale = scale[2]; }
        Ok(CoordinateTransform { primary, secondary: primary, scale, largest_scale })
    }

    // Translated from 0x01147790, 0x010F9DB0, 0x010FA080 and
    // 0x010FBB20 (DDDA.exe 2017-12-13 Steam).
    pub fn read(list: &mteffect::EffectList, index: u16) -> Result<Self, String> {
        if index >= list.auxiliary_count { return Err("effect coordinate index absent".into()); }
        // Native +0x76 is a ushort containing unitCount<<4. Reject the
        // overflowing resource-table case rather than invent an extended format.
        let table = list.units.len().checked_mul(16).filter(|&v| v <= u16::MAX as usize)
            .ok_or("effect coordinate table overflow")?;
        let at = table.checked_add(usize::from(index) * 4).ok_or("coordinate offset overflow")?;
        let offset = u32::from_le_bytes(list.payload.get(at..at + 4)
            .ok_or("truncated coordinate offset table")?.try_into().unwrap()) as usize;
        let resource = list.payload.get(offset..offset.checked_add(0x70)
            .ok_or("coordinate resource overflow")?).ok_or("truncated coordinate resource")?;
        let half = |at: usize| u16::from_le_bytes(resource[at..at + 2].try_into().unwrap());
        let word = |at: usize| u32::from_le_bytes(resource[at..at + 4].try_into().unwrap());
        let float = |at: usize| -> Result<f32, String> {
            let value = f32::from_bits(word(at));
            if !value.is_finite() { return Err("nonfinite coordinate scalar".into()); }
            Ok(value)
        };
        let vector = |at: usize| -> Result<[f32; 4], String> {
            Ok([float(at)?, float(at + 4)?, float(at + 8)?, float(at + 12)?])
        };
        Ok(Self { flags: word(0), binding_options: half(4), binding_bits: half(6),
            scale: [[float(8)?, float(12)?], [float(16)?, float(20)?], [float(24)?, float(28)?]],
            local_offset: [float(0x20)?, float(0x24)?, float(0x28)?], offset_tail: word(0x2C),
            local_quaternion: vector(0x30)?, seed: word(0x60) as i32,
            delay: DurationRange { base: half(0x64) as i16, variation: half(0x66) },
            curve_offsets: [half(0x68), half(0x6A), half(0x6C), half(0x6E)] })
    }

    /// Caller owns the actual global RNG stream selected by effect flags.
    // Translated from 0x010FA080 (DDDA.exe 2017-12-13 Steam).
    pub fn initial_counter(&self, mut global_random: impl FnMut() -> u32) -> u32 {
        if self.seed < 0 { global_random() & 0xFFF } else { self.seed as u32 & 0xFFFF }
    }

    /// Static coordinate subset only. Readable random tables stay installed;
    /// caller supplies table access and the owning effect's sampled delay.
    /// Float multiplication and addition have separate f32 rounding, matching
    /// MULSS/ADDSS at0x010FBFE4..0x010FC046 (not a fused multiply-add).
    /// This advances the initialization sampling counter only: 0x00E6E030
    /// resets the runtime counter to the original low-16-bit seed after the
    /// coordinate's initial transform. Do not start runtime draws at seed+4.
    // Translated from 0x010FBB20, delay0x010FC162
    // (DDDA.exe 2017-12-13 Steam).
    pub fn sample_static(&self, counter: &mut u32, effect_delay: i16,
        mut random_float: impl FnMut(usize) -> f32,
        mut random_word: impl FnMut(usize) -> u32) -> Result<([f32; 3], u16), String> {
        if self.curve_offsets.iter().any(|&offset| offset != 0) {
            return Err("dynamic coordinate curves need native context".into());
        }
        let scale = self.scale.map(|[base, variation]| {
            *counter = counter.wrapping_add(1);
            random_float((*counter & 0xFFF) as usize) * variation + base
        });
        // Counter advances even when variation is0; the word read does not.
        *counter = counter.wrapping_add(1);
        let extra = if self.delay.variation == 0 { 0 } else {
            random_word((*counter & 0xFFF) as usize) % (u32::from(self.delay.variation) + 1)
        };
        Ok((scale, (effect_delay as u16).wrapping_add(self.delay.base as u16).wrapping_add(extra as u16)))
    }
}

/// Native world-query descriptor, resolved relative to the life payload.
/// This is separate from the secondary effect's travel delay at life+0x44.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldCollision {
    pub flags: u32,
    pub radius_growth: f32,
    pub radius: f32,
    pub radius_limit_extra: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorldQuery {
    Sphere { from: [f32; 3], to: [f32; 3], radius: f32 },
    Line { from: [f32; 3], to: [f32; 3] },
}

impl WorldCollision {
    /// Copied from descriptor byte2, rather than a life duration field.
    // Translated from 0x010CD1C0 (DDDA.exe 2017-12-13 Steam).
    pub fn initial_delay(&self) -> u8 { (self.flags >> 16) as u8 }

    /// Every initially nonzero delay suppresses this update, including 1->0.
    // Translated from 0x010F54F0 (DDDA.exe 2017-12-13 Steam).
    pub fn query_due(delay: &mut u8) -> bool {
        if *delay != 0 { *delay -= 1; false } else { true }
    }

    /// Caller supplies actual old/current positions and radius after its
    /// growth update. Floor-only queries and collision-provider filters need
    /// their own scene adapters before issuing these world queries.
    // Translated from 0x010E2A00 (DDDA.exe 2017-12-13 Steam).
    pub fn query(&self, from: [f32; 3], to: [f32; 3], scaled_radius: f32)
        -> Result<WorldQuery, String> {
        if from.iter().chain(to.iter()).chain([scaled_radius].iter()).any(|v| !v.is_finite()) {
            return Err("nonfinite particle world query".into());
        }
        if self.flags & 0x4000 != 0 { return Err("floor-only particle query needs native adapter".into()); }
        if self.flags & 0x1000 != 0 && scaled_radius > 0.00001 {
            return Ok(WorldQuery::Sphere { from, to, radius: scaled_radius });
        }
        let mut direction: [f32; 3] = std::array::from_fn(|i| to[i] - from[i]);
        let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        if length >= f32::EPSILON {
            let inverse = 1.0 / length;
            for component in &mut direction { *component *= inverse; }
        }
        // The original retains tiny unnormalized displacements here.
        Ok(WorldQuery::Line {
            from: std::array::from_fn(|i| from[i] - direction[i] * scaled_radius),
            to: std::array::from_fn(|i| to[i] + direction[i] * scaled_radius),
        })
    }
}

/// Supported point-provider binding: actor-relative offset and unchanged
/// local orientation. The caller supplies the real parent coordinate matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProviderBinding {
    pub local_offset: [f32; 3],
    pub effect_scale: f32,
}

impl ProviderBinding {
    /// Normal create-context fields from 0x00A55BC0/0x004241F0. Other
    /// placement modes, floor alignment and randomized scaling need their
    /// own native adapters before using this binding.
    // Translated from 0x00CB6CF0, 0x004298B0 and 0x0042D8F0's EAX
    // offset argument (DDDA.exe 2017-12-13 Steam).
    pub fn read(element: &mteffect::Element, actor_scale: [f32; 3],
        create_flags: u32) -> Result<Self, String> {
        let p = &element.parameters;
        if p[1] != 0 || p[2] != 0 || p[18] != 6 || p[43] & 2 != 0
            || p[31..38].iter().any(|v| *v != 0)
            || p[9..12].iter().any(|v| f32::from_bits(*v) != 0.0)
            || f32::from_bits(p[29]) != 0.0 {
            return Err("unsupported point-provider binding".into());
        }
        let mut local_offset = std::array::from_fn(|axis| f32::from_bits(p[6 + axis]));
        let effect_scale = f32::from_bits(p[28]);
        if actor_scale.iter().chain(local_offset.iter()).chain([effect_scale].iter()).any(|v| !v.is_finite()) {
            return Err("nonfinite point-provider binding".into());
        }
        if p[43] & 4 == 0 && create_flags & 0x20 == 0 {
            for axis in 0..3 { local_offset[axis] *= actor_scale[axis]; }
        }
        Ok(Self { local_offset, effect_scale })
    }

    /// The helper stores the relative offset in effect+0x160; FE0350
    /// projects it through the parent, rather than translating twice.
    // Translated from 0x00FE0350's parent/joint-1 branch and final scale
    // (DDDA.exe 2017-12-13 Steam).
    pub fn matrix(&self, parent: &mtlmt::pose::Mat4) -> Result<mtlmt::pose::Mat4, String> {
        if parent.iter().flatten().chain(self.local_offset.iter())
            .chain([self.effect_scale].iter()).any(|v| !v.is_finite()) {
            return Err("invalid point-provider parent coordinate".into());
        }
        let local = mtlmt::pose::compose([1.0; 3], [0.0, 0.0, 0.0, 1.0], self.local_offset);
        let mut matrix = mtlmt::pose::mul(&local, parent);
        for row in matrix.iter_mut().take(3) {
            for value in row { *value *= self.effect_scale; }
        }
        Ok(matrix)
    }
}

/// Clock in an effect-owned coordinate block, separate from the effect's
/// fractional clock and every generator's random/update counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateClock {
    initial_delay: u16,
    remaining: u16,
    elapsed: u32,
    phase: CoordinatePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinatePhase { Uninitialized, Waiting, Active }

impl CoordinateClock {
    /// `initial_delay` is the already-sampled native short at block+0x11C,
    /// including the owning effect's offset. This does not select its seed.
    pub fn new(initial_delay: u16) -> Self {
        Self { initial_delay, remaining: 0, elapsed: 0, phase: CoordinatePhase::Uninitialized }
    }

    pub fn elapsed(&self) -> u32 { self.elapsed }

    /// One integer coordinate update. The 1->0 delay update remains waiting;
    /// the following update enters active state and counts its first frame.
    /// Even a negative native short is a nonzero wrapping counter here.
    // Translated from 0x010FA010 (DDDA.exe 2017-12-13 Steam).
    pub fn advance(&mut self) -> bool {
        if self.phase == CoordinatePhase::Uninitialized {
            self.elapsed = 0;
            self.remaining = self.initial_delay;
            self.phase = CoordinatePhase::Waiting;
        }
        if self.phase == CoordinatePhase::Waiting {
            if self.remaining != 0 {
                self.remaining = self.remaining.wrapping_sub(1);
                return false;
            }
            self.phase = CoordinatePhase::Active;
        }
        self.elapsed = self.elapsed.wrapping_add(1);
        true
    }
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

/// Authored type4 path parameters, before native random sampling and the
/// owning generator's scale/direction transform.
#[derive(Debug, Clone, PartialEq)]
pub struct PathParameters {
    pub flags: u16,
    pub iterations: u16,
    pub selectors: u16,
    pub taper_points: u8,
    /// Length, length velocity, angular velocity, force, damping, spring.
    pub ranges: [[f32; 2]; 6],
    pub rotation: [[f32; 2]; 3],
    pub curves: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampledPath {
    pub length: f32,
    pub length_velocity: f32,
    pub angular_velocity: f32,
    pub force: f32,
    pub damping: f32,
    pub spring: f32,
    pub rotation: [f32; 3],
}

impl PathParameters {
    /// Supported uncurved, unconstrained native setup. Each draw advances
    /// the caller's actual selected counter even when its variation is zero.
    /// Ordinary life4 setup uses generator +48, separately from the +44
    /// counter used by initializer and particle birth. Native path-state
    /// mode0x1000 selects +44 instead; resource flags alone do not select it.
    // Translated from 0x010E5620 (DDDA.exe 2017-12-13 Steam).
    pub fn sample(&self, counter: &mut u32,
        mut random_float: impl FnMut(usize) -> f32) -> Result<SampledPath, String> {
        if self.flags != 0 || self.curves != [0; 4] {
            return Err("unsupported curved or constrained collision path setup".into());
        }
        if self.ranges.iter().chain(self.rotation.iter()).flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite collision path parameters".into());
        }
        let mut sample = |range: [f32; 2]| -> Result<f32, String> {
            *counter = counter.wrapping_add(1);
            let random = random_float((*counter & 0xFFF) as usize);
            let value = random * range[1] + range[0];
            if !random.is_finite() || !value.is_finite() { return Err("invalid native path random sample".into()); }
            Ok(value)
        };
        let length = sample(self.ranges[0])?.max(0.0);
        let length_velocity = sample(self.ranges[1])?;
        // Native order is force/damping/spring before angular velocity,
        // despite the authored angular pair preceding them in the resource.
        let force = sample(self.ranges[3])?;
        let damping = sample(self.ranges[4])?;
        let spring = sample(self.ranges[5])?;
        let angular_velocity = sample(self.ranges[2])?;
        let rotation = [sample(self.rotation[0])?, sample(self.rotation[1])?, sample(self.rotation[2])?];
        Ok(SampledPath { length, length_velocity, angular_velocity, force, damping, spring, rotation })
    }
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
    fn generator_seed_selection_preserves_global_draw_order_and_separate_reset_counters() {
        let mut fixed = GeneratorCounters::select(&[0x1234_ABCD], 8,
            |_| panic!("single fixed entry must not consume global RNG")).unwrap();
        assert_eq!((fixed.birth, fixed.path), (0xABCD, 0xABCD),
            "fixed seed is truncated to16 bits, not masked to12");
        fixed.birth += 4;
        fixed.path += 9;
        assert_ne!(fixed.birth, fixed.path);
        fixed.reset();
        assert_eq!((fixed.birth, fixed.path), (0xABCD, 0xABCD));

        let mut calls = Vec::new();
        let mut values = [1, 0x1234_FABC].into_iter();
        let selected = GeneratorCounters::select(&[17, -1, 19], 0xFFFF,
            |selector| { calls.push(selector); values.next().unwrap() }).unwrap();
        assert_eq!(calls, [8, 8], "entry selection precedes the negative-seed draw");
        assert_eq!((selected.birth, selected.path), (0xABC, 0xABC));
        assert_eq!(values.next(), None);
        let mut calls = Vec::new();
        let selected = GeneratorCounters::select(&[17, 18, 19], 0x10,
            |selector| { calls.push(selector); 5 }).unwrap();
        assert_eq!(calls, [0]);
        assert_eq!((selected.birth, selected.path), (19, 19), "selection uses modulo count");
        let random = GeneratorCounters::select(&[-17], 8, |selector| {
            assert_eq!(selector, 8); 0xFFFF_FFFF
        }).unwrap();
        assert_eq!((random.birth, random.path), (0xFFF, 0xFFF));
        assert!(GeneratorCounters::select(&[], 0, |_| panic!("invalid list must not draw")).is_err());
        assert!(GeneratorCounters::select(&vec![0; 256], 0,
            |_| panic!("invalid list must not draw")).is_err());
    }

    #[test]
    fn generator_binding_keeps_coordinate_anchor_and_distinct_scale_modes() {
        let primary = [[0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0], [11.0, 22.0, 33.0, 1.0]];
        let coordinate = CoordinateTransform { primary,
            secondary: mtlmt::pose::IDENTITY, scale: [-2.0, 5.0, -4.0], largest_scale: 5.0 };
        let inherited = coordinate.generator_transform([2.0, 3.0, 4.0], 0.5, 1.25, 0, false).unwrap();
        assert_eq!(inherited.particle_scale, 3.125);
        assert_eq!(inherited.axis_scale, [-2.5, 9.375, -10.0]);
        assert_eq!(inherited.matrix, [[0.0, 0.0, 2.5, 0.0], [0.0, 9.375, 0.0, 0.0],
            [-10.0, 0.0, 0.0, 0.0], [11.0, 22.0, 33.0, 1.0]]);
        // A nonzero native coordinate scale mode bypasses coordinate scales,
        // but does not itself discard coordinate orientation or translation.
        let separate = coordinate.generator_transform([2.0, 3.0, 4.0], 0.5, 1.25, 2, false).unwrap();
        assert_eq!(separate.particle_scale, 0.625);
        assert_eq!(separate.axis_scale, [1.25, 1.875, 2.5]);
        assert_eq!(separate.matrix[0], [0.0, 0.0, -1.25, 0.0]);
        assert_eq!(separate.matrix[3], primary[3]);
        let world = coordinate.generator_transform([2.0, 3.0, 4.0], 0.5, 1.25, 2, true).unwrap();
        assert_eq!(world.matrix, [[1.25, 0.0, 0.0, 0.0], [0.0, 1.875, 0.0, 0.0],
            [0.0, 0.0, 2.5, 0.0], [11.0, 22.0, 33.0, 1.0]]);
        assert!(coordinate.generator_transform([1.0; 3], 1.0, 1.0, 16, false).is_err());
        assert!(coordinate.generator_transform([1.0; 3], f32::NAN, 1.0, 0, false).is_err());
        assert!(coordinate.generator_transform([f32::MAX; 3], 2.0, 2.0, 0, false).is_err());
    }

    #[test]
    fn static_parent_binding_separates_axis_scale_offset_and_both_matrix_blocks() {
        let mut definition = CoordinateDefinition {
            flags: 0, binding_options: 0, binding_bits: 0, scale: [[1.0, 0.0]; 3],
            local_offset: [11.0, 22.0, 33.0], offset_tail: u32::MAX,
            local_quaternion: [0.0, 0.0, 0.0, 1.0], seed: 0,
            delay: DurationRange { base: 0, variation: 0 }, curve_offsets: [0; 4],
        };
        let basis = definition.static_basis([0.5, 2.0, -0.25], [2.0, 3.0, 4.0], false).unwrap();
        let parent = [[2.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0], [100.0, 200.0, 300.0, 1.0]];
        let unscaled = definition.static_parent(basis, &parent, false).unwrap();
        assert_eq!(unscaled.primary[3], [111.0, 222.0, 333.0, 1.0]);
        assert_eq!(unscaled.scale, [2.0, 18.0, -4.0]);
        assert_eq!(unscaled.largest_scale, 18.0);
        assert_eq!(unscaled.primary, unscaled.secondary);
        assert_eq!(unscaled.primary[0], [1.0, 0.0, 0.0, 0.0]);
        definition.binding_options = 0x20;
        let scaled = definition.static_parent(basis, &parent, false).unwrap();
        assert_eq!(scaled.primary[3], [122.0, 596.0, 168.0, 1.0]);
        let rotated = [[0.0, 0.0, -2.0, 0.0], [0.0, 3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0, 0.0], [100.0, 200.0, 300.0, 1.0]];
        let turned = definition.static_parent(basis, &rotated, false).unwrap();
        assert_eq!(turned.primary[3], [-32.0, 596.0, 278.0, 1.0]);
        assert_eq!(turned.primary[0], [0.0, 0.0, -1.0, 0.0]);
        let mut zero_axis = parent;
        zero_axis[0] = [0.0; 4];
        let zero = definition.static_parent(basis, &zero_axis, false).unwrap();
        assert_eq!(zero.primary[0], [0.0; 4]);
        assert_eq!(zero.scale[0], 0.0);
        assert_eq!(zero.primary[3][0], 100.0);
        assert!(definition.static_parent(basis, &parent, true).is_err());
        definition.binding_options |= 0x10;
        assert!(definition.static_parent(basis, &parent, false).is_err());
        definition.binding_options = 0;
        definition.binding_bits = 1;
        assert!(definition.static_parent(basis, &parent, false).is_err());
        definition.binding_bits = 0;
        definition.flags = 1 << 12;
        assert!(definition.static_parent(basis, &parent, false).is_err());
    }

    #[test]
    fn static_coordinate_basis_preserves_raw_rotation_and_separate_scale() {
        let mut definition = CoordinateDefinition {
            flags: 0, binding_options: 0, binding_bits: 0, scale: [[1.0, 0.0]; 3],
            local_offset: [11.0, 22.0, 33.0], offset_tail: u32::MAX,
            // Deliberately nonunit: normalizing would change this rotation.
            local_quaternion: [0.5, 0.0, 0.0, 0.5], seed: 0,
            delay: DurationRange { base: 0, variation: 0 }, curve_offsets: [0; 4],
        };
        assert_eq!(definition.parent_joint(), -1);
        let basis = definition.static_basis([0.5, 2.0, -0.25], [2.0, -3.0, 4.0], false).unwrap();
        assert_eq!(basis.rotation, [[1.0, 0.0, 0.0, 0.0], [0.0, 0.5, 0.5, 0.0],
            [0.0, -0.5, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(basis.offset, [11.0, 22.0, 33.0]);
        assert_eq!(basis.scale, [1.0, -6.0, -1.0]);
        assert_eq!(basis.largest_scale, 1.0, "native maximum is not an absolute radius");
        assert!(definition.static_basis([1.0; 3], [1.0; 3], true).is_err());
        definition.curve_offsets[0] = 1;
        assert!(definition.static_basis([1.0; 3], [1.0; 3], false).is_err());
        definition.curve_offsets[0] = 0;
        assert!(definition.static_basis([f32::MAX; 3], [2.0; 3], false).is_err());
    }

    #[test]
    fn coordinate_resource_resolves_relative_table_and_preserves_native_fields() {
        let mut list = synthetic_list();
        list.auxiliary_count = 1;
        let resource = 0x280;
        list.payload.resize(resource + 0x70, 0);
        let put = |bytes: &mut [u8], at: usize, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(&mut list.payload, 16, resource as u32);
        put(&mut list.payload, resource, 0x1234_5600);
        list.payload[resource + 4..resource + 6].copy_from_slice(&0x28u16.to_le_bytes());
        list.payload[resource + 6..resource + 8].copy_from_slice(&0xCAFEu16.to_le_bytes());
        for (i, value) in [1.0f32, 0.1, 2.0, 0.2, 3.0, 0.3].into_iter().enumerate() {
            put(&mut list.payload, resource + 8 + i * 4, value.to_bits());
        }
        for (i, value) in [11.0f32, 22.0, 33.0, 0.0, 0.1, 0.2, 0.3, 0.9].into_iter().enumerate() {
            put(&mut list.payload, resource + 0x20 + i * 4, value.to_bits());
        }
        put(&mut list.payload, resource + 0x60, (-17i32) as u32);
        put(&mut list.payload, resource + 0x64, (7 << 16) | 0xFFFD);
        for (i, value) in [0x70u16, 0x90, 0xB0, 0xD0].into_iter().enumerate() {
            list.payload[resource + 0x68 + i * 2..resource + 0x6A + i * 2].copy_from_slice(&value.to_le_bytes());
        }
        let definition = CoordinateDefinition::read(&list, 0).unwrap();
        assert_eq!(definition.flags, 0x1234_5600);
        assert_eq!(definition.binding_options, 0x28);
        assert_eq!(definition.binding_bits, 0xCAFE);
        assert_eq!(definition.scale, [[1.0, 0.1], [2.0, 0.2], [3.0, 0.3]]);
        assert_eq!(definition.local_offset, [11.0, 22.0, 33.0]);
        assert_eq!(definition.offset_tail, 0);
        assert_eq!(definition.local_quaternion, [0.1, 0.2, 0.3, 0.9]);
        assert_eq!(definition.seed, -17);
        assert_eq!(definition.delay, DurationRange { base: -3, variation: 7 });
        assert_eq!(definition.curve_offsets, [0x70, 0x90, 0xB0, 0xD0]);
        let mut counter = 17;
        assert!(definition.sample_static(&mut counter, 0, |_| panic!("no dynamic sampling"), |_| panic!("no dynamic sampling")).is_err());
        assert_eq!(counter, 17);
        let mut definition = definition;
        definition.curve_offsets = [0; 4];
        definition.scale = [[1.0, 0.0], [2.0, 0.5], [3.0, -0.25]];
        definition.delay = DurationRange { base: -3, variation: 7 };
        assert_eq!(definition.initial_counter(|| 0xABCD_0FFE), 0xFFE);
        definition.seed = 0x1234_ABCD;
        assert_eq!(definition.initial_counter(|| panic!("fixed seed must not advance global RNG")), 0xABCD);
        let mut floats = Vec::new();
        let mut words = Vec::new();
        counter = 0xFFE;
        let sample = definition.sample_static(&mut counter, -1,
            |index| { floats.push(index); 0.5 }, |index| { words.push(index); 15 }).unwrap();
        assert_eq!(sample, ([1.0, 2.25, 2.875], 3));
        assert_eq!(floats, [0xFFF, 0, 1], "zero-variation scale still consumes its float");
        assert_eq!(words, [2]);
        assert_eq!(counter, 0x1002);
        definition.delay.variation = 0;
        counter = u32::MAX;
        let sample = definition.sample_static(&mut counter, 0,
            |_| 0.0, |_| panic!("zero delay variation must not read word table")).unwrap();
        assert_eq!(sample.1, (-3i16) as u16);
        assert_eq!(counter, 3);
        // Separate MULSS/ADDSS rounds this product to1 before adding-1.
        // Fusing the operation would retain a small negative residual.
        definition.scale = [[-1.0, 1.0 - f32::EPSILON]; 3];
        let sample = definition.sample_static(&mut counter, 0,
            |_| 1.0 + f32::EPSILON, |_| panic!("no word-table access")).unwrap();
        assert_eq!(sample.0, [0.0; 3]);
        assert_ne!((1.0 + f32::EPSILON).mul_add(1.0 - f32::EPSILON, -1.0), 0.0);
        assert!(CoordinateDefinition::read(&list, 1).is_err());
        put(&mut list.payload, resource + 0x2C, u32::MAX);
        assert_eq!(CoordinateDefinition::read(&list, 0).unwrap().offset_tail, u32::MAX,
            "opaque unused offset word must not be rejected as a NaN geometric coordinate");
        put(&mut list.payload, resource + 0x20, f32::NAN.to_bits());
        assert!(CoordinateDefinition::read(&list, 0).is_err());
        list.payload.truncate(19);
        assert!(CoordinateDefinition::read(&list, 0).is_err());
    }

    #[test]
    fn installed_breath_coordinate_is_distinct_from_generator_payload() {
        let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !path.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(&path).unwrap();
        let unit = CollisionUnit::load(&vfs, 0).unwrap();
        let bytes = vfs.read(&unit.path, mtarc::type_hash("rEffectList")).unwrap();
        let list = mteffect::EffectList::parse(&bytes).unwrap();
        assert_eq!(list.auxiliary_count, 1);
        let coordinate_offset = u32::from_le_bytes(list.payload[16..20].try_into().unwrap());
        assert_ne!(coordinate_offset, list.units[0].offsets[0], "coordinate table is not generator data");
        let definition = CoordinateDefinition::read(&list, 0).unwrap();
        assert!(definition.scale.iter().flatten().all(|v| v.is_finite()));
        assert!(CoordinateDefinition::read(&list, 1).is_err());
    }

    #[test]
    fn coordinate_delay_does_not_admit_the_last_wait_update() {
        let mut delayed = CoordinateClock::new(2);
        assert!(!delayed.advance());
        assert_eq!((delayed.remaining, delayed.elapsed()), (1, 0));
        assert!(!delayed.advance());
        assert_eq!((delayed.remaining, delayed.elapsed()), (0, 0));
        assert!(delayed.advance());
        assert_eq!(delayed.elapsed(), 1);
        assert!(delayed.advance());
        assert_eq!(delayed.elapsed(), 2);

        let mut immediate = CoordinateClock::new(0);
        assert!(immediate.advance());
        assert_eq!(immediate.elapsed(), 1);
        immediate.elapsed = u32::MAX;
        assert!(immediate.advance());
        assert_eq!(immediate.elapsed(), 0, "native integer clock wraps");

        let mut negative_short = CoordinateClock::new((-1i16) as u16);
        assert!(!negative_short.advance());
        assert_eq!(negative_short.remaining, u16::MAX - 1);
        assert_eq!(negative_short.elapsed(), 0);
    }

    #[test]
    fn world_query_delay_and_geometry_follow_descriptor_not_secondary_delay() {
        let mut list = synthetic_list();
        list.payload.resize(0x260, 0);
        let life = list.units[0].offsets[3] as usize;
        list.payload[life + 6..life + 8].copy_from_slice(&0x110u16.to_le_bytes());
        list.payload[life + 0x44..life + 0x48].copy_from_slice(&20u32.to_le_bytes());
        let at = life + 0x110;
        list.payload[at..at + 4].copy_from_slice(&0x0005_0200u32.to_le_bytes());
        list.payload[at + 8..at + 12].copy_from_slice(&3.0f32.to_le_bytes());
        let unit = CollisionUnit::read("synthetic".into(), &list).unwrap();
        let descriptor = unit.world_collision.unwrap();
        assert_eq!(descriptor.radius, 3.0);
        let mut delay = descriptor.initial_delay();
        assert_eq!(delay, 5);
        for _ in 0..5 { assert!(!WorldCollision::query_due(&mut delay)); }
        assert_eq!(delay, 0);
        assert!(WorldCollision::query_due(&mut delay));
        assert_eq!(descriptor.query([0.0; 3], [0.0, 0.0, 10.0], 3.0).unwrap(),
            WorldQuery::Line { from: [0.0, 0.0, -3.0], to: [0.0, 0.0, 13.0] });
        let sphere = WorldCollision { flags: descriptor.flags | 0x1000, ..descriptor };
        assert_eq!(sphere.query([0.0; 3], [0.0, 0.0, 10.0], 3.0).unwrap(),
            WorldQuery::Sphere { from: [0.0; 3], to: [0.0, 0.0, 10.0], radius: 3.0 });
        assert!(matches!(sphere.query([0.0; 3], [0.0, 0.0, 10.0], 0.00001).unwrap(), WorldQuery::Line { .. }));
        let floor = WorldCollision { flags: descriptor.flags | 0x4000, ..descriptor };
        assert!(floor.query([0.0; 3], [0.0; 3], 3.0).is_err());
        list.payload.truncate(at + 15);
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err());
    }

    #[test]
    fn path_setup_samples_native_order_and_advances_zero_variation_draws() {
        let mut parameters = PathParameters { flags: 0, iterations: 2,
            selectors: 0x4444, taper_points: 0,
            ranges: [[-2.0, 0.0], [10.0, 1.0], [20.0, 1.0],
                [30.0, 1.0], [40.0, 1.0], [50.0, 1.0]],
            rotation: [[60.0, 1.0], [70.0, 1.0], [80.0, 1.0]], curves: [0; 4] };
        let mut counter = 0xFFFF_FFFE;
        let mut indices = Vec::new();
        let sampled = parameters.sample(&mut counter, |index| {
            indices.push(index); indices.len() as f32
        }).unwrap();
        assert_eq!(indices, [0xFFF, 0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(counter, 7);
        assert_eq!(sampled, SampledPath { length: 0.0, length_velocity: 12.0,
            angular_velocity: 26.0, force: 33.0, damping: 44.0, spring: 55.0,
            rotation: [67.0, 78.0, 89.0] });
        parameters.curves[3] = 1;
        assert!(parameters.sample(&mut counter, |_| panic!("unsupported curves cannot draw")).is_err());
        assert_eq!(counter, 7);
        parameters.curves[3] = 0;
        parameters.flags = 0x10;
        assert!(parameters.sample(&mut counter, |_| panic!("constrained setup cannot draw")).is_err());
    }

    #[test]
    fn provider_binding_keeps_relative_offset_and_parent_scale_without_double_translation() {
        let mut element = mteffect::Element { lists: std::array::from_fn(|_| String::new()),
            effect_2d: String::new(), parameters: [0; 69] };
        element.parameters[18] = 6;
        element.parameters[6] = 3.0_f32.to_bits();
        element.parameters[7] = (-2.0_f32).to_bits();
        element.parameters[28] = 0.5_f32.to_bits();
        let binding = ProviderBinding::read(&element, [2.0, 3.0, 4.0], 0).unwrap();
        assert_eq!(binding.local_offset, [6.0, -6.0, 0.0]);
        // Explicit native parent coordinate, with nonuniform rows. Do not
        // normalize its axes or apply actor position before its translation.
        let parent = [[0.0, 2.0, 0.0, 0.0], [-3.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0], [100.0, 200.0, 300.0, 1.0]];
        let matrix = binding.matrix(&parent).unwrap();
        assert_eq!(matrix[3], [118.0, 212.0, 300.0, 1.0]);
        assert_eq!(matrix[0], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(matrix[1], [-1.5, 0.0, 0.0, 0.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 2.0, 0.0]);
        assert_eq!(ProviderBinding::read(&element, [2.0; 3], 0x20).unwrap().local_offset, [3.0, -2.0, 0.0]);
        element.parameters[43] = 4;
        assert_eq!(ProviderBinding::read(&element, [2.0; 3], 0).unwrap().local_offset, [3.0, -2.0, 0.0]);
        element.parameters[43] = 2;
        assert!(ProviderBinding::read(&element, [1.0; 3], 0).is_err());
        element.parameters[43] = 0;
        element.parameters[29] = 1.0_f32.to_bits();
        assert!(ProviderBinding::read(&element, [1.0; 3], 0).is_err());
    }

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
        let mut payload = vec![0; 0x240];
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
    fn generator_initialization_reads_real_ranges_and_consumes_four_birth_draws() {
        let mut list = synthetic_list();
        for (offset, pair) in [(0x40, [2.0_f32, 4.0]), (0x88, [3.0, 0.0]),
            (0x90, [-2.0, 8.0]), (0x98, [7.0, -4.0])] {
            for (part, value) in pair.into_iter().enumerate() {
                let at = 0x10 + offset + part * 4;
                list.payload[at..at + 4].copy_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        let unit = CollisionUnit::read("synthetic".into(), &list).unwrap();
        let mut counters = unit.generator_counters(0, |_| panic!("fixed seed")).unwrap();
        counters.birth = u32::MAX - 1;
        counters.path = 91;
        let mut indices = Vec::new();
        let mut values = [0.25, 0.75, 0.5, 1.0].into_iter();
        let initialized = unit.sample_generator_initialization(&mut counters, |index| {
            indices.push(index);
            values.next().unwrap()
        }).unwrap();
        assert_eq!(indices, [0xFFF, 0, 1, 2],
            "zero variation still consumes its draw, with native u32 wrap");
        assert_eq!((counters.birth, counters.path), (2, 91));
        assert_eq!(initialized, GeneratorInitialization {
            particle_scale: 3.0, auxiliary_triplet: [3.0, 2.0, 3.0],
        });
        assert_eq!(values.next(), None);
        // Creation E6E130 runs vf07 then vf08: D7210 -> FA570 restores
        // both counters after sampling, before allocation/path/birth.
        counters.reset();
        assert_eq!((counters.birth, counters.path), (0, 0));
        assert_eq!(initialized.particle_scale, 3.0,
            "counter reset does not discard the sampled runtime scalar");
        // The actual sampled +134 feeds vf15; auxiliary values do not
        // accidentally replace effect axis scales or coordinate scales.
        let coordinate = CoordinateTransform { primary: mtlmt::pose::IDENTITY,
            secondary: mtlmt::pose::IDENTITY, scale: [1.0; 3], largest_scale: 1.0 };
        let bound = coordinate.generator_transform([2.0, 3.0, 4.0], 0.5,
            initialized.particle_scale, 1, false).unwrap();
        assert_eq!(bound.particle_scale, 1.5);
        assert_eq!(bound.axis_scale, [3.0, 4.5, 6.0]);
        list.payload[0x10 + 0x90..0x10 + 0x94]
            .copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
        assert!(CollisionUnit::read("invalid".into(), &list).is_err());
    }

    #[test]
    fn life4_birth_consumes_six_float_samples_then_one_word_without_touching_path() {
        let mut unit = CollisionUnit::read("synthetic".into(), &synthetic_list()).unwrap();
        unit.speed = [-2.0, 6.0];
        unit.acceleration = [1.0, -4.0];
        unit.initial_distance = [10.0, 100.0];
        unit.birth_delay = DurationRange { base: -2, variation: 5 };
        let mut counters = unit.generator_counters(0, |_| panic!("fixed seed")).unwrap();
        counters.birth = 0xFFE;
        counters.path = 99;
        let mut floats = Vec::new();
        let mut words = Vec::new();
        let mut values = [0.25, 0.5, 0.75, 0.2, 0.3, 0.4].into_iter();
        let birth = unit.sample_life4_birth(&mut counters, |index| {
            floats.push(index);
            values.next().unwrap()
        }, |index| { words.push(index); 9 }).unwrap();
        assert_eq!(floats, [0xFFF, 0, 1, 2, 3, 4]);
        assert_eq!(words, [5]);
        assert_eq!(values.next(), None);
        assert_eq!(birth, Life4Birth { speed: -0.5, acceleration: -1.0,
            distance: 85.0, secondary_effect_delay: 1 });
        assert_eq!((counters.birth, counters.path), (0x1005, 99));
        unit.birth_delay.variation = 0;
        words.clear();
        let birth = unit.sample_life4_birth(&mut counters, |_| 0.0,
            |index| { words.push(index); u32::MAX }).unwrap();
        assert_eq!(words, [12], "native loads the delay word even with no variation");
        assert_eq!(birth.secondary_effect_delay, -2);
        assert_eq!((counters.birth, counters.path), (0x100C, 99));
        let before = counters.clone();
        unit.birth_curves[0] = 1;
        assert!(unit.sample_life4_birth(&mut counters,
            |_| panic!("unsupported curve cannot draw"), |_| panic!("unsupported curve")).is_err());
        assert_eq!(counters, before);
        unit.birth_curves[0] = 0;
        unit.birth_angles[0][0] = 1.0;
        assert!(unit.sample_life4_birth(&mut counters,
            |_| panic!("unsupported angular birth cannot draw"), |_| panic!("unsupported angular birth")).is_err());
        assert_eq!(counters, before);
    }

    #[test]
    fn life4_travel_uses_native_buffers_old_speed_and_path_end_without_delaying_motion() {
        let path = ParticlePath { points: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 10.0],
            [0.0, 0.0, 30.0]], velocities: vec![[0.0; 3]; 3], total_length: 30.0 };
        // These four synthetic fixtures also ran through whole original
        // 0x010F7190 -> 0x010E2EB0, with no substituted callback or output.
        for (previous, current, distance, speed, next_distance, next_speed, z, at_end) in [
            (0, 1, 10.0, 5.0, 15.0, 4.0, 315.0, false),
            (0, 1, 10.0, -5.0, 5.0, -6.0, 305.0, false),
            (0, 1, 29.0, 5.0, 34.0, 4.0, 330.0, true),
            (1, 0, 10.0, 5.0, 15.0, 4.0, 315.0, false),
        ] {
            let birth = Life4Birth { speed, acceleration: -1.0, distance,
                secondary_effect_delay: 1 };
            let mut travel = Life4Travel::from_birth(birth);
            assert_eq!(travel.distance, [distance; 2], "native birth initializes both buffers");
            travel.distance[current] = 999.0;
            let flags = ((previous as u32) << 25) | ((current as u32) << 24);
            let sample = travel.advance_path(flags, &path, [100.0, 200.0, 300.0]).unwrap();
            assert_eq!(travel.distance[previous], distance);
            assert_eq!(travel.distance[current], next_distance);
            assert_eq!(travel.speed, next_speed);
            assert_eq!(sample, PathSample { position: [100.0, 200.0, z], at_end });
            assert_eq!(travel.secondary_effect_delay, 1,
                "query/end/secondary policy runs after this travel phase");
        }
        let mut travel = Life4Travel::from_birth(Life4Birth { speed: 5.0,
            acceleration: -1.0, distance: 10.0, secondary_effect_delay: i16::MAX });
        travel.advance_path(1 << 24, &path, [100.0, 200.0, 300.0]).unwrap();
        let next = travel.advance_path(1 << 25, &path, [100.0, 200.0, 300.0]).unwrap();
        assert_eq!((travel.distance, travel.speed), ([19.0, 15.0], 3.0));
        assert_eq!(next.position, [100.0, 200.0, 319.0], "secondary delay does not postpone movement");
        let before = travel;
        assert!(travel.advance_path(0, &path, [f32::NAN, 0.0, 0.0]).is_err());
        assert_eq!(travel, before, "project input rejection leaves the phase unchanged");
        let invalid = ParticlePath { points: Vec::new(), velocities: Vec::new(), total_length: 0.0 };
        assert!(travel.advance_path(0, &invalid, [0.0; 3]).is_err());
        assert_eq!(travel, before);
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

    #[test]
    fn collision_generator_seeds_are_bounded_payload_reads_including_native_zero_count() {
        let mut list = synthetic_list();
        let base = list.units[0].offsets[0] as usize;
        list.payload[base + 0x10..base + 0x14].copy_from_slice(&0x1234_ABCDu32.to_le_bytes());
        let unit = CollisionUnit::read("synthetic".into(), &list).unwrap();
        assert_eq!(unit.generator_seeds, [0x1234_ABCD]);
        assert_eq!(unit.generator_counters(0, |_| panic!("zero count still uses its first fixed entry"))
            .unwrap().birth, 0xABCD);
        list.payload[base + 0xF] = 2;
        list.payload[base + 0x14..base + 0x18].copy_from_slice(&(-1i32).to_le_bytes());
        let unit = CollisionUnit::read("synthetic".into(), &list).unwrap();
        assert_eq!(unit.generator_seeds, [0x1234_ABCD, -1]);
        let mut draws = [1, 0xABC].into_iter();
        assert_eq!(unit.generator_counters(8, |_| draws.next().unwrap()).unwrap().path, 0xABC);
        assert_eq!(draws.next(), None);
        list.payload[base + 0xF] = 255;
        assert!(CollisionUnit::read("synthetic".into(), &list).is_err(),
            "corrupt count cannot read beyond the installed payload");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollisionUnit {
    pub path: String,
    pub generator: GeneratorParams,
    /// Generator payload+0xF count and +0x10 signed entries. Native count
    /// zero still reads the first entry; no game seed table is embedded.
    pub generator_seeds: Vec<i32>,
    /// Generator +40/+44, then +88/+8C, +90/+94, +98/+9C.
    /// vf07 samples these four ranges through the birth counter.
    pub generator_initialization_ranges: [[f32; 2]; 4],
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
    /// Life4+10/+18/+20 angular ranges and +38/+3A/+42 curves.
    /// Nonzero angular/curve birth remains unsupported by the sampler.
    pub birth_angles: [[f32; 2]; 3],
    pub birth_curves: [u16; 3],
    /// Life4+44/+46; used by secondary effect dispatch, not motion admission.
    pub birth_delay: DurationRange,
    pub path_points: u8,
    pub path_parameters: PathParameters,
    pub world_collision: Option<WorldCollision>,
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
        // cParticleGenerator::vf14 0x010D7380 resolves runtime+0x128
        // from the ushort at life+6, not life+0x44 or the nested path data.
        let collision_offset = half(life, 6)?;
        let world_collision = if collision_offset == 0 { None } else {
            let base = life.checked_add(usize::from(collision_offset))
                .ok_or("collision descriptor offset overflow")?;
            Some(WorldCollision { flags: word(base, 0)?, radius_growth: float(base, 4)?,
                radius: float(base, 8)?, radius_limit_extra: float(base, 0xC)? })
        };
        // 0x010FA330 binds runtime+2C to this generator payload;
        // 0x010FA600 selects its count+F and entries+10. Count0/1 both
        // read entry0, rather than treating count0 as an absent seed.
        let seed_count_at = generator.checked_add(0xF).ok_or("generator seed offset overflow")?;
        let seed_count = usize::from(*bytes.get(seed_count_at).ok_or("truncated generator seed count")?).max(1);
        let generator_seeds = (0..seed_count).map(|index|
            word(generator, 0x10 + index * 4).map(|value| value as i32))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { path, generator: GeneratorParams { flags: word(generator, 8)?,
                restart_delay: half(generator, 0xAC)? as i16, restart_variation: half(generator, 0xAE)? },
            generator_seeds,
            generator_initialization_ranges: [pair(generator, 0x40)?,
                pair(generator, 0x88)?, pair(generator, 0x90)?, pair(generator, 0x98)?],
            capacity: half(generator, 0xC)?, generator_axis_selectors: word(generator, 0x30)? as u8,
            emission_shape: word(generator, 0x78)? as u8,
            radius: [float(particle, 0x40)?, float(particle, 0x44)?, float(particle, 0x48)?],
            fade: [a?, b?, c?], hold_curve: (curve_word >> 1) & 0x7FFF,
            hold_limit: half(movement, 0xE)?, speed: pair(life, 0x28)?,
            acceleration: pair(life, 0x48)?, initial_distance: pair(life, 0x70)?, path_points,
            birth_angles: [pair(life, 0x10)?, pair(life, 0x18)?, pair(life, 0x20)?],
            birth_curves: [half(life, 0x38)?, half(life, 0x3A)?, half(life, 0x42)?],
            birth_delay: DurationRange { base: half(life, 0x44)? as i16,
                variation: half(life, 0x46)? },
            // Type4's constructor0x010F1230 passes life+0x80 to0x010E5620.
            path_parameters: PathParameters { flags: half(life, 0x80)?,
                iterations: half(life, 0x82)?, selectors: half(life, 0x84)?,
                taper_points: half(life, 0x86)? as u8,
                ranges: [pair(life, 0x88)?, pair(life, 0x90)?, pair(life, 0x98)?,
                    pair(life, 0xA0)?, pair(life, 0xA8)?, pair(life, 0xB0)?],
                rotation: [pair(life, 0xB8)?, pair(life, 0xC0)?, pair(life, 0xC8)?],
                curves: [u32::from(half(life, 0x104)?), u32::from(half(life, 0x106)?),
                    word(life, 0x108)?, word(life, 0x10C)?] }, world_collision })
    }

    pub fn generator_counters(&self, effect_flags: u32,
        global_random: impl FnMut(u32) -> u32) -> Result<GeneratorCounters, String> {
        GeneratorCounters::select(&self.generator_seeds, effect_flags, global_random)
    }

    /// Run after native seed selection and before the first vf15/path/birth.
    /// Creation 0x00E6E130 then calls vf08 (0x010D7210 -> 0x010FA570),
    /// resetting both counters before vf14. Do not omit that separate stage.
    /// The caller reads float table 0x01625420 from the installed exe.
    // Translated from 0x010EC54C..0x010EC5FA
    // (DDDA.exe 2017-12-13 Steam).
    pub fn sample_generator_initialization(&self, counters: &mut GeneratorCounters,
        mut random_float: impl FnMut(usize) -> f32) -> Result<GeneratorInitialization, String> {
        if self.generator_initialization_ranges.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite generator initialization range".into());
        }
        let samples = self.generator_initialization_ranges.map(|[base, variation]| {
            counters.birth = counters.birth.wrapping_add(1);
            let random = random_float((counters.birth & 0xFFF) as usize);
            random * variation + base
        });
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite generator initialization sample".into());
        }
        Ok(GeneratorInitialization { particle_scale: samples[0],
            auxiliary_triplet: [samples[1], samples[2], samples[3]] })
    }

    /// Ordinary generator-owned birth; the alternate particle-owned u16
    /// counter branch is not represented. Zero angular ranges still read
    /// their three float-table entries before the delay word.
    // Translated from 0x010EDC90 and 0x010EE1EC..0x010EE28E
    // (DDDA.exe 2017-12-13 Steam).
    pub fn sample_life4_birth(&self, counters: &mut GeneratorCounters,
        mut random_float: impl FnMut(usize) -> f32,
        mut random_word: impl FnMut(usize) -> u32) -> Result<Life4Birth, String> {
        if self.birth_curves != [0; 3]
            || self.birth_angles.iter().flatten().any(|v| *v != 0.0) {
            return Err("unsupported curved or angular life4 birth".into());
        }
        let ranges = [self.speed, self.acceleration, self.initial_distance,
            self.birth_angles[0], self.birth_angles[1], self.birth_angles[2]];
        if ranges.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite life4 birth range".into());
        }
        let samples = ranges.map(|[base, variation]| {
            counters.birth = counters.birth.wrapping_add(1);
            random_float((counters.birth & 0xFFF) as usize) * variation + base
        });
        counters.birth = counters.birth.wrapping_add(1);
        let delay_word = random_word((counters.birth & 0xFFF) as usize);
        let extra = if self.birth_delay.variation == 0 { 0 } else {
            delay_word % (u32::from(self.birth_delay.variation) + 1)
        };
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite life4 birth sample".into());
        }
        Ok(Life4Birth { speed: samples[0], acceleration: samples[1], distance: samples[2],
            secondary_effect_delay: self.birth_delay.base.wrapping_add(extra as i16) })
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
    #[test]
    fn path_binding_separates_scaled_direction_force_spacing_and_relative_anchor() {
        let generator = GeneratorTransform {
            matrix: [[0.0, 0.0, 2.5, 0.0], [0.0, 9.375, 0.0, 0.0],
                [-10.0, 0.0, 0.0, 0.0], [11.0, 22.0, 33.0, 1.0]],
            axis_scale: [-2.5, 9.375, -10.0], particle_scale: 3.125,
        };
        let parameters = PathParameters { flags: 0, iterations: 1, selectors: 0x4444,
            taper_points: 3, ranges: [[0.0; 2]; 6], rotation: [[0.0; 2]; 3], curves: [0; 4] };
        let sampled = SampledPath { length: 12.0, length_velocity: 0.0,
            angular_velocity: 0.0, force: 2.0, damping: 0.0, spring: 0.0, rotation: [0.0; 3] };
        let bound = generator.bind_path(4, &parameters, sampled).unwrap();
        assert_eq!(bound.dynamics.segment_length, 12.5);
        assert_eq!(bound.dynamics.force, [-20.0, 0.0, 0.0],
            "native force uses scaled direction before the constructor normalizes spacing");
        assert_eq!(bound.path.points,
            vec![[0.0; 3], [-12.5, 0.0, 0.0], [-25.0, 0.0, 0.0], [-37.5, 0.0, 0.0]]);
        assert_eq!(bound.path.velocities[1], [-20.0, 0.0, 0.0]);
        assert_eq!(bound.path.total_length, 37.5);
        assert!(generator.bind_path(1, &parameters, sampled).is_err());
        assert!(generator.bind_path(4, &parameters, SampledPath { angular_velocity: 1.0, ..sampled }).is_err());
        assert!(generator.bind_path(4, &parameters, SampledPath { length_velocity: 1.0, ..sampled }).is_err());
        assert!(generator.bind_path(4, &parameters, SampledPath { rotation: [0.0, 1.0, 0.0], ..sampled }).is_err());
        assert!(generator.bind_path(4, &parameters, SampledPath { force: f32::NAN, ..sampled }).is_err());
    }

