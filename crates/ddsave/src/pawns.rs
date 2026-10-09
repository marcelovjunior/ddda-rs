//! Bounded, read-only fields from cSAVE_DATA_CMC. No identity data is decoded.
//! Schema: reflected save XML, cSAVE_DATA_CMC::vf03 0x00787f30 and
//! cSAVE_DATA_PARAM::vf03 0x00785790 (DDDA.exe 2017-12-13 Steam).
//! Missing fields are None; malformed present fields are errors. Knowledge
//! words are preserved without guessing their quest/enemy/area bit meanings.
use crate::xml::Node;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub reason: &'static str,
}
impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}
impl std::error::Error for FieldError {}
type Result<T> = std::result::Result<T, FieldError>;
fn err(name: &str, reason: &'static str) -> FieldError {
    FieldError {
        field: name.into(),
        reason,
    }
}
fn child<'d, 'a>(n: Node<'d, 'a>, name: &str) -> Result<Option<Node<'d, 'a>>> {
    let mut matches = n.children().filter(|c| c.name().as_deref() == Some(name));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(err(name, "duplicate field"));
    }
    Ok(first)
}
fn scalar<T: FromStr>(n: Node<'_, '_>, name: &str, tag: &str) -> Result<Option<T>> {
    child(n, name)?
        .map(|v| {
            if v.tag() != tag || v.children().next().is_some() {
                return Err(err(name, "wrong scalar type"));
            }
            v.parse().ok_or_else(|| err(name, "invalid scalar"))
        })
        .transpose()
}
fn float(n: Node<'_, '_>, name: &str) -> Result<Option<f32>> {
    let value = scalar::<f32>(n, name, "f32")?;
    if value.is_some_and(|v| !v.is_finite()) {
        return Err(err(name, "non-finite scalar"));
    }
    Ok(value)
}
fn array<T, const N: usize>(
    n: Node<'_, '_>,
    name: &str,
    ty: &str,
    read: impl Fn(Node<'_, '_>) -> Result<T>,
) -> Result<Option<[T; N]>> {
    let Some(a) = child(n, name)? else { return Ok(None) };
    if a.tag() != "array" || a.ty().as_deref() != Some(ty) || a.count() != Some(N) {
        return Err(err(name, "wrong array type or count"));
    }
    let mut values = Vec::with_capacity(N);
    for v in a.children() {
        if values.len() == N {
            return Err(err(name, "too many array elements"));
        }
        if v.tag() != ty {
            return Err(err(name, "wrong array element type"));
        }
        values.push(read(v)?);
    }
    Ok(Some(values.try_into().map_err(|_| err(name, "truncated array"))?))
}
fn numbers<T: FromStr, const N: usize>(n: Node<'_, '_>, name: &str, ty: &str) -> Result<Option<[T; N]>> {
    array(n, name, ty, |v| {
        if v.children().next().is_some() {
            return Err(err(name, "nested scalar"));
        }
        v.parse().ok_or_else(|| err(name, "invalid array scalar"))
    })
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Knowledge {
    pub study_flags: Option<[u32; 322]>,
    pub local_study_flags: Option<[u32; 322]>,
    pub encounter_frames: Option<[f32; 72]>,
    pub kill_counts: Option<[u32; 72]>,
    pub unique_counts: Option<[u8; 116]>,
}
/// Full item payload; flag bit semantics, owner IDs and key meanings remain raw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EquipmentItem {
    pub num: Option<i16>,
    pub item_no: Option<i16>,
    pub flag: Option<u32>,
    pub change_num: Option<u16>,
    pub days: [Option<u16>; 3],
    pub mutation_pool: Option<i8>,
    pub owner_id: Option<i8>,
    pub key: Option<u32>,
}
impl EquipmentItem {
    fn read(n: Node<'_, '_>) -> Result<Self> {
        if n.tag() != "class" || n.ty().as_deref() != Some("sItemManager::cITEM_PARAM_DATA") {
            return Err(err("mEquipItem", "wrong item class"));
        }
        Ok(Self {
            num: scalar(n, "data.mNum", "s16")?,
            item_no: scalar(n, "data.mItemNo", "s16")?,
            flag: scalar(n, "data.mFlag", "u32")?,
            change_num: scalar(n, "data.mChgNum", "u16")?,
            days: [
                scalar(n, "data.mDay1", "u16")?,
                scalar(n, "data.mDay2", "u16")?,
                scalar(n, "data.mDay3", "u16")?,
            ],
            mutation_pool: scalar(n, "data.mMutationPool", "s8")?,
            owner_id: scalar(n, "data.mOwnerId", "s8")?,
            key: scalar(n, "data.mKey", "u32")?,
        })
    }
}
pub const WEAPONS: [&str; 12] = [
    "SWORD", "MACE", "GSWORD", "DAGGER", "WAND", "WAND_DX", "HAMMER", "SHIELD", "SHIELD_L", "BOW", "BOW_L",
    "BOW_MG",
];
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skills {
    /// Indexed by WEAPONS; signed IDs and empty sentinels preserved.
    pub weapon: [Option<[i16; 6]>; 12],
    pub level_one: Option<[u32; 14]>,
    pub level_two: Option<[u32; 14]>,
    pub abilities: Option<[i8; 6]>,
    pub ability_info: Option<[u32; 4]>,
}
pub const INCLINATIONS: [&str; 9] = [
    "BELLIGERENT",
    "PRUDENT",
    "POOR_AIM",
    "STRATEGY",
    "TACTICS",
    "PROTECTION",
    "SAME_SUPPORT",
    "CURIOSITY",
    "GATHER",
];
pub const EXTRA_INCLINATIONS: [&str; 2] = ["TALK", "SKILL_USE"];
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inclination {
    pub info_type: Option<u32>,
    pub value: Option<f32>,
    pub status: Option<u32>,
}
fn info(n: Node<'_, '_>, key: &str, extra: bool) -> Result<Inclination> {
    let prefix = if extra {
        format!("mInfoEx[ INFO_TYPE_EX_{key} ]")
    } else {
        format!("mInfo[ INFO_TYPE_{key} ]")
    };
    Ok(Inclination {
        info_type: scalar(n, &format!("{prefix}.mInfoType"), "u32")?,
        value: float(n, &format!("{prefix}.mValue"))?,
        status: scalar(n, &format!("{prefix}.mStatus"), "u32")?,
    })
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PawnFields {
    pub knowledge: Knowledge,
    pub equipment: Option<[EquipmentItem; 12]>,
    pub present_item: Option<EquipmentItem>,
    pub skills: Skills,
    pub inclinations: [Inclination; 9],
    pub extra_inclinations: [Inclination; 2],
}
impl PawnFields {
    pub fn read(n: Node<'_, '_>) -> Result<Self> {
        if n.tag() != "class" || n.ty().as_deref() != Some("cSAVE_DATA_CMC") {
            return Err(err("pawn", "wrong class"));
        }
        let mut out = Self::default();
        out.knowledge = Knowledge {
            study_flags: numbers(n, "mStudyFlag", "u32")?,
            local_study_flags: numbers(n, "mLocalStudyFlag", "u32")?,
            encounter_frames: numbers(n, "mStudyData.EncountFrame", "f32")?,
            kill_counts: numbers(n, "mStudyData.KillCnt", "u32")?,
            unique_counts: numbers(n, "mStudyData.UniqueCnt", "u8")?,
        };
        if out
            .knowledge
            .encounter_frames
            .as_ref()
            .is_some_and(|v| v.iter().any(|f| !f.is_finite()))
        {
            return Err(err("mStudyData.EncountFrame", "non-finite scalar"));
        }
        if let Some(p) = child(n, "mParam")? {
            if p.tag() != "class" || p.ty().as_deref() != Some("cSAVE_DATA_PARAM") {
                return Err(err("mParam", "wrong class"));
            }
            out.equipment = array(p, "mEquipItem", "class", EquipmentItem::read)?;
            out.present_item = child(p, "mPresentItem")?.map(EquipmentItem::read).transpose()?;
            for (i, weapon) in WEAPONS.iter().enumerate() {
                out.skills.weapon[i] = numbers(p, &format!("mWeaponSkill[nWeapon::{weapon}]"), "s16")?;
            }
            out.skills.level_one = numbers(p, "mSkillLv1", "u32")?;
            out.skills.level_two = numbers(p, "mSkillLv2", "u32")?;
            out.skills.abilities = numbers(p, "mAbility", "s8")?;
            out.skills.ability_info = numbers(p, "mAbilityInfo", "u32")?;
        }
        for (i, key) in INCLINATIONS.iter().enumerate() {
            out.inclinations[i] = info(n, key, false)?;
        }
        for (i, key) in EXTRA_INCLINATIONS.iter().enumerate() {
            out.extra_inclinations[i] = info(n, key, true)?;
        }
        Ok(out)
    }
}
