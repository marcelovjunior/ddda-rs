//! A fight: the Arisen and the enemies, stepped in the original's frame
//! order (docs/FRAME.md 5.7). The host calls, each frame:
//! 1. [`Fight::hits`] before anyone moves (step 33: last frame's volumes);
//! 2. its player movement and attacks (`ddsim::Sim::step`), then
//!    [`Fight::players_moved`] (new attacks free their hit history; the
//!    staff's volleys fire their shells, which fly);
//! 3. [`Fight::enemies`] (line 15: think, action, motion, ground);
//! 4. [`Fight::push_out`] (step 38);
//! 5. [`Fight::volumes`] (pass C: poses and the volumes for the next
//!    frame's hits).
//!
//! Each Arisen must be equipped in the sim ([`Fight::equip`]) for its
//! attacks to play.

use crate::arisen::ArisenData;
use crate::arrow::{Arrow, ArrowData, BowData};
use crate::body::MotionId;
use crate::combat::{Attacker, Fighter, HitEvent, Tables, Victim, hit_pass};
use crate::goblin::{Goblin, GoblinData, Target};
use crate::shell::{Shell, ShellParam};
use ddcombat::volumes::HitHistory;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The data a fight reads.
#[derive(Debug, Clone)]
pub struct FightData {
    /// A level-1 Fighter with an Iron Sword.
    pub arisen: ArisenData,
    /// A level-1 Mage with the starting staff.
    pub mage: ArisenData,
    /// A level-1 Strider with the starting dagger.
    pub strider: ArisenData,
    /// Per-skill bank-seven bodies; selected independently for each seat.
    pub skills: crate::skills::SkillSet,
    /// The staff's shell.
    pub shell: ShellParam,
    pub arrow: ArrowData,
    pub bow: BowData,
    /// The attacks' values from `param\pl\other\m_pl000_00`.
    pub attack_param: ddsim::AttackParam,
    pub goblin: GoblinData,
    pub tables: Tables,
    /// The pawns' AI data (`crate::pawn`).
    pub pawn: ddpawnai::pawn::PawnData,
}

impl FightData {
    /// The data of a kit.
    pub fn kit(&self, kit: Kit) -> &ArisenData {
        match kit {
            Kit::Fighter => &self.arisen,
            Kit::Mage => &self.mage,
            Kit::Strider => &self.strider,
        }
    }

    /// Everything from the install and its exe.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let adjust = |p: &str| -> Result<ddcombat::damage::Adjust, String> {
            ddcombat::damage::Adjust::parse(&crate::body::load::read(vfs, p, "rAdjustParam")?)
                .ok_or_else(|| format!("{p}: bad table"))
        };
        let arisen = ArisenData::fighter(vfs)?;
        let mage = ArisenData::mage(vfs)?;
        let mut strider = ArisenData::strider(vfs)?;
        let bow = BowData::load(vfs)?;
        strider.body.banks.resize(10, None);
        strider.body.banks[9] = Some(bow.player_bank.clone());
        let skills = crate::skills::SkillSet::load(vfs, exe, &arisen, &mage)?;
        Ok(FightData {
            arisen,
            mage,
            strider,
            skills,
            shell: ShellParam::load(vfs)?,
            arrow: ArrowData::load(vfs)?,
            bow,
            attack_param: ddsim::AttackParam::from_adjust(&adjust(ADJUST_PARAMS)?.0)
                .ok_or(format!("{ADJUST_PARAMS}: too short"))?,
            goblin: GoblinData::load(vfs, exe)?,
            tables: Tables {
                game: adjust(r"param\game")?,
                dlc: adjust(r"param\dlc")?,
                status_enemy: status_table(vfs, r"param\status\enemy")?,
                status_player: status_table(vfs, r"param\status\player")?,
            },
            pawn: crate::pawn::load(vfs)?,
        })
    }
}

/// An `rStatusParam` as a `ddcombat::cstatus::StatusTable`.
fn status_table(vfs: &ddvfs::Vfs, path: &str) -> Result<ddcombat::cstatus::StatusTable, String> {
    let p = ddenemy::StatusParam::parse(&crate::body::load::read(vfs, path, "rStatusParam")?)?;
    Ok(ddcombat::cstatus::StatusTable(
        p.entries
            .iter()
            .map(|e| {
                e.as_ref().map(|e| ddcombat::cstatus::StatusEntry {
                    category: e.category,
                    is_overwrite: e.is_overwrite,
                    overwrite_num: e.overwrite_num,
                    timer: e.timer,
                    param0: e.param0,
                    param1: e.param1,
                })
            })
            .collect(),
    ))
}

/// Which of the fight's Arisen kits a seat uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kit {
    Fighter,
    Mage,
    Strider,
}

/// The player's tuning table.
pub const ADJUST_PARAMS: &str = r"param\pl\other\m_pl000_00";

/// An Arisen in the fight (the body moves and attacks in ddsim).
#[derive(Debug, Clone)]
pub struct ArisenFighter {
    pub fighter: Fighter,
    pub kit: Kit,
    /// The character level whose stats the fighter carries
    /// ([`Fight::apply_level`]); the kit's data is level 1.
    pub level: u32,
    /// ddsim's attack and volley counts last seen.
    pub attacks_seen: u32,
    pub volleys_seen: u32,
}

/// Per-seat authoritative bow context, supplied at the release update.
/// `pose` must be the actual bow model (Fight::bow_pose is the stationary
/// provider), not the Arisen root. No nearest-enemy aim is selected.
#[derive(Debug, Clone)]
pub struct BowContext {
    pub pose: crate::body::Posed,
    pub target: [f32; 3],
    /// Host-owned engine RNG words. Two authored-spread draws occur even
    /// at width zero, then two accuracy draws unless augment0x48 skips.
    pub random: std::collections::VecDeque<u32>,
    pub augments: [u32; 6],
    pub secondary_attacker: ddcombat::damage::PlayerAttacker,
}

/// The fight's state.
#[derive(Debug)]
pub struct Fight {
    pub data: Arc<FightData>,
    /// By seat.
    pub arisen: BTreeMap<u8, ArisenFighter>,
    pub goblins: Vec<Goblin>,
    /// Native D00690/D006C0, owned once by the host. Only appearance draws
    /// are integrated here: this is not stream-exact with native global
    /// 018D017C, also consumed by unrelated systems not yet ported.
    appearance_rng: crate::goblin::equipment::NativeRng,
    /// Ordinary NPCs keyed by their distinct combat owner, never by seat.
    pub npcs: BTreeMap<u32, crate::npc_body::NpcFighter>,
    pub npc_errors: BTreeMap<u32, String>,
    pub dragon: Option<crate::dragon::OpeningDragon>,
    next_enemy: u32,
    /// The hits of the last [`Fight::hits`].
    pub events: Vec<HitEvent>,
    /// The status updates of the last [`Fight::hits`].
    pub status_events: Vec<crate::combat::StatusEvent>,
    /// Where each fighter stood when its volumes were placed (for the
    /// victims' front / back reactions).
    pub positions: BTreeMap<u32, [f32; 3]>,
    /// Shells in flight.
    pub shells: Vec<Shell>,
    pub arrows: Vec<Arrow>,
    pub bow_contexts: BTreeMap<u8, BowContext>,
    /// Unsupported missing/invalid provider paths, by seat; no fake spawn.
    pub bow_errors: BTreeMap<u8, String>,
    /// Equipped spell projectiles and their impact children.
    pub spells: crate::skills::spells::SpellWorld,
    /// The Arisen each shell owner id was fired by (kept after the shell).
    pub shooters: BTreeMap<u32, u32>,
    next_shell: u32,
    /// Pawns by seat (each also an [`ArisenFighter`] in `arisen`).
    pub pawns: BTreeMap<u8, crate::pawn::PawnSeat>,
    /// *Placeholder* critical roll (only critical-capable weapons read it).
    roll_seed: u32,
    /// The small-enemy life gauge: the last damaged small enemy's owner id
    /// and the frames it stays shown ([`SMALL_GAUGE_FRAMES`]).
    pub small_gauge: Option<(u32, f32)>,
}

/// `uGUILifeGauge` keeps one small-enemy pointer (+0x400) and refreshes its
/// timer (+0x404) to 450 frames on each damage report (0x0067C110, from
/// `uEnemy::vf69`'s damage path at 0x00AAAFBF via 0x00452EE0); 0x0067AC40
/// counts it down and drops the pointer at 0.
pub const SMALL_GAUGE_FRAMES: f32 = 450.0;

/// Owner ids of shells count from here.
pub const SHELL_OWNER_BASE: u32 = 1000;

/// Owner ids: seats keep their number, enemies count from here.
pub const ENEMY_OWNER_BASE: u32 = 100;

impl Fight {
    pub fn new(data: Arc<FightData>) -> Self {
        Fight {
            data,
            arisen: BTreeMap::new(),
            goblins: Vec::new(),
            appearance_rng: Default::default(),
            npcs: BTreeMap::new(),
            npc_errors: BTreeMap::new(),
            dragon: None,
            next_enemy: ENEMY_OWNER_BASE,
            events: Vec::new(),
            status_events: Vec::new(),
            positions: BTreeMap::new(),
            shells: Vec::new(),
            arrows: Vec::new(),
            bow_contexts: BTreeMap::new(),
            bow_errors: BTreeMap::new(),
            spells: Default::default(),
            shooters: BTreeMap::new(),
            next_shell: SHELL_OWNER_BASE,
            pawns: BTreeMap::new(),
            roll_seed: 0x1234_5678,
            small_gauge: None,
        }
    }

    /// The data of a kit.
    pub fn kit(&self, kit: Kit) -> &ArisenData {
        self.data.kit(kit)
    }

    /// Registers an already-loaded ordinary NPC and returns its combat owner.
    /// Host lifetime replacement/unloading must call `unbind_npc`; retirement
    /// is not a death and does not create a kill award.
    pub fn bind_npc(&mut self, npc: u32, generation: u64, body: Arc<crate::npc_body::NpcBody>,
        region: ddcombat::damage::RegionPool, motion: MotionId) -> u32 {
        let owner = self.allocate_enemy_owner();
        self.npcs.insert(owner, crate::npc_body::NpcFighter::new(npc, generation, owner, body, region, motion));
        owner
    }

    pub fn unbind_npc(&mut self, owner: u32) {
        self.npcs.remove(&owner);
        self.npc_errors.remove(&owner);
        self.positions.remove(&owner);
        for goblin in &mut self.goblins { goblin.distance_hate.retire(owner); }
    }

    /// Applies the simulation's queued landing damage directly to one living
    /// seat, bypassing combat defence. Returns the HP actually removed.
    /// Resilience's fall-damage halving is not yet modelled; callers pass the
    /// base amount derived by ddsim from the installed landing parameters.
    pub fn fall_damage(&mut self, seat: u8, amount: f32) -> f32 {
        if !amount.is_finite() || amount <= 0.0 {
            return 0.0;
        }
        let Some(arisen) = self.arisen.get_mut(&seat) else {
            return 0.0;
        };
        let Victim::Player { hp, .. } = &mut arisen.fighter.victim else {
            return 0.0;
        };
        if *hp <= 0.0 {
            return 0.0;
        }
        let removed = amount.min(*hp);
        *hp -= removed;
        removed
    }

    /// Applies an already authorized restorative to one living seat.
    /// Returns HP restored and status kinds removed; missing/dead seats
    /// return `None` so the inventory caller can avoid consuming an item.
    /// The caller applies stamina first, using curse state before cures
    /// (uPlayerBase::vf183, 0x00b77780, 2017-12-13 Steam).
    pub fn recover_item(&mut self, seat: u8, hp_amount: f32, cure_mask: u64) -> Option<(f32, Vec<u32>)> {
        let pawn = self.pawns.contains_key(&seat);
        let fighter = &mut self.arisen.get_mut(&seat)?.fighter;
        let Victim::Player { hp, hp_max, .. } = &mut fighter.victim else {
            return None;
        };
        if *hp <= 0.0 {
            return None;
        }
        // The fight does not yet retain a recoverable-wound pool. Items
        // may restore all the way to maximum HP, so this ceiling is exact
        // for item recovery; it must not be reused for healing spells.
        let mut recoverable = *hp_max;
        let restored = ddcombat::cstatus::recover_item_hp(hp, &mut recoverable, *hp_max, hp_amount);
        let removed = fighter.statuses.cure_item(cure_mask, pawn);
        Some((restored, removed))
    }

    /// Adds seat `seat`'s Arisen (a level-1 Fighter at full HP).
    pub fn add_arisen(&mut self, seat: u8) {
        self.add_arisen_as(seat, Kit::Fighter);
    }

    /// Adds seat `seat`'s Arisen with `kit`, at full HP.
    pub fn add_arisen_as(&mut self, seat: u8, kit: Kit) {
        let a = self.kit(kit);
        self.arisen.insert(
            seat,
            ArisenFighter {
                fighter: Fighter {
                    owner: u32::from(seat),
                    attacker: Attacker::Player(a.attacker),
                    victim: Victim::Player {
                        defender: a.defender,
                        hp: a.hp_max,
                        hp_max: a.hp_max,
                    },
                    attacks: vec![],
                    hurts: vec![],
                    history: HitHistory::default(),
                    statuses: Default::default(),
                    dots: Default::default(),
                    pools: Default::default(),
                },
                kit,
                level: 1,
                attacks_seen: 0,
                volleys_seen: 0,
            },
        );
    }

    /// Gives seat `seat`'s player in `sim` its kit's weapon (motions,
    /// weapon, level gap) and the sim the attacks' values.
    /// [`Fight::players_moved`] does it too when the weapon differs, a frame
    /// late.
    pub fn equip(&self, sim: &mut ddsim::Sim, seat: u8) {
        if let Some(a) = self.arisen.get(&seat) {
            self.kit(a.kit).equip(sim, seat);
            self.data.skills.install(sim);
            sim.attack_param = self.data.attack_param;
            if a.kit == Kit::Strider {
                self.data.bow.install(sim);
                if let Some(p) = sim.player_mut(seat) {
                    p.secondary_weapon = ddsim::Weapon::Bow;
                    p.secondary_weapon_level_gap =
                        i32::from(self.data.bow.weapon_level) - self.data.strider.level as i32;
                }
            }
        }
    }

    /// The actual drawn bow model pose mounted to the stationary upper
    /// layer. Full upper/lower blend and live camera provider are S27.
    pub fn bow_pose(&self, sim: &ddsim::Sim, seat: u8) -> Result<crate::body::Posed, String> {
        let p = sim.player(seat).ok_or("missing bow seat")?;
        let a = self.arisen.get(&seat).ok_or("missing bow fighter")?;
        self.data.bow.pose(&self.kit(a.kit).body, p)
    }

    /// Gives seat `seat`'s Arisen the stats of character level `level`
    /// (`cPlayerInfo` +0x2A0 max HP, +0x2B4 / +0x2B8 / +0x2BC / +0x2C0
    /// strength, defence, magick, magick defence). A rise refills HP to
    /// the new maximum, as the level-up 0x00B5BD20 does through 0x00888DA0
    /// with the max HP (vf148); a dead Arisen stays dead (0x00888DA0's
    /// vf1D4 check).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_level(
        &mut self,
        seat: u8,
        level: u32,
        hp_max: f32,
        attack: f32,
        defend: f32,
        magic_attack: f32,
        magic_defend: f32,
    ) {
        let Some(a) = self.arisen.get_mut(&seat) else { return };
        if a.level == level {
            return;
        }
        let rose = level > a.level;
        a.level = level;
        if let Attacker::Player(p) = &mut a.fighter.attacker {
            p.base_attack = attack;
            p.base_mgc_attack = magic_attack;
        }
        if let Victim::Player { defender, hp, hp_max: max } = &mut a.fighter.victim {
            defender.base_defence = defend;
            defender.base_mgc_defence = magic_defend;
            *max = hp_max;
            if rose && *hp > 0.0 {
                *hp = hp_max;
            }
            *hp = hp.min(hp_max);
        }
    }

    /// The experience of the goblins killed by this frame's hits
    /// (`uEm0100::vf131` 0x008D3430 -> 0x008573E0 -> award 0x00443470).
    /// *Assumed*: every kill awards; the unit-state gates of 0x008573E0
    /// (+0x206C bits, +0x2B8E, +0x20DC) are not translated.
    pub fn kill_awards(&self) -> Vec<u32> {
        self.events
            .iter()
            .filter(|e| e.killed)
            .filter_map(|e| self.goblins.iter().find(|g| g.fighter.owner == e.victim))
            .map(|g| g.exp)
            .collect()
    }

    /// Adds a goblin; returns its owner id.
    pub fn add_goblin(&mut self, pos: [f32; 3], yaw: f32) -> u32 {
        self.add_goblin_placed(pos, yaw, Default::default())
    }

    /// Adds a goblin with its placement's stat rates.
    pub fn add_goblin_placed(&mut self, pos: [f32; 3], yaw: f32, rates: crate::goblin::PlacementRates) -> u32 {
        self.add_goblin_equipped(pos, yaw, rates, Default::default())
    }

    /// Authored equipment plus placement rates. One resolved appearance per
    /// enemy lifetime, independent of player count or presentation frames.
    pub fn add_goblin_equipped(&mut self, pos: [f32; 3], yaw: f32,
        rates: crate::goblin::PlacementRates, placement: crate::goblin::equipment::Placement) -> u32 {
        let owner = self.allocate_enemy_owner();
        let mut goblin = Goblin::placed(&self.data.goblin, owner, pos, yaw, rates);
        goblin.appearance = Some(self.data.goblin.equipment.choose(placement, &mut self.appearance_rng));
        self.goblins.push(goblin);
        owner
    }

    /// Session ownership ids are port bookkeeping, shared by enemy kinds.
    pub fn allocate_enemy_owner(&mut self) -> u32 {
        let owner = self.next_enemy;
        self.next_enemy += 1;
        owner
    }

    /// Adds a goblin run by the FSM `path` (an `rAIFSM` of the install).
    pub fn add_goblin_with_fsm(
        &mut self,
        vfs: &ddvfs::Vfs,
        path: &str,
        pos: [f32; 3],
        yaw: f32,
    ) -> Result<u32, String> {
        let b = crate::body::load::read(vfs, path, "rAIFSM")?;
        let graph = mtfsm::Graph::parse(&b).map_err(|e| format!("{path}: {e:?}"))?;
        let t = ddai::think::Thinker::new(path, Arc::new(graph)).map_err(|e| e.to_string())?;
        let owner = self.add_goblin(pos, yaw);
        self.goblins.last_mut().expect("just added").fsm = Some(crate::goblin::Fsm(t));
        Ok(owner)
    }

    /// A new attack of `attacker` frees its swing's `mRelation` history
    /// entries on every victim (the attacker flag +0x1EC1 of 0x0076ECA0;
    /// *assumed* to be raised when the attacker starts an attack, its
    /// writer is not traced). Without it one swing's relation would block
    /// the next.
    pub fn release(&mut self, attacker: u32) {
        let victims = self
            .arisen
            .values_mut()
            .map(|a| &mut a.fighter)
            .chain(self.goblins.iter_mut().map(|g| &mut g.fighter))
            .chain(self.npcs.values_mut().map(|n| &mut n.fighter));
        for v in victims {
            v.history
                .entries
                .retain(|e| e.attacker != attacker || e.relation == 0);
        }
    }

    /// Step 33: hits from the volumes of the last frame, damage, pools, and
    /// the victims' reactions.
    pub fn hits(&mut self, dt: f32) -> &[HitEvent] {
        self.hits_observed(dt, None)
    }

    /// Optional headless diagnostics of actual NPC contact valuation.
    /// The caller owns the trace storage; normal gameplay does not allocate it.
    pub fn hits_with_npc_trace(&mut self, dt: f32, trace: &mut Vec<crate::combat::NpcHitTrace>) -> &[HitEvent] {
        self.hits_observed(dt, Some(trace))
    }

    fn hits_observed(&mut self, dt: f32, trace: Option<&mut Vec<crate::combat::NpcHitTrace>>) -> &[HitEvent] {
        let mut fighters: Vec<Fighter> = self.arisen.values().map(|a| a.fighter.clone()).collect();
        fighters.extend(self.goblins.iter().map(|g| g.fighter.clone()));
        fighters.extend(self.dragon.iter().map(|d|d.actor.fighter.clone()));
        fighters.extend(self.npcs.values().map(|n| n.fighter.clone()));
        let shell_fighters = self.shell_fighters();
        fighters.extend(shell_fighters);
        fighters.extend(self.arrow_fighters());
        fighters.extend(self.spell_fighters());
        let mut seed = self.roll_seed;
        let mut roll = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        // statuses tick, poison and burning bite, build-ups decay
        // (docs/STATUS.md; each character's update in the original)
        self.status_events = crate::combat::status_pass(&mut fighters, dt);
        // relation entries hold until their attacker starts its next
        // attack ([`Fight::release`])
        self.events = match trace {
            Some(trace) => crate::combat::hit_pass_with_npc_trace(&mut fighters, &self.data.tables, dt, &mut roll, &|_| true, trace),
            None => hit_pass(&mut fighters, &self.data.tables, dt, &mut roll, &|_| true),
        };
        self.roll_seed = seed;
        let mut it = fighters.into_iter();
        for a in self.arisen.values_mut() {
            a.fighter = it.next().expect("one fighter per Arisen");
        }
        for g in &mut self.goblins {
            g.fighter = it.next().expect("one fighter per goblin");
        }
        if let Some(dragon) = &mut self.dragon {
            dragon.actor.fighter = it.next().expect("one fighter per dragon");
        }
        for npc in self.npcs.values_mut() {
            npc.fighter = it.next().expect("one fighter per NPC");
        }
        // the small-enemy gauge (0x0067C110 / 0x0067AC40): counts down by
        // the frame step, then follows the latest damaged goblin. *Assumed*:
        // every hit event on a goblin is a damage report (the gate inside
        // uEnemy::vf69 is not translated).
        if let Some((_, left)) = &mut self.small_gauge {
            *left -= dt;
        }
        if self.small_gauge.is_some_and(|(_, left)| left <= 0.0) {
            self.small_gauge = None;
        }
        let goblin_owners: Vec<u32> = self.goblins.iter().map(|g| g.fighter.owner).collect();
        if let Some(e) = self
            .events
            .iter()
            .rev()
            .find(|e| goblin_owners.contains(&e.victim))
        {
            self.small_gauge = Some((e.victim, SMALL_GAUGE_FRAMES));
        }
        // a shell that hit is gone (*placeholder*, see `crate::shell`)
        let hit: Vec<u32> = self.events.iter().map(|e| e.attacker).collect();
        self.shells.retain(|s| !hit.contains(&s.owner));
        for arrow in &mut self.arrows {
            arrow.contact(&self.data.arrow.param, hit.contains(&arrow.owner), false);
        }
        self.arrows.retain(|a| a.alive);
        self.spells.hit(&hit, &self.data.skills, &mut self.next_shell);
        for s in &self.spells.shells {
            self.shooters.insert(s.owner, s.shooter);
            self.positions.insert(s.owner, s.pos);
        }
        for e in &self.events {
            let from = self.positions.get(&e.attacker).copied();
            if let Some(g) = self.goblins.iter_mut().find(|g| g.fighter.owner == e.victim) {
                g.react(e, from);
            }
        }
        &self.events
    }

    /// After the players moved (line 10): each Arisen's weapon is put in
    /// the sim if it is not there yet; a new attack frees its hit history;
    /// the target heading for the next attack is set (*placeholder* for the
    /// target choice `0x00B79090`: the nearest live goblin); new WandShot
    /// volleys fire their shells; the shells fly `step` frames.
    pub fn players_moved(&mut self, sim: &mut ddsim::Sim, _ground: &dyn ddsim::Ground, step: f32) {
        let seats: Vec<u8> = self.arisen.keys().copied().collect();
        for seat in seats {
            let kit = self.arisen[&seat].kit;
            let data = self.kit(kit);
            if sim.player(seat).is_some_and(|p| p.weapon != data.weapon()) {
                data.equip(sim, seat);
                self.data.skills.install(sim);
                sim.attack_param = self.data.attack_param;
            }
            let Some(p) = sim.player_mut(seat) else { continue };
            let target = nearest_goblin(&self.goblins, p.position);
            p.attack_heading = target.map(|g| {
                let q = self.goblins[g].pos;
                (q[0] - p.position[0]).atan2(q[2] - p.position[2])
            });
            let (started, volleys) = (p.attacks_started, p.volleys_fired);
            let (feet, yaw) = (p.position, p.yaw);
            let released = std::mem::take(&mut p.skills.released);
            let a = self.arisen.get_mut(&seat).expect("seat listed");
            let new_attack = started != a.attacks_seen;
            let fired = volleys.saturating_sub(a.volleys_seen);
            a.attacks_seen = started;
            a.volleys_seen = volleys;
            if new_attack {
                self.release(u32::from(seat));
            }
            for _ in 0..fired {
                let owner = self.next_shell;
                self.next_shell += 1;
                let target = target.map(|g| self.goblins[g].fighter.owner);
                self.shells.push(Shell::fire(
                    &self.data.shell,
                    owner,
                    u32::from(seat),
                    feet,
                    yaw,
                    target,
                ));
                self.shooters.insert(owner, u32::from(seat));
            }
            for id in released {
                let Some(skill) = self.data.skills.0.get(&id) else {
                    continue;
                };
                let Some(spell) = &skill.spell else { continue };
                let pose = skill.release_pose(feet, yaw);
                let aim = target.map(|g| aim_point(&self.goblins[g]));
                self.spells
                    .fire(spell, id, u32::from(seat), &pose, aim, &mut self.next_shell);
            }
        }
        self.fire_arrows(sim);
        self.arrows
            .retain_mut(|arrow| arrow.step(&self.data.arrow.param, step));
        for arrow in &self.arrows {
            self.positions.insert(arrow.owner, arrow.position);
        }
        let param = self.data.shell;
        let goblins = &self.goblins;
        self.shells.retain_mut(|s| {
            let aim = s
                .target
                .and_then(|t| goblins.iter().find(|g| g.fighter.owner == t && !g.dead()))
                .map(aim_point);
            s.fly(&param, aim, step)
        });
        self.spells.step(&self.data.skills, step);
        for s in &self.spells.shells {
            self.shooters.insert(s.owner, s.shooter);
        }
    }

    fn fire_arrows(&mut self, sim: &mut ddsim::Sim) {
        for (&seat, _) in &self.arisen {
            let Some(p) = sim.player_mut(seat) else { continue };
            for shot in std::mem::take(&mut p.bow_shots) {
                let result = (|| {
                    let context = self
                        .bow_contexts
                        .get_mut(&seat)
                        .ok_or("missing authoritative bow context")?;
                    let steady = context.augments.contains(&0x48);
                    let required = if steady { 2 } else { 4 };
                    if context.random.len() < required {
                        return Err("missing host bow RNG samples".to_string());
                    }
                    context.random.pop_front(); // B1BF90 authored Y (even width0)
                    context.random.pop_front(); // B1BF90 authored X (even width0)
                    let random = if steady {
                        [0; 2]
                    } else {
                        [
                            context.random.pop_front().unwrap(),
                            context.random.pop_front().unwrap(),
                        ]
                    };
                    let range = self.data.arrow.range.multiplier(&context.augments);
                    Arrow::from_shot(
                        &self.data.arrow.param,
                        self.next_shell,
                        u32::from(seat),
                        shot,
                        &context.pose,
                        context.target,
                        random,
                        steady,
                        range,
                    )
                })();
                match result {
                    Ok(arrow) => {
                        self.shooters.insert(arrow.owner, arrow.shooter);
                        self.arrows.push(arrow);
                        self.next_shell += 1;
                        self.bow_errors.remove(&seat);
                    }
                    Err(error) => {
                        self.bow_errors.insert(seat, error);
                        if let Some(bow) = &mut p.bow {
                            bow.unsupported = Some(ddsim::BowUnsupported::Shot);
                        }
                    }
                }
            }
        }
    }

    fn arrow_fighters(&self) -> Vec<Fighter> {
        self.arrows
            .iter()
            .filter_map(|arrow| {
                let seat = u8::try_from(arrow.shooter).ok()?;
                let context = self.bow_contexts.get(&seat)?;
                let mut shooter = self.arisen.get(&seat)?.fighter.clone();
                // 88A910 -> 88AA20: value against the shooter's SECONDARY slot,
                // supplied by the authoritative host, never its dagger slot.
                shooter.attacker = Attacker::Player(context.secondary_attacker);
                Some(arrow.fighter(&self.data.arrow, &shooter))
            })
            .collect()
    }

    fn spell_fighters(&self) -> Vec<Fighter> {
        self.spells
            .shells
            .iter()
            .filter_map(|s| {
                let skill = self.data.skills.0.get(&s.skill)?;
                let spell = skill.spell.as_ref()?;
                let shooter = self.kit(self.arisen.get(&(s.shooter as u8))?.kit);
                Some(Fighter {
                    owner: s.owner,
                    attacker: Attacker::Player(shooter.attacker),
                    victim: Victim::Player {
                        defender: shooter.defender,
                        hp: 1.0,
                        hp_max: 1.0,
                    },
                    attacks: s.volumes(spell),
                    hurts: vec![],
                    history: HitHistory::default(),
                    statuses: Default::default(),
                    dots: Default::default(),
                    pools: Default::default(),
                })
            })
            .collect()
    }

    /// The fighters standing for the shells in the hit pass: attack
    /// volumes only, valued with their shooter's attack.
    fn shell_fighters(&self) -> Vec<Fighter> {
        let mage = &self.data.mage;
        let set = &mage.body.collision[1];
        self.shells
            .iter()
            .map(|s| {
                let shooter = self
                    .arisen
                    .get(&(s.shooter as u8))
                    .map_or(mage, |a| self.kit(a.kit));
                Fighter {
                    owner: s.owner,
                    attacker: Attacker::Player(shooter.attacker),
                    victim: Victim::Player {
                        defender: shooter.defender,
                        hp: 1.0,
                        hp_max: 1.0,
                    },
                    attacks: s.volumes(set, &self.data.shell),
                    hurts: vec![],
                    history: HitHistory::default(),
                    statuses: Default::default(),
                    dots: Default::default(),
                    pools: Default::default(),
                }
            })
            .collect()
    }

    /// Line 15: the enemies think, act and move.
    pub fn enemies(&mut self, sim: &ddsim::Sim, ground: &dyn ddsim::Ground, step: f32) {
        let targets: Vec<Target> = self
            .arisen
            .iter()
            .filter_map(|(&seat, a)| {
                let p = sim.player(seat)?;
                Some(Target {
                    id: u32::from(seat),
                    pos: p.position,
                    alive: !a.fighter.victim.dead(),
                })
            })
            .collect();
        let npcs: Vec<Target> = self.npcs.iter().map(|(&owner, npc)| Target {
            id: owner, pos: npc.position, alive: !npc.fighter.victim.dead(),
        }).collect();
        let mut started = vec![];
        for g in &mut self.goblins {
            let before = g.log.len();
            g.step_with_npcs(&self.data.goblin, &targets, &npcs, ground, step);
            if g.log.len() > before && g.attacking() {
                started.push(g.fighter.owner);
            }
        }
        for a in started {
            self.release(a);
        }
    }

    /// Step 38 (`0x004797E0`, after both move lines): characters pushed
    /// out of each other (`crate::pushout`): per pair of overlapping
    /// push-out volumes the higher rank gives way by the overlap; each
    /// character keeps its deepest push of the frame (0x0076D420) and moves
    /// by it horizontally (0x0076D6B0, pass C). Pairs are taken Arisen
    /// first, then goblins in creation order.
    pub fn push_out(&mut self, sim: &mut ddsim::Sim) {
        let gdata = &self.data.goblin;
        let mut bodies: Vec<crate::pushout::PushBody> = Vec::new();
        let mut seats = Vec::new();
        for (&seat, a) in &self.arisen {
            let Some(p) = sim.player(seat) else { continue };
            let abody = self.data.skills.body(p, &self.kit(a.kit).body);
            let (motion, frame) = arisen_motion(abody, p);
            let pose = abody.pose(motion, frame, p.position, p.yaw);
            let acts = abody.activations(motion, frame);
            bodies.push(crate::pushout::PushBody {
                rank: p.push_rank(),
                kind: 2,
                volumes: abody.push_volumes(&acts, &pose),
            });
            seats.push(seat);
        }
        for g in &self.goblins {
            let pose = gdata.body.pose(g.motion, g.frame, g.pos, g.yaw);
            let acts = gdata.body.activations(g.motion, g.frame);
            bodies.push(crate::pushout::PushBody {
                rank: g.push_rank(gdata.obj_size),
                kind: 0,
                volumes: gdata.body.push_volumes(&acts, &pose),
            });
        }
        // the deepest push per character
        let mut pushes: Vec<Option<[f32; 2]>> = vec![None; bodies.len()];
        for i in 0..bodies.len() {
            for j in i + 1..bodies.len() {
                if let Some((who, d)) = crate::pushout::push(&bodies[i], &bodies[j]) {
                    let k = if who == 0 { i } else { j };
                    let deeper =
                        pushes[k].is_none_or(|q| d[0] * d[0] + d[1] * d[1] > q[0] * q[0] + q[1] * q[1]);
                    if deeper {
                        pushes[k] = Some(d);
                    }
                }
            }
        }
        for (k, d) in pushes.into_iter().enumerate() {
            let Some(d) = d else { continue };
            if k < seats.len() {
                if let Some(p) = sim.player_mut(seats[k]) {
                    p.position[0] += d[0];
                    p.position[2] += d[1];
                }
            } else {
                let g = &mut self.goblins[k - seats.len()];
                g.pos[0] += d[0];
                g.pos[2] += d[1];
            }
        }
    }

    /// Pass C: everyone's pose and the volumes for the next frame's hits.
    pub fn volumes(&mut self, sim: &ddsim::Sim) {
        for (&seat, a) in &mut self.arisen {
            let Some(p) = sim.player(seat) else { continue };
            let body = &self.data.kit(a.kit).body;
            let body = self.data.skills.body(p, body);
            let (motion, frame) = arisen_motion(body, p);
            self.positions.insert(u32::from(seat), p.position);
            let pose = body.pose(motion, frame, p.position, p.yaw);
            let acts = body.activations(motion, frame);
            (a.fighter.attacks, a.fighter.hurts) = body.volumes(&acts, &pose, u32::from(seat));
        }
        for g in &mut self.goblins {
            self.positions.insert(g.fighter.owner, g.pos);
            g.volumes(&self.data.goblin);
        }
        for (&owner, npc) in &mut self.npcs {
            self.positions.insert(owner, npc.position);
            match npc.volumes() {
                Ok(()) => { self.npc_errors.remove(&owner); }
                Err(error) => { self.npc_errors.insert(owner, error); }
            }
        }
        for s in &self.shells {
            self.positions.insert(s.owner, s.pos);
        }
        for s in &self.spells.shells {
            self.positions.insert(s.owner, s.pos);
        }
    }

    /// A hash input of the fight's state, for replays.
    pub fn digest(&self) -> String {
        let mut s = String::new();
        for (seat, a) in &self.arisen {
            s += &format!(
                "a{seat}:{:?}:{}:{};",
                a.fighter.victim.hp(),
                a.attacks_seen,
                a.volleys_seen
            );
        }
        for sh in &self.shells {
            s += &format!("s{}:{:?}:{};", sh.owner, sh.pos, sh.speed);
        }
        for arrow in &self.arrows {
            s += &format!(
                "arrow{}:{}:{:?}:{:?}:{:?}:{}:{}:{}:{}:{};",
                arrow.owner,
                arrow.shooter,
                arrow.position,
                arrow.previous_position,
                arrow.direction,
                arrow.speed,
                arrow.gravity,
                arrow.distance,
                arrow.range_multiplier,
                arrow.alive
            );
        }
        for (seat, context) in &self.bow_contexts {
            s += &format!(
                "bow-context{seat}:{:?}:{:?}:{:?}:{:?}:{:?};",
                context.pose, context.target, context.random, context.augments, context.secondary_attacker
            );
        }
        for (seat, error) in &self.bow_errors {
            s += &format!("bow-error{seat}:{error};");
        }
        for sh in &self.spells.shells {
            s += &format!(
                "spell{}:{}:{}:{:?}:{}:{};",
                sh.owner, sh.shooter, sh.skill, sh.pos, sh.age, sh.impact
            );
        }
        for g in &self.goblins {
            s += &g.digest();
        }
        for (&owner, npc) in &self.npcs {
            s += &format!("npc{owner}:{}:{}:{:?}:{}:{:?}:{}:{:?}:{}:{:?}:{:?};",
                npc.npc, npc.generation, npc.motion, npc.frame, npc.position,
                npc.yaw, npc.scale, npc.flags, npc.fighter, self.npc_errors.get(&owner));
        }
        for (seat, p) in &self.pawns {
            s += &format!("p{seat}:{:?}:{};", p.brain.goal, p.brain.history.len());
        }
        if let Some(d) = &self.dragon {
            s += &format!("dragon:{}:{:?}:{:?}:{:?}:{}:{}:{:?}:{:?};",
                d.actor.fighter.owner,d.actor.pos,d.actor.rotation,d.actor.motion(),d.actor.frame,
                d.actor.done(),d.thinker.current_states(),d.actor.fighter.victim);
        }
        s
    }
}

/// The motion an Arisen shows: its active skill or ddsim state's motion (bank 0
/// `m0000_co`, or bank 7 the weapon's `_at` list for the attacks), the
/// frame wrapped into it.
fn arisen_motion(body: &crate::body::Body, p: &ddsim::Player) -> (MotionId, f32) {
    let id = p.motion_id();
    // ddsim counts a state's frames on; the motion wraps
    let m = MotionId::new((id >> 8) as u8, id & 0xff);
    (m, body.wrap_frame(m, p.frame))
}

/// The live goblin nearest to `from` (horizontally).
fn nearest_goblin(goblins: &[Goblin], from: [f32; 3]) -> Option<usize> {
    goblins
        .iter()
        .enumerate()
        .filter(|(_, g)| !g.dead())
        .map(|(i, g)| (i, (g.pos[0] - from[0]).powi(2) + (g.pos[2] - from[2]).powi(2)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Where a shell aims on a goblin: the middle of its hurt volumes
/// (*placeholder* for the homing's target point), else 50 cm above its feet.
fn aim_point(g: &Goblin) -> [f32; 3] {
    let centers: Vec<[f32; 3]> = g
        .fighter
        .hurts
        .iter()
        .map(|h| match h.volume {
            ddcombat::volumes::Volume::Sphere { c, .. } => c,
            ddcombat::volumes::Volume::Capsule { p0, p1, .. } => [
                (p0[0] + p1[0]) * 0.5,
                (p0[1] + p1[1]) * 0.5,
                (p0[2] + p1[2]) * 0.5,
            ],
        })
        .collect();
    if centers.is_empty() {
        return [g.pos[0], g.pos[1] + 50.0, g.pos[2]];
    }
    let n = centers.len() as f32;
    std::array::from_fn(|k| centers.iter().map(|c| c[k]).sum::<f32>() / n)
}
