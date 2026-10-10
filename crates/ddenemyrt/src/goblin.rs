//! The goblin unit (uEm0100): think, actions, motions and movement.
//!
//! One frame of a goblin, in the order of docs/FRAME.md 5.2 (enemies run
//! the same phases as the Arisen):
//! 1. phase 90, AI: the FSM (`ddai::think::Thinker`, when the unit has one)
//!    and the unit's think (`ddai::thinktbl::em0100::Driver`, 0x008d2360),
//!    which picks an action id from the built-in tables;
//! 2. phase 91, the action: a new action starts its motion; the current one
//!    turns the unit toward its target where it does;
//! 3. phase 92, the motion advance and its root motion
//!    (`0x0105B740`: position += the root travel, turned by the yaw);
//! 4. the unit stays on the ground (`ddsim::Ground`).
//!
//! The actions' motions and ends come from their classes (see [`ACTIONS`]).

use crate::body::{load, Body, CollisionSet, MotionId};
use crate::combat::{Attacker, Fighter, Victim};
use ddai::thinktbl::em0100::{self, Driver};
use ddai::thinktbl::{Host as ThinkHost, Slot, Tables};
use ddai::Error;
use ddcombat::damage::{CollisionRates, EnemyAttacker, EnemyDefender, RegionPool, Resist};
use ddcombat::volumes::HitHistory;
use std::sync::Arc;

#[path = "goblin_equipment.rs"]
pub mod equipment;

/// The goblin's data from the install.
#[derive(Debug, Clone)]
pub struct GoblinData {
    pub equipment: equipment::Catalog,
    pub sensor: crate::npc_sensor::Sensor,
    /// The rModel actually loaded for this family; LAN guests load the same path locally.
    pub model: String,
    pub body: Body,
    pub tables: Arc<Tables>,
    /// Action id -> class name (the unit's action table 0x018335d0).
    pub classes: Vec<String>,
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    /// The region pools a fresh goblin starts with (`rRegionStatus` type 0).
    pub regions: Vec<RegionPool>,
    /// The push-out rank base: `cCharParamEnemy` +0xD4 `OBJ補正サイズ`
    /// (the actor copy at +0x5A84 read by `uEnemy::vf90` 0x00AA96E0;
    /// goblin 13).
    pub obj_size: u32,
    /// Experience for killing it: unit +0x5C24, set by `uEm0100::vf33`
    /// (0x008CDD80) when the placement gave none: 100 for kind 5 (uEm0100;
    /// 550 for the uEm0101 kind 6, 1300 / 2000 for kinds 7 / 142), copied
    /// to +0x59B8 by `uEnemy::vf34` (0x00AA93EC).
    pub exp: u32,
}

/// Motion banks of the goblin, by the bank number its motion ids carry.
pub const BANKS: [(u8, &str); 3] = [
    (0, r"motion\em\e01\e0100_co\e0100_co"),
    (1, r"motion\em\e01\e0100_at\e0100_at"),
    (2, r"motion\em\e01\e0100_dm\e0100_dm"),
];

impl GoblinData {
    /// Loads `em0100` from the install and its think tables from the exe.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let model = r"model\em\e01\e0100\e0100";
        let skeleton = load::skeleton(vfs, model)?;
        let mut banks = vec![None; 3];
        for (n, path) in BANKS {
            banks[usize::from(n)] = Some(load::bank(vfs, path)?);
        }
        let collision: Vec<CollisionSet> = vec![load::collision(
            vfs,
            r"collision\em\e01\e0100\e0100",
            Some(r"collision\em\e01\e0100\e0100_00"),
        )?];
        let gob = ddenemy::EnemyData::load(vfs, ddenemy::EnemyId::parse("em0100")?)?;
        let stats = &gob.common.as_ref().ok_or("em0100: no common params")?.1;
        let rst = gob.regions.as_ref().ok_or("em0100: no region status")?;
        let region = rst
            .regions
            .iter()
            .find(|r| r.kind == 0)
            .ok_or("em0100: no region type 0")?;
        let res = |k: &str| stats.resistances.get(k).copied().unwrap_or(1.0);
        let resist = Resist {
            element: [res("耐炎"), res("耐氷"), res("耐雷"), res("耐聖"), res("耐魔")],
            slash: res("耐斬"),
            strike: res("耐打"),
        };
        let regions = region
            .elements
            .iter()
            .map(|e| {
                let mut r = RegionPool::from_element(
                    e.id,
                    e.hp_max,
                    e.dp_max,
                    e.dp_speed,
                    e.bp_max,
                    e.bp_speed,
                    e.damage_adjustment,
                    e.hit_stop_adjustment,
                    e.attributes,
                    e.surface,
                    e.dp_reset_timer_max,
                    e.bp_reset_timer_max,
                );
                r.resist = resist;
                // status thresholds (0x00770FD0), memory order of
                // cCharParamEnemy +0x54..+0xA0; absent labels keep 999
                let th = |k: &str| stats.resistances.get(k).copied().unwrap_or(999.0);
                r.status = ddcombat::status::StatusPools::from_char_param([
                    th("耐毒"), th("耐のろま"), th("耐暗闇"), th("耐睡眠"), th("耐油まみれ"),
                    th("耐ずぶ濡れ"), th("耐敵化"), th("耐沈黙"), th("耐スキル封印"), th("耐呪い"),
                    th("耐氷漬け"), th("耐延焼"), th("耐落雷"), th("耐久聖"), th("耐久魔"),
                    th("耐石化"), th("耐攻撃力ダウン"), th("耐防御力ダウン"), th("耐魔法攻撃力ダウン"),
                    th("耐魔法防御力ダウン"),
                ]);
                r
            })
            .collect();
        Ok(GoblinData {
            equipment: equipment::Catalog::read(exe)?,
            sensor: crate::npc_sensor::Sensor::load(vfs)?,
            model: model.into(),
            // the unit's scale (owner +0x60..+0x68) is *assumed* 1 for a
            // standard goblin
            body: Body {
                skeleton,
                banks,
                collision,
                scale: 1.0,
            },
            tables: Arc::new(Tables::read(exe).map_err(|e| e.to_string())?),
            classes: em0100::action_classes(exe).map_err(|e| e.to_string())?,
            attacker: EnemyAttacker::new(stats.attack, stats.magic_attack),
            defender: EnemyDefender::new(stats.defense, stats.magic_defense),
            regions,
            exp: 100,
            obj_size: {
                let x = &gob.common.as_ref().ok_or("em0100: no common params")?.0.object;
                match x.get(&x.root, "OBJ補正サイズ") {
                    Some([mtxfs::Value::U(v)]) => *v as u32,
                    _ => return Err("em0100: no OBJ補正サイズ".into()),
                }
            },
        })
    }
}

/// The world-facing answers the goblin's think asks of its unit (think
/// +0x4c / +0x50 and the rand).
#[derive(Debug, Clone)]
pub struct UnitAnswers {
    /// The current action (unit +0x2dd4).
    pub action: i32,
    /// Check ids from 1000 that answer true. The path callbacks 1016 /
    /// 1017 (0x008ab3b0, 0x008ab5b0) are *not ported*: on open ground the
    /// host answers 1017 ("a path to the target") true.
    pub yes: Vec<u32>,
    /// Commits made (set callback ids), for inspection.
    pub sets: Vec<u32>,
    /// *Placeholder* for the game's rand (0x00d00690, one global state):
    /// a xorshift32 per unit keeps a fight deterministic.
    pub seed: u32,
}

impl ThinkHost for UnitAnswers {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, Error> {
        Ok(em0100::check(id, arg, self.action).unwrap_or_else(|| self.yes.contains(&id)))
    }
    fn set(&mut self, id: u32, _arg: u32) -> Result<u32, Error> {
        self.sets.push(id);
        Ok(0)
    }
    fn rand(&mut self) -> u32 {
        let mut x = self.seed.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }
}

/// A unit the goblin may target; NPCs use combat owners, never player seats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub id: u32,
    pub pos: [f32; 3],
    pub alive: bool,
}

/// One goblin.
/// The goblin's FSM (`cAICtrl` +0x2E64), when it has one.
pub struct Fsm(pub ddai::think::Thinker);

impl std::fmt::Debug for Fsm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fsm({:?})", self.0.current_names())
    }
}

#[derive(Debug)]
pub struct Goblin {
    /// Placement equipment and the native vf124 body selection, resolved once.
    pub appearance: Option<equipment::Appearance>,
    /// vf115 initializes group mask1. Dynamic sensor-mask writes from
    /// later AI states have not yet been ported.
    pub sensor_mask: u32,
    pub sensor_error: Option<String>,
    pub distance_hate: crate::npc_sensor::DistanceHate,
    pub pos: [f32; 3],
    pub yaw: f32,
    /// The current action id (unit +0x2dd4), -1 before the first.
    pub action: i32,
    /// The playing motion and its frame.
    pub motion: MotionId,
    pub frame: f32,
    /// The motion reached its last frame (a one-shot motion holds it).
    pub motion_ended: bool,
    /// The action's done byte (action +0x50): the think may replace it.
    pub done: bool,
    /// A forced action (damage, death) the next think serves first
    /// (unit +0x5B58 death, +0x5B74 damage; 0x008D2360).
    pub pending: Option<i32>,
    /// cEm0100ActWait's first-entry byte (+0x609B).
    pub waited: bool,
    /// cEm0100ActWaitAtck's phase (0 the base idle 0, 1 a variant 1..4).
    pub phase: u8,
    pub driver: Driver,
    pub answers: UnitAnswers,
    /// The current target's id.
    pub target: Option<u32>,
    /// The experience its death awards (0x008573E0 reads +0x59B8): the
    /// data's default, or the placement's `mExperienceOW` (cSetInfoEnemy
    /// +0xEC -> unit +0x5C24, 0x007A012C) when that is not 0.
    pub exp: u32,
    /// Its part in the hit pass (pools, history, volumes).
    pub fighter: Fighter,
    /// Actions started, in order: (frame count of the unit, action id).
    pub log: Vec<(u32, i32)>,
    /// Frames stepped.
    pub frames: u32,
    /// The unit's FSM (a quest's `rAIFSM`); enemies without one fight on
    /// their think tables alone.
    pub fsm: Option<Fsm>,
    /// Why the FSM stopped, if it did.
    pub fsm_error: Option<String>,
}

/// A placement's stat rates (`cSetInfoPawn`: `HP倍率設定の有無` /
/// `HPの倍率`, `攻撃力倍率設定の有無` / `攻撃力の変化倍率`), copied to the
/// unit by the set-info apply (vf08, 0x007A4BCD: +0x74/+0x7C -> unit
/// +0x209E/+0x20A0, +0x75/+0x80 -> +0x20A4/+0x20A8). `None` = not set.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlacementRates {
    pub hp: Option<f32>,
    pub attack: Option<f32>,
}

/// `uEm0100::vf33` (0x008CDD80) defaults for rates a placement did not
/// set: HP x2 (0x014F9E04) and attack x1.3 (0x0161C874).
pub const DEFAULT_HP_RATE: f32 = 2.0;
pub const DEFAULT_ATTACK_RATE: f32 = 1.3;

impl Goblin {
    /// A goblin placed without stat rates (the vf33 defaults apply).
    pub fn new(data: &GoblinData, owner: u32, pos: [f32; 3], yaw: f32) -> Self {
        Self::placed(data, owner, pos, yaw, PlacementRates::default())
    }

    /// A goblin with its placement's rates: the HP rate multiplies the
    /// main pool's HP and maximum (0x00889040 on the first region of
    /// actor+0x1D18), the attack rate becomes the attacker's multiplier
    /// (actor+0x20A8, read by `enemy_attack` 0x0088B530).
    pub fn placed(data: &GoblinData, owner: u32, pos: [f32; 3], yaw: f32, rates: PlacementRates) -> Self {
        let hp_rate = rates.hp.unwrap_or(DEFAULT_HP_RATE);
        let mut regions = data.regions.clone();
        if let Some(main) = regions.first_mut() {
            main.hp *= hp_rate;
            main.hp_max *= hp_rate;
        }
        let mut attacker = data.attacker;
        attacker.attack_mul = Some(rates.attack.unwrap_or(DEFAULT_ATTACK_RATE));
        Goblin {
            appearance: None,
            sensor_mask: 1,
            sensor_error: None,
            distance_hate: Default::default(),
            pos,
            yaw,
            action: -1,
            motion: MotionId::new(0, 0),
            frame: 0.0,
            motion_ended: false,
            done: true,
            pending: None,
            waited: false,
            phase: 0,
            driver: Driver::new(1.0),
            answers: UnitAnswers {
                action: -1,
                yes: vec![1017],
                sets: vec![],
                seed: 0x9e37_79b9 ^ owner,
            },
            target: None,
            exp: data.exp,
            fighter: Fighter {
                owner,
                attacker: Attacker::Enemy(attacker),
                victim: Victim::Enemy {
                    defender: data.defender,
                    regions,
                    rates: CollisionRates::default(),
                },
                attacks: vec![],
                hurts: vec![],
                history: HitHistory::default(),
                statuses: Default::default(),
                dots: Default::default(),
                pools: Default::default(),
            },
            log: vec![],
            frames: 0,
            fsm: None,
            fsm_error: None,
        }
    }

    /// Unit +0x2360 bit 0 as the current action leaves it: set by every
    /// standing action (wait, turn, locomotion, attacks) and by a flinch
    /// once it ends; clear while a flinch plays (cEmActDmgShrink states
    /// 0 / 0x14 write 0x40004000) and while dying is not relevant here.
    pub fn bit0(&self) -> bool {
        let shrink = action::SHRINK_FRONT.contains(&self.action)
            || action::SHRINK_BACK.contains(&self.action)
            || self.action == action::SHRINK_PLAIN_FRONT
            || self.action == action::SHRINK_PLAIN_BACK;
        !shrink || self.motion_ended
    }

    /// The push rank (actor +0x1DF0) `uEnemy::vf90` (0x00AA96E0) derives
    /// from the base and the action's +0x2360 bits: +1 for 0x104 (attacks
    /// 0x101, walks 5), else +2 for 0x18 (runs 9, dashes 0x11); the other
    /// goblin actions (waits, turns, flinches, death) add nothing. 0 stays 0.
    pub fn push_rank(&self, base: u32) -> u32 {
        if base == 0 {
            return 0;
        }
        match self.action {
            action::N_ATTACK1 | action::N_ATTACK2 | action::N_ATTACK12 => base + 1,
            action::DASH_TRACE | action::DASH_TRACE_1 => base + 2,
            _ => base,
        }
    }

    /// The goblin is in one of its attacks (unit +0x2360 bit 0x100).
    pub fn attacking(&self) -> bool {
        matches!(
            self.action,
            action::N_ATTACK1 | action::N_ATTACK2 | action::N_ATTACK12
        )
    }

    pub fn dead(&self) -> bool {
        self.fighter.victim.dead()
    }

    /// A replay digest of the unit.
    pub fn digest(&self) -> String {
        format!(
            "g{}:{:?}:{:?}:{}:{:?}:{}:{}:sensor{}:{:?}:{:?};",
            self.fighter.owner,
            self.pos.map(f32::to_bits),
            self.yaw.to_bits(),
            self.action,
            self.motion,
            self.frame.to_bits(),
            self.fighter.victim.hp().to_bits(),
            self.sensor_mask, self.distance_hate, self.sensor_error
        )
    }

    /// Pass C: pose and volumes for the next frame's hits.
    pub fn volumes(&mut self, data: &GoblinData) {
        let pose = data.body.pose(self.motion, self.frame, self.pos, self.yaw);
        let acts = data.body.activations(self.motion, self.frame);
        (self.fighter.attacks, self.fighter.hurts) = data.body.volumes(&acts, &pose, self.fighter.owner);
    }
}

/// Measures `target` from the goblin into a think slot (0x008b6de0).
pub fn slot(g: &Goblin, target: &Target) -> Slot {
    Slot::measure(g.pos, g.yaw, target.pos, 0.0, target.id + 1)
}

/// A motion id as the goblin's actions request it: bank `(id >> 8) & 0xF`,
/// slot `id & 0xFF` (`0x00AB4900`).
pub fn motion_id(id: u16) -> MotionId {
    MotionId::new(((id >> 8) & 0xf) as u8, id & 0xff)
}

/// The damage-bank fallbacks of `0x00AB4900` for motions the goblin's
/// `e0100_dm` lacks: 0x201..0x204 and 0x20A play 0x200, 0x205 plays 0x201
/// (itself 0x200), 0x207 and 0x20D play 0x202.
// Translated from 0x00AB4900 (DDDA.exe 2017-12-13 Steam), the cases the
// goblin's actions reach.
pub fn dm_fallback(id: u16) -> u16 {
    match id {
        0x201..=0x205 | 0x20a => 0x200,
        0x207 | 0x20d => 0x202,
        id => id,
    }
}

/// Degrees to radians as the turn helpers take them.
const DEG: f32 = std::f32::consts::PI / 180.0;

/// Action ids (unit +0x2dd4) and what their classes do (vf05 / vf06).
pub mod action {
    pub const WAIT: i32 = 0;
    pub const WAIT_ATCK: i32 = 1;
    pub const WAIT_ATCK_END: i32 = 2;
    /// cEm0100ActRunTrace (vtable 0x015A4C8C), the approach beyond 20 m.
    pub const RUN_TRACE: i32 = 9;
    /// cEm0100ActRunEnd (vtable 0x015A4CD4).
    pub const RUN_END: i32 = 10;
    /// cEm0100ActRunStumble (vtable 0x015A4D1C).
    pub const RUN_STUMBLE: i32 = 11;
    pub const DASH_TRACE: i32 = 13;
    pub const DASH_TRACE_1: i32 = 14;
    pub const N_ATTACK1: i32 = 101;
    pub const N_ATTACK2: i32 = 102;
    pub const N_ATTACK12: i32 = 104;
    /// `cEmActDmgShrink`, front (122..124) and back (125..127).
    pub const SHRINK_FRONT: [i32; 3] = [0x7a, 0x7c, 0x7b];
    pub const SHRINK_BACK: [i32; 3] = [0x7d, 0x7f, 0x7e];
    /// The common light flinch (bank 3 record 8, `cEmActDmgShrink`) that
    /// `uEnemy::vf70` (0x00AABAE0) picks for a type-1 front hit when the
    /// goblin's own set does not apply (+0x2360 bit 0 clear). The back
    /// one (record 9) is *assumed* to mirror it.
    pub const SHRINK_PLAIN_FRONT: i32 = 0x0300_0008;
    pub const SHRINK_PLAIN_BACK: i32 = 0x0300_0009;
    /// `cEmActActingDie`, front and back.
    pub const DIE_FRONT: i32 = 0x80;
    pub const DIE_BACK: i32 = 0x81;
}

/// The first motion an action plays (its vf05), interpolation left out
/// (motions switch at once: blending is presentation, ddview's).
// Translated from the vf05 of each class: cEm0100ActWait 0x008D4A10,
// cEm0100ActDashTrace 0x008D6920, Turn90 0x008D6E90 / Turn180 0x008D7190,
// NAttack1/12/123 0x008DD200, NAttack2 0x008DCA10, cEmActDmgShrink
// 0x0089F2D0 (motion A of the record), cEmActActingDie 0x008A66E0 (DDDA.exe
// 2017-12-13 Steam).
pub fn first_motion(action: i32) -> Option<u16> {
    Some(match action {
        action::WAIT => 0x00a,
        action::WAIT_ATCK => 0x000,
        action::WAIT_ATCK_END => 0x013,
        action::DASH_TRACE | action::DASH_TRACE_1 => 0x02c,
        // vf05 0x008D62C0 / 0x008D64B0 / 0x008D65E0 (0x323 / 0x325 for a
        // record variant the goblin's records 9/10 don't set)
        action::RUN_TRACE => 0x023,
        action::RUN_END => 0x025,
        action::RUN_STUMBLE => 0x027,
        // turns: Turn90 0x39 + (v & 1), 0x35 + (v & 1) with variant bit 1;
        // Turn180 the same from 0x3b / 0x37
        18 => 0x39,
        20 => 0x3a,
        19 => 0x3b,
        21 => 0x3c,
        26 => 0x35,
        28 => 0x36,
        27 => 0x37,
        29 => 0x38,
        action::N_ATTACK1 | action::N_ATTACK12 => 0x101,
        action::N_ATTACK2 => 0x102,
        // cEmActDmgShrink records 0x7a..0x7f, motion A
        0x7a => 0x200,
        0x7c => 0x205,
        0x7b => 0x204,
        0x7d => 0x202,
        0x7f => 0x208,
        0x7e => 0x207,
        action::SHRINK_PLAIN_FRONT => 0x200,
        // *assumed* (record 9 not read)
        action::SHRINK_PLAIN_BACK => 0x202,
        action::DIE_FRONT => 0x239,
        action::DIE_BACK => 0x23a,
        _ => return None,
    })
}

/// The yaw from `from` toward `to` (0 faces +z).
fn yaw_to(from: [f32; 3], to: [f32; 3]) -> f32 {
    (to[0] - from[0]).atan2(to[2] - from[2])
}

/// `yaw` stepped toward `goal` by at most `step` radians (the turn
/// controller `0x007975D0` / `0x00964550`, *assumed* to take the short
/// way).
fn turn_toward(yaw: f32, goal: f32, step: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut d = (goal - yaw).rem_euclid(tau);
    if d > std::f32::consts::PI {
        d -= tau;
    }
    let y = yaw + d.clamp(-step, step);
    (y + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// Think flags the host sets while the goblin fights a target: the alert
/// state 2 (flag 2, written by 0x008d1280, which sends the default table
/// into combat table 1) and the flags the melee rows require (3, 8, 20).
/// *Placeholder*: their writers are not traced (docs/AI.md); a goblin that
/// has a live target is taken as alerted.
pub const COMBAT_FLAGS: u32 = 1 << 2 | 1 << 3 | 1 << 8 | 1 << 20;

/// The turn rate of DashTrace: unit +0x6008, 120 degrees per second
/// (uEm0100 constructor 0x008CD5B0).
pub const DASH_TURN_DEG_PER_S: f32 = 120.0;

/// The turn rate of RunTrace: unit +0x6004, 90 degrees per second (ctor
/// 0x008CD5B0 from 0x01516F38).
pub const RUN_TURN_DEG_PER_S: f32 = 90.0;
/// NAttack1/2/12 track their target at 0.0523599 rad (3 degrees) per frame
/// (0x008DC820 via 0x00964550).
pub const ATTACK_TRACK_RAD: f32 = 0.052_359_9;

/// The signed angle `a - b`, wrapped to [-pi, pi).
fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Goblin {
    /// Starts `action` (phase 91: the action manager applies the request,
    /// then the new action's vf05).
    fn start(&mut self, data: &GoblinData, action: i32) {
        self.action = action;
        self.answers.action = action;
        self.driver.action = action;
        self.done = false;
        self.log.push((self.frames, action));
        let Some(m) = first_motion(action) else {
            // an action this runtime does not play: stand
            self.play(data, 0x00a, 0.0);
            self.done = true;
            return;
        };
        if action == action::WAIT_ATCK {
            // 0x008D4DD0: keep an idle 0..4 already playing (phase 1), else
            // 50% idle 0 (phase 0), else a random 1..4 (phase 1)
            let idle = self.motion.bank == 0 && self.motion.slot <= 4;
            if idle {
                self.phase = 1;
            } else if self.answers.rand().is_multiple_of(2) {
                self.phase = 0;
                self.play(data, 0x000, 0.0);
            } else {
                self.phase = 1;
                let m = 1 + (self.answers.rand() % 4) as u16;
                self.play(data, m, 0.0);
            }
            self.done = true;
            return;
        }
        let mut start = 0.0;
        if action == action::WAIT && !self.waited {
            // first entry: a random start frame in 0..179 (0x008D4A10)
            self.waited = true;
            start = (self.answers.rand() % 180) as f32;
        }
        self.play(data, m, start);
        // cEm0100ActWait sets its done byte every frame (0x008D4B90)
        self.done = action == action::WAIT;
    }

    fn play(&mut self, data: &GoblinData, id: u16, start: f32) {
        let mut id = id;
        while data.body.motion(motion_id(id)).is_none() && dm_fallback(id) != id {
            id = dm_fallback(id);
        }
        self.motion = motion_id(id);
        self.frame = start;
        self.motion_ended = false;
    }

    /// The hit pass's verdict on this goblin: a death or a flinch becomes
    /// the forced action the next think serves.
    ///
    /// Death (`uEm0100::vf72` 0x008D4080): `cEmActActingDie` 0x80 from the
    /// front, 0x81 from behind (the special deaths: break, burning,
    /// poison, a pending knockdown, are not modelled). Stagger
    /// (`uEm0100::vf70` 0x008D3DA0): the goblin's light set, 50% 0x7A, else
    /// 50% 0x7C, else 0x7B from the front (0x7D / 0x7F / 0x7E behind);
    /// front means the attacker within 90 degrees of the facing.
    pub fn react(&mut self, event: &crate::combat::HitEvent, attacker_pos: Option<[f32; 3]>) {
        if self.action == action::DIE_FRONT
            || self.action == action::DIE_BACK
            || self
                .pending
                .is_some_and(|p| (action::DIE_FRONT..=action::DIE_BACK).contains(&p))
        {
            return;
        }
        let front = attacker_pos
            .is_none_or(|p| wrap(yaw_to(self.pos, p) - self.yaw).abs() <= std::f32::consts::FRAC_PI_2);
        if event.killed {
            self.pending = Some(if front {
                action::DIE_FRONT
            } else {
                action::DIE_BACK
            });
        } else if event.reaction.stagger && !self.bit0() {
            self.pending = Some(if front {
                action::SHRINK_PLAIN_FRONT
            } else {
                action::SHRINK_PLAIN_BACK
            });
        } else if event.reaction.stagger {
            let set = if front {
                action::SHRINK_FRONT
            } else {
                action::SHRINK_BACK
            };
            let pick = if self.answers.rand().is_multiple_of(2) {
                set[0]
            } else if self.answers.rand().is_multiple_of(2) {
                set[1]
            } else {
                set[2]
            };
            self.pending = Some(pick);
        }
    }

    /// One frame of the goblin (FRAME.md 5.2, line 15). `step` is the
    /// unit's step in 30 fps frames.
    pub fn step(&mut self, data: &GoblinData, targets: &[Target], ground: &dyn ddsim::Ground, step: f32) {
        self.step_with_npcs(data, targets, &[], ground, step);
    }

    /// Keeps NPC sensor candidates out of the FSM's player-position roster.
    pub fn step_with_npcs(&mut self, data: &GoblinData, players: &[Target], npcs: &[Target],
        ground: &dyn ddsim::Ground, step: f32) {
        self.frames += 1;
        let all: Vec<_> = players.iter().chain(npcs).copied().collect();
        let mut admitted = vec![];
        self.sensor_error = None;
        for (target, subtype) in players.iter().map(|t| (t, 1)).chain(npcs.iter().map(|t| (t, 4))) {
            if !target.alive { continue; }
            match data.sensor.admits(self.sensor_mask, self.pos, self.yaw, *target, subtype) {
                Ok(true) => admitted.push(*target),
                Ok(false) => {},
                Err(error) => { self.sensor_error = Some(error); break; }
            }
        }
        let target = if self.sensor_error.is_none() {
            self.distance_hate.select(&admitted, &all, self.pos)
                // 0x008D2930 falls back to the primary Arisen if hate is
                // empty. Co-op policy: first live registered Arisen.
                .or_else(|| players.iter().find(|t| t.alive).copied())
        } else { None };
        self.target = target.map(|t| t.id);
        // phase 90: the AI controller runs the FSM (0x0059f440, restarting a
        // finished one once the action allows, `controlled_frame`), then
        // the unit's think (0x008D2360)
        self.run_fsm(data, players);
        // the think: forced actions first, then the table when the current
        // action allows (its done byte)
        if let Some(a) = self.pending.take() {
            self.driver.mgr.reset_to_base();
            self.start(data, a);
        } else if self.done && !self.dead() {
            self.driver.mgr.flags = if target.is_some() { COMBAT_FLAGS } else { 0 };
            self.driver.mgr.slots[0] = target.map_or_else(Slot::default, |t| slot(self, &t));
            self.answers.action = self.action;
            if let Ok(Some(a)) = self.driver.think(&data.tables, &mut self.answers) {
                self.start(data, a);
            }
        }
        // the think's timers count with the unit's step (0x008b7930 from
        // uEm0100 vf08)
        self.driver.mgr.tick(step);
        // phase 91: the action's vf06
        self.act(data, target, step);
        // phase 92: the motion advance and its root travel
        self.advance(data, step);
        // the unit stays on the ground (the goblin's own scenery sweep,
        // capsule r44 from 44 to 106 cm, is *not run*: the height under it)
        if let Some(h) = ground.height(self.pos[0], self.pos[2], self.pos[1] + ddsim::STEP_UP) {
            self.pos[1] = h;
        }
    }

    /// The FSM's frame and its requests: an action becomes a forced action
    /// for the think; a think table is handed to the unit's `cThinkMgr`
    /// (CtrlThinkTbl 0x005a9240 -> vf 0x2f8: built-in table `index`); an
    /// escape point fills think slot 1 (vf186 0x008c75d0); free flags go to
    /// the FSM itself. An FSM error stops the FSM (logged in `fsm_error`).
    fn run_fsm(&mut self, data: &GoblinData, targets: &[Target]) {
        let Some(Fsm(t)) = self.fsm.as_mut() else { return };
        let inputs = ddai::think::Inputs {
            actor: Some(ddai::think::Actor {
                pos: self.pos,
                world_pos: self.pos,
                action_done: self.done,
            }),
            players: targets.iter().map(|t| t.pos).collect(),
            ..Default::default()
        };
        let requests = match t.controlled_frame(&inputs) {
            Ok(r) => r,
            Err(e) => {
                self.fsm_error = Some(e.to_string());
                self.fsm = None;
                return;
            }
        };
        for r in requests {
            match r {
                ddai::think::Request::Action { action } => self.pending = Some(action as i32),
                ddai::think::Request::ThinkTable { table, .. } => {
                    if let Some(root) = data.tables.root(table as u32) {
                        self.driver.mgr.request_table(root);
                    }
                }
                ddai::think::Request::EscapePos { pos } => {
                    self.driver.mgr.slots[1] = Slot::measure(self.pos, self.yaw, pos, 0.0, 0);
                }
                ddai::think::Request::FreeCondition { bits } => {
                    if let Some(Fsm(t)) = self.fsm.as_mut() {
                        t.or_free_condition(bits);
                    }
                }
                _ => {}
            }
        }
    }

    /// The current action's per-frame part (vf06).
    fn act(&mut self, data: &GoblinData, target: Option<Target>, step: f32) {
        let goal = target.map(|t| yaw_to(self.pos, t.pos));
        let frame = self.frame;
        match self.action {
            action::WAIT => {
                // 0x008D4B90: on the motion's end, 50% a variant (0x0F 41%,
                // 0x0B 30%, 0x0C 29%), from a variant back to 0x0A
                if self.motion_ended {
                    let next = if self.motion == motion_id(0x00a) && self.answers.rand().is_multiple_of(2) {
                        match self.answers.rand() % 100 {
                            0..=40 => 0x00f,
                            41..=70 => 0x00b,
                            _ => 0x00c,
                        }
                    } else {
                        0x00a
                    };
                    self.play(data, next, 0.0);
                }
                self.done = true;
            }
            action::WAIT_ATCK => {
                // 0x008D5000: phase 0, on the motion's end 3/4 a random idle
                // 1..4 (phase 1); phase 1 back to idle 0. No turning.
                if self.motion_ended {
                    if self.phase == 0 {
                        if !self.answers.rand().is_multiple_of(4) {
                            let m = 1 + (self.answers.rand() % 4) as u16;
                            self.play(data, m, 0.0);
                            self.phase = 1;
                        }
                    } else {
                        self.play(data, 0x000, 0.0);
                        self.phase = 0;
                    }
                }
                self.done = true;
            }
            action::WAIT_ATCK_END => {
                // 0x0091FB60: done at the motion's end
                if self.motion_ended {
                    self.done = true;
                }
            }
            action::DASH_TRACE | action::DASH_TRACE_1 => {
                // 0x008D69B0: the start motion, then the loop 0x2D; turns
                // toward the goal at unit +0x6008 per second
                if self.motion == motion_id(0x02c) && self.motion_ended {
                    self.play(data, 0x02d, 0.0);
                    self.done = true;
                }
                if let Some(g) = goal {
                    self.yaw = turn_toward(self.yaw, g, DASH_TURN_DEG_PER_S * DEG * step / 30.0);
                }
            }
            action::RUN_TRACE => {
                // 0x008D6340: the start motion, then the loop 0x24 (done
                // from then on); turns toward the goal at unit +0x6004
                if self.motion == motion_id(0x023) && self.motion_ended {
                    self.play(data, 0x024, 0.0);
                    self.done = true;
                }
                if let Some(g) = goal {
                    self.yaw = turn_toward(self.yaw, g, RUN_TURN_DEG_PER_S * DEG * step / 30.0);
                }
            }
            // RunEnd / RunStumble keep the base vf06 0x0091FB60: done at
            // the motion's end
            action::RUN_END | action::RUN_STUMBLE => {
                if self.motion_ended {
                    self.done = true;
                }
            }
            18..=21 | 26..=29 => {
                // 0x008D6FA0: 90 (or 180) degrees over the motion's frames
                let angle = if matches!(self.action, 19 | 21 | 27 | 29) {
                    180.0
                } else {
                    90.0
                };
                let frames = data.body.frames(self.motion).max(1) as f32;
                if let Some(g) = goal {
                    self.yaw = turn_toward(self.yaw, g, angle * DEG / frames * step);
                }
                if self.motion_ended {
                    self.done = true;
                }
            }
            action::N_ATTACK1 | action::N_ATTACK2 | action::N_ATTACK12 => {
                // 0x008DC820 / 0x008DCAC0 / 0x008DCF60
                let second = self.motion == motion_id(0x102);
                let track_until = if second { 31.0 } else { 42.0 };
                if frame <= track_until {
                    if let Some(g) = goal {
                        self.yaw = turn_toward(self.yaw, g, ATTACK_TRACK_RAD * step);
                    }
                }
                if self.action == action::N_ATTACK12 && !second && frame >= 59.0 {
                    self.play(data, 0x102, 16.0);
                } else if self.motion_ended {
                    self.done = true;
                }
            }
            a if (action::SHRINK_FRONT.contains(&a)
                || action::SHRINK_BACK.contains(&a)
                || a == action::SHRINK_PLAIN_FRONT
                || a == action::SHRINK_PLAIN_BACK)
                // cEmActDmgShrink on the ground, no knockback data: done at
                // the motion's end (0x0089F420)
                && self.motion_ended =>
            {
                self.done = true;
            }
            // cEmActActingDie never sets its done byte (0x008A6930); an
            // action not played here stands
            _ => {}
        }
    }

    /// Phase 92 (0x010629F0): the frame advances by `step` and the root
    /// travel moves the unit, turned by its yaw (0x0105B740).
    fn advance(&mut self, data: &GoblinData, step: f32) {
        let Some(m) = data.body.motion(self.motion) else {
            return;
        };
        let last = m.frame_count.saturating_sub(1) as f32;
        let looping = m.loop_frame >= 0;
        let root = ddsim::RootMotion::from_motion(m);
        let d = if looping {
            root.delta(self.frame, step)
        } else {
            root.delta_once(self.frame, step)
        };
        let (s, c) = self.yaw.sin_cos();
        self.pos[0] += d[0] * c + d[2] * s;
        self.pos[1] += d[1];
        self.pos[2] += -d[0] * s + d[2] * c;
        let f = self.frame + step;
        if looping {
            let lf = m.loop_frame as f32;
            self.frame = if f > last {
                lf + (f - last).rem_euclid((last - lf).max(1.0))
            } else {
                f
            };
            self.motion_ended = f > last;
        } else {
            self.frame = f.min(last);
            self.motion_ended = f >= last;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_ids_and_damage_fallbacks() {
        assert_eq!(motion_id(0x101), MotionId::new(1, 1));
        assert_eq!(motion_id(0x239), MotionId::new(2, 0x39));
        assert_eq!(motion_id(0x02d), MotionId::new(0, 0x2d));
        assert_eq!(dm_fallback(0x204), 0x200);
        assert_eq!(dm_fallback(0x20d), 0x202);
        assert_eq!(dm_fallback(0x208), 0x208);
    }

    #[test]
    fn turns_take_the_short_way_and_stop_at_the_goal() {
        let pi = std::f32::consts::PI;
        assert!((turn_toward(0.0, 1.0, 0.1) - 0.1).abs() < 1e-6);
        assert!((turn_toward(0.0, 0.05, 0.1) - 0.05).abs() < 1e-6);
        // across the wrap: from just under pi to just over -pi goes up
        let y = turn_toward(pi - 0.05, -pi + 0.05, 0.03);
        assert!((y - (pi - 0.02)).abs() < 1e-5, "{y}");
        assert!((yaw_to([0.0; 3], [1.0, 0.0, 0.0]) - pi / 2.0).abs() < 1e-6);
    }

    #[test]
    fn first_motions_of_the_actions() {
        assert_eq!(first_motion(action::N_ATTACK1), Some(0x101));
        assert_eq!(first_motion(action::N_ATTACK2), Some(0x102));
        assert_eq!(first_motion(action::DASH_TRACE), Some(0x02c));
        assert_eq!(first_motion(action::RUN_TRACE), Some(0x023));
        assert_eq!(first_motion(action::RUN_STUMBLE), Some(0x027));
        assert_eq!(first_motion(28), Some(0x36));
        assert_eq!(first_motion(action::DIE_FRONT), Some(0x239));
        assert_eq!(first_motion(500), None);
    }
}
