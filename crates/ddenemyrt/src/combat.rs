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
use ddcombat::cstatus::{burning, poison_enemy, poison_player, DotState, StatusTable, Statuses};
use ddcombat::status::{build_up, decay, fire, StatusHit, StatusPools, SLOT_KIND};
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
    /// Active statuses (`cStatus`, actor+0x2698; docs/STATUS.md).
    pub statuses: Statuses,
    /// Poison interval state (actor+0x27B4 / +0x2854).
    pub dots: DotState,
    /// The build-up of a player-side victim (the Arisen's own region; its
    /// thresholds are the constructor defaults, *inferred*: no threshold
    /// writer for players was found). Enemies build up per region.
    pub pools: StatusPools,
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

/// Apply a hit already valued on another enemy collision to this linked
/// collision. The host resolves the destination's optional region remap
/// (+0xd34/+0xd38) and region-ID map (+0xcf4) before passing `mapped_pool`.
/// The main pool remains index zero, as in the ordinary enemy hit pass.
/// Missing mapped pools suppress HP only; main break/status still run.
///
/// `source` retains attacker, original hurt region and merged hit count.
/// The result names the linked victim and its main-pool reaction/death.
/// `status_rates` are this collision's +0xd28/+0xd2c, not damage multipliers.
/// This does not collect contacts, alter history, or walk further links.
// Translated from 0x0076b7e0 and the mirrored branch of 0x0076db10
// (DDDA.exe 2017-12-13 Steam), using the shared HP/break/status helpers.
pub fn apply_linked_enemy_hit(
    victim: &mut Fighter,
    source: &HitEvent,
    valued: &ddcombat::damage::ValuedEnemyHit,
    mapped_pool: Option<usize>,
    status_rates: [f32; 2],
    status_table: &StatusTable,
    roll: &mut dyn FnMut() -> f32,
) -> Option<HitEvent> {
    use ddcombat::damage::{apply_break, apply_hp, attr, resolve_reaction};
    let Victim::Enemy { regions, rates, .. } = &mut victim.victim else {
        return None;
    };
    let links_hp = mapped_pool.and_then(|i| regions.get(i))
        .is_some_and(|r| r.attr & attr::LINK_HP != 0);
    let main = regions.first_mut()?;
    let was_alive = main.hp > 0.0;
    let h = &valued.hit;
    apply_break(main, h, rates);
    let status_hit = StatusHit {
        element: h.element,
        element_buildup: h.element_buildup,
        status: h.status,
        mag: h.mag,
    };
    build_up(&mut main.status, &status_hit, &main.resist.element,
        status_rates[0], status_rates[1], &mut *roll);
    let (slots, frozen) = fire(&mut main.status);
    add_fired(&mut victim.statuses, &slots, status_table);
    let reaction = resolve_reaction(main, frozen);
    if links_hp {
        apply_hp(main, valued.total, rates, h.row.is_no_death);
    }
    Some(HitEvent {
        attacker: source.attacker,
        victim: victim.owner,
        region: source.region,
        hits: source.hits,
        total: if links_hp { valued.total } else { 0.0 },
        reaction,
        killed: was_alive && main.hp <= 0.0,
    })
}

/// The tuning tables the pass reads.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    pub game: Adjust,
    pub dlc: Adjust,
    /// `param\status\enemy` / `param\status\player` (docs/STATUS.md).
    pub status_enemy: StatusTable,
    pub status_player: StatusTable,
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
    // the attacker's status multipliers (0x0088B530 / 0x0088AA20)
    let attackers: Vec<(u32, Attacker)> = fighters
        .iter()
        .map(|f| {
            let (p, m) = f.statuses.attack_muls();
            let a = match f.attacker.clone() {
                Attacker::Enemy(mut e) => {
                    e.phys_status_mul *= p;
                    e.mag_status_mul *= m;
                    Attacker::Enemy(e)
                }
                Attacker::Player(mut pl) => {
                    pl.phys_status_mul *= p;
                    pl.mag_status_mul *= m;
                    Attacker::Player(pl)
                }
            };
            (f.owner, a)
        })
        .collect();
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
        let Some(mut merged) = merge_frame(&values) else {
            continue;
        };
        let status_hit = StatusHit {
            element: merged.element,
            element_buildup: merged.element_buildup,
            status: merged.status,
            mag: merged.mag,
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
                // the victim's statuses: defence downs / ups and curse
                // (0x00AAA490), sleep / frozen / oil / drenched (0x0084D8B0)
                let (dp, dm) = v.statuses.defence_muls();
                let sleep = Some((tables.game.get(0x12), tables.game.get(0x13)));
                let (sp, sm, sb) = v.statuses.victim_scale(merged.element, sleep);
                let mut def = *defender;
                def.phys_status_mul *= dp * sp;
                def.mag_status_mul *= dm * sm;
                merged.shrink *= sb;
                merged.blow *= sb;
                // min damage: the attacker object's +0x14 (*not confirmed*
                // to be the unit step), 1.0 here
                let out = resolve_enemy_hit(merged, &def, regions, at, rates, &tables.game, 1.0);
                // build-up on the hit region (0x007772B0; a main region
                // exists, so it runs whatever the region's mAttr says)
                let r = &mut regions[at];
                let resist = r.resist.element;
                build_up(&mut r.status, &status_hit, &resist, 1.0, 1.0, &mut *roll);
                let (slots, _frozen) = fire(&mut r.status);
                add_fired(&mut v.statuses, &slots, &tables.status_enemy);
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
                // status multipliers after the curve (0x00B7AD90)
                let (dp, dm) = v.statuses.defence_muls();
                let total = (h.phys * dp).max(0.0) + (h.mag * dm).max(0.0);
                // build-up on the Arisen (equipment status resists not
                // modelled: PlayerDefender carries no s[10..])
                build_up(&mut v.pools, &status_hit, &[1.0; 5], 1.0, 1.0, &mut *roll);
                let (slots, _frozen) = fire(&mut v.pools);
                add_fired(&mut v.statuses, &slots, &tables.status_player);
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

/// Fired build-up slots become statuses (the owner's vf74 with the slot,
/// `0x015098C0` slot -> kind, then 0x00837090). Slots without a kind
/// (lightning, holy, dark: chains and effects of `uCharacterBase::vf74`)
/// are skipped.
fn add_fired(statuses: &mut Statuses, slots: &[usize], table: &StatusTable) {
    for &s in slots {
        let k = SLOT_KIND[s];
        if k >= 0 {
            statuses.add(k as u32, table);
        }
    }
}

/// What the status pass did to one fighter.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusEvent {
    pub owner: u32,
    /// HP change this frame (poison, burning; negative).
    pub hp: f32,
    /// Kinds that wore off.
    pub ended: Vec<u32>,
    /// The fighter's step scale (slow: `mParam0`).
    pub step_scale: f32,
}

/// The per-frame status update of every fighter: countdown (0x00836C70)
/// at the frame step `dt`, the damage-over-time handlers (enemy poison
/// 0x00AAFE10, player poison 0x00B75070, burning 0x00852F40) at the
/// fighter's own step (slow scales it, 0x00852670), HP taken from the main
/// pool (0x00888DA0 works on region 0) and the build-up decay
/// (0x007781A0). The original runs this in each character's update; the
/// fight runs it at the start of [`crate::fight::Fight::hits`].
pub fn status_pass(fighters: &mut [Fighter], dt: f32) -> Vec<StatusEvent> {
    let mut out = Vec::new();
    for f in fighters.iter_mut() {
        if f.victim.dead() {
            continue;
        }
        let scale = f.statuses.step_scale();
        let step = dt * scale;
        let ended = f.statuses.tick(dt);
        let hp = match &mut f.victim {
            Victim::Enemy { regions, .. } => {
                let d = poison_enemy(&mut f.dots, &f.statuses, step) + burning(&f.statuses, step);
                if let Some(main) = regions.first_mut() {
                    main.hp = (main.hp + d).max(0.0);
                }
                for r in regions.iter_mut() {
                    decay(&mut r.status, step);
                }
                d
            }
            Victim::Player { hp, .. } => {
                // the player's poison and burning run on the scaled frame step,
                // not the unit step (0x00B75070, 0x00B75890)
                let d = poison_player(&mut f.dots, &f.statuses, dt) + burning(&f.statuses, dt);
                *hp = (*hp + d).max(0.0);
                decay(&mut f.pools, step);
                d
            }
        };
        if hp != 0.0 || !ended.is_empty() || !f.statuses.works.is_empty() {
            out.push(StatusEvent { owner: f.owner, hp, ended, step_scale: scale });
        }
    }
    out
}
