//! The hit pass of one frame (docs/FRAME.md 5.1, docs/COMBAT.md 2-6): the
//! volumes every fighter switched on last frame are tested, each victim's
//! hits are valued, merged and applied to its pools.
//!
//! The pass is generic over fighters: an attacker is valued as an enemy
//! (`enemy_attack`, 0x0088B530) or a player (`player_attack`, 0x0088AA20),
//! a victim is mitigated as an enemy (`resolve_enemy_hit`, 0x00AAA490 and
//! 0x0076DB10) or a player (`player_mitigate`, 0x00B7AD90). Any number of
//! Arisen and enemies take part.

use ddcombat::damage::{
    enemy_attack, merge_frame, player_attack, player_mitigate, resolve_enemy_hit, Adjust, CollisionRates,
    EnemyAttacker, EnemyDefender, HitValue, PlayerAttacker, PlayerDefender, Reaction, RegionPool,
};
use ddcombat::volumes::{contacts, AttackVolume, HitHistory, HurtVolume};

/// How a fighter's attacks are valued.
#[derive(Debug, Clone)]
pub enum Attacker {
    Enemy(EnemyAttacker),
    Player(PlayerAttacker),
}

/// How a fighter takes damage.
#[derive(Debug, Clone)]
pub enum Victim {
    /// Region pools (index 0 the main pool) and defence.
    Enemy {
        defender: EnemyDefender,
        regions: Vec<RegionPool>,
        rates: CollisionRates,
    },
    /// The Arisen's defence and HP. The player's own region scaling
    /// (0x0076DB10 on the player's collision) is *not modelled*: the
    /// mitigated damage is taken as is.
    Player {
        defender: PlayerDefender,
        hp: f32,
        hp_max: f32,
    },
}

impl Victim {
    pub fn dead(&self) -> bool {
        match self {
            Victim::Enemy { regions, .. } => regions.first().is_some_and(|r| r.hp <= 0.0),
            Victim::Player { hp, .. } => *hp <= 0.0,
        }
    }

    pub fn hp(&self) -> f32 {
        match self {
            Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp),
            Victim::Player { hp, .. } => *hp,
        }
    }
}

/// One fighter's part in the pass.
#[derive(Debug, Clone)]
pub struct Fighter {
    /// The owner id the volumes carry.
    pub owner: u32,
    pub attacker: Attacker,
    pub victim: Victim,
    /// Volumes switched on in the fighter's last frame (pass C).
    pub attacks: Vec<AttackVolume>,
    pub hurts: Vec<HurtVolume>,
    /// The victim side's hit history (`cObjCollision`+0xC04).
    pub history: HitHistory,
}

/// One victim's damage this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HitEvent {
    pub attacker: u32,
    pub victim: u32,
    /// The hurt node's region of the first hit.
    pub region: u32,
    /// Hits merged into this one (`HitInfo`+0xD0).
    pub hits: u32,
    /// HP taken.
    pub total: f32,
    /// Stagger / knockdown of the hit region.
    pub reaction: Reaction,
    /// The main pool emptied on this hit (death, vf72).
    pub killed: bool,
}

/// The tuning tables the pass reads.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    pub game: Adjust,
    pub dlc: Adjust,
}

/// Runs the hit pass over `fighters`: each victim's hits from everyone's
/// attack volumes, in fighter order (the original runs one job per
/// collision; the order only matters for simultaneous deaths). `dt` is the
/// frame step in 30 fps frames. `roll` gives the player critical roll
/// (only read for critical-capable weapons).
/// `holds(attacker)` tells whether an attacker's swing still holds its
/// `mRelation` history entries (the attacker flag +0x1EC1 clear,
/// 0x0076ECA0).
pub fn hit_pass(
    fighters: &mut [Fighter],
    tables: &Tables,
    dt: f32,
    roll: &mut dyn FnMut() -> f32,
    holds: &dyn Fn(u32) -> bool,
) -> Vec<HitEvent> {
    let attacks: Vec<AttackVolume> = fighters.iter().flat_map(|f| f.attacks.iter().cloned()).collect();
    let attackers: Vec<(u32, Attacker)> = fighters.iter().map(|f| (f.owner, f.attacker.clone())).collect();
    let mut events = Vec::new();
    for v in fighters.iter_mut() {
        if v.victim.dead() {
            continue;
        }
        let hits = resolve(&attacks, &v.hurts, &mut v.history, dt, holds);
        let Some(&(first, region)) = hits.first() else {
            continue;
        };
        let mut values: Vec<HitValue> = Vec::new();
        for &(ai, _) in &hits {
            let a = &attacks[ai];
            let Some((_, att)) = attackers.iter().find(|(o, _)| *o == a.side.owner) else {
                continue;
            };
            values.push(match att {
                Attacker::Enemy(e) => enemy_attack(e, &a.row),
                Attacker::Player(p) => player_attack(p, &a.row, roll(), &tables.dlc),
            });
        }
        let Some(merged) = merge_frame(&values) else {
            continue;
        };
        let hits_n = merged.hits;
        let attacker = attacks[first].side.owner;
        let event = match &mut v.victim {
            Victim::Enemy {
                defender,
                regions,
                rates,
            } => {
                let at = regions.iter().position(|r| r.id == region).unwrap_or(0);
                let was_alive = regions[0].hp > 0.0;
                // min damage: the attacker object's +0x14 (*not confirmed*
                // to be the unit step), 1.0 here
                let out = resolve_enemy_hit(merged, defender, regions, at, rates, &tables.game, 1.0);
                HitEvent {
                    attacker,
                    victim: v.owner,
                    region,
                    hits: hits_n,
                    total: out.total,
                    reaction: out.reaction,
                    killed: was_alive && regions[0].hp <= 0.0,
                }
            }
            Victim::Player { defender, hp, .. } => {
                let mut h = merged;
                player_mitigate(&mut h, defender, &tables.game);
                let total = h.phys.max(0.0) + h.mag.max(0.0);
                let was_alive = *hp > 0.0;
                *hp = (*hp - total).max(0.0);
                HitEvent {
                    attacker,
                    victim: v.owner,
                    region,
                    hits: hits_n,
                    total,
                    reaction: Reaction::default(),
                    killed: was_alive && *hp <= 0.0,
                }
            }
        };
        events.push(event);
    }
    events
}

/// `ddcombat::volumes::resolve_frame` with the relation hold of the
/// attackers: contacts (0x00779860), the history check (0x0076EAF0), the
/// contacts recorded (0x0076EB60) and the end of the victim's frame
/// (0x0076ECA0). Which attacks set +0x1EC1 is *not traced*: the fight
/// takes an entry as held while its attacker's swing goes on.
fn resolve(
    attacks: &[AttackVolume],
    hurts: &[HurtVolume],
    history: &mut HitHistory,
    dt: f32,
    holds: &dyn Fn(u32) -> bool,
) -> Vec<(usize, u32)> {
    history.begin_frame(dt);
    let mut hits = Vec::new();
    for c in contacts(attacks, hurts) {
        let a = &attacks[c.attack];
        if !history.already_hit(a.side.owner, a.key, a.relation) {
            hits.push((c.attack, hurts[c.hurt].region_id));
        }
        history.record(a.side.owner, a.key, a.relation, a.flags.multi_hit, a.reset_time);
    }
    history.end_frame(holds);
    hits
}
