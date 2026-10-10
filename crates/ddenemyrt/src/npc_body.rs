//! An installed ordinary NPC skeleton, edits and collision events.
//! The host supplies its canonical motion and placement every frame.

use crate::body::{load, Body, MotionId, Posed};
use ddcombat::volumes::{HurtVolume, Mat4};
use mtlmt::pose::{compose, mul, IDENTITY};
use crate::combat::{Attacker, Fighter, Victim};
use std::sync::Arc;

/// One ordinary, non-attacking NPC lifetime, distinct from every player seat.
/// Motion/frame/transform are supplied by the host's authored NPC actor.
#[derive(Debug, Clone)]
pub struct NpcFighter {
    pub npc: u32,
    pub generation: u64,
    pub fighter: Fighter,
    pub body: Arc<NpcBody>,
    pub motion: MotionId,
    pub frame: f32,
    pub position: [f32; 3],
    pub yaw: f32,
    pub scale: [f32; 3],
    pub flags: u32,
}

impl NpcFighter {
    pub fn new(npc: u32, generation: u64, owner: u32, body: Arc<NpcBody>,
        region: ddcombat::damage::RegionPool, motion: MotionId) -> Self {
        Self { npc, generation, body, motion, frame: 0.0, position: [0.0; 3],
            yaw: 0.0, scale: [1.0; 3], flags: 0,
            fighter: Fighter { owner,
                // No NPC attack volumes are registered by this ordinary
                // passive body. Weapon/equipment attacks remain unsupported.
                attacker: Attacker::Player(ddcombat::damage::PlayerAttacker::new(1.0, 1.0, 0.0, 0.0)),
                victim: Victim::Npc { defender: crate::npc_combat::ordinary_defender(), region,
                    rates: crate::npc_combat::ordinary_collision_rates(0) },
                attacks: vec![], hurts: vec![], history: Default::default(), statuses: Default::default(),
                dots: Default::default(), pools: Default::default() } }
    }

    pub fn set_flags(&mut self, flags: u32) {
        self.flags = flags;
        if let Victim::Npc { rates, .. } = &mut self.fighter.victim {
            *rates = crate::npc_combat::ordinary_collision_rates(flags);
        }
    }

    /// Failure removes last frame's hurts rather than retaining a stale pose.
    pub fn volumes(&mut self) -> Result<(), String> {
        self.fighter.hurts.clear();
        let pose = self.body.pose(self.motion, self.frame, self.position, self.yaw, self.scale)?;
        self.fighter.hurts = self.body.hurts(self.motion, self.frame, &pose, self.fighter.owner, self.scale);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NpcBody {
    pub body: Body,
    pub look: ddlook::Look,
    morph: Option<ddlook::morph::BodyMorph>,
    order: Vec<usize>,
    bone_radii: Vec<f32>,
    detail_masks: Vec<Vec<u8>>,
}

/// Native world model bounds in centimetres, used by the ordinary NPC gauge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcBounds {
    pub center: [f32; 3],
    pub extents: [f32; 3],
}

impl NpcBounds {
    /// Ordinary gauge mode0 (0x0067C970), before local offset/camera pull.
    pub fn gauge_top(self) -> [f32; 3] {
        [self.center[0], self.center[1] + self.extents[1], self.center[2]]
    }

    /// World anchor for one view's camera; `scenery_radius` is the real
    /// inline cObjCollision capsule radius, not the model extent.
    // Translated from 0x0069FB00 / 0x0067C970 / 0x0067C870
    // (DDDA.exe 2017-12-13 Steam).
    pub fn gauge_anchor(self, model: &Mat4, local_offset: [f32; 3],
        actor_position: [f32; 3], camera: Option<[f32; 3]>, scenery_radius: f32) -> [f32; 3] {
        let top = self.gauge_top();
        let offset = ddcombat::volumes::transform_point(model, local_offset);
        let point: [f32; 3] = std::array::from_fn(|i| top[i] + offset[i] - actor_position[i]);
        let Some(camera) = camera else { return point };
        let mut direction: [f32; 3] = std::array::from_fn(|i| camera[i] - point[i]);
        let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        if length >= f32::EPSILON {
            for value in &mut direction { *value *= 1.0 / length; }
        }
        std::array::from_fn(|i| point[i] + direction[i] * scenery_radius)
    }
}

impl NpcBody {
    /// `npc_type` is cNpcInfo.mNpcType, not the ledger NPC ID; the
    /// ordinary constructor initializes it to zero (0x00784880).
    /// These bank registrations mirror the reviewed host S32 Banks.
    // Translated from 0x0083FA60 and 0x00BB6060
    // (DDDA.exe 2017-12-13 Steam).
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, npc: u32,
        unit_no: u32, row: u32, npc_type: u32, common_variant: u32) -> Result<Self, String> {
        if npc_type > 32 || ![0, 99].contains(&common_variant) {
            return Err("unsupported NPC motion bank family".into());
        }
        let look = ddlook::npc_look(vfs, npc, unit_no, row).map_err(|e| e.to_string())?.look;
        let model_bytes = load::read(vfs, &look.base, load::RMODEL)?;
        let model = mtmod::parse(&model_bytes).map_err(|e| format!("{}: {e:?}", look.base))?;
        let skeleton = mtlmt::pose::Skeleton::from_bones(
            model.bones.iter().map(|b| (b.anim_map_index, b.parent, b.local)));
        let bone_radii = model.bones.iter().map(|b| b.length).collect();
        // uPlayerBase::vf100 0x00B531B0 reads these masks by actor+0x5720.
        // 0x00B8D980 writes each joint's active flag. This bulk table stays
        // in the player's executable; dynamic LOD selection is not ported.
        let detail_masks = (0..4).map(|detail| exe.u8s(0x014f_2af0 + detail * 256, 256)
            .map_err(|e| e.0)).collect::<Result<Vec<_>, _>>()?;
        let mut banks = vec![None; 14];
        // NPC constructor's collision/motion table 0x014EAC20.
        for (index, bank) in [(1, 1), (2, 7)] {
            let path = exe.cstr(exe.u32(0x014e_ac20 + index * 8).map_err(|e| e.0)?)
                .map_err(|e| e.0)?;
            banks[bank] = Some(load::bank(vfs, &path)?);
        }
        let common = format!(r"motion\npc\{common_variant:02}m000_c\{common_variant:02}m000_c");
        banks[11] = Some(load::bank(vfs, &common)?);
        let table = exe.u32(0x0163_c980 + npc_type * 4).map_err(|e| e.0)?;
        for (index, suffix) in ['c', 'j'].into_iter().enumerate() {
            let bank = ((exe.u32(table + index as u32 * 8).map_err(|e| e.0)? >> 8) & 15) as usize;
            let family = exe.u32(table + index as u32 * 8 + 4).map_err(|e| e.0)?;
            let stem = match family { 9 => "09f000".into(), 25 => "25e008".into(),
                26 => "26e009".into(), 29 => "29e007".into(), _ => format!("{family:02}m000") };
            let path = if index == 0 && family == 0 { common.clone() }
                else { format!(r"motion\npc\{stem}_{suffix}\{stem}_{suffix}") };
            let slot = banks.get_mut(bank).ok_or("NPC motion bank outside supported array")?;
            *slot = Some(load::bank(vfs, &path)?);
        }
        let mut morph = look.body.clone();
        if let Some(shape) = &look.body_shape {
            let extra = ddlook::body::Tables::read(exe).map_err(|e| e.0)?
                .joints(look.sex, shape).map_err(|e| e.to_string())?;
            morph.get_or_insert_with(|| ddlook::morph::BodyMorph {
                model_scale: [1.0; 3], joints: vec![],
            }).joints.extend(extra);
        }
        let parents: Vec<_> = skeleton.joints.iter().map(|j| j.parent).collect();
        let pivots: Vec<_> = skeleton.joints.iter().map(|j| {
            look.face.iter().find(|f| f.id == j.id)
                .and_then(|_| ddlook::face_constraint::binding(j.id))
                .and_then(|(_, pivot)| pivot).and_then(|id| skeleton.index_of(id))
        }).collect();
        let order = ddlook::face_constraint::order(&parents, &pivots);
        Ok(Self { body: Body { skeleton, banks,
            collision: vec![load::collision(vfs, r"collision\npc\m0000", None)?], scale: 1.0 },
            look, morph, order, bone_radii, detail_masks })
    }

    /// The event-action bank12 -> bank11 fallback of 0x0080D750.
    pub fn resolve(&self, authored: i32) -> Result<MotionId, String> {
        let number = authored as u32 & 0xfff;
        let id = MotionId::new((number >> 8) as u8, (number & 255) as u16);
        if self.body.motion(id).is_some() { return Ok(id); }
        let fallback = MotionId::new(11, id.slot);
        if id.bank == 12 && self.body.motion(fallback).is_some() { return Ok(fallback); }
        Err(format!("NPC motion {number:#05x} missing from installed banks"))
    }

    /// Sample motion, then the same source body/face edits as the viewer.
    /// `scale` is the placement scale; HumanEdit entry0 additionally writes
    /// the native uCoord scale in 0x0081DB50. No player seat is implied.
    // Translated from 0x01051B60 and uCnsEdit::vf24 0x00860AC0,
    // with existing source-marked human/hand/eye helpers
    // (DDDA.exe 2017-12-13 Steam).
    pub fn pose(&self, id: MotionId, frame: f32, position: [f32; 3], yaw: f32,
        scale: [f32; 3]) -> Result<Posed, String> {
        let motion = self.body.motion(id).ok_or("unbound NPC motion")?;
        let skeleton = &self.body.skeleton;
        let mut locals: Vec<_> = skeleton.joints.iter()
            .map(|j| (j.scale, j.rotation, j.translation)).collect();
        for track in &motion.tracks {
            if track.joint == 255 { continue; }
            let Some(index) = skeleton.index_of(track.joint) else { continue };
            let v = track.sample(frame);
            match track.usage {
                mtlmt::Usage::LocalRotation => locals[index].1 = v,
                mtlmt::Usage::LocalPosition => locals[index].2 = [v[0], v[1], v[2]],
                mtlmt::Usage::LocalScale => locals[index].0 = [v[0], v[1], v[2]],
                _ => {}
            }
        }
        let model_scale = self.morph.as_ref().map_or([1.0; 3], |m| m.model_scale);
        let root = compose(model_scale, [0.0, 0.0, 0.0, 1.0], [0.0; 3]);
        let mut world = vec![IDENTITY; skeleton.joints.len()];
        for &index in &self.order {
            let joint = &skeleton.joints[index];
            let parent = joint.parent.map_or(root, |p| world[p]);
            let (s, r, t) = locals[index];
            let mut w = mul(&compose(s, r, t), &parent);
            if let Some(edit) = self.morph.as_ref()
                .and_then(|m| m.joints.iter().find(|(id, _)| *id == joint.id)).map(|(_, e)| e) {
                let lengths = axis_lengths(&w);
                let factors = match edit.kind {
                    0xb => ddlook::morph::human_absolute_axis_factors(joint.id, edit.scale,
                        edit.child, model_scale, lengths),
                    9 => ddlook::morph::hand_axis_factors(edit.scale, lengths),
                    _ => edit.scale,
                };
                w = mul(&compose(factors, [0.0, 0.0, 0.0, 1.0], edit.offset), &w);
            }
            if let Some(edit) = self.look.face.iter().find(|e| e.id == joint.id) {
                let (kind, pivot_id) = ddlook::face_constraint::binding(joint.id).unwrap_or((0, None));
                let pivot = pivot_id.and_then(|id| skeleton.index_of(id)).map(|p| flatten(world[p]));
                let angles = edit.rotation.map(f32::to_radians);
                if let Some(corrected) = ddlook::face_constraint::apply(kind, flatten(w), flatten(parent),
                    pivot, edit.offset, edit.scale, angles) {
                    w = unflatten(corrected);
                } else {
                    // Ordinary face constraints are type0; its Euler
                    // correction uses Rx * Ry * Rz row-vector order.
                    w = mul(&face_matrix(edit.scale, angles, edit.offset), &w);
                }
            }
            world[index] = w;
        }
        let half = yaw * 0.5;
        let placement = compose(scale, [0.0, half.sin(), 0.0, half.cos()], position);
        let model = compose(self.unit_scale(scale), [0.0, half.sin(), 0.0, half.cos()], position);
        // HumanEdit is already included once in `world` via the root.
        let joints = skeleton.joints.iter().zip(world).map(|(j, w)| (j.id, mul(&w, &placement))).collect();
        Ok(Posed { model, joints })
    }

    // Translated from 0x0081DB50's owner+0x60/64/68 writes
    // (DDDA.exe 2017-12-13 Steam). Placement transform remains an explicit
    // adapter input; later dynamic actor scale overrides are not ported.
    pub fn unit_scale(&self, placement: [f32; 3]) -> [f32; 3] {
        let human = self.morph.as_ref().map_or([1.0; 3], |m| m.model_scale);
        std::array::from_fn(|i| placement[i] * human[i])
    }

    /// Scaled native bounds (model+0x108 bit0x40), with explicit native
    /// detail row0..3. Additional dynamic constraint mask overrides and
    /// the unscaled branch's parent spheres are outside this adapter.
    // Translated from 0x00FFA620, scaled active-joint branch,
    // and 0x00B531B0 / 0x00B8D980 (DDDA.exe 2017-12-13 Steam).
    pub fn bounds(&self, pose: &Posed, detail: u8) -> Result<NpcBounds, String> {
        let mask = self.detail_masks.get(detail as usize).ok_or("unsupported NPC detail row")?;
        // 0x00B8D980 passes joint+0x93 (MOD byte0 anim_map_index) to
        // vf100. Radius and matrix arrays remain in parallel MOD order.
        scaled_bounds(pose.joints.iter().zip(&self.bone_radii)
            .filter(|((id, _), _)| mask[*id as usize] != 0)
            .map(|((_, matrix), radius)| (*matrix, *radius)))
    }

    pub fn hurts(&self, id: MotionId, frame: f32, pose: &Posed, owner: u32,
        scale: [f32; 3]) -> Vec<HurtVolume> {
        let activations = self.body.activations(id, frame);
        let scale = self.unit_scale(scale);
        ddcombat::volumes::hurt_volumes(&self.body.collision[0].body, &activations, pose,
            (scale[0] + scale[1] + scale[2]) / 3.0, owner)
    }
}

fn scaled_bounds(spheres: impl IntoIterator<Item = (Mat4, f32)>) -> Result<NpcBounds, String> {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut count = 0;
    for (matrix, radius) in spheres {
        if radius < 0.0 || !radius.is_finite() || matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid NPC model bound input".into());
        }
        // Native length includes all four components of each matrix row.
        let scale = matrix[..3].iter().map(|row| row.iter().map(|v| v * v)
            .sum::<f32>().sqrt()).fold(0.0_f32, f32::max);
        let radius = radius * scale;
        if !radius.is_finite() || matrix[3][..3].iter().any(|v| !v.is_finite()) {
            return Err("nonfinite NPC model bounds".into());
        }
        for axis in 0..3 {
            low[axis] = low[axis].min(matrix[3][axis] - radius);
            high[axis] = high[axis].max(matrix[3][axis] + radius);
        }
        count += 1;
    }
    if count == 0 { return Err("no active NPC bound joints".into()); }
    Ok(NpcBounds { center: std::array::from_fn(|i| (high[i] + low[i]) * 0.5),
        extents: std::array::from_fn(|i| (high[i] - low[i]) * 0.5) })
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn native_scaled_spheres_union_uses_largest_four_component_axis() {
        let mut first = IDENTITY;
        first[0] = [3.0, 0.0, 0.0, 4.0];
        first[3] = [10.0, 20.0, 30.0, 1.0];
        let mut second = IDENTITY;
        second[3] = [-10.0, 10.0, 5.0, 1.0];
        let bounds = scaled_bounds([(first, 2.0), (second, 3.0)]).unwrap();
        assert_eq!(bounds.center, [3.5, 18.5, 21.0]);
        assert_eq!(bounds.extents, [16.5, 11.5, 19.0]);
        assert_eq!(bounds.gauge_top(), [3.5, 30.0, 21.0]);
        assert!(scaled_bounds([]).is_err());
        assert!(scaled_bounds([(IDENTITY, f32::NAN)]).is_err());
        assert!(scaled_bounds([(IDENTITY, -1.0)]).is_err());
        let mut invalid_axis = IDENTITY;
        invalid_axis[0][0] = f32::NAN;
        assert!(scaled_bounds([(invalid_axis, 1.0)]).is_err());
    }

    #[test]
    fn gauge_uses_each_camera_and_transformed_offset_without_double_translation() {
        let bounds = NpcBounds { center: [100.0, 20.0, 0.0], extents: [10.0, 30.0, 10.0] };
        let mut model = IDENTITY;
        model[0][0] = 2.0;
        model[3] = [100.0, 0.0, 0.0, 1.0];
        let anchor = |camera| bounds.gauge_anchor(&model, [5.0, 0.0, 0.0],
            [100.0, 0.0, 0.0], camera, 35.0);
        assert_eq!(anchor(None), [110.0, 50.0, 0.0]);
        assert_eq!(anchor(Some([210.0, 50.0, 0.0])), [145.0, 50.0, 0.0]);
        assert_eq!(anchor(Some([10.0, 50.0, 0.0])), [75.0, 50.0, 0.0]);
        assert_eq!(anchor(Some([110.0, 50.0, 0.0])), anchor(None));
    }
}

fn axis_lengths(m: &Mat4) -> [f32; 3] {
    std::array::from_fn(|row| m[row][..3].iter().map(|v| v * v).sum::<f32>().sqrt())
}

fn flatten(m: Mat4) -> [f32; 16] { std::array::from_fn(|i| m[i / 4][i % 4]) }
fn unflatten(m: [f32; 16]) -> Mat4 { std::array::from_fn(|r| std::array::from_fn(|c| m[r * 4 + c])) }

fn face_matrix(scale: [f32; 3], angles: [f32; 3], offset: [f32; 3]) -> Mat4 {
    let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(f32::sin_cos);
    let mut m = [[cy * cz, cy * sz, -sy, 0.0],
        [sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy, 0.0],
        [cx * sy * cz + sx * sz, cx * sy * sz - sx * cz, cx * cy, 0.0],
        [offset[0], offset[1], offset[2], 1.0]];
    for axis in 0..3 { for value in &mut m[axis][..3] { *value *= scale[axis]; } }
    m
}
