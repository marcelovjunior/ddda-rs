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
use crate::body::MotionId;
use crate::beast::{Beast, BeastData};
use ddai::thinktbl::families::Family;
use crate::combat::{hit_pass, Attacker, Fighter, HitEvent, Tables, Victim};
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
    /// Per-skill bank-seven bodies; selected independently for each seat.
    pub skills: crate::skills::SkillSet,
    /// The staff's shell.
    pub shell: ShellParam,
    /// The attacks' values from `param\pl\other\m_pl000_00`.
    pub attack_param: ddsim::AttackParam,
    pub goblin: GoblinData,
    pub wolf: BeastData,
    pub harpy: BeastData,
    pub tables: Tables,
    /// The pawns' AI data (`crate::pawn`).
    pub pawn: ddpawnai::pawn::PawnData,
}

impl FightData {
    /// Everything from the install and its exe.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let adjust = |p: &str| -> Result<ddcombat::damage::Adjust, String> {
            ddcombat::damage::Adjust::parse(&crate::body::load::read(vfs, p, "rAdjustParam")?)
                .ok_or_else(|| format!("{p}: bad table"))
        };
        let arisen = ArisenData::fighter(vfs)?;
        let mage = ArisenData::mage(vfs)?;
        let skills = crate::skills::SkillSet::load(vfs, exe, &arisen, &mage)?;
        Ok(FightData {
            arisen,
            mage,
            skills,
            shell: ShellParam::load(vfs)?,
            attack_param: ddsim::AttackParam::from_adjust(&adjust(ADJUST_PARAMS)?.0)
                .ok_or(format!("{ADJUST_PARAMS}: too short"))?,
            goblin: GoblinData::load(vfs, exe)?,
            wolf: BeastData::load(vfs, exe, Family::Wolf)?,
            harpy: BeastData::load(vfs, exe, Family::Harpy)?,
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
}

/// The player's tuning table.
pub const ADJUST_PARAMS: &str = r"param\pl\other\m_pl000_00";

/// An Arisen in the fight (the body moves and attacks in ddsim).
#[derive(Debug, Clone)]
pub struct ArisenFighter {
    pub fighter: Fighter,
    pub kit: Kit,
    /// ddsim's attack and volley counts last seen.
    pub attacks_seen: u32,
    pub volleys_seen: u32,
}

/// The fight's state.
#[derive(Debug)]
pub struct Fight {
    pub data: Arc<FightData>,
    /// By seat.
    pub arisen: BTreeMap<u8, ArisenFighter>,
    pub goblins: Vec<Goblin>,
    pub beasts: Vec<Beast>,
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
    /// Equipped spell projectiles and their impact children.
    pub spells: crate::skills::spells::SpellWorld,
    /// The Arisen each shell owner id was fired by (kept after the shell).
    pub shooters: BTreeMap<u32, u32>,
    next_shell: u32,
    /// Pawns by seat (each also an [`ArisenFighter`] in `arisen`).
    pub pawns: BTreeMap<u8, crate::pawn::PawnSeat>,
    /// *Placeholder* critical roll (only critical-capable weapons read it).
    roll_seed: u32,
}

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
            beasts: Vec::new(),
            next_enemy: ENEMY_OWNER_BASE,
            events: Vec::new(),
            status_events: Vec::new(),
            positions: BTreeMap::new(),
            shells: Vec::new(),
            spells: Default::default(),
            shooters: BTreeMap::new(),
            next_shell: SHELL_OWNER_BASE,
            pawns: BTreeMap::new(),
            roll_seed: 0x1234_5678,
        }
    }

    /// The data of a kit.
    pub fn kit(&self, kit: Kit) -> &ArisenData {
        match kit {
            Kit::Fighter => &self.data.arisen,
            Kit::Mage => &self.data.mage,
        }
    }

    /// Applies the simulation's queued landing damage directly to one living
    /// seat, bypassing combat defence. Returns the HP actually removed.
    /// Resilience's fall-damage halving is not yet modelled; callers pass the
    /// base amount derived by ddsim from the installed landing parameters.
    pub fn fall_damage(&mut self, seat: u8, amount: f32) -> f32 {
        if !amount.is_finite() || amount <= 0.0 { return 0.0; }
        let Some(arisen) = self.arisen.get_mut(&seat) else { return 0.0; };
        let Victim::Player { hp, .. } = &mut arisen.fighter.victim else { return 0.0; };
        if *hp <= 0.0 { return 0.0; }
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
        }
    }

    /// Adds a goblin; returns its owner id.
    pub fn add_goblin(&mut self, pos: [f32; 3], yaw: f32) -> u32 {
        let owner = self.next_enemy;
        self.next_enemy += 1;
        self.goblins.push(Goblin::new(&self.data.goblin, owner, pos, yaw));
        owner
    }

    /// Live/dead enemy bodies across supported families, for targeting adapters.
    pub fn enemy_fighters(&self) -> impl Iterator<Item = (&Fighter, [f32; 3])> {
        self.goblins.iter().map(|g| (&g.fighter, g.pos))
            .chain(self.beasts.iter().map(|b| (&b.fighter, b.pos)))
    }

    /// Adds a family actor. Its explicit action/target are supplied by the
    /// caller until the family think host is connected; no idle AI is guessed.
    pub fn add_beast(&mut self, family: Family, pos: [f32; 3], yaw: f32) -> u32 {
        let owner = self.next_enemy;
        self.next_enemy += 1;
        let data = match family { Family::Wolf => &self.data.wolf, Family::Harpy => &self.data.harpy };
        self.beasts.push(Beast::new(data, owner, pos, yaw));
        owner
    }

    pub fn start_beast_action(&mut self, owner: u32, action: usize, target: Option<u32>) -> Result<(), String> {
        let beast = self.beasts.iter_mut().find(|b| b.fighter.owner == owner).ok_or("unknown beast owner")?;
        let data = match beast.family { Family::Wolf => &self.data.wolf, Family::Harpy => &self.data.harpy };
        beast.start_action(data, action)?;
        beast.target = target;
        self.release(owner);
        Ok(())
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
            .chain(self.beasts.iter_mut().map(|b| &mut b.fighter));
        for v in victims {
            v.history
                .entries
                .retain(|e| e.attacker != attacker || e.relation == 0);
        }
    }

    /// Step 33: hits from the volumes of the last frame, damage, pools, and
    /// the victims' reactions.
    pub fn hits(&mut self, dt: f32) -> &[HitEvent] {
        let mut fighters: Vec<Fighter> = self.arisen.values().map(|a| a.fighter.clone()).collect();
        fighters.extend(self.goblins.iter().map(|g| g.fighter.clone()));
        fighters.extend(self.beasts.iter().map(|b| b.fighter.clone()));
        let shell_fighters = self.shell_fighters();
        fighters.extend(shell_fighters);
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
        self.events = hit_pass(&mut fighters, &self.data.tables, dt, &mut roll, &|_| true);
        self.roll_seed = seed;
        let mut it = fighters.into_iter();
        for a in self.arisen.values_mut() {
            a.fighter = it.next().expect("one fighter per Arisen");
        }
        for g in &mut self.goblins {
            g.fighter = it.next().expect("one fighter per goblin");
        }
        for b in &mut self.beasts {
            b.fighter = it.next().expect("one fighter per beast");
            if b.dead() { b.fighter.attacks.clear(); }
        }
        // a shell that hit is gone (*placeholder*, see `crate::shell`)
        let hit: Vec<u32> = self.events.iter().map(|e| e.attacker).collect();
        self.shells.retain(|s| !hit.contains(&s.owner));
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
    /// target choice `0x00B79090`: the nearest live enemy); new WandShot
    /// volleys fire their shells; the shells fly `step` frames.
    pub fn players_moved(&mut self, sim: &mut ddsim::Sim, _ground: &dyn ddsim::Ground, step: f32) {
        let enemies: Vec<_> = self.enemy_fighters().filter(|(f, _)| !f.victim.dead())
            .map(|(f, pos)| (f.owner, pos, fighter_aim(f, pos))).collect();
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
            let p_position = p.position;
            let target = enemies.iter().min_by(|a, b| {
                let d = |p: [f32; 3]| (p[0] - p_position[0]).powi(2) + (p[2] - p_position[2]).powi(2);
                d(a.1).total_cmp(&d(b.1))
            }).copied();
            p.attack_heading = target.map(|(_, q, _)| {
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
                let target = target.map(|t| t.0);
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
                let Some(skill) = self.data.skills.0.get(&id) else { continue };
                let Some(spell) = &skill.spell else { continue };
                let pose = skill.release_pose(feet, yaw);
                let aim = target.map(|t| t.2);
                self.spells.fire(spell, id, u32::from(seat), &pose, aim, &mut self.next_shell);
            }
        }
        let param = self.data.shell;
        self.shells.retain_mut(|s| {
            let aim = s
                .target
                .and_then(|t| enemies.iter().find(|e| e.0 == t))
                .map(|e| e.2);
            s.fly(&param, aim, step)
        });
        self.spells.step(&self.data.skills, step);
        for s in &self.spells.shells {
            self.shooters.insert(s.owner, s.shooter);
        }
    }

    fn spell_fighters(&self) -> Vec<Fighter> {
        self.spells.shells.iter().filter_map(|s| {
            let skill = self.data.skills.0.get(&s.skill)?;
            let spell = skill.spell.as_ref()?;
            let shooter = self.kit(self.arisen.get(&(s.shooter as u8))?.kit);
            Some(Fighter {
                owner: s.owner,
                attacker: Attacker::Player(shooter.attacker),
                victim: Victim::Player { defender: shooter.defender, hp: 1.0, hp_max: 1.0 },
                attacks: s.volumes(spell), hurts: vec![],
                history: HitHistory::default(), statuses: Default::default(),
                dots: Default::default(), pools: Default::default(),
            })
        }).collect()
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
        let mut started = vec![];
        for g in &mut self.goblins {
            let before = g.log.len();
            g.step(&self.data.goblin, &targets, ground, step);
            if g.log.len() > before && g.attacking() {
                started.push(g.fighter.owner);
            }
        }
        for beast in &mut self.beasts {
            let data = match beast.family { Family::Wolf => &self.data.wolf, Family::Harpy => &self.data.harpy };
            if beast.dead() {
                // Ordinary em0200 pending death precedes the think graph.
                // Other family/conditional death selection remains unported.
                if beast.begin_standard_wolf_death(data).is_err() { continue; }
            }
            let target = beast.target.and_then(|id| targets.iter().find(|t| t.id == id && t.alive)).map(|t| t.pos);
            beast.step_action(data, target, ground, step);
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
        for beast in &self.beasts {
            let data = match beast.family { Family::Wolf => &self.data.wolf, Family::Harpy => &self.data.harpy };
            let pose = data.body.pose(beast.motion, beast.frame, beast.pos, beast.yaw);
            let active = data.body.activations(beast.motion, beast.frame);
            bodies.push(crate::pushout::PushBody {
                rank: beast.push_rank(data.obj_size), kind: 0,
                volumes: data.body.push_volumes(&active, &pose),
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
            } else if k < seats.len() + self.goblins.len() {
                let g = &mut self.goblins[k - seats.len()];
                g.pos[0] += d[0];
                g.pos[2] += d[1];
            } else {
                let b = &mut self.beasts[k - seats.len() - self.goblins.len()];
                b.pos[0] += d[0];
                b.pos[2] += d[1];
            }
        }
    }

    /// Pass C: everyone's pose and the volumes for the next frame's hits.
    pub fn volumes(&mut self, sim: &ddsim::Sim) {
        for (&seat, a) in &mut self.arisen {
            let Some(p) = sim.player(seat) else { continue };
            let body = match a.kit {
                Kit::Fighter => &self.data.arisen.body,
                Kit::Mage => &self.data.mage.body,
            };
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
        for beast in &mut self.beasts {
            self.positions.insert(beast.fighter.owner, beast.pos);
            let data = match beast.family { Family::Wolf => &self.data.wolf, Family::Harpy => &self.data.harpy };
            beast.volumes(data);
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
        for sh in &self.spells.shells {
            s += &format!("spell{}:{}:{}:{:?}:{}:{};", sh.owner, sh.shooter,
                sh.skill, sh.pos, sh.age, sh.impact);
        }
        for g in &self.goblins {
            s += &g.digest();
        }
        for b in &self.beasts {
            s += &format!("beast{}:{:?}:{:?}:{:?}:{}:{:?}:{}:{};", b.fighter.owner, b.family, b.action, b.motion, b.frame, b.pos, b.yaw, b.fighter.victim.hp());
        }
        for (seat, p) in &self.pawns {
            s += &format!("p{seat}:{:?}:{};", p.brain.goal, p.brain.history.len());
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

/// Average hurt-volume center. The existing goblin fallback of50cm remains
/// a labelled homing approximation for bodies with no active hurt volumes.
fn fighter_aim(fighter: &Fighter, pos: [f32; 3]) -> [f32; 3] {
    let centers: Vec<[f32; 3]> = fighter
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
        return [pos[0], pos[1] + 50.0, pos[2]];
    }
    let n = centers.len() as f32;
    std::array::from_fn(|k| centers.iter().map(|c| c[k]).sum::<f32>() / n)
}
