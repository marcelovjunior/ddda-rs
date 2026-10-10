//! Combat inputs of an ordinary NPC body. Authored motions and lifetime
//! belong to the host's NPC actor; this module does not invent either.

use ddcombat::damage::{Adjust, CollisionRates, PlayerDefender};
pub use ddcombat::damage::RegionPool;

/// The main collision pool of an ordinary NPC. The shared player manager
/// loads PlDefendParam at 0x0047E46D; this NPC branch always uses row zero,
/// rather than a vocation row. Bulk values stay in the player's install.
// Translated from 0x00B5F0A0, ordinary branch 0x00B5F468..0x00B5F508,
// and 0x00770A90 (DDDA.exe 2017-12-13 Steam).
pub fn ordinary_region(vfs: &ddvfs::Vfs, game: &Adjust, hp_max: f32) -> Result<RegionPool, String> {
    let path = r"param\pl\other\PlDefendParam";
    let bytes = crate::body::load::read(vfs, path, "rPlDefendParam")?;
    let x = mtxfs::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
    if x.class_of(&x.root).hash != mtarc::type_hash("rPlDefendParam") {
        return Err(format!("{path}: wrong root class"));
    }
    let Some(mtxfs::Value::Object(Some(row))) = x.get(&x.root, "mArray").and_then(|a| a.first()) else {
        return Err(format!("{path}: missing row zero"));
    };
    if x.class_of(row).hash != mtarc::type_hash("cPlDefendParam") {
        return Err(format!("{path}: wrong row class"));
    }
    let field = |name: &str| -> Result<f32, String> {
        match x.get(row, name) {
            Some([mtxfs::Value::F(v)]) if v.len() == 1 => Ok(v[0] as f32),
            _ => Err(format!("{path}: missing or mistyped {name}")),
        }
    };
    // Damage/hit-stop 1, surface1 and attr0 are the small embedded
    // constructor inputs (0x00B5F391..0x00B5F3A9); resets read game data.
    let mut pool = RegionPool::from_element(0, hp_max,
        field("mShrinkMax")?, field("mShrinkSpeed")?,
        field("mBlowMax")?, field("mBlowSpeed")?,
        1.0, 1.0, 0, 1, game.get(0x1bf), game.get(0x1c0));
    // vf196 replaces the first eighteen constructor status thresholds
    // with 1000 (+0xC0..+0x104, 0x00B5F6BC..0x00B5F744). Slots18/19
    // retain the constructor's 999 / 999.9 values.
    pool.status.threshold[..18].fill(1000.0);
    Ok(pool)
}

/// Constructor pools of `uNpcIntel`, copied into the ordinary NPC's
/// `cNpcInfo` by 0x00BABB00. The ordinary (-1 party-slot) branch of
/// 0x00BBE910 preserves this maximum through 0x00782040.
// Translated from 0x00BBBFA0 (DDDA.exe 2017-12-13 Steam).
pub const ORDINARY_NPC_HP: f32 = 1000.0;

/// Native ordinary intel HP/max, independent of its materialized actor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntelHp { pub current: f32, pub maximum: f32 }

impl Default for IntelHp {
    fn default() -> Self { Self { current: ORDINARY_NPC_HP, maximum: ORDINARY_NPC_HP } }
}

impl IntelHp {
    /// Ordinary intel vf05 applies the stored placement multiplier during
    /// initialization. The saved-current flag+44F keeps damage intact;
    /// maximum is still recomputed, not multiplied from its previous value.
    /// Type1 at+3D0 skips this branch. Party and special-ID overrides that
    /// follow it are not represented by this ordinary body helper.
    // Translated from 0x00BBC4E0 at 0x00BBC70A..0x00BBC755
    // (DDDA.exe 2017-12-13 Steam).
    pub fn initialize(&mut self, intel_kind: u8, preserve_current: bool, rate: Option<f32>) {
        if intel_kind == 1 { return; }
        if let Some(rate) = rate {
            let hp = ORDINARY_NPC_HP * rate;
            if !preserve_current { self.current = hp; }
            self.maximum = hp;
        }
    }
}

/// Defence of an ordinary NPC without a runtime stat override. NPCs use
/// `uPlayerBase::vf68`, not the enemy defence curve. Its NPC class bit
/// skips the equipment sum in 0x00B87450; the two addition arrays cleared
/// by 0x0077FA50 contribute zero in 0x00B87940.
// Translated from 0x0077FA50 and 0x00B7AD90 (DDDA.exe 2017-12-13 Steam).
pub fn ordinary_defender() -> PlayerDefender {
    PlayerDefender::new(1.0, 1.0)
}

/// Last resolved damage owner class stored at uNpc+0x6F5C. Shell and
/// other proxy ownership must be resolved before classifying these bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOwnerClass { Player, Pawn, Npc, Enemy, Class4, Unknown }

/// Gauge-related subset of the ordinary NPC's vf66 permission callback.
/// These are actor/intel facts, independent of collision NoDamage/Invincible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GaugeHitGuard {
    pub linked_intel: bool,
    /// Linked intel byte +0x3D1; nonzero bypasses this ordinary-NPC guard.
    pub intel_hostile: bool,
    /// Actor +0x206E bit0x10 (human enemy).
    pub human_enemy: bool,
    /// Linked intel +0x547, written by SetGaugeDisp.
    pub gauge_enabled: bool,
}

impl Default for GaugeHitGuard {
    fn default() -> Self {
        Self { linked_intel: true, intel_hostile: false, human_enemy: false, gauge_enabled: false }
    }
}

impl GaugeHitGuard {
    /// Other global/event/action guards in 0x00BB7680 are outside this subset.
    /// Shell callers supply the resolved actor class, not the shell's owner ID.
    // Translated from uNpc::vf66 0x00BB6EC0 -> 0x00BB7680,
    // branch 0x00BB778B..0x00BB77D4 (DDDA.exe 2017-12-13 Steam).
    pub fn allows(self, owner: DamageOwnerClass, owner_is_gauged_npc: bool) -> bool {
        if !self.linked_intel || self.intel_hostile || self.human_enemy || !self.gauge_enabled {
            return true;
        }
        !matches!(owner, DamageOwnerClass::Player | DamageOwnerClass::Pawn)
            // This source test uses actor class bit8, independently of the
            // owner classifier's later hostile-NPC -> Enemy distinction.
            && !owner_is_gauged_npc
    }
}

/// Host-selected policy shared by every NPC and every player seat.
/// 0x0041E230 maps Steam country JP to0; only that native region enables
/// the safeguard. The source's other regional modes all disable it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Region { Japan, #[default] Other }

// Translated from uNpc::vf66 0x00BB6EC0 (DDDA.exe 2017-12-13 Steam).
pub fn damage_owner_class(bits_206c: u8, bits_206d: u8, bits_206e: u8) -> DamageOwnerClass {
    if bits_206d & 2 != 0 { DamageOwnerClass::Player }
    else if bits_206d & 4 != 0 { DamageOwnerClass::Pawn }
    else if bits_206d & 8 != 0 {
        if bits_206e & 0x10 == 0 { DamageOwnerClass::Npc } else { DamageOwnerClass::Enemy }
    }
    else if bits_206c & 0x80 != 0 { DamageOwnerClass::Enemy }
    else if bits_206d & 0x10 != 0 { DamageOwnerClass::Class4 }
    else { DamageOwnerClass::Unknown }
}

/// Inputs to the civilian *lethal-hit* dispatch, not a general damage floor.
/// `region` represents native DAT018750F0: JP returns0, US/default1,
/// EU2 and AS3. It is not gameplay difficulty.
#[derive(Debug, Clone, Copy)]
pub struct CivilianDeathContext {
    pub region: Region,
    pub intel_type: Option<u8>,
    pub human_enemy: bool,
    pub ledger_civilian: Option<i8>,
    pub damage_owner: DamageOwnerClass,
    pub current_action: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CivilianSurvival {
    pub hp: f32,
    /// A request for cNpcActEscape; the scalar helper does not fabricate
    /// navigation, a motion or completion of the previous authored graph.
    pub action: Option<u32>,
}

/// Called only after real HP application reaches zero. Returning None
/// takes ordinary uPlayerBase death; every native guard must be present.
// Translated from uNpc::vf72 0x00BB7410 (DDDA.exe 2017-12-13 Steam).
pub fn civilian_lethal_response(context: CivilianDeathContext) -> Option<CivilianSurvival> {
    if context.region != Region::Japan || context.intel_type.is_some_and(|kind| kind != 0)
        || context.human_enemy || context.ledger_civilian != Some(1)
        || !matches!(context.damage_owner,
            DamageOwnerClass::Player | DamageOwnerClass::Pawn | DamageOwnerClass::Class4) {
        return None;
    }
    Some(CivilianSurvival { hp: 1.0,
        action: (![0x0300_0044, 0xa6].contains(&context.current_action)).then_some(0x0300_0044) })
}

/// Convert the canonical NPC intel flags (+0x5C0, obtained by vf180
/// 0x00BB4730) into the ordinary NPC's inline collision flags (+0x1DDC).
/// This is the non-human-enemy branch: +0x206E bit 0x10 is clear.
/// `0x80000` is also tested by the executable; its script meaning has
/// not been established, so it retains its source bit here.
// Translated from 0x00BB21E0 (DDDA.exe 2017-12-13 Steam).
pub fn ordinary_collision_rates(intel_flags: u32) -> CollisionRates {
    let mut flags = 0;
    if intel_flags & (0x0800_0000 | 0x0008_0000) != 0 {
        flags |= 1;
    }
    if intel_flags & 0x0400_0000 != 0 {
        flags |= 2;
    }
    if intel_flags & 0x1000_0000 != 0 {
        flags |= 4;
    }
    if intel_flags & (0x0200_0000 | 0x0008_0000) != 0 {
        flags |= 8;
    }
    CollisionRates { flags, ..Default::default() }
}

/// Invincible rejects valuation/application, while the collision still
/// records the contact in its hit history afterward (0x0076EB60).
/// NoDamage is a later HP-only gate in 0x00776F50.
// Translated from 0x0076B7E0 at 0x0076BD76 (DDDA.exe 2017-12-13 Steam).
pub fn accepts_hit(rates: &CollisionRates) -> bool {
    rates.flags & 4 == 0
}

/// Width of the gauge's filled bar, or hidden. `authored_width` comes
/// from the player's installed `npc_gauge` layout, never a pixel guess.
/// The source does not clamp ratios above one.
// Translated from 0x0069F5A0 (DDDA.exe 2017-12-13 Steam).
pub fn gauge_fill(hp: f32, hp_max: f32, dead: bool, authored_width: f32) -> Option<f32> {
    if !(hp_max.abs() >= f32::EPSILON) {
        return None;
    }
    let ratio = hp / hp_max;
    if !(ratio > 0.0) || dead {
        return None;
    }
    Some((ratio * authored_width).max(3.0))
}
