//! `rObjCollision` (.ocl): a model's hit/hurt volumes, the attacks its hits
//! carry, and the motion index tying them together.
//!
//! Layout from the PC loader `rObjCollision::vf10` at `0x00CCB980`
//! (DDDA.exe 2017-12-13 Steam), which reads the file as a stream of 32-bit
//! little-endian words: a float is its bits, a flag is a word tested for
//! non-zero. Field names and in-memory offsets come from the reflection
//! functions `rObjCollision::Group::vf03` `0x00CCD510`,
//! `Group::Node::vf03` `0x00CCDAA0`, `Attack::vf03` `0x00CCEAE0` and
//! `SeqIndex::vf03` `0x00CD3C70`. Cross-checked against Riftstone's notes
//! (MIT), whose field order this agrees with.

use std::fmt;

/// File version accepted by the loader (`0x00CCB9BB`).
pub const VERSION: u32 = 0x2012_1225;
/// An attack slot holding this `mNo` is empty; nothing else follows it
/// (`0x00CCBD3F`).
pub const EMPTY_ATTACK: u32 = 0x7FFF_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

struct Words<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Words<'a> {
    fn u32(&mut self) -> Result<u32> {
        let b = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| Error(format!("truncated at byte {}", self.pos)))?;
        self.pos += 4;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        self.u32().map(f32::from_bits)
    }
    fn flag(&mut self) -> Result<bool> {
        self.u32().map(|w| w != 0)
    }
    fn count(&mut self, what: &str) -> Result<usize> {
        let n = self.u32()? as usize;
        // Every record is at least one word, so a count larger than the
        // remaining words is corrupt (the game would read zeros past the end).
        if n > (self.data.len() - self.pos) / 4 {
            return Err(Error(format!("{what} count {n} exceeds the file")));
        }
        Ok(n)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjCollision {
    pub version: u32,
    pub resource_id: u32,
    pub groups: Vec<Group>,
    /// Indexed by attack number; `None` is an empty slot.
    pub attacks: Vec<Option<Attack>>,
    pub seq_index: Vec<SeqIndex>,
}

/// `rObjCollision::Group` (0x58 bytes in memory).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub no: u32,
    pub kind: u32,
    pub is_attack: bool,
    pub is_damage: bool,
    pub is_scr_adj: bool,
    pub is_obj_adj: bool,
    pub is_hang: bool,
    pub is_hanged: bool,
    pub is_check: bool,
    pub is_notice: bool,
    pub is_notice2: bool,
    pub nodes: Vec<Node>,
}

/// `rObjCollision::Group::Node` (0x90 bytes in memory).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub no: u32,
    /// 0 capsule, 1 sphere, 4 capsule ending at the points (see COMBAT.md).
    pub shape: u32,
    pub radius: f32,
    pub joint0: i32,
    pub offset0: [f32; 3],
    pub joint1: i32,
    pub offset1: [f32; 3],
    /// Index into the owner's runtime region list (`cObjCollision`+0xC38),
    /// built from the `rRegionStatus` elements.
    pub region_id: u32,
    pub hit_prio: u32,
    pub node_attr: u32,
    pub node_effect_attr: u32,
    pub node_free_work: i32,
}

/// `rObjCollision::SeqIndex`: motion sequence entry naming a group and an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqIndex {
    pub no: u32,
    pub model_id: u32,
    pub group_no: u32,
    pub attack_no: u32,
    pub relation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    F32,
    U32,
    S32,
}

/// One stored attack value, in the loader's order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    F32(f32),
    U32(u32),
    S32(i32),
}
impl Value {
    pub fn as_f32(self) -> f32 {
        match self {
            Value::F32(v) => v,
            Value::U32(v) => v as f32,
            Value::S32(v) => v as f32,
        }
    }
}

/// The 83 attack values in file order: (reflected name, in-memory offset of
/// `rObjCollision::Attack`, storage kind). Offsets are the targets of the
/// stores in `0x00CCB980`; names are joined to them by `Attack::vf03`.
pub const ATTACK_FIELDS: [(&str, u16, Kind); 83] = {
    use Kind::*;
    [
        ("mAttack", 0x2c, F32),
        ("mAttackRate", 0x30, F32),
        ("mSwordRate", 0x34, F32),
        ("mHitRate", 0x38, F32),
        ("mMgcAttack", 0x3c, F32),
        ("mMgcAttackRate", 0x40, F32),
        ("mElement", 0x44, U32),
        ("mElementRate", 0x48, F32),
        ("mShrinkType", 0x4c, U32),
        ("mBlowType", 0x54, U32),
        ("mShrinkAttack", 0x50, F32),
        ("mBlowAttack", 0x58, F32),
        ("mRangeType", 0x5c, U32),
        ("mCritical", 0x60, F32),
        ("mFaint", 0x64, F32),
        ("mHitStopTime", 0x68, F32),
        ("mHitSlowRate", 0x6c, F32),
        ("mAttackID", 0x70, U32),
        ("mWork", 0x74, S32),
        ("mEffectType", 0x78, U32),
        ("mEffectLevel", 0x7c, U32),
        ("mEffectBreakLevel", 0x80, U32),
        ("mEffectAttr", 0x84, U32),
        ("mStaminaAttack", 0x88, F32),
        ("mResetTime", 0x8c, F32),
        ("mPoison", 0x90, F32),
        ("mSlow", 0x94, F32),
        ("mBlind", 0x98, F32),
        ("mSilence", 0x9c, F32),
        ("mSleep", 0xa0, F32),
        ("mOil", 0xa4, F32),
        ("mWet", 0xa8, F32),
        ("mCurse", 0xac, F32),
        ("mEnemy", 0xb0, F32),
        ("mSeal", 0xb4, F32),
        ("mStone", 0xb8, F32),
        ("mAtkDown", 0xbc, F32),
        ("mDefDown", 0xc0, F32),
        ("mMgcAtkDown", 0xc4, F32),
        ("mMgcDefDown", 0xc8, F32),
        ("mWpPhyAtkRate", 0xcc, F32),
        ("mWpMgcAtkRate", 0xd0, F32),
        ("mWpHitRate", 0xd4, F32),
        ("mWpSwdRate", 0xd8, F32),
        ("mWpCriticalRate", 0xdc, F32),
        ("mAngle", 0xe0, F32),
        ("mAngleRand", 0xe4, F32),
        ("mShrinkDistanceZ", 0xe8, F32),
        ("mShrinkGravityZ", 0xec, F32),
        ("mShrinkAirDistanceZ", 0x108, F32),
        ("mShrinkAirGravityZ", 0x10c, F32),
        ("mShrinkGroundHeightY", 0xf0, F32),
        ("mShrinkGroundGravityY", 0xf4, F32),
        ("mShrinkAirHeightY", 0x110, F32),
        ("mShrinkAirGravityY", 0x114, F32),
        ("mShrinkDistanceZRand", 0xf8, F32),
        ("mShrinkGravityZRand", 0xfc, F32),
        ("mShrinkAirDistanceZRand", 0x118, F32),
        ("mShrinkAirGravityZRand", 0x11c, F32),
        ("mShrinkGroundHeightYRand", 0x100, F32),
        ("mShrinkGroundGravityYRand", 0x104, F32),
        ("mShrinkAirHeightYRand", 0x120, F32),
        ("mShrinkAirGravityYRand", 0x124, F32),
        ("mDistanceZ", 0x128, F32),
        ("mGravityZ", 0x12c, F32),
        ("mAirDistanceZ", 0x148, F32),
        ("mAirGravityZ", 0x14c, F32),
        ("mGroundHeightY", 0x130, F32),
        ("mGroundGravityY", 0x134, F32),
        ("mAirHeightY", 0x150, F32),
        ("mAirGravityY", 0x154, F32),
        ("mDistanceZRand", 0x138, F32),
        ("mGravityZRand", 0x13c, F32),
        ("mAirDistanceZRand", 0x158, F32),
        ("mAirGravityZRand", 0x15c, F32),
        ("mGroundHeightYRand", 0x140, F32),
        ("mGroundGravityYRand", 0x144, F32),
        ("mAirHeightYRand", 0x160, F32),
        ("mAirGravityYRand", 0x164, F32),
        ("mSeType", 0x168, U32),
        ("mSeAttr", 0x16c, U32),
        ("mGetGold", 0x170, S32),
        ("mExpRate", 0x174, F32),
    ]
};

/// The 57 attack flags in bit order: word 1 bits 0..31, then word 2 bits
/// 0..24 (`0x00CCBEE0` onwards). Word 2 skips `mIsUseData` (+0x1A9), which
/// is not stored in the file; bits 25..31 of word 2 are never read.
pub const ATTACK_FLAGS: [&str; 57] = [
    "mMultiHit",
    "mIsUseShrinkParam",
    "mIsAllCritical",
    "mIsGuardBreak",
    "mIsNoGuard",
    "mIsFriendHitDamage1",
    "mIsFriendHitDamage1_2",
    "mIsFriendHitDamage1_4",
    "mIsFriendHitDamage0",
    "mIsPlNoHit",
    "mIsEmNoHit",
    "mIsNpcNoHit",
    "mIsEtcNoHit",
    "mIsTurnAttacker",
    "mIsOugi",
    "mIsMakikomi",
    "mIsVerticalReflect",
    "mIsNoDamagePT",
    "mIsNoCritical",
    "mIsNoHitFly",
    "mIsPushOff",
    "mIsPushOffFly",
    "mIsNoShlReflect",
    "mIsElementNG",
    "mIsBigOMBreak",
    "mIsBarrelBomb",
    "mIsOwnHit",
    "mIsArrow",
    "mIsClimbAttack",
    "mIsNoEnchant",
    "mIsNoDeath",
    "mIsForceShrink",
    // word 2
    "mIsForceBlow",
    "mIsForceShrink2",
    "mIsForceBlow2",
    "mIsNoBattle",
    "mIsVSEnemy01",
    "mIsVSEnemy02",
    "mIsVSEnemy03",
    "mIsVSEnemy04",
    "mIsVSEnemy05",
    "mIsVSEnemy06",
    "mIsVSEnemy07",
    "mIsVSEnemy08",
    "mIsVSEnemy09",
    "mIsVSEnemy10",
    "mIsVSEnemy11",
    "mIsVSEnemy12",
    "mIsVSEnemy13",
    "mIsAllGuard",
    "mIsNoDamageNoResetShrink",
    "mIsNoDamageNoResetBlow",
    "mIsSameFrameNoHit00",
    "mIsSameFrameNoHit01",
    "mIsSameFrameNoHit02",
    "mIsSameFrameNoHit03",
    "mIsNoCalcEnemyDefence",
];

/// `rObjCollision::Attack` (0x1B4 bytes in memory).
#[derive(Debug, Clone, PartialEq)]
pub struct Attack {
    pub no: u32,
    /// Raw words in [`ATTACK_FIELDS`] order (float bits preserved).
    pub words: [u32; 83],
    /// The two flag words as stored, including unread high bits.
    pub flag_words: [u32; 2],
}

impl Attack {
    pub fn value(&self, name: &str) -> Option<Value> {
        let i = ATTACK_FIELDS.iter().position(|f| f.0 == name)?;
        let w = self.words[i];
        Some(match ATTACK_FIELDS[i].2 {
            Kind::F32 => Value::F32(f32::from_bits(w)),
            Kind::U32 => Value::U32(w),
            Kind::S32 => Value::S32(w as i32),
        })
    }
    fn f(&self, name: &str) -> f32 {
        self.value(name).map(Value::as_f32).unwrap_or(0.0)
    }
    fn u(&self, name: &str) -> u32 {
        match self.value(name) {
            Some(Value::U32(v)) => v,
            _ => 0,
        }
    }
    pub fn flag(&self, name: &str) -> Option<bool> {
        let i = ATTACK_FLAGS.iter().position(|f| *f == name)?;
        Some(self.flag_words[i / 32] >> (i % 32) & 1 != 0)
    }
    /// All set flags by name.
    pub fn set_flags(&self) -> impl Iterator<Item = &'static str> + '_ {
        ATTACK_FLAGS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.flag_words[i / 32] >> (i % 32) & 1 != 0)
            .map(|(_, n)| *n)
    }
    pub fn attack(&self) -> f32 {
        self.f("mAttack")
    }
    pub fn mgc_attack(&self) -> f32 {
        self.f("mMgcAttack")
    }
    /// Percent of the physical part dealt as slash (`mSwordRate`).
    pub fn sword_rate(&self) -> f32 {
        self.f("mSwordRate")
    }
    /// Percent of the physical part dealt as strike (`mHitRate`).
    pub fn hit_rate(&self) -> f32 {
        self.f("mHitRate")
    }
    pub fn element(&self) -> u32 {
        self.u("mElement")
    }
    pub fn shrink_type(&self) -> u32 {
        self.u("mShrinkType")
    }
    pub fn shrink_attack(&self) -> f32 {
        self.f("mShrinkAttack")
    }
    pub fn blow_type(&self) -> u32 {
        self.u("mBlowType")
    }
    pub fn blow_attack(&self) -> f32 {
        self.f("mBlowAttack")
    }
    pub fn stamina_attack(&self) -> f32 {
        self.f("mStaminaAttack")
    }
}

impl ObjCollision {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Words { data, pos: 0 };
        let version = r.u32()?;
        if version != VERSION {
            return Err(Error(format!("version {version:#x}, expected {VERSION:#x}")));
        }
        let resource_id = r.u32()?;
        let n = r.count("group")?;
        let mut groups = Vec::with_capacity(n);
        for _ in 0..n {
            let no = r.u32()?;
            let kind = r.u32()?;
            let is_attack = r.flag()?;
            let is_damage = r.flag()?;
            let is_scr_adj = r.flag()?;
            let is_obj_adj = r.flag()?;
            let is_hang = r.flag()?;
            let is_hanged = r.flag()?;
            let is_check = r.flag()?;
            let is_notice = r.flag()?;
            let is_notice2 = r.flag()?;
            let nn = r.count("node")?;
            let mut nodes = Vec::with_capacity(nn);
            for _ in 0..nn {
                nodes.push(Node {
                    no: r.u32()?,
                    shape: r.u32()?,
                    radius: r.f32()?,
                    joint0: r.u32()? as i32,
                    offset0: [r.f32()?, r.f32()?, r.f32()?],
                    joint1: r.u32()? as i32,
                    offset1: [r.f32()?, r.f32()?, r.f32()?],
                    region_id: r.u32()?,
                    hit_prio: r.u32()?,
                    node_attr: r.u32()?,
                    node_effect_attr: r.u32()?,
                    node_free_work: r.u32()? as i32,
                });
            }
            groups.push(Group {
                no,
                kind,
                is_attack,
                is_damage,
                is_scr_adj,
                is_obj_adj,
                is_hang,
                is_hanged,
                is_check,
                is_notice,
                is_notice2,
                nodes,
            });
        }
        let n = r.count("attack")?;
        let mut attacks = Vec::with_capacity(n);
        for _ in 0..n {
            let no = r.u32()?;
            if no == EMPTY_ATTACK {
                attacks.push(None);
                continue;
            }
            let mut words = [0u32; 83];
            for w in &mut words {
                *w = r.u32()?;
            }
            let flag_words = [r.u32()?, r.u32()?];
            attacks.push(Some(Attack { no, words, flag_words }));
        }
        let n = r.count("sequence")?;
        let mut seq_index = Vec::with_capacity(n);
        for _ in 0..n {
            seq_index.push(SeqIndex {
                no: r.u32()?,
                model_id: r.u32()?,
                group_no: r.u32()?,
                attack_no: r.u32()?,
                relation: r.u32()?,
            });
        }
        if r.pos != data.len() {
            return Err(Error(format!("{} trailing bytes", data.len() - r.pos)));
        }
        Ok(ObjCollision { version, resource_id, groups, attacks, seq_index })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn table_offsets_are_unique_and_cover_the_attack_block() {
        let mut offs: Vec<u16> = ATTACK_FIELDS.iter().map(|f| f.1).collect();
        offs.sort_unstable();
        offs.dedup();
        assert_eq!(offs.len(), 83);
        // 0x2c..=0x174 every 4 bytes is exactly the 83 stored values.
        assert_eq!(offs, (0..83).map(|i| 0x2c + 4 * i).collect::<Vec<_>>());
    }

    #[test]
    fn minimal_file_with_one_attack_and_empty_slot() {
        let mut w = vec![VERSION, 7, 1];
        // group 0 with one node
        w.extend([0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        w.extend([5, 1, 30f32.to_bits(), 3, 0, 0, 0, u32::MAX, 0, 0, 0, 2, 0, 0, 0, 0]);
        w.push(2); // two attack slots
        w.push(EMPTY_ATTACK);
        w.push(1);
        let mut vals = [0u32; 83];
        vals[0] = 120f32.to_bits(); // mAttack
        vals[2] = 100f32.to_bits(); // mSwordRate
        vals[6] = 2; // mElement
        vals[9] = 3; // mBlowType
        w.extend(vals);
        w.extend([1 | 1 << 31, 1 << 24]); // mMultiHit, mIsForceShrink, mIsNoCalcEnemyDefence
        w.extend([1, 0, 0, 0, 1, 0]); // one seq entry: no, model, group, attack 1, relation
        let o = ObjCollision::parse(&le(&w)).unwrap();
        assert_eq!(o.groups[0].nodes[0].radius, 30.0);
        assert_eq!(o.groups[0].nodes[0].joint1, -1);
        assert!(o.attacks[0].is_none());
        let a = o.attacks[1].as_ref().unwrap();
        assert_eq!(a.attack(), 120.0);
        assert_eq!(a.sword_rate(), 100.0);
        assert_eq!(a.element(), 2);
        assert_eq!(a.blow_type(), 3);
        assert_eq!(a.flag("mMultiHit"), Some(true));
        assert_eq!(a.flag("mIsForceShrink"), Some(true));
        assert_eq!(a.flag("mIsNoCalcEnemyDefence"), Some(true));
        assert_eq!(a.flag("mIsForceBlow"), Some(false));
        assert_eq!(o.seq_index[0].attack_no, 1);
    }

    #[test]
    fn rejects_bad_version_truncation_and_trailing_bytes() {
        assert!(ObjCollision::parse(&le(&[0x2012_1224, 0, 0, 0, 0])).is_err());
        assert!(ObjCollision::parse(&le(&[VERSION, 0, 0, 0])).is_err());
        assert!(ObjCollision::parse(&le(&[VERSION, 0, 0, 0, 0, 9])).is_err());
        assert!(ObjCollision::parse(&le(&[VERSION, 0, 1000, 0, 0])).is_err());
        assert!(ObjCollision::parse(&le(&[VERSION, 0, 0, 0, 0])).is_ok());
    }
}
