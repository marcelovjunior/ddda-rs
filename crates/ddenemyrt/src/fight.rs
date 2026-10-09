//! A fight: the Arisen and the enemies, stepped in the original's frame
//! order (docs/FRAME.md 5.7). The host calls, each frame:
//! 1. [`Fight::hits`] before anyone moves (step 33: last frame's volumes);
//! 2. its player movement (`ddsim::Sim::step`), then [`Fight::players_moved`]
//!    (the Arisen's scripted attacks advance, phase 91-92 of line 10);
//! 3. [`Fight::enemies`] (line 15: think, action, motion, ground);
//! 4. [`Fight::volumes`] (pass C: poses and the volumes for the next
//!    frame's hits).

use crate::arisen::{self, ArisenData, Swing};
use crate::body::MotionId;
use crate::combat::{hit_pass, Attacker, Fighter, HitEvent, Tables, Victim};
use crate::goblin::{Goblin, GoblinData, Target};
use ddcombat::volumes::HitHistory;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The data a fight reads.
#[derive(Debug, Clone)]
pub struct FightData {
    pub arisen: ArisenData,
    pub goblin: GoblinData,
    pub tables: Tables,
}

impl FightData {
    /// Everything from the install and its exe.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let adjust = |p: &str| -> Result<ddcombat::damage::Adjust, String> {
            ddcombat::damage::Adjust::parse(&crate::body::load::read(vfs, p, "rAdjustParam")?)
                .ok_or_else(|| format!("{p}: bad table"))
        };
        Ok(FightData {
            arisen: ArisenData::fighter(vfs)?,
            goblin: GoblinData::load(vfs, exe)?,
            tables: Tables {
                game: adjust(r"param\game")?,
                dlc: adjust(r"param\dlc")?,
            },
        })
    }
}

/// An Arisen in the fight (the body moves in ddsim).
#[derive(Debug, Clone)]
pub struct ArisenFighter {
    pub fighter: Fighter,
    /// The scripted attack playing, if any.
    pub swing: Option<Swing>,
}

/// The fight's state.
#[derive(Debug)]
pub struct Fight {
    pub data: Arc<FightData>,
    /// By seat.
    pub arisen: BTreeMap<u8, ArisenFighter>,
    pub goblins: Vec<Goblin>,
    /// The hits of the last [`Fight::hits`].
    pub events: Vec<HitEvent>,
    /// Where each fighter stood when its volumes were placed (for the
    /// victims' front / back reactions).
    pub positions: BTreeMap<u32, [f32; 3]>,
    /// *Placeholder* critical roll (only critical-capable weapons read it).
    roll_seed: u32,
}

/// Owner ids: seats keep their number, enemies count from here.
pub const ENEMY_OWNER_BASE: u32 = 100;

impl Fight {
    pub fn new(data: Arc<FightData>) -> Self {
        Fight {
            data,
            arisen: BTreeMap::new(),
            goblins: Vec::new(),
            events: Vec::new(),
            positions: BTreeMap::new(),
            roll_seed: 0x1234_5678,
        }
    }

    /// Adds seat `seat`'s Arisen (a level-1 Fighter at full HP).
    pub fn add_arisen(&mut self, seat: u8) {
        let a = &self.data.arisen;
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
                },
                swing: None,
            },
        );
    }

    /// Adds a goblin; returns its owner id.
    pub fn add_goblin(&mut self, pos: [f32; 3], yaw: f32) -> u32 {
        let owner = ENEMY_OWNER_BASE + self.goblins.len() as u32;
        self.goblins.push(Goblin::new(&self.data.goblin, owner, pos, yaw));
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

    /// Starts the scripted sword attack 1 for seat `seat` (ignored while one
    /// plays or the Arisen is down).
    pub fn attack(&mut self, seat: u8) {
        if let Some(a) = self.arisen.get_mut(&seat) {
            if a.swing.is_none() && !a.fighter.victim.dead() {
                a.swing = Some(Swing {
                    motion: arisen::SWORD_ATTACK_1,
                    frame: 0.0,
                });
                self.release(u32::from(seat));
            }
        }
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
            .chain(self.goblins.iter_mut().map(|g| &mut g.fighter));
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
        let mut seed = self.roll_seed;
        let mut roll = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
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
        for e in &self.events {
            let from = self.positions.get(&e.attacker).copied();
            if let Some(g) = self.goblins.iter_mut().find(|g| g.fighter.owner == e.victim) {
                g.react(e, from);
            }
        }
        &self.events
    }

    /// After the players moved: the scripted attacks advance by `step`
    /// motion frames and move their Arisen by the root travel.
    pub fn players_moved(&mut self, sim: &mut ddsim::Sim, ground: &dyn ddsim::Ground, step: f32) {
        let body = &self.data.arisen.body;
        for (&seat, a) in &mut self.arisen {
            let Some(swing) = a.swing.as_mut() else { continue };
            let Some(p) = sim.player_mut(seat) else { continue };
            match arisen::advance(body, swing, step) {
                Some(d) => {
                    let (s, c) = p.yaw.sin_cos();
                    p.position[0] += d[0] * c + d[2] * s;
                    p.position[2] += -d[0] * s + d[2] * c;
                    if let Some(h) =
                        ground.height(p.position[0], p.position[2], p.position[1] + ddsim::STEP_UP)
                    {
                        p.position[1] = h;
                    }
                }
                None => a.swing = None,
            }
        }
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
        let abody = &self.data.arisen.body;
        let gdata = &self.data.goblin;
        let mut bodies: Vec<crate::pushout::PushBody> = Vec::new();
        let mut seats = Vec::new();
        for (&seat, a) in &self.arisen {
            let Some(p) = sim.player(seat) else { continue };
            let (motion, frame) = arisen_motion(abody, a, p);
            let pose = abody.pose(motion, frame, p.position, p.yaw);
            let acts = abody.activations(motion, frame);
            let rank = if a.swing.is_some() {
                arisen::SWORD_PUSH_RANK
            } else {
                p.state.push_rank()
            };
            bodies.push(crate::pushout::PushBody {
                rank,
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
        let body = &self.data.arisen.body;
        for (&seat, a) in &mut self.arisen {
            let Some(p) = sim.player(seat) else { continue };
            let (motion, frame) = arisen_motion(body, a, p);
            self.positions.insert(u32::from(seat), p.position);
            let pose = body.pose(motion, frame, p.position, p.yaw);
            let acts = body.activations(motion, frame);
            (a.fighter.attacks, a.fighter.hurts) = body.volumes(&acts, &pose, u32::from(seat));
        }
        for g in &mut self.goblins {
            self.positions.insert(g.fighter.owner, g.pos);
            g.volumes(&self.data.goblin);
        }
    }

    /// A hash input of the fight's state, for replays.
    pub fn digest(&self) -> String {
        let mut s = String::new();
        for (seat, a) in &self.arisen {
            s += &format!("a{seat}:{:?}:{:?};", a.fighter.victim.hp(), a.swing);
        }
        for g in &self.goblins {
            s += &g.digest();
        }
        s
    }
}

/// The motion an Arisen shows: its scripted swing, else its ddsim state's
/// motion (bank 0 `m0000_co`), the frame wrapped into it.
fn arisen_motion(body: &crate::body::Body, a: &ArisenFighter, p: &ddsim::Player) -> (MotionId, f32) {
    match a.swing {
        Some(s) => (s.motion, s.frame),
        None => {
            let id = p.state.motion_id();
            // ddsim counts a state's frames on; the motion wraps
            let m = MotionId::new((id >> 8) as u8, id & 0xff);
            (m, body.wrap_frame(m, p.frame))
        }
    }
}
