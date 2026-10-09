//! Body morphs: the human-edit controller of a character model, from the
//! edit record to per-joint scale and offset.
//!
//! The chain, all DDDA.exe 2017-12-13 Steam:
//! 1. rHumanEdit (loader 0x00cbb0e0): nine floats, a flag and 13 slots of
//!    (scale vec3, offset vec3). NPC edits ship it; a save character's is
//!    built from presets `edit\parts\<bodyall|body|head|arm|leg>\..._NNN`
//!    (rHumanPartsEdit, 0x00cbcaa0) by 0x00780fa0 with 0x00cbc750 /
//!    0x00cbc810.
//! 2. The controller: slot set-up 0x0081b0d0 (rest lengths per slot, table
//!    0x01832aa0; 33 body entries, table 0x017ed8f8), the edit copied in by
//!    0x0081be30, the slot clamps 0x0081c6c0, the entries 0x0081cff0 and
//!    the joints 0x0081db50 (uCnsEdit scale +0x80, child scale +0xc0,
//!    offset +0x90; setters 0x0081a7c0, 0x0081e630, 0x0081e590). The
//!    controller fields are kept at their byte offsets so the translation
//!    reads like the original.
//! 3. The joints chosen by 0x0081c330: ids 1-20 except 5 and 9, and 0x42,
//!    0x44, 0x90, 0x96, 0x99. uCnsEdit::vf24 0x00860ac0 applies each as
//!    `local' = C x local` (row vectors): C scales the joint's axes and
//!    adds the offset in the joint's own space. Types 0xb (ids 2, 3, 4, 6,
//!    10; 0x0086d180) also take the child scale. Type 9 (8, 12;
//!    0x00867b70) cancels inherited axis length; type 0xc (0x96, 0x99;
//!    0x00868000) is approximated as a plain scale.

use crate::Error;

/// Human-edit hand constraints (joint IDs 8 and 12) have kind 9 and
/// ignore model scale (+0xb0 = 1), set by 0x0081c330. They cancel the
/// current world axis lengths before applying their authored scale.
/// Rotation is zero for these human-edit constraints. This is not the
/// general type-9 matrix rule for arbitrary rotated constraints.
// Translated from 0x00867b70 and 0x00860ac0 (DDDA.exe 2017-12-13 Steam).
pub fn hand_axis_factors(scale: [f32; 3], world_axis_lengths: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| {
        let length = world_axis_lengths[i];
        let compensation = if length > f32::EPSILON { 1.0 / length } else { 1.0 };
        compensation * scale[i]
    })
}

/// One rHumanEdit (version 0x13).
#[derive(Debug, Clone, PartialEq)]
pub struct HumanEdit {
    /// +0x68..+0x88 (controller +0xf70..+0xf90).
    pub floats: [f32; 9],
    /// +0x8c (controller +0xfa0): skip the slot clamps 0x0081c6c0.
    pub fixed: bool,
    /// (scale, offset) per slot.
    pub slots: Vec<([f32; 3], [f32; 3])>,
}

/// Nine skeleton measurements used to fill rHumanEdit +0x68..+0x88.
/// Positions must be the original model's evaluated world-space joints:
/// once without the edit, once after its constraint pass. Supplying rest
/// locals or a neutral pose does not reproduce that evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyMeasurements {
    values: [f32; 9],
}

impl BodyMeasurements {
    /// Translated from the paired measurement passes in 0x00cbbf00
    /// (DDDA.exe 2017-12-13 Steam). `origin_y` is model +0xe4.
    /// Joint IDs resolve through model +0x368, not file bone indices.
    pub fn from_pose(
        position: impl Fn(u8) -> Option<[f32; 3]>,
        origin_y: f32,
    ) -> Result<Self, Error> {
        let joint = |id| position(id).ok_or_else(|| Error::Data(format!("body measurement: missing joint {id}")));
        let (joint0, joint4, joint6, joint7, joint8, joint10, joint13, joint14, joint16, joint18) = (
            joint(0)?, joint(4)?, joint(6)?, joint(7)?, joint(8)?,
            joint(10)?, joint(13)?, joint(14)?, joint(16)?, joint(18)?,
        );
        let distance = |a: [f32; 3], b: [f32; 3]| {
            let x = a[0] - b[0];
            let y = a[1] - b[1];
            let z = a[2] - b[2];
            (x * x + y * y + z * z).sqrt()
        };
        Ok(Self { values: [
            joint4[1] - origin_y,
            distance(joint6, joint10),
            distance(joint14, joint18),
            joint13[1] - origin_y,
            joint16[1] - origin_y,
            distance(joint6, joint8),
            distance(joint6, joint7),
            distance(joint7, joint8),
            distance(joint0, joint8),
        ] })
    }

    /// Edited/reference ratios, in rHumanEdit field order. This translates
    /// 0x00cbc521..0x00cbc715, without clamping or replacing zero reference
    /// measurements with guessed values. Invalid poses return an error.
    pub fn relative_to(&self, reference: &Self) -> Result<[f32; 9], Error> {
        let mut ratios = [0.0; 9];
        for (i, ratio) in ratios.iter_mut().enumerate() {
            if !self.values[i].is_finite() || !reference.values[i].is_finite() || reference.values[i] == 0.0 {
                return Err(Error::Data(format!("body measurement: invalid reference/edited field {i}")));
            }
            *ratio = self.values[i] / reference.values[i];
        }
        Ok(ratios)
    }
}

fn rd_u32(b: &[u8], o: &mut usize) -> Result<u32, Error> {
    let v = b.get(*o..*o + 4).ok_or_else(|| Error::Data("short edit record".into()))?;
    *o += 4;
    Ok(u32::from_le_bytes(v.try_into().unwrap()))
}
fn rd_f32(b: &[u8], o: &mut usize) -> Result<f32, Error> {
    Ok(f32::from_bits(rd_u32(b, o)?))
}
fn rd_v3(b: &[u8], o: &mut usize) -> Result<[f32; 3], Error> {
    Ok([rd_f32(b, o)?, rd_f32(b, o)?, rd_f32(b, o)?])
}

impl HumanEdit {
    /// Neutral record: scale 1, offset 0, floats 1. The runtime constructor
    /// 0x0077d6f0 calls rHumanEdit::vf14 (0x00cbaed0), which resets all nine
    /// floats to1 and the fixed flag to0; presets overwrite their slots.
    pub fn neutral() -> HumanEdit {
        HumanEdit { floats: [1.0; 9], fixed: false, slots: vec![([1.0; 3], [0.0; 3]); 13] }
    }

    // Translated from rHumanEdit::vf10 0x00cbb0e0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<HumanEdit, Error> {
        let mut o = 0;
        if rd_u32(b, &mut o)? != 0x13 {
            return Err(Error::Data("rHumanEdit: version".into()));
        }
        let mut floats = [0.0; 9];
        for f in &mut floats {
            *f = rd_f32(b, &mut o)?;
        }
        let fixed = *b.get(o).ok_or_else(|| Error::Data("rHumanEdit: short".into()))? != 0;
        o += 1;
        o = (o + 3) & !3;
        let n = rd_u32(b, &mut o)? as usize;
        let mut slots = Vec::with_capacity(n);
        for _ in 0..n {
            slots.push((rd_v3(b, &mut o)?, rd_v3(b, &mut o)?));
        }
        Ok(HumanEdit { floats, fixed, slots })
    }

    /// Applies a preset (0x00cbc750): entry i goes to slot
    /// `PRESET_SLOTS[type][i]`, scale when the flag has bit 0, offset when
    /// bit 1. Without the preset file the slots reset (0x00cbc810).
    // Translated from 0x00cbc750 and 0x00cbc810 (DDDA.exe 2017-12-13 Steam).
    pub fn apply_preset(&mut self, part: usize, preset: Option<&HumanParts>) {
        let table = PRESET_SLOTS[part];
        for (i, &(slot, flags)) in table.iter().enumerate() {
            let s = &mut self.slots[slot as usize];
            let e = match preset {
                Some(p) => match p.entries.get(i) {
                    Some(e) => *e,
                    None => continue,
                },
                None => ([1.0; 3], [0.0; 3]),
            };
            if flags & 1 != 0 {
                s.0 = e.0;
            }
            if flags & 2 != 0 {
                s.1 = e.1;
            }
        }
    }
}

/// rHumanPartsEdit (version 0x43): a part type and entries.
#[derive(Debug, Clone, PartialEq)]
pub struct HumanParts {
    pub part: u32,
    pub entries: Vec<([f32; 3], [f32; 3])>,
}

impl HumanParts {
    // Translated from rHumanPartsEdit::vf10 0x00cbcaa0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<HumanParts, Error> {
        let mut o = 0;
        if rd_u32(b, &mut o)? != 0x43 {
            return Err(Error::Data("rHumanPartsEdit: version".into()));
        }
        let part = rd_u32(b, &mut o)?;
        let n = rd_u32(b, &mut o)? as usize;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            entries.push((rd_v3(b, &mut o)?, rd_v3(b, &mut o)?));
        }
        Ok(HumanParts { part, entries })
    }
}

/// Part names of the presets (0x0163af68) and the save field that picks
/// each (cSAVE_DATA_EDIT +0x3e..+0x42, read by 0x00780fa0).
pub const PRESET_PARTS: [(&str, &str); 5] = [
    ("bodyall", "mHumanBodyAll"),
    ("body", "mHumanBody"),
    ("head", "mHumanHead"),
    ("arm", "mHumanArm"),
    ("leg", "mHumanLeg"),
];

/// (slot, flags) per preset entry, by part (tables at 0x014e7c88.., counts
/// 0x014e7cac).
pub const PRESET_SLOTS: [&[(u8, u8)]; 5] = [
    &[(0, 2), (1, 2)],
    &[(0, 1), (5, 1), (2, 3), (3, 3), (12, 2), (6, 2), (1, 0)],
    &[(4, 1), (5, 2)],
    &[(6, 1), (8, 3)],
    &[(9, 3), (11, 3), (1, 1)],
];

/// Rest values per slot (table 0x01832aa0, 0x30 bytes each): a rest
/// length vector (+0x00), a reference position (+0x10) and a joint id
/// (+0x20, -1 none). Read by 0x0081b0d0.
pub const SLOT_INIT: [([f32; 3], [f32; 3], i16); 13] = [
    ([0.0, 180.0, 0.0], [0.0, 0.0, 0.0], -1),
    ([0.0, 111.0, 0.0], [0.0, 0.0, 0.0], -1),
    ([0.0, 41.0, 0.0], [0.0, 111.0, 0.0], 0),
    ([0.0, 31.0, 0.0], [0.0, 111.0, 0.0], 0),
    ([0.0, 10.0, 0.0], [0.0, 164.0, 0.0], 4),
    ([0.0, 5.0, 0.0], [0.0, 152.0, -2.0], 3),
    ([0.0, 0.0, 0.0], [-17.0, 149.0, -4.0], 6),
    ([-26.0, 0.0, 0.0], [-17.0, 149.0, -4.0], 6),
    ([-53.0, 0.0, 0.0], [-17.0, 149.0, -4.0], 6),
    ([-9.0, -13.0, 0.0], [0.0, 111.0, 0.0], 13),
    ([0.0, -44.0, 0.0], [-9.0, 98.0, 0.0], 14),
    ([0.0, -88.0, 0.0], [-9.0, 98.0, 0.0], 14),
    ([-6.5, 11.5, 15.0], [0.0, 126.0, 0.0], 2),
];

/// Per-joint result of the body morph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointMorph {
    /// uCnsEdit +0x80.
    pub scale: [f32; 3],
    /// uCnsEdit +0xc0 (child scale; types 0xb only).
    pub child: [f32; 3],
    /// uCnsEdit +0x90, cm in the joint's space.
    pub offset: [f32; 3],
    /// uCnsEdit +0xa4 (0 default, 9, 0xb, 0xc).
    pub kind: u32,
}

impl JointMorph {
    const IDENTITY: JointMorph = JointMorph { scale: [1.0; 3], child: [1.0; 3], offset: [0.0; 3], kind: 0 };

    /// The axis scale the joint ends with (uCnsEdit::vf24): scale x child
    /// for type 0xb, scale otherwise.
    pub fn axes(&self) -> [f32; 3] {
        if self.kind == 0xb {
            std::array::from_fn(|i| self.scale[i] * self.child[i])
        } else {
            self.scale
        }
    }
}

/// The body morph of one character.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyMorph {
    /// The model's scale (uModel +0x60), from body entry 0.
    pub model_scale: [f32; 3],
    /// By joint id.
    pub joints: Vec<(u8, JointMorph)>,
}

/// Joint ids 0x0081c330 makes a uCnsEdit for, with its type.
// Translated from 0x0081c330 (DDDA.exe 2017-12-13 Steam).
pub fn joint_kind(id: u8) -> Option<u32> {
    let chosen = (id < 0x15 && id != 0 && id != 5 && id != 9) || matches!(id, 0x42 | 0x44 | 0x90 | 0x96 | 0x99);
    if !chosen {
        return None;
    }
    Some(match id {
        2 | 3 | 4 | 6 | 10 => 0xb,
        8 | 12 => 9,
        0x96 | 0x99 => 0xc,
        _ => 0,
    })
}

/// The controller memory: floats at their byte offsets.
struct Ctrl {
    m: Vec<f32>,
}

impl Ctrl {
    fn new() -> Ctrl {
        Ctrl { m: vec![0.0; 0x1000 / 4] }
    }
    fn f(&self, o: usize) -> f32 {
        self.m[o / 4]
    }
    fn set(&mut self, o: usize, v: f32) {
        self.m[o / 4] = v;
    }
    fn v3(&self, o: usize) -> [f32; 3] {
        [self.f(o), self.f(o + 4), self.f(o + 8)]
    }
    fn set3(&mut self, o: usize, v: [f32; 3]) {
        self.set(o, v[0]);
        self.set(o + 4, v[1]);
        self.set(o + 8, v[2]);
    }
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    let mut r = v;
    if v < lo {
        r = lo;
    }
    if hi < v {
        r = hi;
    }
    r
}

/// Entry `k`'s scale (+0x10) and offset (+0x20) offsets.
const fn es(k: usize) -> usize {
    0x720 + k * 0x40 + 0x10
}
const fn et(k: usize) -> usize {
    0x720 + k * 0x40 + 0x20
}

/// Computes the body morph from a human edit. `present(id)` says whether
/// the model has the joint (0x0081c330 only makes uCnsEdits for joints the
/// model has).
pub fn body_morph(edit: &HumanEdit, present: &dyn Fn(u8) -> bool) -> BodyMorph {
    let mut c = Ctrl::new();
    // 0x0081b0d0: slot rest values; scales 1, offsets 0.
    for (k, (r, v, _)) in SLOT_INIT.iter().enumerate() {
        let b = 0x250 + k * 0x60;
        c.set3(b, *r);
        c.set3(b + 0x10, [1.0; 3]);
        c.set3(b + 0x20, [0.0; 3]);
        c.set3(b + 0x30, *v);
    }
    // Entries start neutral (*assumed*: their initial values were not
    // traced; every entry 0x0081db50 reads is either written by
    // 0x0081cff0 or kept neutral).
    for k in 0..33 {
        c.set3(es(k), [1.0; 3]);
        c.set3(et(k), [0.0; 3]);
    }
    // 0x0081be30: the edit into the slots and floats.
    for (k, (s, o)) in edit.slots.iter().take(13).enumerate() {
        c.set3(0x260 + k * 0x60, *s);
        c.set3(0x270 + k * 0x60, *o);
    }
    for (i, f) in edit.floats.iter().enumerate() {
        c.set(0xf70 + i * 4, *f);
    }
    let f84 = c.f(0xf84);
    c.set(0xf88, f84);
    c.set(0xf8c, f84);
    if !edit.fixed {
        clamps(&mut c);
    }
    entries(&mut c);
    joints(&c, present)
}

// Translated from 0x0081c6c0 (DDDA.exe 2017-12-13 Steam).
fn clamps(c: &mut Ctrl) {
    let s = (c.f(0x274) + c.f(0x254)) * 0.005_555_555_7;
    let (lo8, hi14) = (s * 0.8, s * 1.4);
    let x0 = clamp(c.f(0x260), lo8, hi14);
    let z0 = clamp(c.f(0x268), lo8, hi14);
    c.set(0x260, x0);
    c.set(0x268, z0);
    c.set(0x264, 1.0);
    let (ix, iz) = (1.0 / x0, 1.0 / z0);
    let a = clamp(c.f(0x2c0) * x0, lo8, hi14);
    let b = clamp(c.f(0x2c8) * z0, lo8, hi14);
    c.set(0x2c0, ix * a);
    c.set(0x2c8, iz * b);
    let (lo75, hi17) = (s * 0.75, s * 1.7);
    let x2 = ix * clamp(c.f(0x320), lo75, hi17);
    let z2 = iz * clamp(c.f(0x328), lo75, hi17);
    c.set(0x320, x2);
    c.set(0x328, z2);
    let (lo7, hi16, lo6) = (s * 0.7, s * 1.6, s * 0.6);
    let a = clamp(c.f(0x380) * x2, lo7, hi16);
    let b = clamp(c.f(0x388) * z2, lo6, hi16);
    c.set(0x380, a / x2);
    c.set(0x388, b / z2);
    // 0x440: x keeps hi14 when above it, else at least lo6.
    let (lo65, hi13) = (s * 0.65, s * 1.3);
    let v = c.f(0x440);
    c.set(0x440, if v <= hi14 { v.max(lo6) } else { hi14 });
    let v = c.f(0x448);
    c.set(0x448, if v <= hi13 { v.max(lo65) } else { hi13 });
    let (lo9, hi115) = (s * 0.9, s * 1.15);
    for o in [0x3e0, 0x3e4, 0x3e8] {
        let v = clamp(c.f(o), lo9, hi115);
        c.set(o, v);
    }
    // 0x4a0: DAT_01876478 = 1.5.
    let hi15 = s * 1.5;
    let y = c.f(0x4a4) * c.f(0x324);
    let z = c.f(0x4a8) * c.f(0x328);
    let y = if y <= hi15 { y.max(lo7) } else { hi15 };
    let z = if z <= hi15 { z.max(lo7) } else { hi15 };
    c.set(0x4a4, y / c.f(0x324));
    c.set(0x4a8, z / c.f(0x328));
    let x8 = clamp(c.f(0x560), lo8, hi16);
    let y8 = clamp(c.f(0x564), lo8, hi16);
    let v = c.f(0x568);
    let z8 = if v <= hi17 { v.max(lo8) } else { hi17 };
    c.set3(0x560, [x8, y8, z8]);
    let hi145 = s * 1.45;
    let x = c.f(0x5c0) * c.f(0x2c0);
    let z = c.f(0x5c8) * c.f(0x2c8);
    let x = if x <= hi145 { x.max(lo7) } else { hi145 };
    let z = if z <= hi145 { z.max(lo65) } else { hi145 };
    c.set(0x5c8, z / c.f(0x2c8));
    c.set(0x5c0, x / c.f(0x2c0));
    // Slot 10's offset: (0, slot 11's offset y / 2, 0).
    // The exe computes (88 - y x -1) x 0.5 - 44.
    let v = (88.0 + c.f(0x694)) * 0.5 - 44.0;
    c.set3(0x630, [0.0, v, 0.0]);
    let x = clamp(c.f(0x680) * c.f(0x5c0), lo7, s * 1.35);
    let z = clamp(c.f(0x688) * c.f(0x5c8), lo7, hi13);
    c.set(0x680, x / c.f(0x5c0));
    c.set(0x688, z / c.f(0x5c8));
    let x = clamp(c.f(0x6e0), lo7, hi16);
    let z = clamp(c.f(0x6e8), lo6, hi16);
    c.set(0x6e0, x);
    c.set(0x6e8, z);
}

// Translated from 0x0081cff0 (DDDA.exe 2017-12-13 Steam). Entry k has
// kind k (table 0x017ed8f8), so "the entry of kind k" is entry k.
fn entries(c: &mut Ctrl) {
    let h0 = (c.f(0x274) + c.f(0x254)) * 0.005_555_555_7;
    let h1 = c.f(0x2d4) + c.f(0x2b4);
    c.set3(es(0), [c.f(0x260), h0 * h1 * 0.009_009_009, c.f(0x268)]);
    c.set3(es(2), [c.f(0x320), (c.f(0x334) + c.f(0x314)) * 0.024_390_243 * (111.0 / h1), c.f(0x328)]);
    c.set3(es(3), [c.f(0x380), 1.0, c.f(0x388)]);
    c.set3(es(4), [1.0; 3]);
    c.set3(es(5), [c.f(0x440), (c.f(0x454) + c.f(0x434)) * 0.2, c.f(0x448)]);
    c.set3(es(6), [1.0; 3]);
    let v = c.f(0x3e0);
    c.set3(es(7), [v; 3]);
    c.set3(et(9), [-((c.f(0x320) - 1.0) * 2.0), 0.0, 0.0]);
    c.set3(es(9), [1.0; 3]);
    let fore = (c.f(0x570) + c.f(0x550)) * -0.018_867_925;
    c.set3(es(10), [fore, 1.0, 1.0]);
    c.set3(et(11), c.v3(0x4b0));
    c.set3(es(11), [1.0, c.f(0x4a4), c.f(0x4a8)]);
    let s11 = c.f(es(11));
    c.set3(es(12), [1.0 / (s11 * s11), 1.0, 1.0]);
    c.set3(es(13), c.v3(0x560));
    c.set3(et(15), [(c.f(0x320) - 1.0) * 2.0, 0.0, 0.0]);
    c.set3(es(15), [1.0; 3]);
    c.set3(es(16), [fore, 1.0, 1.0]);
    c.set3(et(17), [-c.f(0x4b0), c.f(0x4b4), c.f(0x4b8)]);
    c.set3(es(17), [1.0, c.f(0x4a4), c.f(0x4a8)]);
    let s17 = c.f(es(17));
    c.set3(es(18), [1.0 / (s17 * s17), 1.0, 1.0]);
    c.set3(es(19), c.v3(0x560));
    c.set3(es(20), [c.f(0x2c0), 1.0, c.f(0x2c8)]);
    let hip = c.f(0x5d4) + c.f(0x5b4);
    c.set3(es(21), [1.0, hip * -0.076_923_08, 1.0]);
    let thigh = (hip + c.f(0x5e4)) * 0.010_204_081;
    c.set3(et(22), [c.f(0x5d0), 0.0, 0.0]);
    c.set3(es(22), [c.f(0x5c0), thigh, c.f(0x5c8)]);
    let knee = (c.f(0x634) + c.f(0x614)) * -0.022_727_273;
    c.set3(es(23), [1.0, knee, 1.0]);
    c.set3(es(24), [1.0; 3]);
    let shin = (c.f(0x674) + c.f(0x694) + c.f(0x6a4)) * 0.1;
    c.set3(es(25), [c.f(0x680), shin, c.f(0x688)]);
    c.set3(et(26), [-c.f(0x5d0), 0.0, 0.0]);
    c.set3(es(26), [c.f(0x5c0), thigh, c.f(0x5c8)]);
    c.set3(es(27), [1.0, knee, 1.0]);
    c.set3(es(28), [1.0; 3]);
    c.set3(es(29), [c.f(0x680), shin, c.f(0x688)]);
    // +0xfa4 / +0xfa8: controller fields not written by the traced chain
    // (0 here, *assumed*).
    let z = (c.f(0x6f8) + c.f(0x6d8)) - 15.0 + 1.0;
    c.set3(et(30), [c.f(0x6f0) + c.f(0xfa4), c.f(0x6f4) + c.f(0xfa8), z]);
    c.set3(et(31), [-c.f(0x6f0) - c.f(0xfa4), c.f(0x6f4) + c.f(0xfa8), z]);
    c.set3(es(32), c.v3(0x440));
}

// Translated from 0x0081db50 (DDDA.exe 2017-12-13 Steam), with the setters
// 0x0081a7c0 (scale, kept unless not (1,1,1); components below
// 1.1920929e-7 in size become that), 0x0081e630 (child scale) and
// 0x0081e590 (offset, kept unless not 0).
fn joints(c: &Ctrl, present: &dyn Fn(u8) -> bool) -> BodyMorph {
    let mut out: Vec<(u8, JointMorph)> = vec![];
    fn slot<'a>(out: &'a mut Vec<(u8, JointMorph)>, present: &dyn Fn(u8) -> bool, id: u8) -> Option<&'a mut JointMorph> {
        let kind = joint_kind(id)?;
        if !present(id) {
            return None;
        }
        if let Some(i) = out.iter().position(|(j, _)| *j == id) {
            return Some(&mut out[i].1);
        }
        out.push((id, JointMorph { kind, ..JointMorph::IDENTITY }));
        Some(&mut out.last_mut().unwrap().1)
    }
    let scale = |out: &mut Vec<(u8, JointMorph)>, id: u8, v: [f32; 3]| {
        if v != [1.0; 3] {
            if let Some(j) = slot(out, present, id) {
                j.scale = v.map(|x| if x.abs() < 1.192_092_9e-7 { 1.192_092_9e-7 } else { x });
            }
        }
    };
    let child = |out: &mut Vec<(u8, JointMorph)>, id: u8, v: [f32; 3]| {
        if v != [1.0; 3] {
            if let Some(j) = slot(out, present, id) {
                j.child = v;
            }
        }
    };
    let offset = |out: &mut Vec<(u8, JointMorph)>, id: u8, v: [f32; 3]| {
        if v != [0.0; 3] {
            if let Some(j) = slot(out, present, id) {
                j.offset = v;
            }
        }
    };
    let e = |k: usize| c.v3(es(k));
    let t = |k: usize| c.v3(et(k));
    let mul = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| a[i] * b[i]) };
    let div = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| a[i] / b[i]) };
    let add = |a: [f32; 3], b: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| a[i] + b[i]) };
    let model_scale = e(0);
    let sy0 = c.f(0x734);
    scale(&mut out, 1, e(2));
    scale(&mut out, 2, e(3));
    child(&mut out, 3, [1.0; 3]);
    scale(&mut out, 3, e(5));
    child(&mut out, 3, e(4));
    scale(&mut out, 4, e(7));
    child(&mut out, 4, e(6));
    offset(&mut out, 6, add(t(11), t(9)));
    let (a, b) = (e(11), e(10));
    scale(&mut out, 6, [a[0] * b[0] * sy0, a[1] * b[1], a[2] * b[2]]);
    child(&mut out, 6, e(9));
    scale(&mut out, 7, e(12));
    scale(&mut out, 8, e(13));
    offset(&mut out, 10, add(t(17), t(15)));
    let (a, b) = (e(17), e(16));
    scale(&mut out, 10, [a[0] * b[0] * sy0, a[1] * b[1], a[2] * b[2]]);
    child(&mut out, 10, e(15));
    scale(&mut out, 11, e(18));
    scale(&mut out, 12, e(19));
    let (a, b) = (e(21), e(20));
    scale(&mut out, 13, [a[0] * b[0] / model_scale[0], a[1] * b[1], a[2] * b[2] / model_scale[2]]);
    offset(&mut out, 14, t(23));
    scale(&mut out, 14, mul(e(23), div(e(22), e(21))));
    scale(&mut out, 15, e(24));
    let s15 = out.iter().find(|(j, _)| *j == 15).map_or([1.0; 3], |(_, m)| m.scale);
    scale(&mut out, 16, div(e(25), s15));
    offset(&mut out, 18, t(27));
    scale(&mut out, 18, mul(e(27), div(e(26), e(21))));
    scale(&mut out, 19, e(28));
    let s19 = out.iter().find(|(j, _)| *j == 19).map_or([1.0; 3], |(_, m)| m.scale);
    scale(&mut out, 20, div(e(29), s19));
    offset(&mut out, 0x42, t(30));
    offset(&mut out, 0x44, t(31));
    scale(&mut out, 0x90, e(32));
    // 0x01530d5c = 0.2.
    let bust = (c.f(0xf90) - 1.0) * 0.2 + 1.0;
    scale(&mut out, 0x96, [bust; 3]);
    scale(&mut out, 0x99, [bust; 3]);
    out.sort_by_key(|(j, _)| *j);
    BodyMorph { model_scale, joints: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_size_cancels_nonuniform_inherited_scale() {
        let inherited = [2.0, 0.5, 4.0];
        let authored = [1.25, 0.75, 1.5];
        let factors = hand_axis_factors(authored, inherited);
        assert_eq!(std::array::from_fn::<_, 3, _>(|i| factors[i] * inherited[i]), authored);
        assert_ne!(std::array::from_fn::<_, 3, _>(|i| authored[i] * inherited[i]), authored);
    }

    #[test]
    fn hand_axis_cancellation_preserves_source_degenerate_boundary() {
        let authored = [2.0, 3.0, 4.0];
        assert_eq!(hand_axis_factors(authored, [0.0, f32::EPSILON, f32::EPSILON / 2.0]), authored);
        assert_eq!(hand_axis_factors([1.0; 3], [2.0 * f32::EPSILON; 3]),
            [1.0 / (2.0 * f32::EPSILON); 3]);
    }

    #[test]
    fn measurements_use_world_joint_ids_and_preserve_height_distance_order() {
        let pose = |id: u8| Some([f32::from(id), 2.0 * f32::from(id) + 7.0, 3.0 * f32::from(id)]);
        let base = BodyMeasurements::from_pose(pose, 7.0).unwrap();
        let length = 14.0_f32.sqrt();
        let expected = [8.0, 4.0 * length, 4.0 * length, 26.0, 32.0,
            2.0 * length, length, length, 8.0 * length];
        for (actual, expected) in base.values.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-5);
        }
        assert_eq!(base.relative_to(&base).unwrap(), [1.0; 9]);
        let edited = BodyMeasurements::from_pose(|id| {
            let [x, y, z] = pose(id).unwrap();
            Some([x * 2.0 + 90.0, (y - 7.0) * 3.0 - 11.0, z * 4.0 - 80.0])
        }, -11.0).unwrap();
        let ratios = edited.relative_to(&base).unwrap();
        let distance_ratio = 184.0_f32.sqrt() / length;
        for (index, ratio) in ratios.into_iter().enumerate() {
            let expected = if matches!(index, 0 | 3 | 4) { 3.0 } else { distance_ratio };
            assert!((ratio - expected).abs() < 1e-5, "field {index}: {ratio}");
        }
    }

    #[test]
    fn measurements_reject_missing_or_degenerate_pose_without_neutral_fallback() {
        assert!(BodyMeasurements::from_pose(|_| None, 0.0).is_err());
        let zero = BodyMeasurements::from_pose(|_| Some([0.0; 3]), 0.0).unwrap();
        assert!(zero.relative_to(&zero).is_err());
        let bad = BodyMeasurements { values: [f32::NAN; 9] };
        assert!(bad.relative_to(&BodyMeasurements { values: [1.0; 9] }).is_err());
        let neutral = HumanEdit::neutral();
        assert_eq!(neutral.floats, [1.0; 9]);
        assert!(!neutral.fixed);
        assert_eq!(neutral.slots, vec![([1.0; 3], [0.0; 3]); 13]);
    }

    #[test]
    fn neutral_edit_is_identity() {
        let m = body_morph(&HumanEdit::neutral(), &|_| true);
        assert_eq!(m.model_scale, [1.0; 3]);
        // A neutral edit leaves the joints neutral, up to float rounding of
        // 41 x (1/41) on joint 1, except the chest offsets of 0x42 / 0x44:
        // z = (offset z + rest 15) - 15 + 1 = 1 cm (0x0081cff0).
        for (id, j) in &m.joints {
            match id {
                0x42 | 0x44 => assert_eq!(j.offset, [if *id == 0x44 { -0.0 } else { 0.0 }, 0.0, 1.0]),
                _ => assert!(j.scale.iter().all(|s| (s - 1.0).abs() < 1e-6) && j.offset == [0.0; 3], "{id}: {j:?}"),
            }
        }
    }

    #[test]
    fn height_and_clamps() {
        // Slot 0 offset y +18 cm: height factor (180 + 18) / 180 = 1.1 on
        // model scale y (with slot 1 neutral: (111 + 0) / 111 = 1).
        let mut e = HumanEdit::neutral();
        e.slots[0].1 = [0.0, 18.0, 0.0];
        let m = body_morph(&e, &|_| true);
        assert!((m.model_scale[1] - 1.1).abs() < 1e-5, "{:?}", m.model_scale);
        // Width 2.0 is clamped to 1.4 x the height factor (0x0081c6c0).
        e.slots[0].0 = [2.0, 1.0, 0.5];
        let m = body_morph(&e, &|_| true);
        assert!((m.model_scale[0] - 1.1 * 1.4).abs() < 1e-5);
        assert!((m.model_scale[2] - 1.1 * 0.8).abs() < 1e-5);
        // The fixed flag skips the clamps.
        e.fixed = true;
        let m = body_morph(&e, &|_| true);
        assert_eq!(m.model_scale[0], 2.0);
    }

    #[test]
    fn arm_length_reaches_the_forearm() {
        // Slot 8 offset x -5.3 cm on a -53 cm rest: entry 10 scale x
        // (-53 - 5.3) / -53 = 1.1, joint 6 scale x (with entries 11 neutral).
        let mut e = HumanEdit::neutral();
        e.slots[8].1 = [-5.3, 0.0, 0.0];
        e.fixed = true;
        let m = body_morph(&e, &|_| true);
        let j6 = m.joints.iter().find(|(j, _)| *j == 6).unwrap().1;
        assert!((j6.scale[0] - 1.1).abs() < 1e-5, "{j6:?}");
        assert_eq!(j6.kind, 0xb);
        // Joints the model lacks get nothing.
        let m = body_morph(&e, &|id| id != 6);
        assert!(m.joints.iter().all(|(j, _)| *j != 6));
    }

    #[test]
    fn presets_fill_their_slots() {
        let mut e = HumanEdit::neutral();
        let p = HumanParts { part: 3, entries: vec![([1.1, 1.2, 1.3], [9.0; 3]), ([0.9; 3], [1.0, 2.0, 3.0])] };
        e.apply_preset(3, Some(&p));
        // arm: entry 0 -> slot 6 scale only; entry 1 -> slot 8 both.
        assert_eq!(e.slots[6], ([1.1, 1.2, 1.3], [0.0; 3]));
        assert_eq!(e.slots[8], ([0.9; 3], [1.0, 2.0, 3.0]));
        e.apply_preset(3, None);
        assert_eq!(e.slots[8], ([1.0; 3], [0.0; 3]));
    }
}
