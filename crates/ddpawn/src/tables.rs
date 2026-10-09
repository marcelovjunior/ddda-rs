//! The other resources the pawn builder reads, and one table from the exe.

/// `etc\skill\skill` (`rSkillList`, magic `skl\0`, version 0x200).
///
/// Layout from the loader `0x00cdec00`: three counted lists of u32
/// records. Field order on disk is (id, job points, job level) for normal
/// skills and augments and (id, skill level, job points, job level) for
/// weapon skills; the builder reads the id and the job level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillList {
    pub normal: Vec<SkillEntry>,
    pub custom: Vec<SkillEntry>,
    pub ability: Vec<SkillEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillEntry {
    pub id: i32,
    /// Custom skills only: 1 for the base skill, 2 for its upgraded form
    /// (sets `mSkillLv1` or `mSkillLv2`, `0x00796080`). 0 elsewhere.
    pub skill_lv: i32,
    pub job_point: i32,
    pub job_level: i32,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.p..self.p + 4).ok_or_else(|| format!("truncated at {}", self.p))?;
        self.p += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        self.u32().map(|v| v as i32)
    }
}

impl SkillList {
    pub fn parse(b: &[u8]) -> Result<SkillList, String> {
        let mut r = Reader { b, p: 0 };
        if r.u32()? != 0x006c_6b73 || r.u32()? != 0x200 {
            return Err("not skl v0x200".into());
        }
        let n = r.u32()? as usize;
        let mut normal = Vec::with_capacity(n);
        for _ in 0..n {
            normal.push(SkillEntry { id: r.i32()?, skill_lv: 0, job_point: r.i32()?, job_level: r.i32()? });
        }
        let n = r.u32()? as usize;
        let mut custom = Vec::with_capacity(n);
        for _ in 0..n {
            custom.push(SkillEntry { id: r.i32()?, skill_lv: r.i32()?, job_point: r.i32()?, job_level: r.i32()? });
        }
        let n = r.u32()? as usize;
        let mut ability = Vec::with_capacity(n);
        for _ in 0..n {
            ability.push(SkillEntry { id: r.i32()?, skill_lv: 0, job_point: r.i32()?, job_level: r.i32()? });
        }
        if r.p != b.len() {
            return Err(format!("{} trailing bytes", b.len() - r.p));
        }
        Ok(SkillList { normal, custom, ability })
    }
}

/// `AI\preset_charainfo` (`rAIPresetCharaInfo`, magic `PCIF`, version 1):
/// the loader `0x00ca7170` keeps the file as is. Records of five i32 at
/// +0xc: three inclination indices (0xff none) and the talk and skill-use
/// kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharaInfo {
    pub entries: Vec<[i32; 5]>,
}

impl CharaInfo {
    pub fn parse(b: &[u8]) -> Result<CharaInfo, String> {
        let mut r = Reader { b, p: 0 };
        if r.u32()? != 0x4649_4350 || r.u32()? != 1 {
            return Err("not PCIF v1".into());
        }
        let n = r.u32()? as usize;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            entries.push([r.i32()?, r.i32()?, r.i32()?, r.i32()?, r.i32()?]);
        }
        Ok(CharaInfo { entries })
    }
}

/// `AI\preset_study` (`rAIPresetStudy`, magic `PCST`, version 1), layout
/// from the loader `0x00ca75d0`: a six-word header, then `index[h2]`,
/// `entries[h3]` of three words `(kind, start, end)`, `groups[h4]` (entry
/// numbers) and `bits[h5]` (knowledge bit numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Study {
    pub index: Vec<u32>,
    pub entries: Vec<[u32; 3]>,
    pub groups: Vec<u32>,
    pub bits: Vec<u32>,
}

impl Study {
    pub fn parse(b: &[u8]) -> Result<Study, String> {
        let mut r = Reader { b, p: 0 };
        if r.u32()? != 0x5453_4350 || r.u32()? != 1 {
            return Err("not PCST v1".into());
        }
        let (n_index, n_entries, n_groups, n_bits) =
            (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        let mut words = |n: usize| -> Result<Vec<u32>, String> { (0..n).map(|_| r.u32()).collect() };
        let index = words(n_index)?;
        let flat = words(n_entries * 3)?;
        let groups = words(n_groups)?;
        let bits = words(n_bits)?;
        let entries = flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        Ok(Study { index, entries, groups, bits })
    }

    /// Sets the knowledge bits of `entry` in `flags` (`mStudyFlag`).
    ///
    /// Translated from `0x00ca7670` (DDDA.exe 2017-12-13 Steam): kind 0
    /// sets the bit numbers `bits[start..end]` (those below 0x2840); any
    /// other kind applies the entries `groups[start..end]` recursively.
    /// (The exe also posts a notification for a newly set bit once the
    /// game is past a point, `sGameSys+0x30 > 1099`; not modelled.)
    pub fn apply(&self, entry: usize, flags: &mut [u32]) -> Result<(), String> {
        self.apply_depth(entry, flags, 0)
    }

    fn apply_depth(&self, entry: usize, flags: &mut [u32], depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("study entries recurse too deep".into());
        }
        let [kind, start, end] = *self.entries.get(entry).ok_or("study entry out of range")?;
        for i in start..end {
            if kind == 0 {
                let bit = *self.bits.get(i as usize).ok_or("study bit index out of range")?;
                if bit < 0x2840 {
                    flags[(bit >> 5) as usize] |= 1 << (bit & 31);
                }
            } else {
                let e = *self.groups.get(i as usize).ok_or("study group index out of range")?;
                self.apply_depth(e as usize, flags, depth + 1)?;
            }
        }
        Ok(())
    }
}

/// The two `rItemList` fields the equipment code reads (`etc\item\itemList`,
/// magic `ITL2`, 128-byte records, record N = item N; layout per the
/// Riftstone reference, MIT).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSlots {
    /// `+0x34 >> 28`: the equipment slot (category) of each item.
    pub slot: Vec<u8>,
    /// `+0x38 & 0xc00`: items with either bit are never equipped
    /// (`0x00786b90`).
    pub not_equippable: Vec<bool>,
}

impl ItemSlots {
    pub fn parse(b: &[u8]) -> Result<ItemSlots, String> {
        if b.get(..4) != Some(b"ITL2") {
            return Err("not ITL2".into());
        }
        let count = u32::from_le_bytes(b.get(8..12).ok_or("truncated")?.try_into().unwrap()) as usize;
        if b.len() != 16 + count * 128 {
            return Err(format!("{} bytes for {count} records", b.len()));
        }
        let word = |i: usize, o: usize| u32::from_le_bytes(b[16 + i * 128 + o..16 + i * 128 + o + 4].try_into().unwrap());
        Ok(ItemSlots {
            slot: (0..count).map(|i| (word(i, 0x34) >> 28) as u8).collect(),
            not_equippable: (0..count).map(|i| word(i, 0x38) & 0xc00 != 0).collect(),
        })
    }
}

/// Which vocations may use each skill: the first word of the 32-byte
/// records at `0x014f3660` in DDDA.exe's `.rdata` (bit `job` set = usable),
/// read by `0x00796080`.
///
/// The table is read at run time from the player's own `DDDA.exe` through
/// `ddexe` (build check, address mapping), so no exe bytes are kept in this
/// repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMasks {
    pub masks: Vec<u32>,
}

/// Address and record count of the skill table.
pub const SKILL_TABLE_VA: u32 = 0x014f_3660;
pub const SKILL_TABLE_LEN: usize = 430;

impl SkillMasks {
    /// Reads the masks from the bytes of `DDDA.exe` (packed or unpacked).
    pub fn from_exe(exe: &[u8]) -> Result<SkillMasks, String> {
        Self::from_image(&ddexe::Exe::parse(exe)?)
    }

    /// Reads the masks from a parsed image (`ddexe::ExeFile::image`).
    pub fn from_image(exe: &ddexe::Exe) -> Result<SkillMasks, String> {
        let records = exe.records(SKILL_TABLE_VA, 32, SKILL_TABLE_LEN)?;
        let masks = records.iter().map(|r| r.u32(0)).collect::<Result<_, _>>()?;
        Ok(SkillMasks { masks })
    }
}

/// Built-in skill attachments of items 0x74b..0x76c: the 89 records of 12
/// bytes at `0x01539f78` in `.rdata` (`(item, _, a, _, b, _)` as u16), read
/// by `0x0045e130`. Read from the player's `DDDA.exe` like [`SkillMasks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemKeyTable {
    pub rows: Vec<(i16, u16, u16)>,
}

pub const ITEM_KEY_TABLE_VA: u32 = 0x0153_9f78;
pub const ITEM_KEY_TABLE_LEN: usize = 89;

impl ItemKeyTable {
    pub fn from_exe(exe: &[u8]) -> Result<ItemKeyTable, String> {
        Self::from_image(&ddexe::Exe::parse(exe)?)
    }

    /// Reads the rows from a parsed image (`ddexe::ExeFile::image`).
    pub fn from_image(exe: &ddexe::Exe) -> Result<ItemKeyTable, String> {
        let records = exe.records(ITEM_KEY_TABLE_VA, 12, ITEM_KEY_TABLE_LEN)?;
        let rows = records
            .iter()
            .map(|r| Ok((r.i16(0)?, r.u16(4)?, r.u16(8)?)))
            .collect::<Result<_, ddexe::Error>>()?;
        Ok(ItemKeyTable { rows })
    }
}

