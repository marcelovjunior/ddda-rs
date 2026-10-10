//! Native scalar body updates required by the beach Demo reaction actor.
//! Collision queries and model-derived capsule dimensions remain the body's
//! responsibility. These helpers never infer contact from an elapsed clock.

/// The actual scenery observation written by cScrAdjust. Unknown observations
/// must not be supplied as zero flags: the body keeps its previous bytes when
/// the native update gate is disabled.
#[derive(Debug, Clone, Copy)]
pub(super) struct Scenery {
    pub update_flags: u32,
    pub axis_present: bool,
    pub extended_ground_check: bool,
    pub contact_flags: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct GroundBytes {
    pub ground: u8,
    pub wall: u8,
    pub ceiling: u8,
}

impl GroundBytes {
    // Translated from 0x00885E30 (DDDA.exe 2017-12-13 Steam).
    pub fn observe(&mut self, scenery: Scenery) {
        if scenery.update_flags & 4 == 0 { return; }
        let flags = scenery.contact_flags;
        let mask = if scenery.extended_ground_check { 0x0010_0003 } else { 3 };
        self.ground = u8::from(!(scenery.axis_present && flags & 0x400 != 0)
            && flags & mask != 0);
        self.wall = ((flags >> 2) & 1) as u8;
        self.ceiling = ((flags >> 3) & 1) as u8;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HorizontalVelocity {
    pub speed: f32,
    pub acceleration: f32,
    pub velocity: [f32; 4],
}

impl HorizontalVelocity {
    /// `sin_cos` is the body's yaw resolved by 0x012F0906/0x012F0A55.
    /// `base` is the original runtime vector at 0x018CFE80..88, rather
    /// than an assumed replacement for those uninitialized data words.
    /// Vertical velocity is retained. Projection uses the old speed;
    /// deceleration and its crossing clamp happen only after that write.
    // Translated from 0x00B73490 (DDDA.exe 2017-12-13 Steam).
    pub fn update(&mut self, sin_cos: [f32; 2], base: [f32; 3], delta: f32)
        -> Result<(), &'static str>
    {
        if !self.speed.is_finite() || !self.acceleration.is_finite()
            || !self.velocity.iter().all(|v| v.is_finite())
            || !sin_cos.iter().all(|v| v.is_finite())
            || !base.iter().all(|v| v.is_finite()) || !delta.is_finite() || delta < 0.0
        { return Err("invalid Demo horizontal body observation"); }
        let [sin, cos] = sin_cos;
        let old_speed = self.speed;
        let x = (cos * base[0] + base[1] * 0.0) + sin * old_speed;
        let z = ((-sin) * base[0] + base[1] * 0.0) + cos * old_speed;
        let next = delta * self.acceleration + old_speed;
        if !x.is_finite() || !z.is_finite() || !next.is_finite() {
            return Err("Demo horizontal body update overflow");
        }
        self.velocity[0] = x;
        self.velocity[2] = z;
        self.velocity[3] = 0.0;
        self.speed = next;
        if next * old_speed <= 0.0 {
            self.speed = 0.0;
            self.acceleration = 0.0;
        }
        Ok(())
    }
}

/// Restore only the native common-damage entry mask, then set damage and
/// clear movement flags. Model-joint mAttr changes are a separate operation.
// Translated from 0x0080E190 (DDDA.exe 2017-12-13 Steam).
pub(super) fn damage_entry_flags(current: u32, saved: u32) -> u32 {
    let restored = (current & !0x0e3f) | (saved & 0x0e3f);
    (restored | 0x10) & !0x0c
}

/// Native body bytes+0x2EA0..0x2EA3. Demo Damage(flag) reads the rising
/// edge at+0x2EA1, rather than the persistent active byte or an HP change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DamageEdges {
    pub active: bool,
    pub started: bool,
    pub ended: bool,
    pub other: bool,
}

impl DamageEdges {
    /// Actual body fields and linked NPC vf180 flags must be supplied.
    /// Unknown NPC flags reject the update without inventing an edge.
    // Translated from uCharacterBase::vf111 0x00850100..0x00850181 and
    // Demo getter0x007AB800 (DDDA.exe 2017-12-13 Steam).
    pub fn observe(&mut self, body_flags: u32, body_category: u8,
        npc_flags: Option<u32>, collision_flags: u32) -> Result<(), &'static str>
    {
        let npc_damage = if body_category & 8 != 0 {
            npc_flags.ok_or("Demo damage edge needs actual linked NPC flags")? & 0x80 != 0
        } else { false };
        let active = body_flags & 0x10 != 0 || npc_damage || collision_flags & 0x4000 != 0;
        self.started = active && !self.active;
        self.ended = !active && self.active;
        self.other = false;
        self.active = active;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_ground_uses_axis_and_extended_masks_and_keeps_disabled_observation() {
        let mut bytes = GroundBytes { ground: 1, wall: 1, ceiling: 1 };
        let mut observation = Scenery { update_flags: 0, axis_present: true,
            extended_ground_check: false, contact_flags: 0 };
        bytes.observe(observation);
        assert_eq!(bytes, GroundBytes { ground: 1, wall: 1, ceiling: 1 });
        observation.update_flags = 4;
        observation.contact_flags = 1 | 4 | 8 | 0x400;
        bytes.observe(observation);
        assert_eq!(bytes, GroundBytes { ground: 0, wall: 1, ceiling: 1 });
        observation.axis_present = false;
        bytes.observe(observation);
        assert_eq!(bytes.ground, 1, "axis failure applies only to an actual axis");
        observation.contact_flags = 0x0010_0000;
        bytes.observe(observation);
        assert_eq!(bytes, GroundBytes::default());
        observation.extended_ground_check = true;
        bytes.observe(observation);
        assert_eq!(bytes.ground, 1);
        observation.contact_flags = 2;
        bytes.observe(observation);
        assert_eq!(bytes.ground, 1, "slope contact is grounded in both masks");
    }

    #[test]
    fn demo_velocity_projects_before_deceleration_and_clamps_a_sign_crossing() {
        let mut body = HorizontalVelocity { speed: 8.0, acceleration: -3.0,
            velocity: [91.0, 12.0, 93.0, 94.0] };
        body.update([1.0, 0.0], [2.0, 0.0, 0.0], 2.0).unwrap();
        assert_eq!(body.velocity, [8.0, 12.0, -2.0, 0.0]);
        assert_eq!(body.speed, 2.0);
        body.update([0.0, 1.0], [0.0; 3], 1.0).unwrap();
        assert_eq!(body.velocity, [0.0, 12.0, 2.0, 0.0]);
        assert_eq!((body.speed, body.acceleration), (0.0, 0.0));
        let before = body;
        assert!(body.update([f32::NAN, 1.0], [0.0; 3], 1.0).is_err());
        assert_eq!(body, before);
        body.acceleration = 9.0;
        body.update([0.0, 1.0], [0.0; 3], 1.0).unwrap();
        assert_eq!((body.speed, body.acceleration), (0.0, 0.0), "zero old speed also clamps");
    }

    #[test]
    fn demo_damage_entry_preserves_unrelated_flags_and_restores_native_subset() {
        assert_eq!(damage_entry_flags(0xa5a5_500f, 0x0e23),
            ((0xa5a5_500f & !0x0e3f) | 0x0e23 | 0x10) & !0x0c);
        assert_eq!(damage_entry_flags(u32::MAX, 0) & 0x0e3f, 0x10);
        assert_eq!(damage_entry_flags(0, u32::MAX) & 0x0e3f, 0x0e33);
    }

    #[test]
    fn demo_damage_flag_is_a_native_rising_edge_with_distinct_body_npc_and_collision_sources() {
        let mut edges = DamageEdges::default();
        edges.observe(0x10, 8, Some(0), 0).unwrap();
        assert!(edges.active && edges.started && !edges.ended && !edges.other);
        edges.observe(0x10, 8, Some(0), 0).unwrap();
        assert!(edges.active && !edges.started && !edges.ended);
        edges.observe(0, 8, Some(0), 0).unwrap();
        assert!(!edges.active && !edges.started && edges.ended);
        edges.observe(0, 8, Some(0x80), 0).unwrap();
        assert!(edges.active && edges.started && !edges.ended);
        let before = edges;
        assert!(edges.observe(0, 8, None, 0).is_err());
        assert_eq!(edges, before, "unknown linked NPC observation cannot mutate the edge");
        edges.observe(0, 0, Some(0x80), 0).unwrap();
        assert!(!edges.active && edges.ended, "NPC mask does not apply to a non-NPC body");
        edges.observe(0, 0, None, 0x4000).unwrap();
        assert!(edges.active && edges.started);
        edges.observe(0, 0, None, 0x4000).unwrap();
        assert!(edges.active && !edges.started && !edges.ended);
        edges.observe(0, 0, None, 0).unwrap();
        assert!(!edges.active && !edges.started && edges.ended);
    }
}
