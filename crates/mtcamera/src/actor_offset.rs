//! Actor-collision camera offset. Source: DDDA.exe 2017-12-13 Steam.
//! Geometry and active-entry selection are caller responsibilities; in
//! particular, all hurt volumes are not equivalent to the selected
//! object-adjustment entries. Each view owns its accumulator and smoother.

/// Geometry inputs built by 0x00888750 for one eligible actor. All points
/// are world-space centimetres. The broad phase is the capsule joining
/// desired/current eye; the narrow phase is the moving sphere below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorQuery {
    pub desired_eye: [f32; 3],
    pub current_eye: [f32; 3],
    /// Read from installed global 0x01875dec (shared with stage100 fade).
    pub broad_radius: f32,
    pub sphere_start: [f32; 3],
    pub sphere_radius: f32,
    pub displacement: [f32; 3],
}

impl ActorQuery {
    /// Translated from 0x00888750. A degenerate desired view skips the
    /// narrow phase. The source starts 32 cm beyond target but retains
    /// the full target-to-eye displacement, rather than shortening it.
    pub fn new(
        desired_eye: [f32; 3],
        current_eye: [f32; 3],
        target: [f32; 3],
        broad_radius: f32,
    ) -> Option<Self> {
        let displacement = std::array::from_fn(|i| desired_eye[i] - target[i]);
        let length_sq: f32 = displacement.iter().map(|v| v * v).sum();
        if length_sq <= f32::EPSILON {
            return None;
        }
        let scale = 32.0 / length_sq.sqrt(); // 0x01517928
        Some(Self {
            desired_eye,
            current_eye,
            broad_radius,
            sphere_start: std::array::from_fn(|i| target[i] + displacement[i] * scale),
            sphere_radius: 30.0, // 0x014ea160
            displacement,
        })
    }
}

/// Explicit boundary for the original actor's selected collision entries.
///
/// An implementation must apply the actor gate in 0x00888750, then use
/// collection1 from 0x00773340 (including activation and owner vf64
/// filtering). Only source shape0/1 entries with radius strictly >32 cm
/// qualify. If none overlaps the query's broad capsule, emit nothing;
/// otherwise cast the query sphere against every qualifying entry and
/// emit each contact position returned by 0x00d62bb0/0x00d62e90.
///
/// Do not substitute all hurt volumes, render bounds, or a nearest-hit
/// only result. The producer takes the greatest contact-to-eye distance.
/// No default implementation is supplied: current gameplay push volumes
/// do not yet preserve every required actor gate and runtime-created entry.
pub trait EligibleEntryProvider {
    fn contact_positions(&self, query: &ActorQuery, emit: &mut dyn FnMut([f32; 3]));
}

/// Per-frame contact distances submitted by eligible actors.
#[derive(Clone, Copy, Debug, Default)]
pub struct Contacts {
    largest: f32,
}

impl Contacts {
    /// Translated from 0x00888b7b..0x00888bc8: reduce an eligible actor's
    /// contacts to distances from the desired eye, then the frame maximum.
    pub fn query_actor(&mut self, provider: &impl EligibleEntryProvider, query: &ActorQuery) {
        provider.contact_positions(query, &mut |point| {
            let squared: f32 = point
                .iter()
                .zip(query.desired_eye)
                .map(|(p, eye)| (p - eye) * (p - eye))
                .sum();
            self.submit(squared.sqrt());
        });
    }
    /// Translated from 0x00421d60: retain the greatest absolute input,
    /// preserving its sign and retaining the earlier value on a tie.
    /// The actor producer 0x00888750 submits nonnegative contact distances.
    pub fn submit(&mut self, distance: f32) {
        if self.largest.abs() < distance.abs() {
            self.largest = distance;
        }
    }

    /// Consume one frame's value and clear it, corresponding to the reset
    /// in 0x00420910. Reset is independent of the smoothing state.
    pub fn take(&mut self) -> f32 {
        std::mem::take(&mut self.largest)
    }
}

/// The scalar object at camera +0xB94: value +0xB98, velocity +0xB9C.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorOffset {
    value: f32,
    velocity: f32,
}

impl ActorOffset {
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Apply the source pose shift before the stage sweep (0x007e1400).
    /// `allowed` combines the active-camera check and source transition
    /// gate (+0x520 <= +0x51C or byte +0x280 == 0). Eye and target must be
    /// distinct. Returns whether the source marks an actor offset active.
    pub fn shift_pose(&self, eye: &mut [f32; 3], target: &mut [f32; 3], allowed: bool) -> bool {
        if !allowed || self.value.abs() <= 0.1 {
            return false;
        }
        let toward: [f32; 3] = std::array::from_fn(|i| target[i] - eye[i]);
        let length = toward.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(length > 0.0, "camera eye and target must be distinct");
        let scale = self.value / length;
        let shift: [f32; 3] = toward.map(|v| v * scale);
        for i in 0..3 {
            eye[i] += shift[i];
        }
        let shift_length = shift.iter().map(|v| v * v).sum::<f32>().sqrt();
        // 0x007e19da..0x007e1a5b compares SHIFT length, not view length.
        if shift_length - 16.0 < self.value {
            let scale = self.value / shift_length;
            for i in 0..3 {
                target[i] += shift[i] * scale;
            }
        }
        true
    }

    /// Translated from 0x007dcf30 and 0x007bc800, with virtual methods
    /// 0x007bc910..0x007bc960 (vtable 0x01599174).
    /// `normal_camera` is the source's 0x00421ce0 selection result.
    /// Delta uses source game frames and must be finite.
    pub fn step(&mut self, target: f32, delta_frames: f32, normal_camera: bool) -> f32 {
        assert!(delta_frames.is_finite(), "camera frame delta must be finite");
        if !normal_camera {
            // Source clears the output only, not its retained velocity.
            self.value = 0.0;
        } else if delta_frames < 0.0 {
            self.value = target;
        } else {
            // 0x007dcf30 sets coefficient 0.1 before each update.
            let coefficient = 0.1_f32;
            let damping = (2.0 * coefficient.sqrt()).min(1.0);
            let mut remaining = delta_frames;
            while remaining > 0.0 {
                // 0x007bc800 chunks at 1.1 source frames. The old velocity
                // is scaled by h too; this is not conventional Euler.
                let h = remaining.min(1.1);
                let acceleration = -coefficient * (self.value - target) - damping * self.velocity;
                self.velocity = self.velocity * h + acceleration * h;
                self.value += self.velocity;
                remaining -= 1.1;
            }
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.0001, "{a} != {b}");
    }

    #[test]
    fn contact_maximum_is_per_frame_and_per_view() {
        let mut a = Contacts::default();
        let mut b = Contacts::default();
        for x in [3.0, -7.0, 7.0, 4.0] {
            a.submit(x);
        }
        b.submit(2.0);
        assert_eq!(a.take(), -7.0);
        assert_eq!(a.take(), 0.0);
        assert_eq!(b.take(), 2.0);
    }

    #[test]
    fn scalar_source_chunks_and_retains_velocity_on_snap_and_bypass() {
        let mut s = ActorOffset::default();
        near(s.step(100.0, 1.0, true), 10.0);
        // Half-frame update scales the prior velocity as well as acceleration.
        near(s.step(100.0, 0.5, true), 16.337723);
        let velocity = s.velocity;
        near(s.step(50.0, -1.0, true), 50.0);
        near(s.velocity, velocity);
        near(s.step(90.0, 0.0, true), 50.0);
        near(s.step(90.0, 1.0, false), 0.0);
        near(s.velocity, velocity);

        let mut whole = ActorOffset::default();
        let mut chunks = ActorOffset::default();
        whole.step(80.0, 2.5, true);
        for dt in [1.1, 1.1, 0.3] {
            chunks.step(80.0, dt, true);
        }
        near(whole.value(), chunks.value());
        near(whole.velocity, chunks.velocity);
        near(ActorOffset::default().value(), 0.0);
    }

    #[test]
    fn query_uses_full_displacement_and_all_eligible_contacts() {
        let query = ActorQuery::new([0.0, 0.0, -100.0], [10.0, 0.0, -90.0], [0.0; 3], 80.0).unwrap();
        assert_eq!(query.sphere_start, [0.0, 0.0, -32.0]);
        assert_eq!(query.displacement, [0.0, 0.0, -100.0]);
        assert_eq!(query.sphere_radius, 30.0);
        assert_eq!(query.current_eye, [10.0, 0.0, -90.0]);
        assert!(ActorQuery::new([0.0; 3], [1.0; 3], [0.0; 3], 80.0).is_none());

        // A provider fixture supplies already-filtered contact positions;
        // this verifies reduction, not the unimplemented geometry provider.
        struct SelectedContacts;
        impl EligibleEntryProvider for SelectedContacts {
            fn contact_positions(&self, _: &ActorQuery, emit: &mut dyn FnMut([f32; 3])) {
                emit([0.0, 0.0, -90.0]);
                emit([0.0, 0.0, -40.0]);
                emit([0.0, 0.0, -80.0]);
            }
        }
        let mut contacts = Contacts::default();
        contacts.query_actor(&SelectedContacts, &query);
        assert_eq!(contacts.take(), 60.0);
    }

    #[test]
    fn pose_shift_uses_shift_length_and_strict_gate() {
        let mut offset = ActorOffset::default();
        let (mut eye, mut target) = ([0.0, 0.0, -200.0], [0.0; 3]);
        offset.step(0.1, -1.0, true);
        assert!(!offset.shift_pose(&mut eye, &mut target, true));
        offset.step(20.0, -1.0, true);
        assert!(!offset.shift_pose(&mut eye, &mut target, false));
        assert!(offset.shift_pose(&mut eye, &mut target, true));
        assert_eq!(eye, [0.0, 0.0, -180.0]);
        assert_eq!(target, [0.0, 0.0, 20.0]);

        // Signed input is supported by the accumulator, though the actor
        // contact producer supplies nonnegative distances. At -8 exactly,
        // abs(shift)-16 == offset and the target stays fixed.
        offset.step(-8.0, -1.0, true);
        assert!(offset.shift_pose(&mut eye, &mut target, true));
        assert_eq!(eye, [0.0, 0.0, -188.0]);
        assert_eq!(target, [0.0, 0.0, 20.0]);
    }
}
