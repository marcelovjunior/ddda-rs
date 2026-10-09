//! The separate cBodyEdit controller, not cHumanEdit's height/limb presets.
//! Bulk interpolation and limit tables stay in the installed executable.

use crate::{morph::JointMorph, Error};

pub const JOINT_COUNT: usize = 37;

/// rBodyEdit plus the bust parameter supplied by cEditCtrl (0x0080bb10).
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub slender: f32,
    pub muscle: f32,
    pub bust: f32,
    pub fixed: bool,
    pub scales: Vec<[f32; 3]>,
}

impl Edit {
    /// Translated from 0x00780fa0 and 0x0080add0 (DDDA.exe 2017-12-13
    /// Steam). The resource's percentage values become controller ratios.
    pub fn from_save(field: &dyn Fn(&str) -> Option<u32>) -> Result<Self, Error> {
        let select = |name: &str, values: &[f32; 5], neutral: f32| {
            match field(name) {
                None => Ok(neutral),
                Some(i) => values.get(i as usize).copied()
                    .ok_or_else(|| Error::Data(format!("{name}: slider outside 0..5"))),
            }
        };
        // Stack tables at 0x00780fe2..0x0078122c and 0x00781281,
        // 0x007813c1, 0x00781431..43; consumed at 0x0078177e and 0x00781ca2.
        Ok(Self {
            slender: select("mBodySlender", &[-60.0, -30.0, 0.0, 30.0, 60.0], 0.0)? * 0.01,
            muscle: select("mBodyMuscle", &[0.0, 25.0, 50.0, 75.0, 100.0], 0.0)? * 0.01,
            bust: select("mBustScale", &[-1.0, -0.35, 0.0, 0.35, 1.0], 0.0)?,
            fixed: false,
            scales: vec![[1.0; 3]; JOINT_COUNT],
        })
    }

    /// Translated from rBodyEdit::vf10 0x00cad170 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8], bust: f32) -> Result<Self, Error> {
        let mut r = crate::Rd { b: bytes, o: 0, what: "rBodyEdit" };
        if r.u32()? != 0x13 { return Err(Error::Data("rBodyEdit: version".into())); }
        let slender = r.f32()? * 0.01;
        let muscle = r.f32()? * 0.01;
        r.f32()?; // resource +0x68, not copied by 0x0080add0
        let fixed = r.u8()? != 0;
        r.o = (r.o + 3) & !3;
        let count = r.u32()? as usize;
        if count > bytes.len().saturating_sub(r.o) / 12 {
            return Err(Error::Data("rBodyEdit: short scale list".into()));
        }
        let mut scales = Vec::with_capacity(count);
        for _ in 0..count { scales.push([r.f32()?, r.f32()?, r.f32()?]); }
        Ok(Self { slender, muscle, bust, fixed, scales })
    }
}

/// Four body shapes and two limits per sex, each with 37 padded vectors.
pub struct Tables {
    shapes: [[[ [f32; 3]; JOINT_COUNT]; 4]; 2],
    upper: [[[f32; 3]; JOINT_COUNT]; 2],
    lower: [[[f32; 3]; JOINT_COUNT]; 2],
}

impl Tables {
    /// Read the player's installed tables used by 0x0080aed0. These are
    /// bulk data, never copied into source or replaced with fitted values.
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, ddexe::Error> {
        let mut out = Self { shapes: [[[[0.0; 3]; JOINT_COUNT]; 4]; 2],
            upper: [[[0.0; 3]; JOINT_COUNT]; 2], lower: [[[0.0; 3]; JOINT_COUNT]; 2] };
        for sex in 0..2 {
            for joint in 0..JOINT_COUNT {
                for axis in 0..3 {
                    let offset = ((sex * JOINT_COUNT + joint) * 16 + axis * 4) as u32;
                    out.upper[sex][joint][axis] = exe.f32(0x0183_13c0 + offset)?;
                    out.lower[sex][joint][axis] = exe.f32(0x0183_1860 + offset)?;
                    for shape in 0..4 {
                        let offset = (((sex * 4 + shape) * JOINT_COUNT + joint) * 16 + axis * 4) as u32;
                        out.shapes[sex][shape][joint][axis] = exe.f32(0x0183_0140 + offset)?;
                    }
                }
            }
        }
        Ok(out)
    }

    /// Translated from 0x0080aed0; joint ids and plain constraint type
    /// from 0x0080ad20 / 0x00860660 (DDDA.exe 2017-12-13 Steam).
    pub fn joints(&self, sex: u8, edit: &Edit) -> Result<Vec<(u8, JointMorph)>, Error> {
        let sex = usize::from(sex);
        if sex >= 2 { return Err(Error::Data("cBodyEdit: invalid sex".into())); }
        let (thin, heavy) = if edit.slender < 0.0 { (-edit.slender, 0.0) } else { (0.0, edit.slender) };
        let mut out = Vec::new();
        for i in 0..JOINT_COUNT {
            let authored = edit.scales.get(i).copied().unwrap_or([1.0; 3]);
            let mut scale = [0.0; 3];
            for axis in 0..3 {
                let base = self.shapes[sex][0][i][axis];
                let mut value = (base + (self.shapes[sex][1][i][axis] - base) * thin
                    + (self.shapes[sex][2][i][axis] - base) * edit.muscle
                    + (self.shapes[sex][3][i][axis] - base) * heavy) * authored[axis];
                if sex == 1 && matches!(i, 4 | 14) && edit.bust != 0.0 {
                    value += (self.upper[sex][i][axis] - self.lower[sex][i][axis]) * edit.bust;
                }
                if !edit.fixed {
                    if self.upper[sex][i][axis] < value { value = self.upper[sex][i][axis]; }
                    else if value < self.lower[sex][i][axis] { value = self.lower[sex][i][axis]; }
                }
                scale[axis] = value;
            }
            if scale != [1.0; 3] {
                for value in &mut scale {
                    if value.abs() < f32::EPSILON { *value = f32::EPSILON; }
                }
                out.push((0xaa + i as u8, JointMorph { scale, child: [1.0; 3], offset: [0.0; 3], kind: 0 }));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> Tables {
        Tables { shapes: [[[[1.0; 3]; JOINT_COUNT]; 4]; 2],
            upper: [[[3.0; 3]; JOINT_COUNT]; 2], lower: [[[0.5; 3]; JOINT_COUNT]; 2] }
    }
    #[test]
    fn signed_slender_muscle_authored_scale_and_limits() {
        let mut t = tables();
        t.shapes[0][1][0] = [0.5, 2.0, 3.0];
        t.shapes[0][2][0] = [2.0, 3.0, 4.0];
        t.shapes[0][3][0] = [2.0, 2.0, 2.0];
        let mut e = Edit::from_save(&|_| None).unwrap();
        e.slender = -0.5; e.muscle = 0.5; e.scales[0] = [2.0, 1.0, 2.0];
        assert_eq!(t.joints(0, &e).unwrap()[0].1.scale, [2.5, 2.5, 3.0]);
        e.fixed = true;
        assert_eq!(t.joints(0, &e).unwrap()[0].1.scale, [2.5, 2.5, 7.0]);
        e.slender = 0.5; e.muscle = 0.0; e.scales[0] = [1.0; 3];
        assert_eq!(t.joints(0, &e).unwrap()[0].1.scale, [1.5; 3]);
    }
    #[test]
    fn bust_only_changes_two_female_joints_and_zero_is_positive_epsilon() {
        let t = tables();
        let mut e = Edit::from_save(&|_| None).unwrap();
        e.bust = 0.5;
        assert!(t.joints(0, &e).unwrap().is_empty());
        let joints = t.joints(1, &e).unwrap();
        assert_eq!(joints.iter().map(|j| j.0).collect::<Vec<_>>(), [0xae, 0xb8]);
        assert!(joints.iter().all(|j| j.1.scale == [2.25; 3]));
        e.bust = 0.0; e.fixed = true; e.scales[36] = [0.0, -f32::EPSILON * 0.5, 1.0];
        assert_eq!(t.joints(0, &e).unwrap()[0].1.scale, [f32::EPSILON, f32::EPSILON, 1.0]);
    }
    #[test]
    fn save_slider_ranges_and_resource_layout() {
        let e = Edit::from_save(&|name| Some(if name == "mBodySlender" { 0 } else { 4 })).unwrap();
        assert_eq!((e.slender, e.muscle, e.bust), (-0.59999996, 1.0, 1.0));
        assert!(Edit::from_save(&|_| Some(5)).is_err());
        let mut bytes = 0x13_u32.to_le_bytes().to_vec();
        for value in [-30.0_f32, 75.0, 123.0] { bytes.extend(value.to_le_bytes()); }
        bytes.extend([1, 0, 0, 0]); bytes.extend(1_u32.to_le_bytes());
        for value in [1.1_f32, 1.2, 1.3] { bytes.extend(value.to_le_bytes()); }
        let e = Edit::parse(&bytes, 0.35).unwrap();
        assert_eq!(e.scales, [[1.1, 1.2, 1.3]]); assert!(e.fixed);
        assert_eq!((e.slender, e.muscle, e.bust), (-0.29999998, 0.75, 0.35));
        assert!(Edit::parse(&bytes[..bytes.len()-1], 0.0).is_err());
    }
}
