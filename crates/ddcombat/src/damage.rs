//! Damage rules of a hit, as pure functions.
//!
//! Every function is a translation of the named routine of DDDA.exe
//! (2017-12-13 Steam build, see `docs/METHODOLOGY.md`); `docs/COMBAT.md`
//! explains the pipeline and the inputs. Nothing here holds global state:
//! callers pass the attacker, the victim and the tuning tables, so any
//! number of players and pawns can be resolved independently.
//!
//! Float arithmetic is f32 and follows the operation order of the
//! disassembly (SSE single precision), so results match the game's bits
//! as closely as a reimplementation can.
//!
//! Not covered here (named in COMBAT.md): status build-up and the timed
//! statuses, guard, the species-specific `vf68` overrides, the stamina
//! damage consumer and the knockback trajectory.

use crate::ocl::Attack;

/// The attack values the damage rules read, taken from an
/// `rObjCollision::Attack` row (names are the reflected field names).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AttackRow {
    pub attack: f32,
    pub attack_rate: f32,
    /// Percent of the physical part dealt as slash.
    pub sword_rate: f32,
    /// Percent of the physical part dealt as strike.
    pub hit_rate: f32,
    pub mgc_attack: f32,
    pub mgc_attack_rate: f32,
    /// 0 none, 1 fire, 2 ice, 3 thunder, 4 holy, 5 dark.
    pub element: u32,
    pub element_rate: f32,
    /// 0 = no stagger damage.
    pub shrink_type: u32,
    pub shrink_attack: f32,
    /// 3 = no knockdown damage.
    pub blow_type: u32,
    pub blow_attack: f32,
    pub stamina_attack: f32,
    pub wp_phy_atk_rate: f32,
    pub wp_mgc_atk_rate: f32,
    pub wp_critical_rate: f32,
    pub is_all_critical: bool,
    pub is_no_critical: bool,
    pub is_no_enchant: bool,
    pub is_no_death: bool,
    pub is_force_shrink: bool,
    pub is_force_shrink2: bool,
    pub is_force_blow: bool,
    pub is_force_blow2: bool,
    pub is_no_damage_no_reset_shrink: bool,
    pub is_no_damage_no_reset_blow: bool,
    pub is_no_calc_enemy_defence: bool,
    /// `mIsSameFrameNoHit00..03` as bits 0..3.
    pub same_frame_no_hit: u8,
    /// `mPoison`, `mSlow`, `mBlind`, `mSilence`, `mSleep`, `mOil`, `mWet`,
    /// `mCurse`, `mEnemy`, `mSeal`, `mStone`, `mAtkDown`, `mDefDown`,
    /// `mMgcAtkDown`, `mMgcDefDown` (attack order; see `status::SLOT_SOURCE`).
    pub status: [f32; 15],
}

impl AttackRow {
    pub fn from_attack(a: &Attack) -> Self {
        let f = |n: &str| a.value(n).map(|v| v.as_f32()).unwrap_or(0.0);
        let u = |n: &str| match a.value(n) {
            Some(crate::ocl::Value::U32(v)) => v,
            _ => 0,
        };
        let b = |n: &str| a.flag(n).unwrap_or(false);
        let mut same = 0u8;
        for (i, n) in ["mIsSameFrameNoHit00", "mIsSameFrameNoHit01", "mIsSameFrameNoHit02", "mIsSameFrameNoHit03"]
            .iter()
            .enumerate()
        {
            if b(n) {
                same |= 1 << i;
            }
        }
        AttackRow {
            attack: f("mAttack"),
            attack_rate: f("mAttackRate"),
            sword_rate: f("mSwordRate"),
            hit_rate: f("mHitRate"),
            mgc_attack: f("mMgcAttack"),
            mgc_attack_rate: f("mMgcAttackRate"),
            element: u("mElement"),
            element_rate: f("mElementRate"),
            shrink_type: u("mShrinkType"),
            shrink_attack: f("mShrinkAttack"),
            blow_type: u("mBlowType"),
            blow_attack: f("mBlowAttack"),
            stamina_attack: f("mStaminaAttack"),
            wp_phy_atk_rate: f("mWpPhyAtkRate"),
            wp_mgc_atk_rate: f("mWpMgcAtkRate"),
            wp_critical_rate: f("mWpCriticalRate"),
            is_all_critical: b("mIsAllCritical"),
            is_no_critical: b("mIsNoCritical"),
            is_no_enchant: b("mIsNoEnchant"),
            is_no_death: b("mIsNoDeath"),
            is_force_shrink: b("mIsForceShrink"),
            is_force_shrink2: b("mIsForceShrink2"),
            is_force_blow: b("mIsForceBlow"),
            is_force_blow2: b("mIsForceBlow2"),
            is_no_damage_no_reset_shrink: b("mIsNoDamageNoResetShrink"),
            is_no_damage_no_reset_blow: b("mIsNoDamageNoResetBlow"),
            is_no_calc_enemy_defence: b("mIsNoCalcEnemyDefence"),
            same_frame_no_hit: same,
            status: ["mPoison", "mSlow", "mBlind", "mSilence", "mSleep", "mOil", "mWet", "mCurse", "mEnemy", "mSeal", "mStone", "mAtkDown", "mDefDown", "mMgcAtkDown", "mMgcDefDown"].map(f),
        }
    }
}

/// One hit after the attacker's stats are folded in (the attack entry's
/// `mAttack` / `mMgcAttack` overwritten, plus the attack row it came from).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitValue {
    pub row: AttackRow,
    pub phys: f32,
    pub mag: f32,
    pub element: u32,
    pub element_rate: f32,
    pub shrink: f32,
    pub blow: f32,
}

/// An `rAdjustParam` table (`param\game` or `param\dlc`). The game's
/// getter `rAdjustParam::vf15` (0x00CA47D0) returns `values[i]`; a missing
/// table reads as 0 at every call site.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Adjust(pub Vec<f32>);
impl Adjust {
    pub fn get(&self, i: usize) -> f32 {
        self.0.get(i).copied().unwrap_or(0.0)
    }
    /// Parse `ajp\0`, u32 0x0201, u32 count, f32 values.
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.get(..4)? != b"ajp\0" {
            return None;
        }
        let n = u32::from_le_bytes(d.get(8..12)?.try_into().ok()?) as usize;
        let body = d.get(12..12 + 4 * n)?;
        Some(Adjust(body.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()))
    }
}

// ---------------------------------------------------------------- attacker

/// An enemy's attack stats: the copy of its `cCharParamEnemy` at
/// actor+0x59B0 and the run-time multipliers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyAttacker {
    /// actor+0x59BC (`攻撃力`).
    pub attack: f32,
    /// actor+0x59C4 (`魔法攻撃力`).
    pub mgc_attack: f32,
    /// actor+0x20A8 when actor+0x20A4 is set.
    pub attack_mul: Option<f32>,
    /// actor+0x20B8 when actor+0x20B4 is set.
    pub mgc_attack_mul: Option<f32>,
    /// Product of the active `cStatus` entries of type 0x0E or 0x12, times
    /// status 9 when the status flag 0x200 is set (`0x00837480`); 1 if none.
    pub phys_status_mul: f32,
    /// The same for types 0x10 / 0x14.
    pub mag_status_mul: f32,
}

impl EnemyAttacker {
    pub fn new(attack: f32, mgc_attack: f32) -> Self {
        EnemyAttacker { attack, mgc_attack, attack_mul: None, mgc_attack_mul: None, phys_status_mul: 1.0, mag_status_mul: 1.0 }
    }
}

/// Translated from 0x0088B530 (DDDA.exe 2017-12-13 Steam).
/// Overwrites the entry's `mAttack` / `mMgcAttack`; the data's `mAttack`
/// does not enter (it is 0 in enemy tables anyway). Enemies never roll
/// criticals.
pub fn enemy_attack(att: &EnemyAttacker, row: &AttackRow) -> HitValue {
    let mut phys = att.attack * row.attack_rate;
    let mut mag = att.mgc_attack * row.mgc_attack_rate;
    if let Some(m) = att.attack_mul {
        phys *= m;
    }
    if let Some(m) = att.mgc_attack_mul {
        mag *= m;
    }
    HitValue {
        row: *row,
        phys: att.phys_status_mul * phys,
        mag: att.mag_status_mul * mag,
        element: row.element,
        element_rate: row.element_rate,
        shrink: row.shrink_attack,
        blow: row.blow_attack,
    }
}

/// The weapon slot of `cCalcPlayerAttackParam` selected by P+0x04.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponSlot {
    /// P+0x10 (slot 0) / P+0x18 (slot 1): physical attack of the equipment.
    pub attack: f32,
    /// P+0x14 / P+0x1C: magick attack of the equipment.
    pub mgc_attack: f32,
    /// P+0x24 / P+0x2C: when > 0 the enchantment bonus counts 0.4 instead
    /// of 1. What this value is is *unknown*.
    pub aux: f32,
    /// P+0x30 / P+0x38: stagger power in percent (default 100).
    pub stagger_power: f32,
    /// P+0x34 / P+0x3C: knockdown power in percent (default 100).
    pub knockdown_power: f32,
}

/// `uObjModel::cCalcPlayerAttackParam` at player+0x3E80 (ctor 0x0088B760,
/// filled by 0x00B6BC80), plus the attacker's run-time multipliers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerAttacker {
    /// P+0x08 = `cPlayerInfo.mBasicAttack` (live-checked, board #758).
    pub base_attack: f32,
    /// P+0x0C = `cPlayerInfo.mBasicMgcAttack`.
    pub base_mgc_attack: f32,
    /// None when P+0x04 is neither 0 nor 1.
    pub slot: Option<WeaponSlot>,
    /// P+0x40 / P+0x44: added to the slot's stagger / knockdown power.
    pub stagger_bonus: f32,
    pub knockdown_bonus: f32,
    /// P+0x58 element, P+0x5C element rate, P+0x60 magick bonus.
    pub enchant: Option<(u32, f32, f32)>,
    /// P+0x48 (default 1), P+0x4C (default 0), P+0x50 (1), P+0x54 (0).
    pub phys_mul: f32,
    pub phys_add: f32,
    pub mgc_mul: f32,
    pub mgc_add: f32,
    /// P+0x144: the equipment can roll the special critical.
    pub crit_capable: bool,
    /// P+0x7C / P+0x80: integer counts scaling attack through
    /// `param\dlc`[0x0C..0x0F]; source *unknown*.
    pub count_phys: u32,
    pub count_mgc: u32,
    /// P+0x8C (default 1): knockdown multiplier.
    pub blow_mul: f32,
    /// Status products as in [`EnemyAttacker`], already multiplied by
    /// actor+0x20A8 / +0x20B8 when actor+0x206D bit 3 and +0x20A4 / +0x20B4
    /// are set.
    pub phys_status_mul: f32,
    pub mag_status_mul: f32,
}

impl PlayerAttacker {
    /// Defaults of the constructor 0x0088B760 with the given base stats and
    /// main-weapon values.
    pub fn new(base_attack: f32, base_mgc_attack: f32, weapon_attack: f32, weapon_mgc_attack: f32) -> Self {
        PlayerAttacker {
            base_attack,
            base_mgc_attack,
            slot: Some(WeaponSlot { attack: weapon_attack, mgc_attack: weapon_mgc_attack, aux: 0.0, stagger_power: 100.0, knockdown_power: 100.0 }),
            stagger_bonus: 0.0,
            knockdown_bonus: 0.0,
            enchant: None,
            phys_mul: 1.0,
            phys_add: 0.0,
            mgc_mul: 1.0,
            mgc_add: 0.0,
            crit_capable: false,
            count_phys: 0,
            count_mgc: 0,
            blow_mul: 1.0,
            phys_status_mul: 1.0,
            mag_status_mul: 1.0,
        }
    }
}

/// The critical multiplier of 0x0088AA20 (0x0088AE49..0x0088B0C1).
/// `roll` is the game's uniform `rand() * 2^-32` in [0, 1). Returns 1 when
/// no roll happens.
pub fn critical_multiplier(p: &PlayerAttacker, row: &AttackRow, roll: f32, dlc: &Adjust) -> f32 {
    if !(p.crit_capable || row.is_all_critical) || row.is_no_critical {
        return 1.0;
    }
    // COMISS chance*0.01, roll; JC -> miss
    if dlc.get(0x19) * 0.01 < roll {
        dlc.get(0x1B)
    } else {
        dlc.get(0x1A)
    }
}

/// Translated from 0x0088AA20 (DDDA.exe 2017-12-13 Steam): the attack of
/// a player or pawn hit. `roll` feeds [`critical_multiplier`]. Status
/// build-up additions from the weapon are not included.
pub fn player_attack(p: &PlayerAttacker, row: &AttackRow, roll: f32, dlc: &Adjust) -> HitValue {
    let (wp_atk, wp_mgc, aux, stagger, knock) = match p.slot {
        Some(s) => (
            s.attack,
            s.mgc_attack,
            s.aux,
            (p.stagger_bonus + s.stagger_power) * 0.01,
            (p.knockdown_bonus + s.knockdown_power) * 0.01,
        ),
        None => (0.0, 0.0, 0.0, 1.0, 1.0),
    };
    let mut element = row.element;
    let mut element_rate = row.element_rate;
    let mut enchant_bonus = 0.0;
    if let Some((e, rate, bonus)) = p.enchant {
        if e != 0 && !row.is_no_enchant {
            enchant_bonus = bonus;
            element = e;
            element_rate = rate;
        }
    }
    let crit = critical_multiplier(p, row, roll, dlc);
    let k = if aux <= 0.0 { 1.0 } else { 0.4 };
    let mut phys = ((row.attack_rate * p.base_attack + wp_atk * row.wp_phy_atk_rate * crit * row.wp_critical_rate)
        * p.phys_mul
        + row.attack
        + p.phys_add)
        * p.phys_status_mul;
    let mut mag = (((k * enchant_bonus + wp_mgc) * row.wp_mgc_atk_rate * crit * row.wp_critical_rate
        + row.mgc_attack_rate * p.base_mgc_attack)
        * p.mgc_mul
        + row.mgc_attack
        + p.mgc_add)
        * p.mag_status_mul;
    phys *= (dlc.get(0x0D) - 1.0) / dlc.get(0x0C) * p.count_phys as f32 + 1.0;
    mag *= (dlc.get(0x0F) - 1.0) / dlc.get(0x0E) * p.count_mgc as f32 + 1.0;
    HitValue {
        row: *row,
        phys,
        mag,
        element,
        element_rate,
        shrink: row.shrink_attack * stagger,
        blow: p.blow_mul * row.blow_attack * knock,
    }
}

// ----------------------------------------------------------- per-frame merge

/// The victim's `HitInfo` for one frame (`cObjCollision`+0x640).
#[derive(Debug, Clone, PartialEq)]
pub struct MergedHit {
    /// The first merged hit's row (its flags and rates are the ones the
    /// mitigation reads).
    pub row: AttackRow,
    /// HitInfo+0x74 / +0x78.
    pub phys: f32,
    pub mag: f32,
    /// HitInfo+0xB8 + 4*element: element build-up per element.
    pub element_buildup: [f32; 6],
    pub element: u32,
    /// HitInfo+0x104 / +0x10C / +0x13C.
    pub shrink: f32,
    pub blow: f32,
    pub stamina: f32,
    /// HitInfo+0x144..+0x17C: status build-ups, summed (0x00776810).
    pub status: [f32; 15],
    /// HitInfo+0xD0: hits merged; defence is subtracted once per hit.
    pub hits: u32,
    /// HitInfo+0x235 bits 3 / 4: the hit has a physical / magick part.
    pub has_phys: bool,
    pub has_mag: bool,
}

/// The merge loop of 0x0076B7E0 (0x0076C0xx..0x0076C21E): the first
/// accepted hit is copied (0x00775D00); each later one is added unless it
/// shares a `mIsSameFrameNoHit` bit with the first. Physical, magick,
/// stagger, knockdown and stamina values add up and the hit count grows.
/// The element build-up of a later hit is added to the slot of the element
/// the merge holds at that moment; the merged element, slash/strike rates
/// and stagger/knockdown types are those of the strongest magick,
/// physical, stagger and knockdown hit (`FUN_00776980`, `FUN_00776A80`).
/// Returns None for no hits.
pub fn merge_frame(hits: &[HitValue]) -> Option<MergedHit> {
    let first = hits.first()?;
    let mut m = MergedHit {
        row: first.row,
        phys: first.phys,
        mag: first.mag,
        element_buildup: [0.0; 6],
        element: first.element,
        shrink: first.shrink,
        blow: first.blow,
        stamina: first.row.stamina_attack,
        status: first.row.status,
        hits: 1,
        has_phys: true,
        has_mag: true,
    };
    if (first.element as usize) < 6 {
        m.element_buildup[first.element as usize] = first.element_rate;
    }
    let blocked = first.row.same_frame_no_hit;
    let (mut max_phys, mut max_mag, mut max_shrink, mut max_blow) = (first.phys, first.mag, first.shrink, first.blow);
    for h in &hits[1..] {
        if blocked & h.row.same_frame_no_hit != 0 {
            continue;
        }
        m.phys += h.phys;
        if max_phys < h.phys {
            max_phys = h.phys;
            m.row.sword_rate = h.row.sword_rate;
            m.row.hit_rate = h.row.hit_rate;
        }
        m.mag += h.mag;
        if (m.element as usize) < 6 {
            m.element_buildup[m.element as usize] += h.element_rate;
        }
        if max_mag < h.mag {
            max_mag = h.mag;
            m.element = h.element;
        }
        m.shrink += h.shrink;
        if max_shrink < h.shrink {
            max_shrink = h.shrink;
            m.row.shrink_type = h.row.shrink_type;
        }
        m.blow += h.blow;
        if max_blow < h.blow {
            max_blow = h.blow;
            m.row.blow_type = h.row.blow_type;
        }
        m.stamina += h.row.stamina_attack;
        for (s, v) in m.status.iter_mut().zip(h.row.status) {
            *s += v;
        }
        m.hits += 1;
    }
    Some(m)
}

// ------------------------------------------------------------- mitigation

/// Resistance multipliers of a runtime region (`cObjCollision::RegionStatus`
/// +0x54..+0x6C, filled by 0x00770FD0 from the `cCharParamEnemy` array).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resist {
    /// fire, ice, thunder, holy, dark (`耐炎`..`耐魔`).
    pub element: [f32; 5],
    pub slash: f32,
    pub strike: f32,
}
impl Default for Resist {
    fn default() -> Self {
        Resist { element: [1.0; 5], slash: 1.0, strike: 1.0 }
    }
}

/// An enemy's defence (actor+0x59C0 / +0x59C8) and run-time multipliers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyDefender {
    pub defence: f32,
    pub mgc_defence: f32,
    /// actor+0x20B0 when actor+0x20AC is set.
    pub defence_mul: Option<f32>,
    /// actor+0x20C0 when actor+0x20BC is set.
    pub mgc_defence_mul: Option<f32>,
    /// `param\dlc`[0] when 0x00858270 says so (condition *unknown*).
    pub pre_mul: Option<f32>,
    /// Status products for types 0x0F|0x13 and 0x11|0x15 (with status 9).
    pub phys_status_mul: f32,
    pub mag_status_mul: f32,
}
impl EnemyDefender {
    pub fn new(defence: f32, mgc_defence: f32) -> Self {
        EnemyDefender { defence, mgc_defence, defence_mul: None, mgc_defence_mul: None, pre_mul: None, phys_status_mul: 1.0, mag_status_mul: 1.0 }
    }
}

fn phys_present(h: &MergedHit) -> bool {
    h.row.sword_rate * 0.01 * h.phys + h.row.hit_rate * 0.01 * h.phys > 0.0
}

/// Translated from `uEnemy::vf68` 0x00AAA490 (DDDA.exe 2017-12-13 Steam),
/// the core up to the status multipliers. The actor-flag multipliers that
/// follow (actor+0x2360 bit 0x20000000, status flag 0x10, a global flag)
/// and `uCharacterBase::vf68` are not applied here.
pub fn enemy_mitigate(h: &mut MergedHit, d: &EnemyDefender, r: &Resist) {
    if !phys_present(h) {
        h.has_phys = false;
    }
    if h.mag <= 0.0 {
        h.has_mag = false;
    }
    if let Some(m) = d.pre_mul {
        h.phys *= m;
        h.mag *= m;
    }
    let skip = h.row.is_no_calc_enemy_defence;
    let mut def = if skip { 0.0 } else { d.defence };
    if let Some(m) = d.defence_mul {
        def *= m;
    }
    let n = h.hits as f32;
    h.phys = (h.phys - n * def).max(0.0);
    let mut emul = 1.0;
    if !skip && (1..=5).contains(&h.element) {
        emul = r.element[h.element as usize - 1];
        if emul <= 0.0 {
            h.has_mag = false;
        }
    }
    h.mag *= emul;
    let mut mdef = if skip { 0.0 } else { d.mgc_defence };
    if let Some(m) = d.mgc_defence_mul {
        mdef *= m;
    }
    h.mag = (h.mag - n * mdef).max(0.0);
    let p = h.phys;
    h.phys = h.row.sword_rate * 0.01 * p * r.slash + h.row.hit_rate * 0.01 * p * r.strike;
    h.phys *= d.phys_status_mul;
    h.mag *= d.mag_status_mul;
}

/// A player's or pawn's defence inputs (0x00B87450 sums and player fields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerDefender {
    /// s[0], s[1]: equipment physical / magick defence.
    pub equip_defence: f32,
    pub equip_mgc_defence: f32,
    /// `cPlayerInfo.mBasicDefend` (+0x2B8) / `mBasicMgcDefend` (+0x2C0).
    pub base_defence: f32,
    pub base_mgc_defence: f32,
    /// s[2] = 100 - slash resist, s[3] = 100 - strike resist.
    pub slash_pct: f32,
    pub strike_pct: f32,
    /// s[4..9]: element resist in percent, index = element (0 = none, 0).
    pub element_resist_pct: [f32; 6],
    /// player+0x4B88 / +0x4B9C (default 1), already times actor+0x20B0 /
    /// +0x20C0 when actor+0x206D bit 3 and +0x20AC / +0x20BC are set.
    pub c_phys: f32,
    pub c_mag: f32,
    /// player+0x4B8C / +0x4BA0 (default 0): flat reduction.
    pub d_phys: f32,
    pub d_mag: f32,
    /// player+0x4BA4: 1 if player+0x273C bit 1, else 1 + 0.2 * augment 40.
    pub e: f32,
}
impl PlayerDefender {
    pub fn new(base_defence: f32, base_mgc_defence: f32) -> Self {
        PlayerDefender {
            equip_defence: 0.0,
            equip_mgc_defence: 0.0,
            base_defence,
            base_mgc_defence,
            slash_pct: 100.0,
            strike_pct: 100.0,
            element_resist_pct: [0.0; 6],
            c_phys: 1.0,
            c_mag: 1.0,
            d_phys: 0.0,
            d_mag: 0.0,
            e: 1.0,
        }
    }
}

/// Translated from 0x00B83EE0 (DDDA.exe 2017-12-13 Steam):
/// `(x - x*D/(D + game[5])*c - d) * e` with `D = game[6]*a + b`.
pub fn defence_curve(x: f32, a: f32, b: f32, c: f32, d: f32, e: f32, game: &Adjust) -> f32 {
    let big_d = game.get(6) * a + b;
    let reduction = big_d * x / (big_d + game.get(5)) * c + d;
    (x - reduction) * e
}

/// Translated from `uPlayerBase::vf68` 0x00B7AD90 (DDDA.exe 2017-12-13
/// Steam), the core: element resist, slash/strike, the defence curve, and
/// the clamps. The merges for player+0x3EF0 bits 1 / 0x2000, the guard
/// halving (+0x4CA0), status 0x13/0x15 and the `param\dlc`[2]/[3] ability
/// multipliers that follow are not applied here.
pub fn player_mitigate(h: &mut MergedHit, p: &PlayerDefender, game: &Adjust) {
    let er = p.element_resist_pct.get(h.element as usize).copied().unwrap_or(0.0);
    h.mag = (100.0 - er) * h.mag * 0.01;
    if !phys_present(h) {
        h.has_phys = false;
    }
    if h.mag <= 0.0 {
        h.has_mag = false;
    }
    let x = (h.row.hit_rate * 0.01 * h.phys * p.strike_pct + h.row.sword_rate * 0.01 * h.phys * p.slash_pct) * 0.01;
    h.phys = defence_curve(x, p.equip_defence, p.base_defence, p.c_phys, p.d_phys, p.e, game).max(0.0);
    h.mag = defence_curve(h.mag, p.equip_mgc_defence, p.base_mgc_defence, p.c_mag, p.d_mag, p.e, game).max(0.0);
}

// ------------------------------------------------------------ region pools

/// `mAttr` bits of a region (see COMBAT.md 1.2).
pub mod attr {
    pub const NO_HP: u32 = 0x01;
    pub const NO_DP: u32 = 0x02;
    pub const NO_BP: u32 = 0x04;
    pub const LINK_HP: u32 = 0x08;
    pub const LINK_BREAK: u32 = 0x10;
    pub const HP_FLOOR_1: u32 = 0x20;
    pub const DP_FLOOR_1: u32 = 0x40;
    pub const BP_FLOOR_1: u32 = 0x80;
    pub const MAIN_RESIST: u32 = 0x100;
}

/// Per-collision rates and flags (`cObjCollision`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionRates {
    /// +0xD1C, +0xD20 (BP), +0xD24 (DP); ctor default 1.
    pub damage: f32,
    pub bp: f32,
    pub dp: f32,
    /// +0xD14; 0 by default, set per unit state (meaning *unknown*).
    pub mode: u32,
    /// +0xCFC flag word (bit 0 no HP damage, bit 1 HP floor 1, 0x8/0x10 no
    /// DP/BP damage, 0x100/0x200 DP/BP frozen, 0x1800/0x4000 allow forced
    /// stagger, 0x2000/0x8000 allow forced knockdown).
    pub flags: u32,
}
impl Default for CollisionRates {
    fn default() -> Self {
        CollisionRates { damage: 1.0, bp: 1.0, dp: 1.0, mode: 0, flags: 0 }
    }
}

/// Events a region raised (RegionStatus+0x110 bits 0..2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegionEvents {
    pub dead: bool,
    pub staggered: bool,
    pub knocked_down: bool,
}

/// One runtime `cObjCollision::RegionStatus` (ctor 0x00776BA0, built by
/// 0x00770910 from an `rRegionStatus` element).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionPool {
    /// `mId`; 0 is the main pool.
    pub id: u32,
    pub hp: f32,
    pub hp_max: f32,
    pub dp: f32,
    pub dp_max: f32,
    pub dp_speed: f32,
    pub bp: f32,
    pub bp_max: f32,
    pub bp_speed: f32,
    pub damage_adj: f32,
    pub hit_stop_adj: f32,
    pub attr: u32,
    pub surface: u32,
    /// +0x1E0: added to the class (source *unknown*, 0 by default).
    pub class_offset: i32,
    /// +0x1C0: weak-point class 0..4 (ctor default 1).
    pub class: u32,
    /// +0x4C / +0x50: DP / BP damage multipliers (default 1).
    pub dp_rate: f32,
    pub bp_rate: f32,
    pub dp_reset_max: f32,
    pub bp_reset_max: f32,
    /// +0x34/+0x38 and +0x40/+0x44: refill timers (running, elapsed).
    pub dp_timer: Option<f32>,
    pub bp_timer: Option<f32>,
    pub resist: Resist,
    pub events: RegionEvents,
    /// The status build-up of this region (+0x70..+0x1B0, `crate::status`).
    pub status: crate::status::StatusPools,
}

impl RegionPool {
    /// Field copy of 0x00770910 from an `rRegionStatus` element.
    #[allow(clippy::too_many_arguments)]
    pub fn from_element(
        id: u32,
        hp_max: f32,
        dp_max: f32,
        dp_speed: f32,
        bp_max: f32,
        bp_speed: f32,
        damage_adj: f32,
        hit_stop_adj: f32,
        attr: u32,
        surface: u32,
        dp_reset_max: f32,
        bp_reset_max: f32,
    ) -> Self {
        RegionPool {
            id,
            hp: hp_max,
            hp_max,
            dp: dp_max,
            dp_max,
            dp_speed,
            bp: bp_max,
            bp_max,
            bp_speed,
            damage_adj,
            hit_stop_adj,
            attr,
            surface,
            class_offset: 0,
            class: 1,
            dp_rate: 1.0,
            bp_rate: 1.0,
            dp_reset_max,
            bp_reset_max,
            dp_timer: None,
            bp_timer: None,
            resist: Resist::default(),
            events: RegionEvents::default(),
            status: crate::status::StatusPools::default(),
        }
    }
}

/// The weak-point class update of 0x007781A0 (0x00778298..): with
/// collision mode 0 the class restarts from `mSurface`; with mode 1,
/// surface 1/2/3 restart it at 0/1/2; otherwise (other modes or surfaces)
/// the previous class is kept. The offset (+0x1E0) is then added and a
/// result above 4 (unsigned) becomes 0 for a negative offset, else 4.
pub fn region_class(surface: u32, offset: i32, mode: u32, previous: u32) -> u32 {
    let base: u32 = match (mode, surface) {
        (0, s) => s,
        (1, 1) => 0,
        (1, 2) => 1,
        (1, 3) => 2,
        _ => previous,
    };
    let c = base.wrapping_add(offset as u32);
    if c > 4 {
        if offset < 0 {
            0
        } else {
            4
        }
    } else {
        c
    }
}

/// The region-scaling part of 0x0076DB10 that follows the victim's
/// mitigation. Returns the total HP damage (HitInfo+0x7C).
/// `extra_mul` is HitInfo+0x90 when hit flag 0x234 bit 6 is set.
/// `min_damage` is 0x00775C70's floor: the attacker object's +0x14 value,
/// or 1 with no attacker.
pub fn scale_for_region(h: &mut MergedHit, region: &RegionPool, rates: &CollisionRates, game: &Adjust, extra_mul: Option<f32>, min_damage: f32) -> f32 {
    if rates.mode == 1 {
        h.phys *= game.get(0x1F);
        h.mag *= game.get(0x1F);
        h.shrink *= game.get(0x20);
        h.blow *= game.get(0x21);
    }
    if let Some(m) = extra_mul {
        if m <= 0.0 {
            h.has_phys = false;
            h.has_mag = false;
        }
        h.phys *= m;
        h.mag *= m;
    }
    for f in [rates.damage, region.damage_adj] {
        if f <= 0.0 {
            h.has_phys = false;
            h.has_mag = false;
        }
        h.phys *= f;
        h.mag *= f;
    }
    let c = region.class.min(4) as usize;
    let cm = game.get(0x22 + 3 * c);
    if cm <= 0.0 {
        h.has_phys = false;
        h.has_mag = false;
    }
    h.phys *= cm;
    h.mag *= cm;
    min_damage_rule(h, min_damage);
    let total = h.phys + h.mag;
    h.shrink *= game.get(0x23 + 3 * c);
    h.blow *= game.get(0x24 + 3 * c);
    total
}

/// Translated from 0x00775C70 (DDDA.exe 2017-12-13 Steam): a part that is
/// present but mitigated to <= 0 deals `min_damage`; if neither part dealt
/// real damage, stagger and knockdown damage are dropped unless the attack
/// has `mIsNoDamageNoReset*`. (HitInfo+0x235 bit 6, set by species
/// overrides, disables the drop; not modelled.)
pub fn min_damage_rule(h: &mut MergedHit, min_damage: f32) {
    let mut p = 0;
    if h.has_phys {
        p = 2;
        if h.phys <= 0.0 {
            h.phys = min_damage;
            p = 1;
        }
    }
    let mut q = 0;
    if h.has_mag {
        q = 2;
        if h.mag <= 0.0 {
            h.mag = min_damage;
            q = 1;
        }
    }
    const TABLE: [u8; 9] = [1, 1, 0, 1, 1, 0, 0, 0, 0]; // 0x015947A8
    if TABLE[3 * p + q] != 0 {
        if !h.row.is_no_damage_no_reset_shrink {
            h.shrink = 0.0;
        }
        if !h.row.is_no_damage_no_reset_blow {
            h.blow = 0.0;
        }
    }
}

/// Translated from 0x00776F50 (DDDA.exe 2017-12-13 Steam). `no_death` is
/// the attack's `mIsNoDeath` (it keeps only the main pool at >= 1).
pub fn apply_hp(region: &mut RegionPool, total: f32, rates: &CollisionRates, no_death: bool) {
    if region.attr & attr::NO_HP != 0 || rates.flags & 1 != 0 {
        return;
    }
    region.hp -= total;
    if region.attr & attr::HP_FLOOR_1 != 0 || rates.flags & 2 != 0 || (no_death && region.id == 0) {
        if region.events.dead || region.hp > 1.0 {
            return;
        }
        region.hp = 1.0;
        return;
    }
    if region.hp <= 0.0 {
        region.events.dead = true;
        region.hp = 0.0;
    }
}

/// Translated from 0x00777110 (DDDA.exe 2017-12-13 Steam): stagger (DP)
/// and knockdown (BP) damage.
pub fn apply_break(region: &mut RegionPool, h: &MergedHit, rates: &CollisionRates) {
    let f = rates.flags;
    let mut dp = if f & 0x100 != 0 { 1.0 } else { region.dp };
    if region.attr & attr::NO_DP == 0 && f & 0x18 == 0 && h.row.shrink_type != 0 {
        dp -= h.shrink * region.dp_rate * rates.dp;
        if region.attr & attr::DP_FLOOR_1 == 0 {
            if dp <= 0.0 {
                region.events.staggered = true;
                dp = 0.0;
            }
        } else if dp <= 1.0 {
            dp = 1.0;
        }
    }
    if region.attr & attr::NO_DP == 0
        && f & 0x18 == 0
        && h.row.shrink_type != 0
        && ((h.row.is_force_shrink && f & 0x1800 != 0) || (h.row.is_force_shrink2 && f & 0x4000 != 0))
    {
        region.events.staggered = true;
        dp = 0.0;
    }
    if f & 0x100 == 0 {
        region.dp = dp;
    }
    let mut bp = if f & 0x200 != 0 { 1.0 } else { region.bp };
    if region.attr & attr::NO_BP == 0 && f & 0x18 == 0 && h.row.blow_type != 3 {
        bp -= h.blow * region.bp_rate * rates.bp;
        if region.attr & attr::BP_FLOOR_1 != 0 {
            if bp <= 1.0 {
                bp = 1.0;
            }
        } else if bp <= 0.0 {
            region.events.knocked_down = true;
            bp = 0.0;
        }
    }
    if region.attr & attr::NO_BP == 0
        && h.row.blow_type != 3
        && ((h.row.is_force_blow && f & 0x2000 != 0) || (h.row.is_force_blow2 && f & 0x8000 != 0))
    {
        region.events.knocked_down = true;
        bp = 0.0;
    }
    if f & 0x200 == 0 {
        region.bp = bp;
    }
}

/// What 0x0076E340 dispatches after a hit (the stagger / knockdown part;
/// the status part needs the build-up, not modelled). The callbacks are
/// vf70 (stagger) and vf71 (knockdown) of the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reaction {
    pub stagger: bool,
    pub knockdown: bool,
}

/// Translated from the end of 0x0076E340 (DDDA.exe 2017-12-13 Steam).
/// `frozen_triggered` is status 11 (`耐氷漬け`) firing on this hit: it
/// cancels both reactions and refills DP and BP.
pub fn resolve_reaction(region: &mut RegionPool, frozen_triggered: bool) -> Reaction {
    let mut r = Reaction::default();
    if frozen_triggered {
        region.events.staggered = false;
        region.events.knocked_down = false;
        region.bp = region.bp_max;
        region.dp = region.dp_max;
        return r;
    }
    if region.events.knocked_down {
        r.knockdown = true;
        region.events.knocked_down = false;
        region.dp = region.dp_max;
        region.dp_timer = None;
        if region.bp_timer.is_none() {
            region.bp_timer = Some(0.0);
        }
        region.bp = 0.0;
    }
    if region.events.staggered && !region.events.knocked_down {
        r.stagger = !r.knockdown;
        region.events.staggered = false;
        if region.dp_timer.is_none() {
            region.dp_timer = Some(0.0);
        }
        region.dp = 0.0;
    }
    r
}

/// Translated from the pool part of 0x007781A0 (DDDA.exe 2017-12-13
/// Steam): per-frame recovery, refill timers and the class update. `dt` is
/// the owner unit's step in 1/30-s frames (`cUnit.mDeltaTime`).
pub fn tick_region(region: &mut RegionPool, dt: f32, rates: &CollisionRates) {
    region.dp = (region.dp_speed * dt + region.dp).min(region.dp_max);
    region.bp = (region.bp_speed * dt + region.bp).min(region.bp_max);
    if let Some(t) = region.dp_timer {
        let t = t + dt;
        if region.dp_reset_max <= t && t != region.dp_reset_max {
            region.dp = region.dp_max;
            region.dp_timer = None;
        } else {
            region.dp_timer = Some(t);
        }
    }
    if let Some(t) = region.bp_timer {
        let t = t + dt;
        if region.bp_reset_max <= t && t != region.bp_reset_max {
            region.bp = region.bp_max;
            region.bp_timer = None;
        } else {
            region.bp_timer = Some(t);
        }
    }
    region.class = region_class(region.surface, region.class_offset, rates.mode, region.class);
}

/// What one frame's hit did to an enemy.
#[derive(Debug, Clone, PartialEq)]
pub struct HitOutcome {
    pub total: f32,
    pub reaction: Reaction,
    pub main_reaction: Reaction,
    pub dead_parts: Vec<u32>,
}

/// The application order of 0x0076DB10 on an enemy: mitigation with the
/// hit region's resistances (or the main region's under `mAttr` 0x100),
/// region scaling, DP/BP and HP on the hit region, then on the main region
/// (index 0) according to `mAttr` 0x10 / 0x08. Status build-up is skipped.
pub fn resolve_enemy_hit(
    mut h: MergedHit,
    defender: &EnemyDefender,
    regions: &mut [RegionPool],
    hit_region: usize,
    rates: &CollisionRates,
    game: &Adjust,
    min_damage: f32,
) -> HitOutcome {
    let resist_from = if regions[hit_region].attr & attr::MAIN_RESIST != 0 { 0 } else { hit_region };
    let resist = regions[resist_from].resist;
    enemy_mitigate(&mut h, defender, &resist);
    let total = scale_for_region(&mut h, &regions[hit_region], rates, game, None, min_damage);
    let no_death = h.row.is_no_death;
    let mut out = HitOutcome { total, reaction: Reaction::default(), main_reaction: Reaction::default(), dead_parts: vec![] };
    {
        let r = &mut regions[hit_region];
        apply_break(r, &h, rates);
        apply_hp(r, total, rates, no_death);
        out.reaction = resolve_reaction(r, false);
        if r.events.dead {
            out.dead_parts.push(r.id);
        }
    }
    if hit_region != 0 && !regions.is_empty() {
        let a = regions[hit_region].attr;
        let main = &mut regions[0];
        if a & attr::LINK_BREAK != 0 {
            apply_break(main, &h, rates);
        }
        out.main_reaction = resolve_reaction(main, false);
        if a & attr::LINK_HP != 0 {
            apply_hp(main, total, rates, no_death);
            if main.events.dead && !out.dead_parts.contains(&main.id) {
                out.dead_parts.push(main.id);
            }
        }
    }
    out
}
