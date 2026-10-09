//! Hit volumes: placement on joints, the shape tests, the pair filter,
//! the per-frame dedup and the hit history that limits repeated hits.
//!
//! Every function names the routine of DDDA.exe (2017-12-13 Steam build)
//! it is translated from; `docs/COMBAT.md` section 2 has the context.
//! Joint transforms are inputs (the skeleton is posed elsewhere), so the
//! module stays headless and works for any number of characters.

use crate::events::Activation;
use crate::ocl::{Group, Node, ObjCollision};

pub type Vec3 = [f32; 3];
/// A 4x4 matrix as the game stores it: rows 0..2 are the basis, row 3 the
/// translation; points are row vectors (`p * M`).
pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `x*row0 + y*row1 + z*row2 + row3` (the multiply in 0x00CCDE30).
pub fn transform_point(m: &Mat4, p: Vec3) -> Vec3 {
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        *o = m[1][k] * p[1] + m[0][k] * p[0] + m[2][k] * p[2] + m[3][k];
    }
    out
}

/// 0x00CCE190 with its "normalize" argument 0: rows 0..2 are normalized as
/// 4-vectors, so joint scale does not stretch the offsets.
pub fn normalize_rows(mut m: Mat4) -> Mat4 {
    for row in m.iter_mut().take(3) {
        let l = 1.0 / (row[0] * row[0] + row[1] * row[1] + row[2] * row[2] + row[3] * row[3]).sqrt();
        for v in row.iter_mut() {
            *v *= l;
        }
    }
    m
}

/// The posed skeleton of one model, as the caller provides it.
pub trait Pose {
    /// World matrix of the model (`uModel`+0xB0), used for joint -1.
    fn model_matrix(&self) -> Mat4;
    /// World matrix of the joint with this id (the model's joint id, not
    /// the bone index), or None if the model has no such joint.
    fn joint_matrix(&self, id: i32) -> Option<Mat4>;
}

/// Translated from 0x00CCDE30 / 0x00CCE190 (DDDA.exe 2017-12-13 Steam):
/// a node point. Joint -1 is the model's own frame; -3, -4, -5, -6 are the
/// midpoints of the offset placed on joints 14 & 18, 15 & 19, 16 & 20,
/// 17 & 21; -2 (a Y rotation by an angle argument at the world origin) is
/// treated as the identity here (*the angle source is not traced*). A
/// missing joint uses the identity matrix (the default at 0x016295C0), so
/// the point is the offset itself.
pub fn node_point(pose: &dyn Pose, joint: i32, offset: Vec3) -> Vec3 {
    let at = |j: i32| -> Vec3 {
        let m = if j == -1 { pose.model_matrix() } else { pose.joint_matrix(j).unwrap_or(IDENTITY) };
        transform_point(&normalize_rows(m), offset)
    };
    match joint {
        -6..=-3 => {
            let a = (-joint - 3) + 14;
            let (p, q) = (at(a), at(a + 4));
            [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5, (p[2] + q[2]) * 0.5]
        }
        -2 => transform_point(&IDENTITY, offset),
        j => at(j),
    }
}

/// A placed volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Volume {
    Sphere { c: Vec3, r: f32 },
    Capsule { p0: Vec3, p1: Vec3, r: f32 },
}

/// Translated from 0x00773830 (DDDA.exe 2017-12-13 Steam): build a node's
/// volume. `scale` is the owner's scale factor, the mean of its scale
/// vector (owner +0x60..+0x68), applied because entry flag 0x318 bit 6 is
/// set by default (0x007735C0). Shape 0: capsule; 1: sphere; 4: capsule
/// pulled in by the radius at both ends, or a sphere one radius from
/// point 0 towards point 1 when the points are closer than two radii.
/// Shapes 2 and 3 build nothing on this path.
pub fn build_volume(node: &Node, pose: &dyn Pose, scale: f32) -> Option<Volume> {
    let r = node.radius * scale;
    match node.shape {
        0 => Some(Volume::Capsule {
            p0: node_point(pose, node.joint0, node.offset0),
            p1: node_point(pose, node.joint1, node.offset1),
            r,
        }),
        1 => Some(Volume::Sphere { c: node_point(pose, node.joint0, node.offset0), r }),
        4 => {
            let a = node_point(pose, node.joint0, node.offset0);
            let b = node_point(pose, node.joint1, node.offset1);
            let d = sub(b, a);
            let len2 = dot(d, d);
            let len = len2.sqrt();
            if r * 2.0 <= len {
                let inv = 1.0 / len;
                let o = [d[0] * inv * r, d[1] * inv * r, d[2] * inv * r];
                Some(Volume::Capsule { p0: [a[0] + o[0], a[1] + o[1], a[2] + o[2]], p1: [b[0] - o[0], b[1] - o[1], b[2] - o[2]], r })
            } else {
                // default direction 0x01629540 (+Z assumed: *not read*)
                let dir = if 1.192_092_9e-7 < len2 { [d[0] / len, d[1] / len, d[2] / len] } else { [0.0, 0.0, 1.0] };
                Some(Volume::Sphere { c: [a[0] + r * dir[0], a[1] + r * dir[1], a[2] + r * dir[2]], r })
            }
        }
        _ => None,
    }
}

/// Squared distance from a point to a segment (the game's 0x00D54AE0 /
/// 0x00D53C70 compute the same quantity).
pub fn point_segment_dist2(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    let d = sub(p, q);
    dot(d, d)
}

/// Squared distance between two segments. The game's 0x00D54B20 computes
/// this quantity with its own parallel-case handling (threshold 0.001 on
/// 1 - cos^2); this is the exact closest-points form, so results agree up
/// to float rounding.
pub fn segment_segment_dist2(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> f32 {
    let d1 = sub(a1, a0);
    let d2 = sub(b1, b0);
    let r = sub(a0, b0);
    let a = dot(d1, d1);
    let e = dot(d2, d2);
    let f = dot(d2, r);
    let (s, t);
    if a <= 1e-12 && e <= 1e-12 {
        return dot(r, r);
    }
    if a <= 1e-12 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= 1e-12 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > 0.0 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    let p = [a0[0] + d1[0] * s, a0[1] + d1[1] * s, a0[2] + d1[2] * s];
    let q = [b0[0] + d2[0] * t, b0[1] + d2[1] * t, b0[2] + d2[2] * t];
    let d = sub(p, q);
    dot(d, d)
}

const EPS: f32 = 1.192_092_9e-7;

/// The overlap test of two volumes: sphere-sphere 0x00D13600, capsule-
/// sphere 0x00D139A0, capsule-capsule 0x00D154E0 (DDDA.exe 2017-12-13
/// Steam). All three hit when the squared core distance is <= the squared
/// sum of the radii (touching counts); a capsule whose points coincide
/// (length^2 < 1.19e-7) acts as a sphere at point 0.
pub fn overlap(a: &Volume, b: &Volume) -> bool {
    fn core(v: &Volume) -> (Vec3, Vec3, f32) {
        match *v {
            Volume::Sphere { c, r } => (c, c, r),
            Volume::Capsule { p0, p1, r } => (p0, p1, r),
        }
    }
    let (a0, a1, ra) = core(a);
    let (b0, b1, rb) = core(b);
    let rr = (ra + rb) * (ra + rb);
    let la = dot(sub(a1, a0), sub(a1, a0)) >= EPS;
    let lb = dot(sub(b1, b0), sub(b1, b0)) >= EPS;
    let d2 = match (la, lb) {
        (true, true) => segment_segment_dist2(a0, a1, b0, b1),
        (true, false) => point_segment_dist2(b0, a0, a1),
        (false, true) => point_segment_dist2(a0, b0, b1),
        (false, false) => dot(sub(b0, a0), sub(b0, a0)),
    };
    d2 <= rr
}

// ------------------------------------------------------------ pair filter

/// `0x0150A5A0`: bit of each group kind (`rObjCollision::Group.mKind`).
pub const KIND_BIT: [u32; 9] = [0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x60, 0x80];
/// `0x0150A5E8`: kinds an attack of kind k may hit (row 0) and may hit when
/// a friendly-hit flag is set (row 1, then the team matrix decides).
pub const HIT_MASK: [[u32; 9]; 2] = [
    [0x9e, 0x1d, 0x9f, 0x1e, 0x9f, 0x9e, 0x1d, 0x9f, 0x1f],
    [0x9f, 0x1f, 0x9f, 0x1f, 0x9f, 0x1f, 0x9f, 0x9f, 0x9f],
];

/// One side of a candidate pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Side {
    /// The owning character (any stable id).
    pub owner: u32,
    /// `Group.mKind` of the node's group (0 player side, 1 enemy, ...).
    pub kind: u32,
}

/// Translated from the pair callback 0x0076D730 (DDDA.exe 2017-12-13
/// Steam), without the team matrix (0x0150A630, indexed by the two
/// collisions' +0xD30 teams; *its team ids are not traced*): `same_team`
/// stands for a matrix value of 1. Returns the damage factor stored at the
/// victim entry +0x174 (1, or 0.5 / 0.25 / 0 for the friendly-hit flags;
/// its consumer is *not traced*), or None when the pair is rejected.
pub fn pair_filter(flags: &AttackFlags, att: Side, vic: Side) -> Option<f32> {
    if att.owner == vic.owner && !flags.own_hit {
        return None;
    }
    let no_hit = [(flags.pl_no_hit, 0), (flags.em_no_hit, 1), (flags.etc_no_hit, 2), (flags.npc_no_hit, 3)];
    if no_hit.iter().any(|&(f, k)| f && vic.kind == k) {
        return None;
    }
    let mut friendly = false;
    let mut factor = 1.0f32;
    if flags.friend_hit1 {
        friendly = true;
    }
    if flags.friend_hit1_2 {
        factor = 0.5; // 0x014F0E9C
        friendly = true;
    }
    if flags.friend_hit1_4 {
        factor *= 0.25; // 0x014EA14C
        friendly = true;
    }
    if flags.friend_hit0 {
        factor *= 0.0;
        friendly = true;
    }
    let vbit = KIND_BIT.get(vic.kind as usize).copied().unwrap_or(0);
    let ak = (att.kind as usize).min(8);
    if HIT_MASK[0][ak] & vbit != 0 {
        return Some(factor);
    }
    if friendly && HIT_MASK[1][ak] & vbit != 0 {
        return Some(factor);
    }
    if att.owner == vic.owner && flags.own_hit {
        return Some(factor);
    }
    None
}

/// The attack flags the pair filter and the history read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AttackFlags {
    pub own_hit: bool,
    pub pl_no_hit: bool,
    pub em_no_hit: bool,
    pub etc_no_hit: bool,
    pub npc_no_hit: bool,
    pub friend_hit1: bool,
    pub friend_hit1_2: bool,
    pub friend_hit1_4: bool,
    pub friend_hit0: bool,
    pub multi_hit: bool,
}
impl AttackFlags {
    pub fn from_attack(a: &crate::ocl::Attack) -> Self {
        let b = |n: &str| a.flag(n).unwrap_or(false);
        AttackFlags {
            own_hit: b("mIsOwnHit"),
            pl_no_hit: b("mIsPlNoHit"),
            em_no_hit: b("mIsEmNoHit"),
            etc_no_hit: b("mIsEtcNoHit"),
            npc_no_hit: b("mIsNpcNoHit"),
            friend_hit1: b("mIsFriendHitDamage1"),
            friend_hit1_2: b("mIsFriendHitDamage1_2"),
            friend_hit1_4: b("mIsFriendHitDamage1_4"),
            friend_hit0: b("mIsFriendHitDamage0"),
            multi_hit: b("mMultiHit"),
        }
    }
}

// ------------------------------------------------------- active volumes

/// An attack volume live this frame (one node of an activated sequence).
#[derive(Debug, Clone, PartialEq)]
pub struct AttackVolume {
    pub side: Side,
    pub volume: Volume,
    pub row: crate::damage::AttackRow,
    pub flags: AttackFlags,
    /// `mResetTime` (frames) for multi-hit attacks.
    pub reset_time: f32,
    /// Entry +0x178: `(SeqIndex.mNo << 8 | Group.mNo) << 16` (the low 16
    /// bits, entry +0x184, are 0 for entries built by 0x007735C0).
    pub key: u32,
    /// `SeqIndex.mRelation`.
    pub relation: u32,
}

/// A hurt volume (`mIsDamage` group node) of a victim.
#[derive(Debug, Clone, PartialEq)]
pub struct HurtVolume {
    pub side: Side,
    pub volume: Volume,
    pub region_id: u32,
    pub hit_prio: u32,
}

/// The volumes an activation switches on (0x0076F660 .. 0x0076FAB0): the
/// sequence's group nodes, with the attack from the body file's table or,
/// when that slot is empty, from the paired attack file. Returns the group
/// and the attack index used, or None when the sequence or group is
/// missing.
pub fn activated<'a>(body: &'a ObjCollision, attacks: Option<&'a ObjCollision>, act: &Activation) -> Option<(&'a Group, Option<&'a crate::ocl::Attack>, u32, u32)> {
    let seq = body.seq_index.get(act.seq as usize)?;
    let group = body.groups.get(seq.group_no as usize)?;
    let attack = body
        .attacks
        .get(seq.attack_no as usize)
        .and_then(|a| a.as_ref())
        .or_else(|| attacks.and_then(|f| f.attacks.get(seq.attack_no as usize)).and_then(|a| a.as_ref()));
    let key = ((seq.no << 8) | group.no) << 16;
    Some((group, attack, key, seq.relation))
}

/// Attack volumes of one model for one frame, from its motion activations.
pub fn attack_volumes(body: &ObjCollision, attacks: Option<&ObjCollision>, acts: &[Activation], pose: &dyn Pose, scale: f32, owner: u32) -> Vec<AttackVolume> {
    let mut out = Vec::new();
    for act in acts {
        if act.resource_id != body.resource_id {
            continue;
        }
        let Some((group, Some(attack), key, relation)) = activated(body, attacks, act) else { continue };
        if !group.is_attack {
            continue;
        }
        for node in &group.nodes {
            if let Some(volume) = build_volume(node, pose, scale) {
                out.push(AttackVolume {
                    side: Side { owner, kind: group.kind },
                    volume,
                    row: crate::damage::AttackRow::from_attack(attack),
                    flags: AttackFlags::from_attack(attack),
                    reset_time: attack.value("mResetTime").map(|v| v.as_f32()).unwrap_or(0.0),
                    key,
                    relation,
                });
            }
        }
    }
    out
}

/// Hurt volumes of one model for one frame.
pub fn hurt_volumes(body: &ObjCollision, acts: &[Activation], pose: &dyn Pose, scale: f32, owner: u32) -> Vec<HurtVolume> {
    let mut out = Vec::new();
    for act in acts {
        if act.resource_id != body.resource_id {
            continue;
        }
        let Some((group, _, _, _)) = activated(body, None, act) else { continue };
        if !group.is_damage {
            continue;
        }
        for node in &group.nodes {
            if let Some(volume) = build_volume(node, pose, scale) {
                out.push(HurtVolume { side: Side { owner, kind: group.kind }, volume, region_id: node.region_id, hit_prio: node.hit_prio });
            }
        }
    }
    out
}

// ------------------------------------------------------- dedup & history

/// One accepted attack-vs-hurt contact.
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    pub attack: usize,
    pub hurt: usize,
    pub factor: f32,
}

/// Overlapping, filter-passing pairs of one frame, deduplicated as
/// 0x00779860 does: per attacker owner, contacts with the same key or the
/// same non-zero `mRelation` collapse to one, the hurt node with the lower
/// `mHitPrio` winning (a later one replaces an earlier one only when its
/// priority value is strictly lower).
pub fn contacts(attacks: &[AttackVolume], hurts: &[HurtVolume]) -> Vec<Contact> {
    let mut out: Vec<Contact> = Vec::new();
    for (ai, a) in attacks.iter().enumerate() {
        for (hi, h) in hurts.iter().enumerate() {
            if !overlap(&a.volume, &h.volume) {
                continue;
            }
            let Some(factor) = pair_filter(&a.flags, a.side, h.side) else { continue };
            let c = Contact { attack: ai, hurt: hi, factor };
            let dup = out.iter_mut().find(|o| {
                let oa = &attacks[o.attack];
                oa.side.owner == a.side.owner
                    && hurts[o.hurt].side.owner == h.side.owner
                    && ((a.relation != 0 && oa.relation == a.relation) || oa.key == a.key)
            });
            match dup {
                Some(o) => {
                    if h.hit_prio < hurts[o.hurt].hit_prio {
                        *o = c;
                    }
                }
                None => out.push(c),
            }
        }
    }
    out
}

/// One remembered hit on a victim (`cObjCollision`+0xC04 entries).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub attacker: u32,
    pub key: u32,
    pub relation: u32,
    /// Some(frames left) for `mMultiHit` attacks (`mResetTime`).
    pub timer: Option<f32>,
}

/// A victim's hit history.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HitHistory {
    pub entries: Vec<HistoryEntry>,
    /// This frame's contacts (`cObjCollision`+0xBF0, at most 0x40).
    pub frame: Vec<HistoryEntry>,
}

impl HitHistory {
    /// Translated from 0x0076EF10: at the start of the victim's frame the
    /// contact list is cleared and multi-hit entries count down by the
    /// attacker's step; they are dropped at <= 0.
    pub fn begin_frame(&mut self, dt: f32) {
        self.frame.clear();
        self.entries.retain_mut(|e| match e.timer.as_mut() {
            Some(t) => {
                *t -= dt;
                *t > 0.0
            }
            None => true,
        });
    }

    /// Translated from 0x0076EAF0: true if this attacker already hit with
    /// this key (or with the same non-zero `mRelation` and the same low 16
    /// key bits).
    pub fn already_hit(&self, attacker: u32, key: u32, relation: u32) -> bool {
        let mut r = false;
        for e in &self.entries {
            if e.attacker == attacker && (e.key ^ key) & 0xFFFF == 0 {
                if relation != 0 && e.relation == relation {
                    r = true;
                }
                if (e.key ^ key) & 0xFFFF_0000 == 0 {
                    return true;
                }
            }
        }
        r
    }

    /// Translated from 0x0076EB60: remember a contact of this frame.
    pub fn record(&mut self, attacker: u32, key: u32, relation: u32, multi_hit: bool, reset_time: f32) {
        if self.frame.iter().any(|e| e.attacker == attacker && e.key == key && e.relation == relation) {
            return;
        }
        if self.frame.len() < 0x40 {
            self.frame.push(HistoryEntry { attacker, key, relation, timer: multi_hit.then_some(reset_time) });
        }
    }

    /// Translated from 0x0076ECA0 (end of the victim's frame). A plain
    /// entry survives only while the same contact repeats every frame,
    /// unless it has a non-zero `mRelation` (kept while the attacker's
    /// flag +0x1EC1 is clear: `relation_hold`); timed entries survive until
    /// their timer runs out. This frame's new contacts are then added.
    pub fn end_frame(&mut self, relation_hold: impl Fn(u32) -> bool) {
        let frame = &self.frame;
        self.entries.retain(|e| {
            e.timer.is_some()
                || (e.relation != 0 && relation_hold(e.attacker))
                || frame.iter().any(|c| c.attacker == e.attacker && c.key == e.key && c.relation == e.relation)
        });
        let mut fresh = Vec::new();
        for c in self.frame.drain(..) {
            let mut c = c;
            let mut dup = false;
            for e in &self.entries {
                if e.relation != 0 && e.relation == c.relation {
                    c.timer = e.timer;
                }
                if e.attacker == c.attacker && e.key == c.key && e.relation == c.relation {
                    dup = true;
                }
            }
            if !dup {
                fresh.push(c);
            }
        }
        self.entries.extend(fresh);
    }
}

/// One frame of hit detection against one victim: contacts, the history
/// check, and the contacts recorded. Returns the indices of the attack
/// volumes that deal damage this frame, with the hurt region they hit.
pub fn resolve_frame(attacks: &[AttackVolume], hurts: &[HurtVolume], history: &mut HitHistory, dt: f32) -> Vec<(usize, u32)> {
    history.begin_frame(dt);
    let mut hits = Vec::new();
    for c in contacts(attacks, hurts) {
        let a = &attacks[c.attack];
        if !history.already_hit(a.side.owner, a.key, a.relation) {
            hits.push((c.attack, hurts[c.hurt].region_id));
        }
        history.record(a.side.owner, a.key, a.relation, a.flags.multi_hit, a.reset_time);
    }
    history.end_frame(|_| true);
    hits
}
