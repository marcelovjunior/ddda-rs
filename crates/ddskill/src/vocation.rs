//! Vocation ids and the vocation-change rules.
//!
//! Ids are `cPlayerInfo.mJob` (+0x10). English names come from the labels
//! `job_1`..`job_9` in `id\DDN\message\common\common_message_eng`; the
//! internal names are the `rPlLevel` / `rPlJobLevel` file names loaded by
//! 0x0047e2b0 (index = id; index 0 repeats Fighter).

/// A vocation (`mJob`). Discriminants are the game's ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Vocation {
    Fighter = 1,
    Strider = 2,
    Mage = 3,
    MysticKnight = 4,
    Assassin = 5,
    MagickArcher = 6,
    Warrior = 7,
    Ranger = 8,
    Sorcerer = 9,
}

/// Item weapon category: the high byte of a weapon item id (`mMainWpnId >> 24`,
/// 0x00780590). Index into the per-category tables below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WeaponCategory {
    Unarmed = 0,
    Sword = 1,
    Mace = 2,
    Longsword = 3,
    Dagger = 4,
    Staff = 5,
    Archistaff = 6,
    Warhammer = 7,
    Shield = 8,
    MagickShield = 9,
    Bow = 10,
    Longbow = 11,
    MagickBow = 12,
}

impl WeaponCategory {
    pub const ALL: [Self; 13] = [
        Self::Unarmed, Self::Sword, Self::Mace, Self::Longsword, Self::Dagger,
        Self::Staff, Self::Archistaff, Self::Warhammer, Self::Shield,
        Self::MagickShield, Self::Bow, Self::Longbow, Self::MagickBow,
    ];

    pub fn from_item_id(item_id: i32) -> Self {
        if item_id == -1 {
            return Self::Unarmed;
        }
        let c = (item_id as u32 >> 24) & 0xff;
        Self::ALL.get(c as usize).copied().unwrap_or(Self::Unarmed)
    }

    /// Bit used in the skill table's weapon mask (`1 << category`).
    pub fn mask(self) -> u32 {
        1 << self as u8
    }

    /// First skill id of the category's skill range and its length
    /// (0x014f6c18 / 0x014f6c4c).
    pub fn skill_range(self) -> (i32, i32) {
        const FIRST: [i32; 13] = [0, 10, 10, 70, 120, 180, 180, 70, 250, 290, 330, 380, 330];
        const LEN: [i32; 13] = [10, 60, 60, 50, 60, 70, 70, 50, 40, 40, 50, 40, 50];
        (FIRST[self as usize], LEN[self as usize])
    }

    /// First weapon ("custom") skill id and count (0x014f6c80 / 0x014f6cb4).
    /// A weapon skill's stamina row in `PlStamina_<weapon>` is
    /// `skill_id - first` (0x00b60e20).
    pub fn weapon_skills(self) -> (i32, i32) {
        const FIRST: [i32; 13] = [10, 40, 40, 100, 150, 210, 210, 100, 270, 310, 350, 400, 350];
        const COUNT: [i32; 13] = [0, 20, 20, 10, 20, 30, 30, 10, 9, 15, 18, 9, 18];
        (FIRST[self as usize], COUNT[self as usize])
    }

    /// `param\pl\stamina\PlStamina_<suffix>` for weapon-skill stamina
    /// (path table 0x0163e62c).
    pub fn stamina_table(self) -> &'static str {
        const T: [&str; 13] = [
            "Unarm", "Sword", "Sword", "GSword", "Dagger", "Wand", "Wand", "GSword",
            "Shield", "ShieldMG", "Bow", "BowL", "Bow",
        ];
        T[self as usize]
    }

    /// Motion banks loaded for the weapon (table 0x0163df8c; bank keys
    /// 0x600 / 0x700 for primary weapons, 0x900 for secondary ones).
    pub fn motion_banks(self) -> &'static [&'static str] {
        match self {
            Self::Unarmed => &["m0004_co", "m0004_at"],
            Self::Sword | Self::Mace => &["m0001_co", "m0001_at"],
            Self::Longsword | Self::Warhammer => &["m0003_co", "m0003_at"],
            Self::Dagger => &["m0005_co", "m0005_at"],
            Self::Staff | Self::Archistaff => &["m0007_co", "m0007_at"],
            Self::Shield => &["m0002_at"],
            Self::MagickShield => &["m0009_at"],
            Self::Bow => &["m0006_at"],
            Self::Longbow => &["m0008_at"],
            Self::MagickBow => &["m0010_at"],
        }
    }
}

impl Vocation {
    pub const ALL: [Self; 9] = [
        Self::Fighter, Self::Strider, Self::Mage, Self::MysticKnight, Self::Assassin,
        Self::MagickArcher, Self::Warrior, Self::Ranger, Self::Sorcerer,
    ];

    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.get((id as usize).wrapping_sub(1)).copied()
    }

    pub fn id(self) -> u32 {
        self as u32
    }

    /// Label of the English name in `common_message`.
    pub fn label(self) -> &'static str {
        ["job_1", "job_2", "job_3", "job_4", "job_5", "job_6", "job_7", "job_8", "job_9"]
            [self as usize - 1]
    }

    /// File suffix of `param\pl\level\Lv<..>` and `param\pl\JobLevel\JobLv<..>`.
    pub fn param_name(self) -> &'static str {
        ["Fighter", "Strider", "MagicUser", "Paladin", "Assassin", "Trickster",
         "Battler", "Hunter", "Wizard"][self as usize - 1]
    }

    /// Bit in the skill table's vocation mask (`1 << id`; 0x006fdb40).
    pub fn mask(self) -> u32 {
        1 << self as u32
    }

    /// Weapon categories whose skill sets the vocation keeps (0x0163e784,
    /// used by 0x00780840 to clear other skill slots on a vocation change).
    pub fn weapon_categories(self) -> &'static [WeaponCategory] {
        use WeaponCategory::*;
        match self {
            Self::Fighter => &[Sword, Shield],
            Self::Strider => &[Dagger, Bow],
            Self::Mage => &[Staff],
            Self::MysticKnight => &[Sword, Mace, Staff, MagickShield],
            Self::Assassin => &[Dagger, Sword, Shield, Bow],
            Self::MagickArcher => &[Dagger, Staff, MagickBow],
            Self::Warrior => &[Longsword, Warhammer],
            Self::Ranger => &[Dagger, Longbow],
            Self::Sorcerer => &[Archistaff],
        }
    }

    /// Weapon skill slots per staff: 6 for Mage and Sorcerer, 3 for others
    /// (0x00780840 clears staff slots 3..5 unless the vocation is 3 or 9;
    /// text: "Mystic Knights and Magick Archers may only enable three skills.").
    pub fn staff_slots(self) -> usize {
        if matches!(self, Self::Mage | Self::Sorcerer) { 6 } else { 3 }
    }

    /// Discipline cost of first switching to this vocation (0x01517a7c, the
    /// same row for the Arisen and the main pawn). Charged only while the
    /// vocation's unlock flag is clear (see `unlock_flag`).
    pub fn change_cost(self) -> i32 {
        match self {
            Self::MysticKnight | Self::Assassin | Self::MagickArcher => 1500,
            _ => 1000,
        }
    }

    /// Game flag (sGameSys+0xac6e4 bit set, MSB-first) that marks this
    /// vocation as already paid for, per character (0x01517a54).
    /// `pawn = true` for the main pawn; `None` where the pawn cannot take it.
    pub fn unlock_flag(self, pawn: bool) -> Option<u16> {
        const ARISEN: [i16; 9] = [540, 541, 542, 546, 547, 548, 543, 544, 545];
        const PAWN: [i16; 9] = [549, 550, 551, -1, -1, -1, 552, 553, 554];
        let v = if pawn { PAWN } else { ARISEN }[self as usize - 1];
        (v >= 0).then_some(v as u16)
    }

    /// Whether the main pawn can take this vocation (0x01517a3c; also the
    /// -1 entries of 0x01517a54).
    pub fn pawn_allowed(self) -> bool {
        !matches!(self, Self::MysticKnight | Self::Assassin | Self::MagickArcher)
    }
}

/// The vocations offered by the change screen, in display order.
/// Translated from 0x00675780 (`uGUIJobChange`): the free first choice
/// (screen flag +0x298) and characters below level 10 get the three basic
/// vocations; the Arisen at 10+ gets all nine; the main pawn at 10+ gets six.
pub fn offered(pawn: bool, level: u32, first_choice: bool) -> &'static [Vocation] {
    use Vocation::*;
    const BASIC: [Vocation; 3] = [Fighter, Strider, Mage];
    const ARISEN: [Vocation; 9] =
        [Fighter, Strider, Mage, Warrior, Ranger, Sorcerer, MysticKnight, Assassin, MagickArcher];
    const PAWN: [Vocation; 6] = [Fighter, Strider, Mage, Warrior, Ranger, Sorcerer];
    if first_choice || level < 10 {
        &BASIC
    } else if pawn {
        &PAWN
    } else {
        &ARISEN
    }
}

/// Discipline charged for a change (0x00671b70 fills the cost table,
/// 0x00672c10 subtracts it): zero on the free first choice or when the
/// vocation's unlock flag is already set.
pub fn change_cost(v: Vocation, pawn: bool, first_choice: bool, flag_set: impl Fn(u16) -> bool) -> i32 {
    match v.unlock_flag(pawn) {
        _ if first_choice => 0,
        None => 0,
        Some(f) if flag_set(f) => 0,
        Some(_) => v.change_cost(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_masks() {
        assert_eq!(Vocation::from_id(4), Some(Vocation::MysticKnight));
        assert_eq!(Vocation::from_id(0), None);
        assert_eq!(Vocation::from_id(10), None);
        // Sword skills 40-44 carry mask 50 = Fighter | Mystic Knight | Assassin.
        let m = Vocation::Fighter.mask() | Vocation::MysticKnight.mask() | Vocation::Assassin.mask();
        assert_eq!(m, 50);
        // Staff skills 210-215 carry mask 600.
        let m = Vocation::Mage.mask() | Vocation::MysticKnight.mask()
            | Vocation::MagickArcher.mask() | Vocation::Sorcerer.mask();
        assert_eq!(m, 600);
        assert_eq!(WeaponCategory::Sword.mask() | WeaponCategory::Mace.mask(), 6);
        assert_eq!(WeaponCategory::Longsword.mask() | WeaponCategory::Warhammer.mask(), 136);
        assert_eq!(WeaponCategory::from_item_id(0x0c00_0001), WeaponCategory::MagickBow);
        assert_eq!(WeaponCategory::from_item_id(-1), WeaponCategory::Unarmed);
    }

    #[test]
    fn offered_lists() {
        assert_eq!(offered(false, 9, false).len(), 3);
        assert_eq!(offered(false, 10, false).len(), 9);
        assert_eq!(offered(true, 10, false).len(), 6);
        assert_eq!(offered(false, 50, true).len(), 3);
        assert!(offered(true, 50, false).iter().all(|v| v.pawn_allowed()));
    }

    #[test]
    fn costs() {
        let none = |_| false;
        assert_eq!(change_cost(Vocation::Warrior, false, false, none), 1000);
        assert_eq!(change_cost(Vocation::Assassin, false, false, none), 1500);
        assert_eq!(change_cost(Vocation::Assassin, false, true, none), 0);
        assert_eq!(change_cost(Vocation::Warrior, false, false, |f| f == 543), 0);
        assert_eq!(change_cost(Vocation::Assassin, true, false, none), 0);
        assert_eq!(Vocation::Warrior.unlock_flag(true), Some(552));
    }
}
