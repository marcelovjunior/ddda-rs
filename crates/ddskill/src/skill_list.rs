//! `rSkillList` (`etc\skill\skill`): what each skill and augment costs to learn.
//!
//! Binary layout, from the loader `rSkillList` vtable slot 10 (0x00cdec00) and
//! the saver slot 11 (0x00cdee30). All values are little-endian `i32`:
//!
//! ```text
//! u32 magic 0x006c6b73 ("skl\0"), u32 version 0x200
//! u32 n; n x (skill_id, discipline_cost, rank)               core skills
//! u32 n; n x (skill_id, skill_level, discipline_cost, rank)  weapon skills
//! u32 n; n x (augment_id, discipline_cost, rank)             augments
//! ```
//!
//! Field meanings come from the consumers in the skill-learning screen
//! (`uGUISkillLearn`): 0x006fd940 (core), 0x006fd7e0 / 0x006fdb40 (weapon
//! skills) and 0x006fe050 / 0x006fdc30 (augments) compare `rank` with the
//! character's vocation rank (`cPlayerInfo` +0x2cc + vocation * 4) and show
//! `discipline_cost`. The reflected MtDTI field names (`mJobLevel`, `mSkillId`,
//! `mSkillLv`, `mJobPoint`) exist, but the DTI offsets list `mJobLevel` first
//! while the loader stores the rank last; the order here follows the code.

use crate::Error;

pub const MAGIC: u32 = 0x006c_6b73;
pub const VERSION: u32 = 0x200;

/// A core skill learned with discipline (e.g. skill 30, Dire Onslaught).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSkill {
    pub skill_id: i32,
    pub cost: i32,
    /// Vocation rank required (0..9).
    pub rank: i32,
}

/// One level of a weapon ("custom") skill. Level 1 is the base skill, level 2
/// the upgraded one; level 2 also requires level 1 learned (0x006fdb40 calls
/// 0x00780ba0). Level 3 (Dark Arisen) is not in this resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponSkill {
    pub skill_id: i32,
    pub level: i32,
    pub cost: i32,
    pub rank: i32,
}

/// An augment. Ids below 100 belong to vocation `id / 10 + 1` (0x006fdc30);
/// ids 100 and up have rank 0 and are unlocked per save (see VOCATIONS.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Augment {
    pub augment_id: i32,
    pub cost: i32,
    pub rank: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillList {
    pub core: Vec<CoreSkill>,
    pub weapon: Vec<WeaponSkill>,
    pub augments: Vec<Augment>,
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn u32(&mut self, what: &str) -> Result<u32, Error> {
        let end = self.pos + 4;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| Error(format!("truncated at {:#x} reading {what}", self.pos)))?;
        self.pos = end;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn i32(&mut self, what: &str) -> Result<i32, Error> {
        self.u32(what).map(|v| v as i32)
    }
    fn count(&mut self, what: &str, record: usize) -> Result<usize, Error> {
        let n = self.u32(what)? as usize;
        let left = self.data.len() - self.pos;
        if n.checked_mul(record).is_none_or(|b| b > left) {
            return Err(Error(format!("{what}: {n} records do not fit in {left} bytes")));
        }
        Ok(n)
    }
}

impl SkillList {
    pub const RESOURCE_CLASS: &'static str = "rSkillList";
    pub const PATH: &'static str = "etc\\skill\\skill";

    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        let mut c = Cursor { data, pos: 0 };
        let magic = c.u32("magic")?;
        if magic != MAGIC {
            return Err(Error(format!("bad magic {magic:#x}")));
        }
        let version = c.u32("version")?;
        if version != VERSION {
            return Err(Error(format!("unsupported version {version:#x}")));
        }
        let n = c.count("core skill count", 12)?;
        let mut core = Vec::with_capacity(n);
        for _ in 0..n {
            core.push(CoreSkill {
                skill_id: c.i32("skill id")?,
                cost: c.i32("cost")?,
                rank: c.i32("rank")?,
            });
        }
        let n = c.count("weapon skill count", 16)?;
        let mut weapon = Vec::with_capacity(n);
        for _ in 0..n {
            weapon.push(WeaponSkill {
                skill_id: c.i32("skill id")?,
                level: c.i32("skill level")?,
                cost: c.i32("cost")?,
                rank: c.i32("rank")?,
            });
        }
        let n = c.count("augment count", 12)?;
        let mut augments = Vec::with_capacity(n);
        for _ in 0..n {
            augments.push(Augment {
                augment_id: c.i32("augment id")?,
                cost: c.i32("cost")?,
                rank: c.i32("rank")?,
            });
        }
        if c.pos != data.len() {
            return Err(Error(format!("{} trailing bytes", data.len() - c.pos)));
        }
        Ok(Self { core, weapon, augments })
    }

    /// The writer's layout (0x00cdee30), for round-trip tests.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut put = |v: u32| out.extend_from_slice(&v.to_le_bytes());
        put(MAGIC);
        put(VERSION);
        put(self.core.len() as u32);
        for s in &self.core {
            [s.skill_id, s.cost, s.rank].iter().for_each(|&v| put(v as u32));
        }
        put(self.weapon.len() as u32);
        for s in &self.weapon {
            [s.skill_id, s.level, s.cost, s.rank].iter().for_each(|&v| put(v as u32));
        }
        put(self.augments.len() as u32);
        for a in &self.augments {
            [a.augment_id, a.cost, a.rank].iter().for_each(|&v| put(v as u32));
        }
        out
    }

    /// Vocation an augment under 100 belongs to: `id / 10 + 1`
    /// (0x006fdc30: listed when `id / 10 == vocation - 1`). `None` for the
    /// save-unlocked augments (id >= 100), which every vocation can learn.
    pub fn augment_vocation(augment_id: i32) -> Option<u8> {
        (0..100).contains(&augment_id).then(|| (augment_id / 10 + 1) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SkillList {
        SkillList {
            core: vec![CoreSkill { skill_id: 30, cost: 800, rank: 1 }],
            weapon: vec![
                WeaponSkill { skill_id: 40, level: 1, cost: 0, rank: 0 },
                WeaponSkill { skill_id: 40, level: 2, cost: 600, rank: 3 },
            ],
            augments: vec![Augment { augment_id: 0, cost: 700, rank: 2 }],
        }
    }

    #[test]
    fn round_trip() {
        let s = sample();
        let b = s.to_bytes();
        assert_eq!(&b[..4], b"skl\0");
        assert_eq!(SkillList::from_bytes(&b).unwrap(), s);
    }

    #[test]
    fn rejects_bad_input() {
        let b = sample().to_bytes();
        for cut in 0..b.len() {
            assert!(SkillList::from_bytes(&b[..cut]).is_err(), "cut at {cut}");
        }
        let mut extra = b.clone();
        extra.push(0);
        assert!(SkillList::from_bytes(&extra).is_err());
        let mut bad = b.clone();
        bad[4] = 1;
        assert!(SkillList::from_bytes(&bad).is_err());
        let mut huge = b;
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(SkillList::from_bytes(&huge).is_err());
    }

    #[test]
    fn augment_vocations() {
        assert_eq!(SkillList::augment_vocation(0), Some(1));
        assert_eq!(SkillList::augment_vocation(86), Some(9));
        assert_eq!(SkillList::augment_vocation(100), None);
    }
}
