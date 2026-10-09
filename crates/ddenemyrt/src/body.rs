//! A character's body for the fight: its rest skeleton, its motion banks
//! (with their collision events) and its collision files, and what one
//! frame of it puts in the world: the posed joints and the attack and hurt
//! volumes its playing motion switches on (docs/COMBAT.md 2.1-2.2).

use ddcombat::events::{Activation, MotionEvents};
use ddcombat::volumes::{self, AttackVolume, HurtVolume, Mat4, Pose};
use ddcombat::ObjCollision;
use mtlmt::pose::Skeleton;
use mtlmt::{Motion, MotionList};

/// A motion: the bank (the body's own numbering of its loaded lists) and
/// the slot in that list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MotionId {
    pub bank: u8,
    pub slot: u16,
}

impl MotionId {
    pub const fn new(bank: u8, slot: u16) -> Self {
        MotionId { bank, slot }
    }
}

/// One loaded motion list with its event blocks.
#[derive(Debug, Clone)]
pub struct MotionBank {
    pub path: String,
    pub list: MotionList,
    pub events: Vec<Option<MotionEvents>>,
}

/// A collision body file and the attack file paired with it (goblins keep
/// their attacks in `_00`, the Arisen's sword in `m0001_00_lv1`).
#[derive(Debug, Clone)]
pub struct CollisionSet {
    pub body: ObjCollision,
    pub attacks: Option<ObjCollision>,
}

/// Everything a body needs from the install.
#[derive(Debug, Clone)]
pub struct Body {
    pub skeleton: Skeleton,
    /// Indexed by [`MotionId::bank`].
    pub banks: Vec<Option<MotionBank>>,
    pub collision: Vec<CollisionSet>,
    /// The owner's scale (the mean of owner +0x60..+0x68): volume radii
    /// are multiplied by it (0x00773830).
    pub scale: f32,
}

/// `p * M` for the row-vector matrices of the pose.
fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    mtlmt::pose::mul(a, b)
}

/// The model's world matrix: a turn of `yaw` about +y (0 faces +z, as ddsim
/// and uCoord mAngle.y; *assumed*, not traced), then the position.
pub fn model_matrix(pos: [f32; 3], yaw: f32) -> Mat4 {
    let (s, c) = yaw.sin_cos();
    // row-vector form: +z (0,0,1) maps to (sin, 0, cos)
    [
        [c, 0.0, -s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [s, 0.0, c, 0.0],
        [pos[0], pos[1], pos[2], 1.0],
    ]
}

/// A posed model: world matrices of its joints for `ddcombat::volumes`.
#[derive(Debug, Clone, PartialEq)]
pub struct Posed {
    pub model: Mat4,
    /// (joint id, world matrix).
    pub joints: Vec<(u8, Mat4)>,
}

impl Pose for Posed {
    fn model_matrix(&self) -> Mat4 {
        self.model
    }
    fn joint_matrix(&self, id: i32) -> Option<Mat4> {
        let id = u8::try_from(id).ok()?;
        self.joints.iter().find(|j| j.0 == id).map(|j| j.1)
    }
}

impl Body {
    pub fn motion(&self, id: MotionId) -> Option<&Motion> {
        self.banks
            .get(usize::from(id.bank))?
            .as_ref()?
            .list
            .motions
            .get(usize::from(id.slot))?
            .as_ref()
    }

    pub fn events(&self, id: MotionId) -> Option<&MotionEvents> {
        self.banks
            .get(usize::from(id.bank))?
            .as_ref()?
            .events
            .get(usize::from(id.slot))?
            .as_ref()
    }

    /// The motion's frame count (0 when missing).
    pub fn frames(&self, id: MotionId) -> u32 {
        self.motion(id).map_or(0, |m| m.frame_count)
    }

    /// `frame` brought into the motion: a looping motion (loop frame >= 0)
    /// wraps past its last frame back to the loop frame, a one-shot one
    /// holds its last frame.
    pub fn wrap_frame(&self, id: MotionId, frame: f32) -> f32 {
        let Some(m) = self.motion(id) else { return frame };
        let last = m.frame_count.saturating_sub(1) as f32;
        if frame <= last {
            return frame;
        }
        if m.loop_frame < 0 {
            return last;
        }
        let lf = m.loop_frame as f32;
        lf + (frame - last).rem_euclid((last - lf).max(1.0))
    }

    /// The root-motion track of a motion as ddsim reads it.
    pub fn root_motion(&self, id: MotionId) -> Option<ddsim::RootMotion> {
        self.motion(id).map(ddsim::RootMotion::from_motion)
    }

    /// The collision sequences the motion switches on at `frame`
    /// (`ddcombat::events::activations`, 0x0076F430: the frame truncated).
    pub fn activations(&self, id: MotionId, frame: f32) -> Vec<Activation> {
        self.events(id)
            .map_or_else(Vec::new, |e| ddcombat::events::activations(e, frame))
    }

    /// The skeleton posed by the motion at `frame`, placed at `pos` / `yaw`.
    pub fn pose(&self, id: MotionId, frame: f32, pos: [f32; 3], yaw: f32) -> Posed {
        let model = model_matrix(pos, yaw);
        let local = self.skeleton.pose(self.motion(id), frame);
        let joints = self
            .skeleton
            .joints
            .iter()
            .zip(&local)
            .map(|(j, m)| (j.id, mul(m, &model)))
            .collect();
        Posed { model, joints }
    }

    /// The attack and hurt volumes of one frame.
    pub fn volumes(
        &self,
        acts: &[Activation],
        pose: &Posed,
        owner: u32,
    ) -> (Vec<AttackVolume>, Vec<HurtVolume>) {
        let mut attacks = Vec::new();
        let mut hurts = Vec::new();
        for c in &self.collision {
            attacks.extend(volumes::attack_volumes(
                &c.body,
                c.attacks.as_ref(),
                acts,
                pose,
                self.scale,
                owner,
            ));
            hurts.extend(volumes::hurt_volumes(&c.body, acts, pose, self.scale, owner));
        }
        (attacks, hurts)
    }
}

impl Body {
    /// The push-out volumes of one frame: the nodes of the activated
    /// groups with `mIsObjAdj` (docs/COMBAT.md, the push-out).
    pub fn push_volumes(&self, acts: &[Activation], pose: &Posed) -> Vec<ddcombat::volumes::Volume> {
        let mut out = Vec::new();
        for c in &self.collision {
            for act in acts.iter().filter(|a| a.resource_id == c.body.resource_id) {
                let Some((group, _, _, _)) = volumes::activated(&c.body, None, act) else {
                    continue;
                };
                if group.is_obj_adj {
                    out.extend(
                        group
                            .nodes
                            .iter()
                            .filter_map(|n| volumes::build_volume(n, pose, self.scale)),
                    );
                }
            }
        }
        out
    }
}

/// Loading from the install.
pub mod load {
    use super::*;

    pub const RMODEL: &str = "rModel";
    pub const RMOTIONLIST: &str = "rMotionList";
    pub const ROBJCOLLISION: &str = "rObjCollision";

    pub fn read(vfs: &ddvfs::Vfs, path: &str, class: &str) -> Result<Vec<u8>, String> {
        vfs.read(path, mtarc::type_hash(class))
            .map_err(|e| format!("{path}: {e}"))
    }

    /// The rest skeleton of a model file.
    pub fn skeleton(vfs: &ddvfs::Vfs, model: &str) -> Result<Skeleton, String> {
        let d = read(vfs, model, RMODEL)?;
        let m = mtmod::parse(&d).map_err(|e| format!("{model}: {e:?}"))?;
        Ok(Skeleton::from_bones(
            m.bones.iter().map(|b| (b.anim_map_index, b.parent, b.local)),
        ))
    }

    pub fn bank(vfs: &ddvfs::Vfs, path: &str) -> Result<MotionBank, String> {
        let d = read(vfs, path, RMOTIONLIST)?;
        let list = mtlmt::parse(&d).map_err(|e| format!("{path}: {e}"))?;
        let events = ddcombat::events::parse_lmt_events(&d).map_err(|e| format!("{path}: {e}"))?;
        Ok(MotionBank {
            path: path.to_string(),
            list,
            events,
        })
    }

    pub fn collision(vfs: &ddvfs::Vfs, body: &str, attacks: Option<&str>) -> Result<CollisionSet, String> {
        let parse = |p: &str| -> Result<ObjCollision, String> {
            ObjCollision::parse(&read(vfs, p, ROBJCOLLISION)?).map_err(|e| format!("{p}: {e:?}"))
        };
        Ok(CollisionSet {
            body: parse(body)?,
            attacks: attacks.map(parse).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_matrix_faces_its_yaw() {
        let m = model_matrix([10.0, 0.0, 0.0], std::f32::consts::FRAC_PI_2);
        // +z of the model (ahead) goes to +x of the world
        let p = ddcombat::volumes::transform_point(&m, [0.0, 0.0, 100.0]);
        assert!((p[0] - 110.0).abs() < 1e-3 && p[2].abs() < 1e-3, "{p:?}");
    }
}
