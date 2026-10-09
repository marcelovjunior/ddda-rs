//! Reader for `rLayout` (`.lot`) files: every placement a stage cell
//! makes. That covers static models and objects (`_s00` and `_pNN` files),
//! enemies (`_eNN`), NPCs and hostile humans (`_nNN`), and AI sensor
//! targets (`_tNN`).
//!
//! Grammar (DDDA.exe Steam 2017-12-13):
//!
//! ```text
//! rLayout::load            0x00CC1790  "lot\0", u32 version 16, u32 record count
//! rLayout::SetInfo::load   0x00CC1F60  per record: s32 id, u32 kind (< 75), then the
//!                                      kind's class: its own fields, then its parent's,
//!                                      ..., cSetInfoCoord's (name and transform) last
//! ```
//!
//! The two loader addresses were corroborated in our Ghidra project: the
//! MtDTI labels place them at `rLayout` vtable slot 10 and
//! `rLayout::SetInfo` slot 5. The per-class field tables below are from
//! Riftstone (`src/riftstone/lot.py`, MIT, see
//! `third_party/riftstone-LICENSE.txt`), which read them from each class's
//! loader and `createProperty` and proved them on all 6,209 vanilla layouts.
//! `ddtool lot-scan` re-checks them against the install.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    U8,
    S16,
    U16,
    S32,
    U32,
    F32,
    V3,
    Str,
    /// u32 count (at most 16), then that many s16.
    S16List,
    /// u32 presence flag; if non-zero, a class name string and that sensor
    /// class's fields.
    Sensor,
}

use FieldType::*;

type Fields = &'static [(&'static str, FieldType)];

const COORD: Fields = &[
    ("mSetID", S32),
    ("mName", Str),
    ("mOrder", U32),
    ("mPosition", V3),
    ("mAngle", V3),
    ("mScale", V3),
    ("mDrawDistance", F32),
    ("mIsOnSplitAreaIgnore", U8),
];
const PAWN: Fields = &[
    ("mAreaHitNo", S32),
    ("HP倍率設定の有無", U8),
    ("HPの倍率", F32),
    ("攻撃力倍率設定の有無", U8),
    ("攻撃力の変化倍率", F32),
    ("防御力倍率設定の有無", U8),
    ("防御力の変化倍率", F32),
    ("魔法力倍率設定の有無", U8),
    ("魔法力の変化倍率", F32),
    ("魔法防御力倍率設定の有無", U8),
    ("魔法防御力の変化倍率", F32),
    ("mAIKnowledgeFlag", U32),
    ("mAIKind", U32),
    ("mIsAINoTarget", U8),
    ("mIsUseItemEveryone", U8),
    ("mIsDragonBallDrop", U8),
];
const ENEMY: Fields = &[
    ("mLifePointGroup", U32),
    ("mUseFirstSetThinkTbl", U8),
    ("mStartTableNo", S32),
    ("mBossFlag", U8),
    ("mBgmBossFlag", U8),
    ("mEmItemFlag", S32),
    ("mEmItemTable", S32),
    ("mDieSet", U32),
    ("死体速攻消滅フラグ", U8),
    ("死亡消滅時間延長", U8),
    ("mRandomSetIgnore", U8),
    ("mExperienceOW", U32),
    ("mFsmFilePath", Str),
    ("センサー半径倍率を使用する", U8),
    ("センサー半径倍率", F32),
];
const EM0100: Fields = &[
    ("mEquipType", U32),
    ("mIsLeader", U8),
    ("mIsWeaponHandStart", U8),
    ("mIsBallistaLicense", U8),
];
const EM0101: Fields = &[("mEquipType", U32), ("mIsLeader", U8), ("mIsWeaponHandStart", U8)];
const EM0103: Fields = &[
    ("mEquipType", U32),
    ("mIsLeader", U8),
    ("mIsWeaponHandStart", U8),
    ("mIsBallistaLicense", U8),
    ("mType", S32),
];
const EM0200: Fields = &[("mWolfType", U32), ("mIsRandamScale", U8), ("mWolfScale", F32)];
const EM0400: Fields = &[("壁スタート", U8), ("光学迷彩", U8)];
const EM0500: Fields = &[("mPartsVariationNo", U32)];
const EM0503: Fields = &[("mEquipType", S32), ("mPartsVariationNo", U32)];
const EM0600: Fields = &[
    ("mHoverMainMode", U8),
    ("mbOmBreakCheck", U8),
    ("mOmBreakGroup", U32),
    ("mOmBreakID", U32),
    ("mHoverActionStart", U8),
];
const EM0700: Fields = &[("mPowerUpLv", U32)];
const EM5000: Fields = &[
    ("mType", U32),
    ("mWeaponType", U32),
    ("mTuskType", U32),
    ("西部サイクロ", U8),
    ("覚者の証サイクロ", U8),
    ("覚者の証サイクロのときの経験値", U32),
    ("覚者の証サイクロのときの残りHPの倍率", F32),
    ("覚者の証サイクロのときの足ののけぞり値", F32),
    ("覚者の証サイクロのときの足のぶっとび値", F32),
];
const EM5001: Fields = &[
    ("mType", U32),
    ("mIsWeaponInitRemove", U8),
    ("mWeaponInitPos", V3),
    ("mWeaponInitAngle", V3),
];
const EM5100: Fields = &[("mInitMediumBreak", U32)];
const EM5101: Fields = &[("mGolemNo", U32)];
const EM5101_00: Fields = &[("mGolemNo", U32), ("mMediumRegionNo", U32)];
const EM5200: Fields = &[("死亡演出カメラをする", U8)];
const EM5300: Fields = &[("mStartWaitTime", F32)];
const EM5500B: Fields = &[
    ("触手が魔法出来る数", U32),
    ("触手がワープ出来る数", U32),
    ("触手がワープ出来る範囲", V3),
];
const EM5800: Fields = &[("mSeqButtleSts", U32)];
const DIESET: Fields = &[("mIsDieSet", U8)];
const EM5900: Fields = &[
    ("mSetType", U32),
    ("飛ばない", U8),
    ("固有喋り", U8),
    ("リッチひょうい", U8),
    ("旋回飛行しない", U8),
];
const EM6000: Fields = &[("死神特別設置か？", U8), ("死神特別設置フレーム", F32)];
const EM9807: Fields = &[("動作時間(秒)", F32), ("サウンドOFF", U8)];
const INSMODEL: Fields = &[
    ("mLightGroup", U32),
    ("mOverwriteLightGroup", U8),
    ("mTransMode", U32),
    ("mOverwriteTransMode", U8),
];
const OMMODEL: Fields = &[
    ("mSetTableID", S16),
    ("mSetTableIDNight", S16),
    ("mSetItemNo", S16),
    ("mSetItemNoNight", S16),
    ("mStartHour", S32),
    ("mEndHour", S32),
    ("mFree00", U32),
    ("mFree01", U32),
    ("mAngleType", U32),
    ("mFlag", U32),
];
const SEMAP: Fields = &[("mSeGroupId", S16), ("mSePriority", S16), ("mMapIconType", S16)];
const DOORMSG: Fields = &[
    ("mLockMsgNo", S16),
    ("mOutMsgNo", S16),
    ("mOpenMsgNo", S16),
    ("mCheckAngle", F32),
    ("mCheckRange", F32),
];
const ANGLE: Fields = &[("mCheckAngle", F32), ("mCheckRange", F32)];
const FSMPATH: Fields = &[("FSMPath", Str)];
const NPC: Fields = &[
    ("mNpcPriority", S32),
    ("FSMPath", Str),
    ("mNpcId", S32),
    ("mScrAdjustOff", U8),
    ("mObjAdjustOff", U8),
    ("mPriority", S32),
    ("mGoodsOff", U8),
    ("mSimpleModelType", S32),
    ("mPartsVariationNo", U32),
    ("mClothType", U32),
    ("mUseMouseJoint", U8),
    ("mHumanEnemyKind", U32),
    ("mHumanEnemyID", U32),
    ("mRank", U32),
    ("mMultiNpcKind", U32),
    ("mUse24Schedule", U8),
    ("mIsHumanEnemyLeader", U8),
    ("mExperienceOW", U32),
    ("mpTarget", Sensor),
    ("外部指定", U8),
    ("待機行動タイプ", S32),
    ("内部フラグ", S32),
    ("待機行動番号", S32),
    ("基本待機時間", F32),
    ("ランダム待機時間", F32),
    ("しぐさ行動番号", S32),
    ("基本しぐさ時間", F32),
    ("ランダムしぐさ時間", F32),
];
const SENSOR_TARGET: Fields = &[("mpTarget", Sensor)];

const SENSOR_BASE: Fields = &[
    ("mTypeBit", U32),
    ("mSndLv", S32),
    ("mStatusFlag", U32),
    ("mPos", V3),
    ("mDir", V3),
    ("mRange", F32),
    ("mObjectID", S32),
    ("mAttr", U32),
    ("mSphereRange", F32),
    ("mJntNo", S32),
];
const SENSOR_GENERAL_POINT: Fields = &[
    ("mGroup", U32),
    ("mId", U32),
    ("mActType", U32),
    ("mIsAllDay", U8),
    ("mSTime", U32),
    ("mETime", U32),
    ("mShape", S32),
    ("mSize", V3),
];
const SENSOR_NPC: Fields = &[
    ("mEnable", U8),
    ("mActType", U32),
    ("mMotType", U32),
    ("mActFrame", F32),
    ("mNearFlag", U8),
    ("mRotFlag", U8),
    ("mIsAllDay", U8),
    ("mStartTime", U32),
    ("mEndTime", U32),
    ("mFix", U8),
    ("mMessQuestNo", S32),
    ("mMessId", S32),
    ("mMessDispSec", F32),
    ("mNpcId", S32),
];
const SENSOR_STAGE_ACTION: Fields = &[
    ("mStageActionType", U32),
    ("mPawnID", S32),
    ("mOnOffKind", U32),
    ("mResetTime", F32),
    ("mWeight", U32),
    ("CHK_TYPE", U8),
    ("ITEM_NO", U16),
    ("ITEM_NUM", U8),
    ("mActParamIdx", S16List),
    ("mActParamQuestNo", S16List),
];
const SENSOR_UNIT: Fields = &[("mOffsetPos", V3), ("mOffsetDir", V3)];

/// A sensor target class's own fields (its base's follow).
fn sensor_fields(class: &str) -> Option<Fields> {
    Some(match class {
        "cAISensorTarget" => &[],
        "cAISensorTargetGeneralPoint" => SENSOR_GENERAL_POINT,
        "cAISensorTargetNpc" => SENSOR_NPC,
        "cAISensorTargetStageAction" => SENSOR_STAGE_ACTION,
        "cAISensorTargetUnit" => SENSOR_UNIT,
        _ => return None,
    })
}

/// The class of a record kind and its field groups, most derived first.
/// `None` for kinds the loader refuses or that no layout uses.
pub fn kind(kind: u32) -> Option<(&'static str, Vec<Fields>)> {
    let enemy = |own: Fields| vec![own, ENEMY, PAWN, COORD];
    let om = |own: Vec<Fields>| own.into_iter().chain([OMMODEL, PAWN, COORD]).collect::<Vec<_>>();
    Some(match kind {
        1 => ("cSetInfoCoord", vec![COORD]),
        2 => ("cSetInfoPawn", vec![PAWN, COORD]),
        3 => ("cSetInfoEnemy", vec![ENEMY, PAWN, COORD]),
        4 => ("cSetInfoEnemy0100", enemy(EM0100)),
        5 => ("cSetInfoEnemy0101", enemy(EM0101)),
        6 | 55 => ("cSetInfoEnemy0102", enemy(EM0101)),
        7 => ("cSetInfoEnemy0200", enemy(EM0200)),
        8 => ("cSetInfoEnemy0201", enemy(EM0200)),
        9 => ("cSetInfoEnemy0202", enemy(EM0200)),
        10 => ("cSetInfoEnemy0400", enemy(EM0400)),
        11 => ("cSetInfoEnemy0401", enemy(EM0400)),
        12 => ("cSetInfoEnemy0402", enemy(EM0400)),
        13 => ("cSetInfoEnemy0403", enemy(EM0400)),
        14 => ("cSetInfoEnemy0500", enemy(EM0500)),
        15 => ("cSetInfoEnemy0501", enemy(EM0500)),
        16 => ("cSetInfoEnemy0502", enemy(EM0500)),
        17 => ("cSetInfoEnemy0503", enemy(EM0503)),
        18 => ("cSetInfoEnemy0504", enemy(EM0500)),
        19 => ("cSetInfoEnemy0600", enemy(EM0600)),
        20 => ("cSetInfoEnemy0700", enemy(EM0700)),
        21 => ("cSetInfoEnemy0900", enemy(&[])),
        22 => ("cSetInfoEnemy2000", enemy(EM0500)),
        23 => ("cSetInfoEnemy5000", enemy(EM5000)),
        24 => ("cSetInfoEnemy5100", enemy(EM5100)),
        25 => ("cSetInfoEnemy5101", enemy(EM5101)),
        26 => ("cSetInfoEnemy5101_00", enemy(EM5101_00)),
        27 => ("cSetInfoEnemy5200", enemy(EM5200)),
        28 => ("cSetInfoEnemy5300", enemy(EM5300)),
        29 => ("cSetInfoEnemy5500", enemy(&[])),
        30 => ("cSetInfoEnemy5500B", enemy(EM5500B)),
        31 => ("cSetInfoEnemy5501", enemy(&[])),
        32 => ("cSetInfoEnemy5800", enemy(EM5800)),
        33 => ("cSetInfoEnemy5801", enemy(DIESET)),
        34 => ("cSetInfoEnemy5900", enemy(EM5900)),
        35 => ("cSetInfoEnemy6000", enemy(EM6000)),
        36 => ("cSetInfoEnemy8000", enemy(&[])),
        37 => ("cSetInfoEnemy8100", enemy(&[])),
        38 => ("cSetInfoEnemy8200", enemy(&[])),
        39 => ("cSetInfoEnemy8300", enemy(&[])),
        40 => ("cSetInfoEnemy8500", enemy(&[])),
        41 => ("cSetInfoEnemy8600", enemy(&[])),
        42 => ("cSetInfoEnemy8700", enemy(&[])),
        43 => ("cSetInfoEnemy8900", enemy(&[])),
        44 => ("cSetInfoEnemy9000", enemy(&[])),
        45 => ("cSetInfoEnemy9807", enemy(EM9807)),
        46 => ("cSetInfoInsModel", vec![INSMODEL, PAWN, COORD]),
        47 => ("cSetInfoNpc", vec![NPC, PAWN, COORD]),
        48 => ("cSetInfoOmModel", om(vec![])),
        49 => ("cSetInfoOmFSM", om(vec![FSMPATH])),
        50 => ("cSetInfoOmFsmPlusSM", om(vec![SEMAP, FSMPATH])),
        51 => ("cSetInfoOmPlusAng", om(vec![ANGLE])),
        52 => ("cSetInfoOmPlusSM", om(vec![SEMAP])),
        53 => ("cSetInfoOmDoorMsg", om(vec![DOORMSG, SEMAP])),
        54 => ("cSetInfoSensorTarget", vec![SENSOR_TARGET]),
        56 => ("cSetInfoEnemy0203", enemy(EM0200)),
        57 => ("cSetInfoEnemy0204", enemy(EM0200)),
        58 => ("cSetInfoEnemy0404", enemy(EM0400)),
        59 => ("cSetInfoEnemy0405", enemy(EM0400)),
        60 => ("cSetInfoEnemy0406", enemy(EM0400)),
        61 => ("cSetInfoEnemy0407", enemy(EM0400)),
        62 => ("cSetInfoEnemy0408", enemy(EM0400)),
        63 => ("cSetInfoEnemy0505", enemy(EM0500)),
        64 => ("cSetInfoEnemy0506", enemy(EM0500)),
        65 => ("cSetInfoEnemy0507", enemy(EM0500)),
        66 => ("cSetInfoEnemy5001", enemy(EM5001)),
        67 => ("cSetInfoEnemy7000", enemy(DIESET)),
        68 => ("cSetInfoEnemy9100", enemy(&[])),
        69 => ("cSetInfoEnemy0103", enemy(EM0103)),
        70 => ("cSetInfoEnemy5500C", enemy(EM5500B)),
        71 => ("cSetInfoEnemy7001", enemy(&[])),
        72 => ("cSetInfoEnemy5400", enemy(&[])),
        73 => ("cSetInfoEnemy5401", enemy(&[])),
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    F32(f32),
    V3([f32; 3]),
    Str(Vec<u8>),
    S16List(Vec<i16>),
    Sensor(Option<Box<SensorTarget>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SensorTarget {
    pub class: String,
    pub fields: Vec<(&'static str, Value)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub id: i32,
    pub kind: u32,
    pub class: &'static str,
    /// Every field in file order, most derived class first.
    pub fields: Vec<(&'static str, Value)>,
}

/// Name and transform shared by every record (`cSetInfoCoord`).
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub name: String,
    pub position: [f32; 3],
    /// Euler angles in radians (order not yet traced).
    pub angle: [f32; 3],
    pub scale: [f32; 3],
    pub draw_distance: f32,
}

impl Record {
    pub fn get(&self, name: &str) -> Option<&Value> {
        // The coordinate block is last, so search from the end: a derived
        // class never shadows it.
        self.fields.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn placement(&self) -> Option<Placement> {
        let v3 = |n| match self.get(n) {
            Some(Value::V3(v)) => Some(*v),
            _ => None,
        };
        Some(Placement {
            name: match self.get("mName")? {
                Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                _ => return None,
            },
            position: v3("mPosition")?,
            angle: v3("mAngle")?,
            scale: v3("mScale")?,
            draw_distance: match self.get("mDrawDistance")? {
                Value::F32(f) => *f,
                _ => return None,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LOT at 0x{:x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let b = self.d.get(self.p..self.p + N).ok_or_else(|| Error {
            offset: self.p,
            message: "the file ends inside a record".into(),
        })?;
        self.p += N;
        Ok(b.try_into().unwrap())
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn string(&mut self) -> Result<Vec<u8>, Error> {
        let rest = &self.d[self.p..];
        let n = rest.iter().position(|&b| b == 0).ok_or_else(|| Error {
            offset: self.p,
            message: "unterminated string".into(),
        })?;
        let s = rest[..n].to_vec();
        self.p += n + 1;
        Ok(s)
    }

    fn fields(&mut self, groups: &[Fields], out: &mut Vec<(&'static str, Value)>) -> Result<(), Error> {
        for group in groups {
            for &(name, ty) in group.iter() {
                let v = match ty {
                    U8 => Value::Int(i64::from(self.take::<1>()?[0])),
                    S16 => Value::Int(i64::from(i16::from_le_bytes(self.take()?))),
                    U16 => Value::Int(i64::from(u16::from_le_bytes(self.take()?))),
                    S32 => Value::Int(i64::from(i32::from_le_bytes(self.take()?))),
                    U32 => Value::Int(i64::from(self.u32()?)),
                    F32 => Value::F32(f32::from_le_bytes(self.take()?)),
                    V3 => {
                        let b: [u8; 12] = self.take()?;
                        Value::V3(std::array::from_fn(|i| {
                            f32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap())
                        }))
                    }
                    Str => Value::Str(self.string()?),
                    S16List => {
                        let at = self.p;
                        let n = self.u32()? as usize;
                        if n > 16 {
                            return Err(Error {
                                offset: at,
                                message: format!("{name}: {n} entries, the game copies 16"),
                            });
                        }
                        let mut l = Vec::with_capacity(n);
                        for _ in 0..n {
                            l.push(i16::from_le_bytes(self.take()?));
                        }
                        Value::S16List(l)
                    }
                    Sensor => {
                        if self.u32()? == 0 {
                            Value::Sensor(None)
                        } else {
                            let at = self.p;
                            let class = String::from_utf8_lossy(&self.string()?).into_owned();
                            let own = sensor_fields(&class).ok_or_else(|| Error {
                                offset: at,
                                message: format!("unknown sensor target class {class:?}"),
                            })?;
                            let mut f = Vec::new();
                            self.fields(&[own, SENSOR_BASE], &mut f)?;
                            Value::Sensor(Some(Box::new(SensorTarget { class, fields: f })))
                        }
                    }
                };
                out.push((name, v));
            }
        }
        Ok(())
    }
}

/// Parses a complete layout file.
pub fn parse(d: &[u8]) -> Result<Vec<Record>, Error> {
    if d.len() < 12 || &d[0..4] != b"lot\0" {
        return Err(Error {
            offset: 0,
            message: "not a layout file".into(),
        });
    }
    let mut r = Reader { d, p: 4 };
    let version = r.u32()?;
    if version != 16 {
        return Err(Error {
            offset: 4,
            message: format!("version {version}, the game loads 16"),
        });
    }
    let count = r.u32()? as usize;
    if count > (d.len() - 12) / 8 {
        return Err(Error {
            offset: 8,
            message: format!("{count} records cannot fit"),
        });
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = r.p;
        let id = i32::from_le_bytes(r.take()?);
        let k = r.u32()?;
        let (class, groups) = kind(k).ok_or_else(|| Error {
            offset: at + 4,
            message: format!("record {i}: kind {k} not loaded"),
        })?;
        let mut fields = Vec::new();
        r.fields(&groups, &mut fields)?;
        out.push(Record {
            id,
            kind: k,
            class,
            fields,
        });
    }
    if r.p != d.len() {
        return Err(Error {
            offset: r.p,
            message: format!("{} bytes after the last record", d.len() - r.p),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(out: &mut Vec<u8>, name: &str, pos: [f32; 3]) {
        out.extend_from_slice(&5i32.to_le_bytes()); // mSetID
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(&0u32.to_le_bytes()); // mOrder
        for v in pos.iter().chain(&[0.0, 1.5, 0.0]).chain(&[1.0, 1.0, 1.0]) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(-1.0f32).to_le_bytes());
        out.push(0);
    }

    #[test]
    fn parses_a_coord_record() {
        let mut d = b"lot\0".to_vec();
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&7i32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        coord(&mut d, "om0545_58m50n_U100", [10.0, 20.0, 30.0]);
        let recs = parse(&d).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].class, "cSetInfoCoord");
        let p = recs[0].placement().unwrap();
        assert_eq!(p.name, "om0545_58m50n_U100");
        assert_eq!(p.position, [10.0, 20.0, 30.0]);
        assert_eq!(p.angle, [0.0, 1.5, 0.0]);
        assert_eq!(p.draw_distance, -1.0);
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut d = b"lot\0".to_vec();
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.push(0);
        assert!(parse(&d).is_err());
    }
}
