//! Typed pawn records read from the save tree.
//!
//! Field names are the engine's (the save XML is a DTI property dump). Where
//! a field's meaning is inferred rather than traced, the doc comment says so.
//! See `docs/PAWNS.md` for the evidence.

use crate::xml::Node;

/// Where a pawn record's data comes from, decoded from `mRomPawnUniqueID`.
///
/// Encoding traced in `0x00795010` (build a pawn from its ID) and
/// `0x00794f10` / `0x007951c0` (which write `(level + 0x1000) << 16 | index`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomPawnId {
    /// `0xFFFFFFFF`: not a built-in pawn (the player's own main pawn, or one
    /// identified by its network ID instead).
    None,
    /// `0x1LLLIIII`: a generated ("nora") pawn: record `index` of the
    /// `etc\editPattern\romNoraPawnNN` file for `level`'s band, built at
    /// `level`. The exe takes `level` from bits 16..24 (`& 0xff`, values over
    /// 200 become 0).
    Nora { level: u8, index: u16 },
    /// `0x2000IIII`: slot `index` of the 120-slot pawn pool in sGameSys
    /// (`+0x16e0`, stride `0x14e8`) when `index < 20`, otherwise record
    /// `index` of `etc\editPattern\romPawnInfo`. Values over 99 are read as 20.
    Pool { index: u16 },
    /// Any other value (the exe builds nothing for it).
    Other(u32),
}

impl RomPawnId {
    pub fn decode(id: u32) -> RomPawnId {
        match id {
            0xFFFF_FFFF => RomPawnId::None,
            _ if id & 0xF000_0000 == 0x1000_0000 => {
                RomPawnId::Nora { level: (id >> 16) as u8, index: id as u16 }
            }
            _ if id & 0xF000_0000 == 0x2000_0000 => RomPawnId::Pool { index: id as u16 },
            _ => RomPawnId::Other(id),
        }
    }

    /// The `romNoraPawnNN` file for a nora pawn's level: the first band whose
    /// upper bound is at least `level`. Bounds are the 33 bytes at
    /// `0x0150a384` (read by `0x00794f10`, `0x00795010`, `0x007951c0`).
    pub fn nora_file(level: u8) -> Option<usize> {
        NORA_LEVEL_BANDS.iter().position(|&max| level <= max)
    }
}

/// Upper level bound of each `romNoraPawnNN` file (bytes at `0x0150a384`).
pub const NORA_LEVEL_BANDS: [u8; 33] = [
    5, 9, 13, 17, 21, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110,
    115, 120, 125, 130, 135, 140, 145, 150, 170, 200,
];

/// Appearance and identity (`cSAVE_DATA_EDIT`). Only the fields needed to
/// identify a pawn are typed; the rest stay in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// `(u8*)mNameStr`, UTF-8, NUL-terminated in 25 bytes.
    pub name: String,
    /// `mGender`: 0 male, 1 female (inferred: female nora names are drawn
    /// from the `...Female` lists for 1; see docs/PAWNS.md).
    pub gender: u8,
    /// `mNickname`: index into `id\message\common\nickname_<lang>`.
    pub nickname: u32,
    pub voice: u8,
    /// `mPersonality`: untraced.
    pub personality: u8,
    pub hair: u8,
}

impl Edit {
    pub fn read(n: Node<'_, '_>) -> Option<Edit> {
        Some(Edit {
            name: n.byte_string("(u8*)mNameStr")?,
            gender: n.get("mGender")?,
            nickname: n.get("mNickname")?,
            voice: n.get("mVoice")?,
            personality: n.get("mPersonality")?,
            hair: n.get("mHairNo")?,
        })
    }
}

/// One equipped item (`sItemManager::cITEM_PARAM_DATA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub num: i16,
    /// `data.mItemNo`: index into the item list; -1 is empty.
    pub item_no: i16,
    pub flag: u32,
}

impl Item {
    pub fn read(n: Node<'_, '_>) -> Option<Item> {
        Some(Item { num: n.get("data.mNum")?, item_no: n.get("data.mItemNo")?, flag: n.get("data.mFlag")? })
    }
}

/// Level, vocation and progression (`cSAVE_DATA_PARAM`).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub level: u8,
    /// `mJob`: vocation number. Pawns use 1 Fighter, 2 Strider, 3 Mage,
    /// 7 Warrior, 8 Ranger, 9 Sorcerer (the level tables `0x00795610`
    /// selects for them; see docs/PAWNS.md).
    pub job: u8,
    /// `mJobLevel`, ten entries indexed by vocation number - 1 (inferred).
    pub job_levels: Vec<u8>,
    /// `mEquipItem`, twelve slots.
    pub equipment: Vec<Item>,
    pub exp: u32,
    pub gold: i32,
    /// `OnlinePoint`: for the Arisen, the Rift Crystal balance (inferred: it
    /// equals the adventure log's `mLogTotalRim` in a save with no spending).
    pub online_point: i32,
    pub hp_max: f32,
    pub stamina: f32,
}

impl Param {
    pub fn read(n: Node<'_, '_>) -> Option<Param> {
        Some(Param {
            level: n.get("mLevel")?,
            job: n.get("mJob")?,
            job_levels: n.array("mJobLevel")?,
            equipment: n.child("mEquipItem")?.children().map(Item::read).collect::<Option<_>>()?,
            exp: n.get("mExp")?,
            gold: n.get("mGold")?,
            online_point: n.get("OnlinePoint")?,
            hp_max: n.get("mHpMax")?,
            stamina: n.get("mStamina")?,
        })
    }
}

/// One of the nine pawn inclinations or two extra values stored as
/// `mInfo[ INFO_TYPE_X ]` / `mInfoEx[ INFO_TYPE_EX_X ]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// The enum name inside the brackets, e.g. `INFO_TYPE_BELLIGERENT`.
    pub key: String,
    pub info_type: u32,
    pub value: f32,
    /// Present for `mInfo`, absent for `mInfoEx`.
    pub status: Option<u32>,
}

fn read_infos(n: Node<'_, '_>) -> Vec<Info> {
    let mut out: Vec<Info> = Vec::new();
    for c in n.children() {
        let Some(name) = c.name() else { continue };
        let Some(open) = name.find("[ ") else { continue };
        if !(name.starts_with("mInfo[") || name.starts_with("mInfoEx[")) {
            continue;
        }
        let Some(close) = name.find(" ].") else { continue };
        let key = &name[open + 2..close];
        let field = &name[close + 3..];
        let idx = match out.iter().position(|i| i.key == key) {
            Some(i) => i,
            None => {
                out.push(Info { key: key.to_string(), info_type: 0, value: 0.0, status: None });
                out.len() - 1
            }
        };
        match field {
            "mInfoType" => out[idx].info_type = c.parse().unwrap_or(0),
            "mValue" => out[idx].value = c.parse().unwrap_or(0.0),
            "mStatus" => out[idx].status = c.parse(),
            _ => {}
        }
    }
    out
}

/// A pawn's network-side bookkeeping (`cOnlinePawnInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct OnlineInfo {
    /// `mNetUniqueId.mData`, 64 bytes; all zero for built-in pawns.
    pub net_unique_id: Vec<u8>,
    pub with_time: u32,
    pub employ_count: u32,
    pub die_count: u32,
    pub clear_quest_count: u32,
    pub lent_count: u32,
    pub rom_id: RomPawnId,
    pub rom_id_raw: u32,
    pub arisen_name: String,
    pub arisen_level: u32,
}

impl OnlineInfo {
    pub fn read(n: Node<'_, '_>) -> Option<OnlineInfo> {
        let raw: u32 = n.get("mRomPawnUniqueID")?;
        Some(OnlineInfo {
            net_unique_id: n.child("mNetUniqueId")?.array("mData")?,
            with_time: n.get("mWithTime")?,
            employ_count: n.get("mEmployCnt")?,
            die_count: n.get("mDieCnt")?,
            clear_quest_count: n.get("mClearQuestCnt")?,
            lent_count: n.get("mLentCnt")?,
            rom_id: RomPawnId::decode(raw),
            rom_id_raw: raw,
            arisen_name: n.child("mArisenName")?.byte_string("( u8* )mEditName")?,
            arisen_level: n.get("mArisenLevel")?,
        })
    }
}

/// A full pawn (`cSAVE_DATA_CMC`): the main pawn, a hired pawn, a remembered
/// close friend, or an entry of the built-in `rRomPawnInfo` list.
#[derive(Debug, Clone, PartialEq)]
pub struct Pawn {
    pub edit: Edit,
    pub param: Param,
    /// `mPawnType`: 1 for the player's own main pawn, 0 for others
    /// (observed in saves; untraced).
    pub pawn_type: i32,
    /// `mInfo[...]` inclinations and `mInfoEx[...]` values.
    pub infos: Vec<Info>,
    pub online: OnlineInfo,
}

impl Pawn {
    pub fn read(n: Node<'_, '_>) -> Option<Pawn> {
        Some(Pawn {
            edit: Edit::read(n.child("mEdit")?)?,
            param: Param::read(n.child("mParam")?)?,
            pawn_type: n.get("mPawnType")?,
            infos: read_infos(n),
            online: OnlineInfo::read(n.child("mOnlinePawnInfo")?)?,
        })
    }
}

/// A remembered pawn reference (`cSAVE_DATA_ONLINE_PAWN`): favourites, past
/// summons and pawns that hired yours.
#[derive(Debug, Clone, PartialEq)]
pub struct PawnRef {
    pub net_unique_id: Vec<u8>,
    pub employ_count: u32,
    pub rom_id: RomPawnId,
    pub rom_id_raw: u32,
    pub arisen_name: String,
}

impl PawnRef {
    pub fn read(n: Node<'_, '_>) -> Option<PawnRef> {
        let raw: u32 = n.get("mRomPawnUniqueID")?;
        Some(PawnRef {
            net_unique_id: n.array("mNetworkUniqueID")?,
            employ_count: n.get("mEmployCount")?,
            rom_id: RomPawnId::decode(raw),
            rom_id_raw: raw,
            arisen_name: n.child("mArisenName")?.byte_string("( u8* )mEditName")?,
        })
    }
}

/// An entry of the summon history (`cPawnHistoryInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub edit: Edit,
    pub online: OnlineInfo,
    pub level: u32,
    pub job: i32,
    pub skills: Vec<i32>,
    pub equipment: Vec<i32>,
    pub flag: u8,
}

impl HistoryEntry {
    pub fn read(n: Node<'_, '_>) -> Option<HistoryEntry> {
        Some(HistoryEntry {
            edit: Edit::read(n.child("mPawnEdit")?)?,
            online: OnlineInfo::read(n.child("mOnlinePawnInfo")?)?,
            level: n.get("mLevel")?,
            job: n.get("mJob")?,
            skills: n.array("mSkill")?,
            equipment: n.array("mEquip")?,
            flag: n.get("mFlag")?,
        })
    }
}

/// The pawn-related state of one `sSave::playerData` slot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerPawns {
    /// The Arisen's `OnlinePoint` (Rift Crystals, inferred).
    pub arisen_online_point: i32,
    pub arisen_level: u8,
    /// `mPlCmcEditAndParam.mCmc`: three slots, main pawn first (observed:
    /// slot 0 has `mPawnType` 1 and no built-in ID; slots 1-2 are hired).
    pub party: Vec<Pawn>,
    /// `mCmcBeFlag`: whether each party slot is occupied (inferred).
    pub party_present: Vec<bool>,
    pub favorites: Vec<PawnRef>,
    pub borrowed: Vec<PawnRef>,
    pub lent: Vec<PawnRef>,
    pub close_friends: Vec<Pawn>,
    pub history: Vec<HistoryEntry>,
}

fn first_n<T>(list: Option<Node<'_, '_>>, n: usize, f: impl Fn(Node<'_, '_>) -> Option<T>) -> Option<Vec<T>> {
    list?.children().take(n).map(f).collect()
}

impl PlayerPawns {
    /// Reads one `sSave::playerData` (the root's `mPlayerDataManual` or
    /// `mPlayerDataBase`). Lists are cut to their stored counts.
    pub fn read(player: Node<'_, '_>) -> Option<PlayerPawns> {
        let ep = player.child("mPlCmcEditAndParam")?;
        let arisen = Param::read(ep.path("mPl/mParam")?)?;
        let party = ep.child("mCmc")?.children().map(Pawn::read).collect::<Option<Vec<_>>>()?;
        let game = player.child("mPlGameData")?;
        let net = player.child("mNetGameData")?;
        let count = |name: &str| net.get::<i64>(name).map(|v| v.max(0) as usize);
        Some(PlayerPawns {
            arisen_online_point: arisen.online_point,
            arisen_level: arisen.level,
            party,
            party_present: game.array("mCmcBeFlag")?,
            favorites: first_n(net.child("mFavoritePawn"), count("mFavoritePawnNum")?, PawnRef::read)?,
            borrowed: first_n(net.child("mBorrowPawn"), count("mBorrowPawnNum")?, PawnRef::read)?,
            lent: first_n(net.child("mLentPawn"), count("mLentPawnNum")?, PawnRef::read)?,
            close_friends: first_n(net.child("mCloseFriendPawn"), count("mCloseFriendPawnCount")?, Pawn::read)?,
            history: first_n(net.child("mPawnHistory"), count("mPawnHistoryNum")?, HistoryEntry::read)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_id_decoding() {
        assert_eq!(RomPawnId::decode(u32::MAX), RomPawnId::None);
        // (level + 0x1000) << 16 | index, as written by 0x00794f10.
        let id = (4u32 + 0x1000) * 0x10000 + 634;
        assert_eq!(id, 0x1004_027A);
        assert_eq!(RomPawnId::decode(id), RomPawnId::Nora { level: 4, index: 634 });
        assert_eq!(RomPawnId::decode(0x2000_000E), RomPawnId::Pool { index: 14 });
        assert_eq!(RomPawnId::decode(0x3000_0001), RomPawnId::Other(0x3000_0001));
    }

    #[test]
    fn nora_level_bands() {
        assert_eq!(RomPawnId::nora_file(1), Some(0));
        assert_eq!(RomPawnId::nora_file(5), Some(0));
        assert_eq!(RomPawnId::nora_file(6), Some(1));
        assert_eq!(RomPawnId::nora_file(151), Some(31));
        assert_eq!(RomPawnId::nora_file(200), Some(32));
        assert_eq!(RomPawnId::nora_file(201), None);
    }
}
