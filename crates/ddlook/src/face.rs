//! Face morphs: the face-edit controller of a character model (cFaceEdit,
//! 0x4660 bytes), from the edit record to per-joint offset, scale and
//! rotation.
//!
//! The chain, all DDDA.exe 2017-12-13 Steam:
//! 1. rFaceEdit (loader 0x00cb90e0): 101 vectors, one per marker. NPC edits
//!    ship it; a save character's is built from six presets
//!    `edit\parts\<base|eyeblow|eye|nose|mouth|ear>\..._NNN`
//!    (rFacePartsEdit) by 0x00780fa0 with 0x00cb9280 / 0x00cb9320.
//! 2. The controller: 101 markers (+0x280, 0x50 each: +4 kind, +0x10 rest
//!    position, +0x20 edit value, +0x30 adjustment; table 0x01831df0) and
//!    116 joint controllers (+0x2210, 0x50 each: +4 joint id, +0x10 offset,
//!    +0x20 scale, +0x30 rotation, +0x40 parent controller; table
//!    0x017edc70), set up by 0x00811780 / 0x00811a90. A marker reads as an
//!    offset (kind 0, 0x00811390), a scale (kind 1, 1 + ..., 0x00811410) or
//!    a rotation (kind 2, 0x00811490): `default + value + adjustment`.
//! 3. The update 0x00812820: 0x00812fb0 and 0x00815c60
//!    ([`crate::face_controllers`]), then 0x0081a370 sums each controller
//!    with its parents (offset and rotation add, scale multiplies) and sets
//!    the joint's uCnsEdit (offset +0x90 0x0081e590, scale +0x80
//!    0x0081a7c0, rotation +0x70 in degrees 0x0081a700; each skipped at its
//!    default).
//! 4. The joints: uCnsEdit records for ids 0x50-0x95 and 0xd2-0xfe
//!    (0x00812c80). uCnsEdit::vf24 0x00860ac0 type 0 builds
//!    `C = diag(scale) x Rx x Ry x Rz` (row vectors) with the offset as its
//!    translation row and applies `W' = C x W` to the joint. The eye joints
//!    get other types (0x00812330): 0x51 and 0x58 type 0x10, 0x52-0x57 and
//!    0x59-0x5e type 0xd, 0xe2-0xe3 type 0xf, 0xe4-0xe9 type 0xe; those are
//!    not translated ([`special_kind`]).

use crate::Error;

/// Markers and controllers of the face controller.
pub const MARKERS: usize = 101;
pub const CONTROLLERS: usize = 116;

/// One rFaceEdit (version 0x13): a vector per marker.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEdit {
    pub values: Vec<[f32; 3]>,
}

fn rd_u32(b: &[u8], o: &mut usize) -> Result<u32, Error> {
    let v = b.get(*o..*o + 4).ok_or_else(|| Error::Data("short face edit record".into()))?;
    *o += 4;
    Ok(u32::from_le_bytes(v.try_into().unwrap()))
}
fn rd_v3(b: &[u8], o: &mut usize) -> Result<[f32; 3], Error> {
    Ok([f32::from_bits(rd_u32(b, o)?), f32::from_bits(rd_u32(b, o)?), f32::from_bits(rd_u32(b, o)?)])
}

impl FaceEdit {
    /// All zero: *assumed* state of the runtime record a save character's
    /// presets are written into (the presets cover the markers the face
    /// parts name; a missing preset zeroes its components, 0x00cb9320).
    pub fn neutral() -> FaceEdit {
        FaceEdit { values: vec![[0.0; 3]; MARKERS] }
    }

    // Translated from rFaceEdit::vf10 0x00cb90e0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<FaceEdit, Error> {
        let mut o = 0;
        if rd_u32(b, &mut o)? != 0x13 {
            return Err(Error::Data("rFaceEdit: version".into()));
        }
        let n = rd_u32(b, &mut o)? as usize;
        let mut values = Vec::with_capacity(n);
        for _ in 0..n {
            values.push(rd_v3(b, &mut o)?);
        }
        Ok(FaceEdit { values })
    }

    /// Applies a preset (0x00cb9280): entry i goes to marker
    /// `FACE_PRESET_SLOTS[part][i].0`, the components whose bit is set in
    /// `.1` (1 x, 2 y, 4 z). Without the preset file those components are
    /// zeroed (0x00cb9320).
    // Translated from 0x00cb9280 and 0x00cb9320 (DDDA.exe 2017-12-13 Steam).
    pub fn apply_preset(&mut self, part: usize, preset: Option<&FaceParts>) {
        for (i, &(slot, flags)) in FACE_PRESET_SLOTS[part].iter().enumerate() {
            let Some(d) = self.values.get_mut(slot as usize) else { continue };
            let e = match preset {
                Some(p) => match p.values.get(i) {
                    Some(e) => *e,
                    None => continue,
                },
                None => [0.0; 3],
            };
            for (k, x) in d.iter_mut().enumerate() {
                if flags >> k & 1 != 0 {
                    *x = e[k];
                }
            }
        }
    }
}

/// rFacePartsEdit (version 0x13): a part type and vectors. Layout read
/// from the files (every one parses to its length); consumer 0x00cb9280.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceParts {
    pub part: u32,
    pub values: Vec<[f32; 3]>,
}

impl FaceParts {
    pub fn parse(b: &[u8]) -> Result<FaceParts, Error> {
        let mut o = 0;
        if rd_u32(b, &mut o)? != 0x13 {
            return Err(Error::Data("rFacePartsEdit: version".into()));
        }
        let part = rd_u32(b, &mut o)?;
        let n = rd_u32(b, &mut o)? as usize;
        let mut values = Vec::with_capacity(n);
        for _ in 0..n {
            values.push(rd_v3(b, &mut o)?);
        }
        if o != b.len() {
            return Err(Error::Data("rFacePartsEdit: trailing bytes".into()));
        }
        Ok(FaceParts { part, values })
    }
}

/// Part names (0x0163af94) and the save field that picks each
/// (cSAVE_DATA_EDIT +0x2b..+0x30, read by 0x00780fa0). The preset number
/// is the field plus 100 for a female unless mHumanBodyAll >= 200, as for
/// the body presets.
pub const FACE_PRESET_PARTS: [(&str, &str); 6] = [
    ("base", "mFaceBase"),
    ("eyeblow", "mFaceEyebrow"),
    ("eye", "mFaceEye"),
    ("nose", "mFaceNose"),
    ("mouth", "mFaceMouth"),
    ("ear", "mFaceEar"),
];

/// (marker, component bits) per preset entry, by part (tables at
/// 0x0163af7c, counts 0x014e7d94).
pub const FACE_PRESET_SLOTS: [&[(u8, u8)]; 6] = [
    &[
        (0, 7),
        (48, 7),
        (49, 7),
        (57, 7),
        (58, 7),
        (70, 7),
        (71, 7),
        (74, 7),
        (76, 7),
        (77, 7),
        (88, 7),
        (89, 7),
        (1, 7),
        (75, 7),
        (50, 7),
        (100, 7),
        (34, 1),
        (39, 1),
        (6, 4),
        (20, 4),
        (59, 4),
        (56, 4),
    ],
    &[(2, 7), (4, 7), (3, 7), (5, 7), (78, 7), (79, 7), (3, 7), (5, 7), (90, 7), (91, 7), (94, 7), (95, 7), (96, 7), (97, 7)],
    &[
        (7, 7),
        (21, 7),
        (8, 7),
        (22, 7),
        (9, 7),
        (23, 7),
        (10, 7),
        (24, 7),
        (12, 7),
        (26, 7),
        (11, 7),
        (25, 7),
        (6, 3),
        (20, 3),
        (82, 7),
        (83, 7),
        (15, 7),
        (29, 7),
        (14, 7),
        (28, 7),
        (16, 7),
        (30, 7),
        (13, 7),
        (27, 7),
        (17, 7),
        (31, 7),
        (19, 7),
        (33, 7),
        (18, 7),
        (32, 7),
    ],
    &[(44, 7), (46, 7), (51, 7), (56, 3), (54, 7), (55, 7), (45, 7), (47, 7), (52, 7), (92, 7), (93, 7), (53, 7), (98, 7), (99, 7)],
    &[(60, 7), (61, 7), (62, 7), (63, 7), (59, 3), (64, 7), (65, 7), (67, 7), (68, 7), (66, 7), (86, 7), (87, 7), (84, 7), (85, 7), (69, 7)],
    &[(34, 6), (39, 6), (35, 7), (40, 7), (37, 7), (42, 7), (36, 7), (41, 7), (38, 7), (43, 7)],
];

/// The ten face parameters (controller +0x240, +0x244, +0x248, +0x250,
/// +0x254, +0x260, +0x264, +0x268, +0x270, +0x274). A save character's
/// come from its slider fields through the tables of 0x00780fa0 (stored
/// at edit info +0xf0..+0x110) and 0x0080bb10; NPCs keep the zeros of
/// 0x00811780.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FaceParams(pub [f32; 10]);

/// Slider tables of 0x00780fa0 (stack tables, five steps each).
const EYE_X: [f32; 5] = [-0.3, -0.12, 0.0, 0.12, 0.3];
const EYE_Y: [f32; 5] = [-0.6, -0.24, 0.0, 0.24, 0.6];
const NOSE_Y: [f32; 5] = [-0.45, -0.18, 0.0, 0.24, 0.6];
const MOUTH_Y: [f32; 5] = [-0.6, -0.24, 0.0, 0.16, 0.4];
const EAR_Y: [f32; 5] = [-0.9, -0.45, 0.0, 0.45, 0.9];
const EAR_Z: [f32; 5] = [-0.6, -0.3, 0.0, 0.3, 0.6];
const EYE_SCALE: [f32; 5] = [-0.1, -0.05, 0.0, 0.08, 0.16];
const NOSE_SCALE: [f32; 5] = [0.3, 0.15, 0.0, -0.35, -0.7];

impl FaceParams {
    /// From the save's slider fields (cSAVE_DATA_EDIT +0x31..+0x3a; the
    /// field order is the save's, *inferred* from the eyebrow offsets
    /// +0x33/+0x34 being read with the eyebrow +0x2c). Out-of-range values
    /// leave 0.
    // Translated from 0x00780fa0 and 0x0080bb10 (DDDA.exe 2017-12-13 Steam).
    pub fn from_save(field: &dyn Fn(&str) -> Option<u8>) -> FaceParams {
        let pick = |name: &str, t: &[f32; 5]| field(name).and_then(|i| t.get(i as usize).copied()).unwrap_or(0.0);
        FaceParams([
            pick("mOfsXFaceEye", &EYE_X),
            pick("mOfsYFaceEye", &EYE_Y),
            0.0,
            pick("mOfsYFaceNose", &NOSE_Y),
            pick("mOfsYFaceMouth", &MOUTH_Y),
            0.0,
            pick("mOfsYFaceEar", &EAR_Y),
            pick("mOfsZFaceEar", &EAR_Z),
            pick("mSclFaceEye", &EYE_SCALE),
            pick("mSclFaceNose", &NOSE_SCALE),
        ])
    }
}

/// (parent joint id, joint id) per controller (table 0x017edc70, 8-byte
/// rows; a joint id below 1 is none).
const CONTROLLER_JOINTS: [(i16, i16); CONTROLLERS] = [
    (-1, 2), (-1, 3), (-1, 4), (-1, 80), (80, 103), (80, 104), (80, 105), (80, 106), (80, 107),
    (80, 108), (80, 109), (80, 110), (-1, 121), (-1, 122), (-1, 123), (-1, 124), (-1, 125), (80, 134),
    (80, 135), (80, 212), (212, 95), (-1, 99), (212, 96), (-1, 100), (212, 97), (-1, 101), (212, 98),
    (-1, 102), (212, 234), (212, 235), (212, 236), (-1, 213), (213, 111), (213, 112), (-1, 117),
    (213, 113), (-1, 118), (213, 114), (-1, 119), (213, 115), (-1, 120), (213, 116), (-1, 145),
    (-1, 146), (-1, 147), (-1, 214), (-1, 215), (-1, 216), (-1, 217), (-1, 218), (-1, 219), (-1, 220),
    (-1, 81), (81, 82), (-1, 237), (-1, 244), (81, 83), (-1, 238), (81, 84), (-1, 239), (81, 85),
    (-1, 240), (-1, 241), (81, 86), (-1, 242), (81, 87), (-1, 243), (-1, 226), (-1, 88), (88, 89),
    (-1, 245), (-1, 252), (88, 90), (-1, 246), (88, 91), (-1, 247), (88, 92), (-1, 248), (-1, 249),
    (88, 93), (-1, 250), (88, 94), (-1, 251), (-1, 227), (-1, 126), (-1, 127), (-1, 128), (-1, 129),
    (-1, 130), (-1, 131), (-1, 132), (-1, 133), (-1, 136), (-1, 137), (-1, 138), (-1, 139), (-1, 140),
    (-1, 141), (-1, 142), (-1, 143), (-1, 144), (-1, 173), (-1, 210), (-1, 211), (-1, 221),
    (221, 228), (221, 229), (221, 230), (-1, 222), (-1, 223), (-1, 224), (-1, 225), (225, 231),
    (225, 232), (225, 233), (0, 0),
];

/// The uCnsEdit type 0x00812330 gives an eye joint (not translated: the
/// viewer leaves these joints alone). `None` for the plain type 0.
pub fn special_kind(id: u8) -> Option<u32> {
    match id {
        0x51 | 0x58 => Some(0x10),
        0x52..=0x57 | 0x59..=0x5e => Some(0xd),
        0xe2 | 0xe3 => Some(0xf),
        0xe4..=0xe9 => Some(0xe),
        _ => None,
    }
}

/// Whether 0x00812c80 gives a joint id a face uCnsEdit.
pub fn has_face_cns(id: u8) -> bool {
    (0x50..0x96).contains(&id) || (0xd2..0xff).contains(&id)
}

/// One morphed face joint: the uCnsEdit fields 0x0081a370 sets. Fields at
/// their default were skipped by the setter and stay at the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceJoint {
    pub id: u8,
    /// +0x90, cm.
    pub offset: [f32; 3],
    /// +0x80.
    pub scale: [f32; 3],
    /// +0x70, degrees (applied X, then Y, then Z: `Rx x Ry x Rz` for row
    /// vectors).
    pub rotation: [f32; 3],
}

/// The face morph of an edit: every joint 0x0081a370 changes (with a face
/// uCnsEdit and present in the skeleton), by id.
pub fn face_morph(edit: &FaceEdit, params: &FaceParams, present: &dyn Fn(u8) -> bool) -> Vec<FaceJoint> {
    let mut v = [[0.0f32; 3]; MARKERS];
    for (d, s) in v.iter_mut().zip(&edit.values) {
        *d = *s;
    }
    let mut c = [[[0.0f32; 3], [1.0; 3], [0.0; 3]]; CONTROLLERS];
    crate::face_controllers::controllers(&v, &params.0, &mut c);
    // Parent links (0x00811a90): the first controller with the parent's
    // joint id.
    let joint = |k: usize| {
        let j = CONTROLLER_JOINTS[k].1;
        if j < 1 { 0xff } else { j as u32 }
    };
    let parent: Vec<Option<usize>> = (0..CONTROLLERS)
        .map(|k| {
            let p = CONTROLLER_JOINTS[k].0;
            if joint(k) == 0xff {
                return None;
            }
            (0..CONTROLLERS).find(|&q| joint(q) == p as u32)
        })
        .collect();
    let mut out: Vec<FaceJoint> = vec![];
    // Translated from 0x0081a370 (DDDA.exe 2017-12-13 Steam): +0x4657 is
    // set by 0x00811780, so each controller sums its chain.
    for k in 0..CONTROLLERS {
        let id = joint(k);
        if id == 0xff {
            continue;
        }
        let (mut t, mut s, mut r) = ([0.0f32; 3], [1.0f32; 3], [0.0f32; 3]);
        let mut q = Some(k);
        while let Some(i) = q {
            for a in 0..3 {
                t[a] += c[i][0][a];
                s[a] *= c[i][1][a];
                r[a] += c[i][2][a];
            }
            q = parent[i];
        }
        let id = id as u8;
        if !has_face_cns(id) || !present(id) {
            continue;
        }
        let set_t = t != [0.0; 3];
        let set_s = s != [1.0; 3];
        let set_r = r != [0.0; 3];
        if !(set_t || set_s || set_r) {
            continue;
        }
        let eps = 1.192_092_9e-7;
        let mut j = match out.iter().position(|x| x.id == id) {
            Some(i) => out.remove(i),
            None => FaceJoint { id, offset: [0.0; 3], scale: [1.0; 3], rotation: [0.0; 3] },
        };
        if set_t {
            j.offset = t;
        }
        if set_s {
            j.scale = s.map(|x| if x.abs() < eps { eps } else { x });
        }
        if set_r {
            j.rotation = r.map(|x| x * 57.295_776);
        }
        out.push(j);
    }
    out.sort_by_key(|j| j.id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift32 to f32 in [-1, 1) times a power of two (exact in f32),
    /// the generator of the emulation that produced the expected values.
    struct Rng(u32);
    impl Rng {
        fn f(&mut self, scale: f32) -> f32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            ((x >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0) * scale
        }
    }

    fn inputs(seed: u32, sv: f32, sp: f32) -> (FaceEdit, FaceParams) {
        let mut r = Rng(seed);
        let values = (0..MARKERS).map(|_| [r.f(sv), r.f(sv), r.f(sv)]).collect();
        let mut p = [0.0; 10];
        for x in &mut p {
            *x = r.f(sp);
        }
        (FaceEdit { values }, FaceParams(p))
    }

    fn fnv(out: &[FaceJoint]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |b: &[u8]| {
            for &x in b {
                h ^= u64::from(x);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for j in out {
            eat(&[j.id]);
            for v in j.offset.iter().chain(&j.scale).chain(&j.rotation) {
                let v = if *v == 0.0 { 0.0f32 } else { *v };
                eat(&v.to_le_bytes());
            }
        }
        h
    }

    #[test]
    fn neutral_face_is_identity() {
        assert!(face_morph(&FaceEdit::neutral(), &FaceParams::default(), &|_| true).is_empty());
    }

    // Expected values: 0x00811780, 0x00811a90, the edit and parameters
    // written in, then 0x00812fb0, 0x00815c60 and 0x0081a370 executed in an
    // x86 emulator on DDDA.exe's own code, a uCnsEdit per joint id.
    #[test]
    fn matches_the_emulated_exe() {
        let (e, p) = inputs(1, 0.5, 0.5);
        let out = face_morph(&e, &p, &|_| true);
        assert_eq!(out.len(), 100);
        let j80 = out.iter().find(|j| j.id == 80).unwrap();
        assert_eq!(j80.offset, [0.0, 0.464_140_7, 0.0]);
        let j81 = out.iter().find(|j| j.id == 81).unwrap();
        assert_eq!(j81.offset, [-0.226_686_18, 0.584_142_15, 0.482_178_84]);
        assert_eq!(j81.scale, [0.634_846; 3]);
        let j251 = out.iter().find(|j| j.id == 251).unwrap();
        assert_eq!(j251.offset, [0.0, -0.277_277_38, 0.277_277_38]);
        assert_eq!(j251.rotation, [-1.783_973_6, 0.0, 0.0]);
        assert_eq!(fnv(&out), 0x165f_4bd8_0ad9_8ce8);

        let (e, p) = inputs(7, 2.0, 1.0);
        let out = face_morph(&e, &p, &|_| true);
        assert_eq!(out.len(), 100);
        assert_eq!(fnv(&out), 0xedbb_6123_e298_8382);
    }

    #[test]
    fn presets_fill_their_components() {
        let mut e = FaceEdit::neutral();
        let p = FaceParts { part: 5, values: vec![[1.0, 2.0, 3.0]; 10] };
        e.apply_preset(5, Some(&p));
        // (34, 6): y and z only.
        assert_eq!(e.values[34], [0.0, 2.0, 3.0]);
        assert_eq!(e.values[35], [1.0, 2.0, 3.0]);
        e.apply_preset(5, None);
        assert_eq!(e.values[35], [0.0; 3]);
    }

    #[test]
    fn save_sliders() {
        let f = |n: &str| match n {
            "mOfsXFaceEye" => Some(4),
            "mSclFaceNose" => Some(0),
            _ => Some(2),
        };
        let p = FaceParams::from_save(&f);
        assert_eq!(p.0[0], 0.3);
        assert_eq!(p.0[9], 0.3);
        assert_eq!(p.0[1..9], [0.0; 8]);
    }
}
