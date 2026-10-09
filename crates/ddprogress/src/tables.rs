//! The data progression reads, loaded once by the host.

use ddparam::{AbilityEntry, ExpEntry, JobLevelEntry, LevelEntry};
use ddskill::catalogue::{ExeTables, SkillRecord};
use ddskill::skill_list::SkillList;
use ddskill::vocation::Vocation;

/// `rAdjustParam` (`ajp\0`, version 0x0201, u32 count, f32 values;
/// docs/FORMATS.md). The game's getter `rAdjustParam::vf15` (0x00ca47d0)
/// returns `values[i]` without a bound check.
pub fn parse_adjust(data: &[u8]) -> Result<Vec<f32>, String> {
    let word = |o: usize| -> Result<u32, String> {
        Ok(u32::from_le_bytes(data.get(o..o + 4).ok_or("truncated rAdjustParam")?.try_into().unwrap()))
    };
    if data.get(..4) != Some(&b"ajp\0"[..]) || word(4)? != 0x0201 {
        return Err("not an rAdjustParam v0x0201".into());
    }
    let n = word(8)? as usize;
    if data.len() != 12 + 4 * n {
        return Err(format!("rAdjustParam: {} bytes for {n} values", data.len()));
    }
    (0..n).map(|i| word(12 + 4 * i).map(f32::from_bits)).collect()
}

/// Everything the rules read.
#[derive(Debug, Clone)]
pub struct Tables {
    /// `param\pl\level\Lv<..>`, index = vocation id (index 0 repeats
    /// Fighter, as the loader 0x0047e2b0 does).
    pub growth: Vec<Vec<LevelEntry>>,
    /// `param\pl\JobLevel\JobLv<..>`, same indexing.
    pub ranks: Vec<Vec<JobLevelEntry>>,
    /// `param\pl\level\exp`.
    pub exp: Vec<ExpEntry>,
    /// `param\pl\other\PlAbilityParam`, index = augment id.
    pub abilities: Vec<AbilityEntry>,
    /// `etc\skill\skill`.
    pub skills: SkillList,
    /// The exe's skill records (vocation and weapon masks, kind).
    pub records: Vec<SkillRecord>,
    /// `param\game` (sGameSys+0xb8754, loaded by 0x00444740).
    pub game: Vec<f32>,
    /// `param\dlc` (sGameSys+0xb875c, loaded by 0x004447c0).
    pub dlc: Vec<f32>,
}

impl Tables {
    /// Loads from the install and the player's DDDA.exe.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe) -> Result<Self, String> {
        let read = |path: &str, class: &str| vfs.read(path, mtarc::type_hash(class));
        let mut growth = vec![];
        let mut ranks = vec![];
        for id in 0..10u32 {
            let v = Vocation::from_id(id.max(1)).unwrap();
            let l = read(&format!("param\\pl\\level\\Lv{}", v.param_name()), "rPlLevel")?;
            growth.push(ddparam::Level::from_bytes(&l).map_err(|e| e.0)?.entries);
            let r = read(&format!("param\\pl\\JobLevel\\JobLv{}", v.param_name()), "rPlJobLevel")?;
            ranks.push(ddparam::JobLevel::from_bytes(&r).map_err(|e| e.0)?.entries);
        }
        let exp = ddparam::Exp::from_bytes(&read("param\\pl\\level\\exp", "rPlExp")?).map_err(|e| e.0)?.entries;
        let abilities = ddparam::Ability::from_bytes(&read("param\\pl\\other\\PlAbilityParam", "rPlAbilityParam")?)
            .map_err(|e| e.0)?
            .entries;
        let skills = SkillList::from_bytes(&read(SkillList::PATH, SkillList::RESOURCE_CLASS)?).map_err(|e| e.0)?;
        let records = ExeTables::from_image(exe).map_err(|e| e.0)?.skills;
        let game = parse_adjust(&read("param\\game", "rAdjustParam")?)?;
        let dlc = parse_adjust(&read("param\\dlc", "rAdjustParam")?)?;
        Ok(Self { growth, ranks, exp, abilities, skills, records, game, dlc })
    }

    pub fn growth(&self, v: Vocation) -> &[LevelEntry] {
        &self.growth[v as usize]
    }
    pub fn ranks(&self, v: Vocation) -> &[JobLevelEntry] {
        &self.ranks[v as usize]
    }

    /// Pawn catch-up rate: `param\game` index 0 (read by 0x00443470 through
    /// `rAdjustParam::vf15(0)`; 0 when the resource is missing).
    pub fn pawn_catch_up(&self) -> f32 {
        self.game.first().copied().unwrap_or(0.0)
    }
    /// Discipline factor of item 0x361 (Asura Armor) worn in slot +0x3e4:
    /// `param\dlc` index 7 (0x00782d10).
    pub fn asura_armor_rate(&self) -> f32 {
        self.dlc.get(7).copied().unwrap_or(0.0)
    }
    /// Discipline factor of item 0x47f (Ring of Perseverance) per ring
    /// slot (+0x3f4, +0x3f8): `param\dlc` index 0x1c (0x00782d10).
    pub fn perseverance_rate(&self) -> f32 {
        self.dlc.get(0x1c).copied().unwrap_or(0.0)
    }
}

/// Augment ids whose value 0x00783340 reports: equipped -> `mFVal0`,
/// otherwise `mFVal1`. Other ids leave the caller's default untouched.
// Translated from the jump table of 0x00783340 (DDDA.exe 2017-12-13 Steam).
fn plain_value(id: u16) -> bool {
    matches!(id,
        0..=6 | 0xb | 0xc | 0xe..=0x10 | 0x14..=0x1a | 0x1e..=0x24 | 0x28..=0x2e
        | 0x32..=0x34 | 0x36..=0x38 | 0x3c..=0x42 | 0x46 | 0x47 | 0x4a..=0x4c
        | 0x50..=0x56 | 0x5a..=0x5e | 100..=0x69 | 0x6b..=0x72)
}

/// Value of augment `id` for a character with `equipped` augments.
/// Translated from 0x00783340 (DDDA.exe 2017-12-13 Steam). `None` where
/// the exe writes nothing (unknown ids, missing rows); id 10
/// (Leg-Strength) gives -1.0 / 0.0; id 53 also has `mSVal0` (not returned).
pub fn augment_value(tables: &Tables, equipped: &[Option<u16>], id: u16) -> Option<f32> {
    let row = tables.abilities.get(id as usize)?;
    let on = equipped.contains(&Some(id));
    if id == 10 {
        return Some(if on { -1.0 } else { 0.0 });
    }
    (plain_value(id) || id == 0x35).then_some(if on { row.float_0 } else { row.float_1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjust_format() {
        let mut b = b"ajp\0".to_vec();
        b.extend_from_slice(&0x0201u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&0.7f32.to_le_bytes());
        b.extend_from_slice(&24f32.to_le_bytes());
        assert_eq!(parse_adjust(&b).unwrap(), [0.7, 24.0]);
        assert!(parse_adjust(&b[..b.len() - 1]).is_err());
        b[4] = 2;
        assert!(parse_adjust(&b).is_err());
    }

    #[test]
    fn handled_augments() {
        assert!(plain_value(0x5d) && plain_value(0x3e) && plain_value(0x6c));
        assert!(!plain_value(10) && !plain_value(0x6a) && !plain_value(7) && !plain_value(0x35));
    }
}
