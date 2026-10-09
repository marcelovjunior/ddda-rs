//! `cSAVE_DATA_CMC`: one complete pawn record, as the builders produce it and
//! as the save and `rRomPawnInfo` store it. Only the fields the builders
//! write or copy are modelled; names are the engine's (see docs/PAWNS.md §9).

use ddsave::xml::Node;
use mtxfs::{Object, Value, Xfs};

/// The `u8` appearance fields of `cSAVE_DATA_EDIT` other than the name, in
/// save order. `mNickname` (u32) is kept separately.
pub const EDIT_U8_FIELDS: [&str; 41] = [
    "mGender",
    "mVoice",
    "mPersonality",
    "mFaceBase",
    "mFaceEye",
    "mFaceEyebrow",
    "mFaceNose",
    "mFaceMouth",
    "mFaceEar",
    "mOfsXFaceEye",
    "mOfsYFaceEye",
    "mOfsXFaceEyebrow",
    "mOfsYFaceEyebrow",
    "mOfsYFaceNose",
    "mOfsYFaceMouth",
    "mOfsYFaceEar",
    "mOfsZFaceEar",
    "mSclFaceEye",
    "mSclFaceNose",
    "mBodySlender",
    "mBodyMuscle",
    "mBustScale",
    "mHumanBodyAll",
    "mHumanHead",
    "mHumanBody",
    "mHumanArm",
    "mHumanLeg",
    "mMovement",
    "mPosture",
    "mWrinkleFace",
    "mMakeupFace",
    "mScarNo",
    "mColorHair",
    "mColorEyeR",
    "mColorEyeL",
    "mColorBody",
    "mColorBeard",
    "mColorEyebrow",
    "mColorMakeup",
    "mHairNo",
    "mBeardNo",
];

/// Weapon classes of `mWeaponSkill[nWeapon::X]`, index = enum value - 1
/// (the exe stores class `w` at `cSAVE_DATA_PARAM + 0x64 + w * 12`; class
/// 0 is unnamed and never serialised).
pub const WEAPONS: [&str; 12] =
    ["SWORD", "MACE", "GSWORD", "DAGGER", "WAND", "WAND_DX", "HAMMER", "SHIELD", "SHIELD_L", "BOW", "BOW_L", "BOW_MG"];

/// The nine inclinations `mInfo[ INFO_TYPE_X ]`, index = `mInfoType`.
pub const INFO_TYPES: [&str; 9] = [
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
/// `mInfoEx[ INFO_TYPE_EX_X ]`.
pub const INFO_EX_TYPES: [&str; 2] = ["TALK", "SKILL_USE"];

/// Number of `u32` words in `mStudyFlag` (322 words = 0x2840 bits).
pub const STUDY_WORDS: usize = 322;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// `(u8*)mNameStr`, 25 bytes (UTF-8, NUL padded).
    pub name: [u8; 25],
    pub nickname: u32,
    /// Values of [`EDIT_U8_FIELDS`].
    pub fields: [u8; 41],
}

impl Edit {
    pub fn get(&self, field: &str) -> Option<u8> {
        EDIT_U8_FIELDS.iter().position(|f| *f == field).map(|i| self.fields[i])
    }
    pub fn set(&mut self, field: &str, value: u8) {
        let i = EDIT_U8_FIELDS.iter().position(|f| *f == field).expect("known edit field");
        self.fields[i] = value;
    }
    pub fn name_str(&self) -> String {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(25);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }
    /// Copies a NUL-terminated name the way `strncpy(dst, src, 0x19)` does.
    pub fn set_name(&mut self, src: &[u8]) {
        let mut out = [0u8; 25];
        let n = src.iter().position(|&b| b == 0).unwrap_or(src.len()).min(25);
        out[..n].copy_from_slice(&src[..n]);
        self.name = out;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equip {
    pub num: i16,
    pub item_no: i16,
    pub flag: u32,
    /// `data.mChgNum`, `data.mDay1..3`: for items with built-in skills
    /// (0x74b..0x76c), the two skill numbers and 0xffff, 0xffff.
    pub extra: [u16; 4],
}

impl Equip {
    pub const EMPTY: Equip = Equip { num: 0, item_no: -1, flag: 0, extra: [0; 4] };
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub level: u8,
    pub job: u8,
    /// `mJobLevel[job]`, ten vocations.
    pub job_level: [u8; 10],
    /// `mWeaponSkill[nWeapon::X]`, see [`WEAPONS`].
    pub weapon_skill: [[i16; 6]; 12],
    pub skill_lv1: [u32; 14],
    pub skill_lv2: [u32; 14],
    pub ability: [i8; 6],
    pub ability_info: [u32; 4],
    pub equip: [Equip; 12],
    pub hp: f32,
    pub hp_max: f32,
    pub hp_max_white: f32,
    pub stamina: f32,
    pub stamina_base: f32,
    pub stamina_lv: f32,
    pub attack: f32,
    pub defend: f32,
    pub mgc_attack: f32,
    pub mgc_defend: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cmc {
    pub edit: Edit,
    pub param: Param,
    pub status: i32,
    pub study_flag: Vec<u32>,
    /// `mInfo[k].mValue` and `.mStatus`; `mInfoType` is always `k`.
    pub info_value: [f32; 9],
    pub info_status: [u32; 9],
    /// `mInfoEx[k].mValue`.
    pub info_ex_value: [f32; 2],
    pub use_scene_talk: [u32; 6],
    pub pawn_type: i32,
    pub employ_game_play_time: u32,
    /// `mOnlinePawnInfo.mRomPawnUniqueID`.
    pub rom_id: u32,
}

impl Cmc {
    /// A record as the initialisers leave it: `0x00787dc0` with
    /// `0x00787b10` (edit), `0x00786540` (param), `0x005d0940` (mInfo) and
    /// `0x0077b2a0` (online info). Only modelled fields are set.
    ///
    /// Translated from those functions (DDDA.exe 2017-12-13 Steam). The
    /// `mInfo` default value is the BSS float at `0x018fa5b4`, which has no
    /// writer in the exe and is therefore 0.0 (compared: freshly built
    /// pawns in a real save have 0.0 there). `mInfoEx` starts at
    /// `1000.0 * 0.5` (`0x0182e0d0`).
    ///
    /// The edit defaults of `0x00787b10` are not modelled (left zero): both
    /// builders overwrite the whole edit with `0x00787b70`.
    pub fn initial() -> Cmc {
        Cmc {
            edit: Edit { name: [0; 25], nickname: 0, fields: [0; 41] },
            param: Param {
                level: 1,
                job: 0,
                job_level: [1; 10],
                weapon_skill: [[-1; 6]; 12],
                skill_lv1: [0; 14],
                skill_lv2: [0; 14],
                ability: [-1; 6],
                ability_info: [0; 4],
                equip: [Equip::EMPTY; 12],
                hp: 1.0,
                hp_max: 1.0,
                hp_max_white: 1.0,
                stamina: 1.0,
                stamina_base: 0.0,
                stamina_lv: 0.0,
                attack: 1.0,
                defend: 1.0,
                mgc_attack: 1.0,
                mgc_defend: 1.0,
            },
            status: 0,
            study_flag: vec![0; STUDY_WORDS],
            info_value: [0.0; 9],
            info_status: [0; 9],
            info_ex_value: [500.0; 2],
            use_scene_talk: [0; 6],
            pawn_type: 0,
            employ_game_play_time: 0,
            rom_id: u32::MAX,
        }
    }

    /// Reads a `cSAVE_DATA_EDIT` save node.
    pub fn from_save_edit(e: Node<'_, '_>) -> Option<Edit> {
        let mut edit = Edit { name: [0; 25], nickname: e.get("mNickname")?, fields: [0; 41] };
        let name: Vec<u8> = e.array("(u8*)mNameStr")?;
        edit.name.copy_from_slice(name.get(..25)?);
        for (i, f) in EDIT_U8_FIELDS.iter().enumerate() {
            edit.fields[i] = e.get(f)?;
        }
        Some(edit)
    }

    /// Reads a record from a save tree node of type `cSAVE_DATA_CMC`.
    pub fn from_save(n: Node<'_, '_>) -> Option<Cmc> {
        let edit = Self::from_save_edit(n.child("mEdit")?)?;
        let p = n.child("mParam")?;
        let mut weapon_skill = [[0i16; 6]; 12];
        for (w, name) in WEAPONS.iter().enumerate() {
            let v: Vec<i16> = p.array(&format!("mWeaponSkill[nWeapon::{name}]"))?;
            weapon_skill[w].copy_from_slice(v.get(..6)?);
        }
        let mut equip = [Equip::EMPTY; 12];
        for (slot, item) in p.child("mEquipItem")?.children().enumerate().take(12) {
            equip[slot] = Equip {
                num: item.get("data.mNum")?,
                item_no: item.get("data.mItemNo")?,
                flag: item.get("data.mFlag")?,
                extra: [
                    item.get("data.mChgNum")?,
                    item.get("data.mDay1")?,
                    item.get("data.mDay2")?,
                    item.get("data.mDay3")?,
                ],
            };
        }
        let mut info_value = [0.0; 9];
        let mut info_status = [0; 9];
        for (k, t) in INFO_TYPES.iter().enumerate() {
            info_value[k] = n.get(&format!("mInfo[ INFO_TYPE_{t} ].mValue"))?;
            info_status[k] = n.get(&format!("mInfo[ INFO_TYPE_{t} ].mStatus"))?;
        }
        let mut info_ex_value = [0.0; 2];
        for (k, t) in INFO_EX_TYPES.iter().enumerate() {
            info_ex_value[k] = n.get(&format!("mInfoEx[ INFO_TYPE_EX_{t} ].mValue"))?;
        }
        Some(Cmc {
            edit,
            param: Param {
                level: p.get("mLevel")?,
                job: p.get("mJob")?,
                job_level: arr(p.array("mJobLevel")?)?,
                weapon_skill,
                skill_lv1: arr(p.array("mSkillLv1")?)?,
                skill_lv2: arr(p.array("mSkillLv2")?)?,
                ability: arr(p.array("mAbility")?)?,
                ability_info: arr(p.array("mAbilityInfo")?)?,
                equip,
                hp: p.get("mHp")?,
                hp_max: p.get("mHpMax")?,
                hp_max_white: p.get("mHpMaxWhite")?,
                stamina: p.get("mStamina")?,
                stamina_base: p.get("mStaminaBase")?,
                stamina_lv: p.get("mStaminaLv")?,
                attack: p.get("mBasicAttack")?,
                defend: p.get("mBasicDefend")?,
                mgc_attack: p.get("mBasicMgcAttack")?,
                mgc_defend: p.get("mBasicMgcDefend")?,
            },
            status: n.get("mStatus")?,
            study_flag: n.array("mStudyFlag")?,
            info_value,
            info_status,
            info_ex_value,
            use_scene_talk: arr(n.array("mUseSceneTalk")?)?,
            pawn_type: n.get("mPawnType")?,
            employ_game_play_time: n.get("mEmployGamePlayTime")?,
            rom_id: n.child("mOnlinePawnInfo")?.get("mRomPawnUniqueID")?,
        })
    }

    /// Reads a record from an XFS `cSAVE_DATA_CMC` object (`rRomPawnInfo`).
    pub fn from_xfs(x: &Xfs, o: &Object) -> Result<Cmc, String> {
        let e = xobj(x, o, "mEdit")?;
        let p = xobj(x, o, "mParam")?;
        let mut edit = Edit { name: [0; 25], nickname: xint(x, e, "mNickname")? as u32, fields: [0; 41] };
        let name = xints(x, e, "(u8*)mNameStr")?;
        for (i, b) in name.iter().take(25).enumerate() {
            edit.name[i] = *b as u8;
        }
        for (i, f) in EDIT_U8_FIELDS.iter().enumerate() {
            edit.fields[i] = xint(x, e, f)? as u8;
        }
        let mut weapon_skill = [[0i16; 6]; 12];
        for (w, name) in WEAPONS.iter().enumerate() {
            let v = xints(x, p, &format!("mWeaponSkill[nWeapon::{name}]"))?;
            for (k, s) in v.iter().take(6).enumerate() {
                weapon_skill[w][k] = *s as i16;
            }
        }
        let mut equip = [Equip::EMPTY; 12];
        for (slot, item) in xobjs(x, p, "mEquipItem")?.into_iter().enumerate().take(12) {
            equip[slot] = Equip {
                num: xint(x, item, "data.mNum")? as i16,
                item_no: xint(x, item, "data.mItemNo")? as i16,
                flag: xint(x, item, "data.mFlag")? as u32,
                extra: [
                    xint(x, item, "data.mChgNum")? as u16,
                    xint(x, item, "data.mDay1")? as u16,
                    xint(x, item, "data.mDay2")? as u16,
                    xint(x, item, "data.mDay3")? as u16,
                ],
            };
        }
        let mut info_value = [0.0; 9];
        let mut info_status = [0; 9];
        for (k, t) in INFO_TYPES.iter().enumerate() {
            info_value[k] = xf32(x, o, &format!("mInfo[ INFO_TYPE_{t} ].mValue"))?;
            info_status[k] = xint(x, o, &format!("mInfo[ INFO_TYPE_{t} ].mStatus"))? as u32;
        }
        let mut info_ex_value = [0.0; 2];
        for (k, t) in INFO_EX_TYPES.iter().enumerate() {
            info_ex_value[k] = xf32(x, o, &format!("mInfoEx[ INFO_TYPE_EX_{t} ].mValue"))?;
        }
        let online = xobj(x, o, "mOnlinePawnInfo")?;
        let ints_as = |name: &str| -> Result<Vec<i64>, String> { xints(x, p, name) };
        Ok(Cmc {
            edit,
            param: Param {
                level: xint(x, p, "mLevel")? as u8,
                job: xint(x, p, "mJob")? as u8,
                job_level: arr(ints_as("mJobLevel")?.into_iter().map(|v| v as u8).collect()).ok_or("mJobLevel")?,
                weapon_skill,
                skill_lv1: arr(ints_as("mSkillLv1")?.into_iter().map(|v| v as u32).collect()).ok_or("mSkillLv1")?,
                skill_lv2: arr(ints_as("mSkillLv2")?.into_iter().map(|v| v as u32).collect()).ok_or("mSkillLv2")?,
                ability: arr(ints_as("mAbility")?.into_iter().map(|v| v as i8).collect()).ok_or("mAbility")?,
                ability_info: arr(ints_as("mAbilityInfo")?.into_iter().map(|v| v as u32).collect())
                    .ok_or("mAbilityInfo")?,
                equip,
                hp: xf32(x, p, "mHp")?,
                hp_max: xf32(x, p, "mHpMax")?,
                hp_max_white: xf32(x, p, "mHpMaxWhite")?,
                stamina: xf32(x, p, "mStamina")?,
                stamina_base: xf32(x, p, "mStaminaBase")?,
                stamina_lv: xf32(x, p, "mStaminaLv")?,
                attack: xf32(x, p, "mBasicAttack")?,
                defend: xf32(x, p, "mBasicDefend")?,
                mgc_attack: xf32(x, p, "mBasicMgcAttack")?,
                mgc_defend: xf32(x, p, "mBasicMgcDefend")?,
            },
            status: xint(x, o, "mStatus")? as i32,
            study_flag: xints(x, o, "mStudyFlag")?.into_iter().map(|v| v as u32).collect(),
            info_value,
            info_status,
            info_ex_value,
            use_scene_talk: arr(xints(x, o, "mUseSceneTalk")?.into_iter().map(|v| v as u32).collect())
                .ok_or("mUseSceneTalk")?,
            pawn_type: xint(x, o, "mPawnType")? as i32,
            employ_game_play_time: xint(x, o, "mEmployGamePlayTime")? as u32,
            rom_id: xint(x, online, "mRomPawnUniqueID")? as u32,
        })
    }

    /// Every modelled field as `(save path, value)`, for comparisons.
    pub fn fields(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut push = |k: String, v: String| out.push((k, v));
        push("mEdit/(u8*)mNameStr".into(), format!("{:?}", self.edit.name_str()));
        push("mEdit/mNickname".into(), self.edit.nickname.to_string());
        for (i, f) in EDIT_U8_FIELDS.iter().enumerate() {
            push(format!("mEdit/{f}"), self.edit.fields[i].to_string());
        }
        let p = &self.param;
        push("mParam/mLevel".into(), p.level.to_string());
        push("mParam/mJob".into(), p.job.to_string());
        push("mParam/mJobLevel".into(), format!("{:?}", p.job_level));
        for (w, name) in WEAPONS.iter().enumerate() {
            push(format!("mParam/mWeaponSkill[nWeapon::{name}]"), format!("{:?}", p.weapon_skill[w]));
        }
        push("mParam/mSkillLv1".into(), format!("{:?}", p.skill_lv1));
        push("mParam/mSkillLv2".into(), format!("{:?}", p.skill_lv2));
        push("mParam/mAbility".into(), format!("{:?}", p.ability));
        push("mParam/mAbilityInfo".into(), format!("{:?}", p.ability_info));
        for (slot, e) in p.equip.iter().enumerate() {
            push(format!("mParam/mEquipItem[{slot}].mItemNo"), e.item_no.to_string());
            push(format!("mParam/mEquipItem[{slot}].mNum"), e.num.to_string());
            push(format!("mParam/mEquipItem[{slot}].mFlag"), format!("{:#x}", e.flag));
            push(format!("mParam/mEquipItem[{slot}].extra"), format!("{:?}", e.extra));
        }
        for (k, v) in [
            ("mHp", p.hp),
            ("mHpMax", p.hp_max),
            ("mHpMaxWhite", p.hp_max_white),
            ("mStamina", p.stamina),
            ("mStaminaBase", p.stamina_base),
            ("mStaminaLv", p.stamina_lv),
            ("mBasicAttack", p.attack),
            ("mBasicDefend", p.defend),
            ("mBasicMgcAttack", p.mgc_attack),
            ("mBasicMgcDefend", p.mgc_defend),
        ] {
            push(format!("mParam/{k}"), format!("{v:?}"));
        }
        push("mStatus".into(), self.status.to_string());
        push("mStudyFlag".into(), format!("{:x?}", self.study_flag));
        for (k, t) in INFO_TYPES.iter().enumerate() {
            push(format!("mInfo[ INFO_TYPE_{t} ].mValue"), format!("{:?}", self.info_value[k]));
            push(format!("mInfo[ INFO_TYPE_{t} ].mStatus"), self.info_status[k].to_string());
        }
        for (k, t) in INFO_EX_TYPES.iter().enumerate() {
            push(format!("mInfoEx[ INFO_TYPE_EX_{t} ].mValue"), format!("{:?}", self.info_ex_value[k]));
        }
        push("mUseSceneTalk".into(), format!("{:?}", self.use_scene_talk));
        push("mPawnType".into(), self.pawn_type.to_string());
        push("mEmployGamePlayTime".into(), self.employ_game_play_time.to_string());
        push("mOnlinePawnInfo/mRomPawnUniqueID".into(), format!("{:#x}", self.rom_id));
        out
    }

    /// Field paths whose values differ, with both values.
    pub fn diff(&self, other: &Cmc) -> Vec<(String, String, String)> {
        self.fields()
            .into_iter()
            .zip(other.fields())
            .filter(|(a, b)| a.1 != b.1)
            .map(|(a, b)| (a.0, a.1, b.1))
            .collect()
    }
}

fn arr<T: Copy + Default, const N: usize>(v: Vec<T>) -> Option<[T; N]> {
    if v.len() < N {
        return None;
    }
    let mut out = [T::default(); N];
    out.copy_from_slice(&v[..N]);
    Some(out)
}

fn xfield<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a [Value], String> {
    x.get(o, name).ok_or_else(|| format!("{}: no field {name}", x.class_of(o).name().unwrap_or("?")))
}

fn xint(x: &Xfs, o: &Object, name: &str) -> Result<i64, String> {
    match xfield(x, o, name)?.first() {
        Some(Value::U(u)) => Ok(*u as i64),
        Some(Value::S(s)) => Ok(*s),
        Some(Value::Bool(b)) => Ok(*b as i64),
        other => Err(format!("{name}: not an integer: {other:?}")),
    }
}

fn xf32(x: &Xfs, o: &Object, name: &str) -> Result<f32, String> {
    match xfield(x, o, name)?.first() {
        Some(Value::F(f)) if !f.is_empty() => Ok(f[0] as f32),
        other => Err(format!("{name}: not a float: {other:?}")),
    }
}

fn xints(x: &Xfs, o: &Object, name: &str) -> Result<Vec<i64>, String> {
    xfield(x, o, name)?
        .iter()
        .map(|v| match v {
            Value::U(u) => Ok(*u as i64),
            Value::S(s) => Ok(*s),
            Value::Bool(b) => Ok(*b as i64),
            other => Err(format!("{name}: not an integer: {other:?}")),
        })
        .collect()
}

fn xobjs<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<Vec<&'a Object>, String> {
    xfield(x, o, name)?
        .iter()
        .map(|v| match v {
            Value::Object(Some(b)) => Ok(&**b),
            other => Err(format!("{name}: not an object: {other:?}")),
        })
        .collect()
}

fn xobj<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Object, String> {
    xobjs(x, o, name)?.into_iter().next().ok_or_else(|| format!("{name}: empty"))
}
