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
    enemy_attack, merge_frame, player_attack, player_mitigate, resolve_enemy_hit_valued_with, Adjust, CollisionRates,
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
    /// An ordinary NPC inherits player mitigation, with its own main
    /// collision pool and canonical script flags (0x00BB21E0). The host
    /// supplies the pool built by uPlayerBase::vf196 (0x00B5F0A0).
    Npc {
        defender: PlayerDefender,
        region: RegionPool,
        rates: CollisionRates,
    },
}

impl Victim {
    pub fn dead(&self) -> bool {
        match self {
            Victim::Enemy { regions, .. } => regions.first().is_some_and(|r| r.hp <= 0.0),
            Victim::Player { hp, .. } => *hp <= 0.0,
            Victim::Npc { region, .. } => region.hp <= 0.0,
        }
    }

    pub fn hp(&self) -> f32 {
        match self {
            Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp),
            Victim::Player { hp, .. } => *hp,
            Victim::Npc { region, .. } => region.hp,
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
    /// Valued damage before HP floors, no-HP flags and clamping; this need
    /// not equal the observed HP difference. A linked hit without the
    /// mapped region's LINK_HP attribute reports zero here.
    pub total: f32,
    /// Stagger / knockdown of the hit region.
    pub reaction: Reaction,
    /// The main pool emptied on this hit (death, vf72).
    pub killed: bool,
}

/// Actor facts consumed by uPlayerBase::vf60, independent of damage pools.
/// `faction` is the result of 0x008571D0 (status/Intel/character flags),
/// not a collision group kind. None retains an unresolved provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactActor {
    pub character: bool,
    pub enemy: bool,
    pub vf117: bool,
    pub faction: Option<bool>,
}

/// An authored checking contact, before history, invincibility and damage
/// callbacks. This is deliberately separate from the merged [`HitEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedContact {
    pub attacker: u32,
    pub victim: u32,
    pub key: u32,
    pub region: u32,
    pub attack_kind: u32,
    pub target: Option<ContactActor>,
    pub attacker_faction: Option<bool>,
}

impl AcceptedContact {
    /// Known vf60 bits. Missing faction facts leave bits40/80 unresolved;
    /// callers must not infer them from team IDs, damage or an HP change.
    // Translated from 0x00B7CF69..0x00B7D01D (DDDA.exe 2017-12-13 Steam).
    pub fn known_flags(self) -> u32 {
        let mut flags = match self.attack_kind { 0 => 1, 1 => 2, 2 => 4, _ => 0 };
        if let Some(target) = self.target.filter(|target| target.character) {
            if target.enemy { flags |= 8; }
            flags |= if target.vf117 { 0x20 } else { 0x10 };
            if let (Some(attacker), Some(victim)) = (self.attacker_faction, target.faction) {
                if attacker != victim {
                    flags |= 0x40;
                    if !target.vf117 { flags |= 0x80; }
                }
            }
        }
        flags
    }
}

/// Explicit providers for native Group.mIsCheck and actor callback facts.
/// Ordinary actor contacts currently lack the optional native LOS/ignore-zone
/// gates (0x00779270/0x00779B10); this observer does not invent those gates.
pub struct ContactObserver<'a> {
    pub is_check: &'a dyn Fn(&AttackVolume) -> bool,
    pub actor: &'a dyn Fn(u32) -> Option<ContactActor>,
    pub output: &'a mut Vec<AcceptedContact>,
}

/// Optional headless evidence for an accepted NPC contact. No IO or RNG
/// lives in this observer; the caller chooses where to retain its output.
#[derive(Debug, Clone)]
pub struct NpcContactTrace {
    pub attacker: u32,
    pub key: u32,
    pub relation: u32,
    pub multi_hit: bool,
    pub reset_time: f32,
    pub attacker_values: Attacker,
    pub raw: HitValue,
}

#[derive(Debug, Clone)]
pub struct NpcHitTrace {
    pub victim: u32,
    pub region: u32,
    pub contacts: Vec<NpcContactTrace>,
    pub before_mitigation: ddcombat::damage::MergedHit,
    pub after_region: ddcombat::damage::MergedHit,
    pub defender: PlayerDefender,
    pub rates: CollisionRates,
    pub valued_total: f32,
    pub hp_before: f32,
    pub hp_after: f32,
}

/// Apply a hit already valued on another enemy collision to this linked
/// collision. The host resolves the destination's optional region remap
/// (+0xd34/+0xd38) and region-ID map (+0xcf4) before passing `mapped_pool`.
/// The main pool remains index zero, as in the ordinary enemy hit pass.
/// Missing mapped pools suppress HP only; main break/status still run.
///
/// `source` retains attacker, original hurt region and merged hit count.
/// The result names the linked victim and its main-pool reaction/death.
/// Its total is the carried value when LINK_HP is present, even when a
/// no-HP flag or no-death floor prevents some or all HP loss.
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

/// Host-selected collision mappings and linked-collision chain.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyCollisionConfig {
    pub owner: u32,
    /// Collision +0xcf4: raw hurt-region index -> installed life-pool ID.
    pub hit_to_pool: Vec<u32>,
    /// Destination collision +0xd34/+0xd38, applied only to mirrored hits.
    /// Indices beyond this optional remap retain their original value.
    pub linked_region_remap: Option<Vec<u32>>,
    /// Explicit ordered collision +0xcc8 chain for this source. The host
    /// supplies the complete chain; destination chains are not re-expanded.
    pub linked_owners: Vec<u32>,
    /// Collision +0xd28/+0xd2c, physical and magick status build-up rates.
    pub status_rates: [f32; 2],
}

impl EnemyCollisionConfig {
    fn pool_index(&self, region: u32, pools: &[RegionPool], mirrored: bool) -> Option<usize> {
        let region = if mirrored {
            self.linked_region_remap.as_ref().and_then(|m| m.get(region as usize)).copied().unwrap_or(region)
        } else { region };
        let id = self.hit_to_pool.get(region as usize)?;
        pools.iter().position(|p| p.id == *id)
    }
}

/// Refreshed before each local hit, after preceding local/linked HP changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatOwnerState {
    pub owner: u32,
    /// Combat HP observation, not the actor death marker (uEnemy +0x5bd0).
    pub has_hp: bool,
}

/// Actor-specific callbacks in collision execution order. The policy owns
/// actor observations such as accepted-death markers; HP snapshots do not
/// substitute for those markers. Damage-result callbacks run only for
/// configured enemies; the contact permission callback runs before merging.
pub trait EnemyHitPolicy {
    /// Victim vf66 callback after attack valuation, before frame merging.
    /// Contact history and the attack's critical roll have already run.
    fn accepts_contact(&mut self, _victim: u32, _attacker: u32) -> bool { true }

    fn after_mitigation(
        &mut self,
        _owner: u32,
        _pool_id: u32,
        _hit: &mut ddcombat::damage::MergedHit,
        _owners: &[CombatOwnerState],
    ) {}

    /// Runs immediately after a configured local or mirrored result, before
    /// the next collision is processed. `killed` is HP depletion; accepting
    /// an actor's death still requires its source-derived guards/selector.
    /// In the original, 0x0076e7c0 dispatches death during 0x0076db10, and
    /// common vf72 calls vf119 (0x00ab37d0) before 0x00aab4e4 writes the kill.
    fn after_hit(&mut self, _event: &HitEvent) {}
}

impl EnemyHitPolicy for () {}

/// Configured enemy collision pass. Configuration errors are rejected before
/// HP, status, history or RNG changes. Ordinary unmapped hurt regions are not
/// supported: 0x0076db10 skips local valuation there, leaving a carried total
/// whose earlier producer is not yet represented by MergedHit. Such inputs
/// return an error, never fall back to main HP. A mirrored missing mapping
/// retains the separate sourced status/break-only behavior.
///
/// Post-mitigation policy sees current owner HP state. Actor death-marker
/// observations remain the host policy's responsibility. Owner and chain order
/// are explicit; all unconfigured fighters retain the legacy pass behavior.
pub fn hit_pass_configured(
    fighters: &mut [Fighter],
    tables: &Tables,
    dt: f32,
    roll: &mut dyn FnMut() -> f32,
    holds: &dyn Fn(u32) -> bool,
    config: &[EnemyCollisionConfig],
    after_mitigation: &mut dyn FnMut(u32, u32, &mut ddcombat::damage::MergedHit, &[CombatOwnerState]),
) -> Result<Vec<HitEvent>, String> {
    struct CallbackOnly<'a>(&'a mut dyn FnMut(u32, u32, &mut ddcombat::damage::MergedHit, &[CombatOwnerState]));
    impl EnemyHitPolicy for CallbackOnly<'_> {
        fn after_mitigation(&mut self, owner: u32, pool_id: u32, hit: &mut ddcombat::damage::MergedHit, owners: &[CombatOwnerState]) {
            self.0(owner, pool_id, hit, owners);
        }
    }
    hit_pass_configured_with_policy(fighters, tables, dt, roll, holds, config, &mut CallbackOnly(after_mitigation))
}

/// The configured pass with immediate actor observation in addition to the
/// post-mitigation extension. Validation and unsupported-input behavior are
/// identical to [`hit_pass_configured`]. This does not assign layout kills,
/// actor actions or death markers; the policy handles its actor state.
pub fn hit_pass_configured_with_policy(
    fighters: &mut [Fighter],
    tables: &Tables,
    dt: f32,
    roll: &mut dyn FnMut() -> f32,
    holds: &dyn Fn(u32) -> bool,
    config: &[EnemyCollisionConfig],
    policy: &mut dyn EnemyHitPolicy,
) -> Result<Vec<HitEvent>, String> {
    let mut owners = std::collections::BTreeSet::new();
    for fighter in fighters.iter() {
        if !owners.insert(fighter.owner) {
            return Err(format!("duplicate fighter owner {}", fighter.owner));
        }
    }
    let mut configured = std::collections::BTreeSet::new();
    for c in config {
        if !configured.insert(c.owner) {
            return Err(format!("duplicate collision owner {}", c.owner));
        }
        let fighter = fighters.iter().find(|f| f.owner == c.owner)
            .ok_or_else(|| format!("missing collision owner {}", c.owner))?;
        let Victim::Enemy { regions, .. } = &fighter.victim else {
            return Err(format!("collision owner {} is not an enemy", c.owner));
        };
        if regions.first().is_none_or(|p| p.id != 0) {
            return Err(format!("collision owner {} has no main pool", c.owner));
        }
        for hurt in &fighter.hurts {
            if c.pool_index(hurt.region_id, regions, false).is_none() {
                return Err(format!("owner {} has unsupported ordinary region {}", c.owner, hurt.region_id));
            }
        }
        let mut chain = std::collections::BTreeSet::from([c.owner]);
        for owner in &c.linked_owners {
            if !chain.insert(*owner) {
                return Err(format!("owner {} has duplicate linked owner {owner}", c.owner));
            }
            if !owners.contains(owner) || !config.iter().any(|target| target.owner == *owner) {
                return Err(format!("owner {} has missing linked collision owner {owner}", c.owner));
            }
        }
    }
    Ok(hit_pass_inner(fighters, tables, dt, roll, holds, config, policy, None))
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
    hit_pass_inner(fighters, tables, dt, roll, holds, &[], &mut (), None)
}

/// Same ordinary pass, with per-contact NPC values exposed for source
/// verification. Observation never changes hit selection or valuation.
pub fn hit_pass_with_npc_trace(
    fighters: &mut [Fighter], tables: &Tables, dt: f32,
    roll: &mut dyn FnMut() -> f32, holds: &dyn Fn(u32) -> bool,
    trace: &mut Vec<NpcHitTrace>,
) -> Vec<HitEvent> {
    hit_pass_inner(fighters, tables, dt, roll, holds, &[], &mut (), Some(trace))
}

/// Ordinary pass with explicit NPC actor permissions. Projectile owners keep
/// independent histories; `owners` resolves their actual originating actors.
/// Each owner tuple contains its ID, resolved class, and whether actor class
/// bit8 is set with a linked intel whose gauge byte is nonzero. The third
/// fact is independent of the hostile-NPC -> Enemy class distinction.
pub fn hit_pass_with_npc_guards(
    fighters: &mut [Fighter], tables: &Tables, dt: f32,
    roll: &mut dyn FnMut() -> f32, holds: &dyn Fn(u32) -> bool,
    guards: &[(u32, crate::npc_combat::GaugeHitGuard)],
    owners: &[(u32, crate::npc_combat::DamageOwnerClass, bool)],
    trace: Option<&mut Vec<NpcHitTrace>>,
) -> Vec<HitEvent> {
    hit_pass_with_npc_guards_and_contacts(fighters, tables, dt, roll, holds,
        guards, owners, trace, None)
}

/// Same damage pass with native attacker-vf60 observation. 0x0076BCA9
/// dispatches this after same-frame pair dedup, BEFORE persistent-history
/// 0x0076BD27, invincibility 0x0076BD76 and victim vf59/vf66 acceptance.
/// Consequently a guarded, repeated or zero-damage contact still notifies
/// its attacker. Authored Group.mIsCheck gates the callback (0x0076BC6A;
/// 0x0076FAB0 -> entry+1C0 -> 0x00773EE0 pair+1D8).
// Translated from 0x0076B7E0 (DDDA.exe 2017-12-13 Steam).
pub fn hit_pass_with_npc_guards_and_contacts(
    fighters: &mut [Fighter], tables: &Tables, dt: f32,
    roll: &mut dyn FnMut() -> f32, holds: &dyn Fn(u32) -> bool,
    guards: &[(u32, crate::npc_combat::GaugeHitGuard)],
    owners: &[(u32, crate::npc_combat::DamageOwnerClass, bool)],
    trace: Option<&mut Vec<NpcHitTrace>>,
    observer: Option<ContactObserver<'_>>,
) -> Vec<HitEvent> {
    if let Some(observer) = observer {
        let attacks: Vec<_> = fighters.iter().flat_map(|f| f.attacks.iter().cloned()).collect();
        for victim in fighters.iter() {
            for contact in contacts(&attacks, &victim.hurts) {
                let attack = &attacks[contact.attack];
                if !(observer.is_check)(attack) { continue; }
                observer.output.push(AcceptedContact {
                    attacker: attack.side.owner, victim: victim.owner,
                    key: attack.key, region: victim.hurts[contact.hurt].region_id,
                    attack_kind: attack.side.kind,
                    target: (observer.actor)(victim.owner),
                    attacker_faction: (observer.actor)(attack.side.owner).and_then(|a| a.faction),
                });
            }
        }
    }
    struct Guards<'a> {
        guards: &'a [(u32, crate::npc_combat::GaugeHitGuard)],
        owners: &'a [(u32, crate::npc_combat::DamageOwnerClass, bool)],
    }
    impl EnemyHitPolicy for Guards<'_> {
        fn accepts_contact(&mut self, victim: u32, attacker: u32) -> bool {
            let Some((_, guard)) = self.guards.iter().find(|(owner, _)| *owner == victim) else { return true };
            let (class, gauge) = self.owners.iter().find(|(owner, _, _)| *owner == attacker)
                .map(|(_, class, gauge)| (*class, *gauge))
                .unwrap_or((crate::npc_combat::DamageOwnerClass::Unknown, false));
            guard.allows(class, gauge)
        }
    }
    hit_pass_inner(fighters, tables, dt, roll, holds, &[], &mut Guards { guards, owners }, trace)
}

fn hit_pass_inner(
    fighters: &mut [Fighter],
    tables: &Tables,
    dt: f32,
    roll: &mut dyn FnMut() -> f32,
    holds: &dyn Fn(u32) -> bool,
    config: &[EnemyCollisionConfig],
    policy: &mut dyn EnemyHitPolicy,
    mut npc_trace: Option<&mut Vec<NpcHitTrace>>,
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
    for victim_index in 0..fighters.len() {
        let current_owners: Vec<_> = if config.is_empty() { Vec::new() } else {
            fighters.iter().map(|f| CombatOwnerState { owner: f.owner, has_hp: !f.victim.dead() }).collect()
        };
        let v = &mut fighters[victim_index];
        if v.victim.dead() {
            continue;
        }
        // 0x0076B7E0 rejects valuation under Invincible (+0xCFC bit4),
        // but still records contact history after this gate.
        let accepts = match &v.victim {
            Victim::Npc { rates, .. } => crate::npc_combat::accepts_hit(rates),
            _ => true,
        };
        let hits = resolve(&attacks, &v.hurts, &mut v.history, dt, holds, accepts);
        if hits.is_empty() {
            continue;
        }
        let mut accepted = Vec::new();
        let mut values: Vec<HitValue> = Vec::new();
        let mut npc_contacts = (npc_trace.is_some() && matches!(v.victim, Victim::Npc { .. }))
            .then(Vec::new);
        for &(ai, region) in &hits {
            let a = &attacks[ai];
            let Some((_, att)) = attackers.iter().find(|(o, _)| *o == a.side.owner) else {
                continue;
            };
            let value = match att {
                Attacker::Enemy(e) => enemy_attack(e, &a.row),
                Attacker::Player(p) => player_attack(p, &a.row, roll(), &tables.dlc),
            };
            // 0x0076B7E0 calls valuation 0x0076C4F0 before victim vf66
            // (+0xC7C). Its rejection retains RNG and recorded history,
            // but excludes HP, reactions and status from frame merging.
            if !policy.accepts_contact(v.owner, a.side.owner) { continue; }
            accepted.push((ai, region));
            if let Some(contacts) = &mut npc_contacts {
                contacts.push(NpcContactTrace { attacker: a.side.owner, key: a.key,
                    relation: a.relation, multi_hit: a.flags.multi_hit, reset_time: a.reset_time,
                    attacker_values: att.clone(), raw: value });
            }
            values.push(value);
        }
        let Some(mut merged) = merge_frame(&values) else {
            continue;
        };
        let (first, region) = accepted[0];
        let status_hit = StatusHit {
            element: merged.element,
            element_buildup: merged.element_buildup,
            status: merged.status,
            mag: merged.mag,
        };
        let hits_n = merged.hits;
        let npc_unmitigated = npc_contacts.as_ref().map(|_| merged.clone());
        let attacker = attacks[first].side.owner;
        let collision = config.iter().find(|c| c.owner == v.owner);
        let mut carried = None;
        let event = match &mut v.victim {
            Victim::Enemy {
                defender,
                regions,
                rates,
            } => {
                let at = match collision {
                    Some(c) => c.pool_index(region, regions, false).expect("mapping validated before hit pass"),
                    None => regions.iter().position(|r| r.id == region).unwrap_or(0),
                };
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
                let (out, valued) = resolve_enemy_hit_valued_with(merged, &def, regions, at, rates, &tables.game, 1.0,
                    |pool_id, hit| {
                        if collision.is_some() {
                            policy.after_mitigation(v.owner, pool_id, hit, &current_owners);
                        }
                    });
                // build-up on the hit region (0x007772B0; a main region
                // exists, so it runs whatever the region's mAttr says)
                let r = &mut regions[at];
                let resist = r.resist.element;
                let (status_hit, status_rates) = if let Some(c) = collision {
                    (StatusHit { element: valued.hit.element, element_buildup: valued.hit.element_buildup,
                        status: valued.hit.status, mag: valued.hit.mag }, c.status_rates)
                } else { (status_hit, [1.0, 1.0]) };
                build_up(&mut r.status, &status_hit, &resist, status_rates[0], status_rates[1], &mut *roll);
                let (slots, _frozen) = fire(&mut r.status);
                add_fired(&mut v.statuses, &slots, &tables.status_enemy);
                carried = Some(valued);
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
            Victim::Npc { defender, region: pool, rates } => {
                use ddcombat::damage::{apply_break, apply_hp, resolve_reaction, scale_for_region};
                // Translated from 0x00B7AD90 -> 0x0076DB10 -> 0x00776F50
                // (DDDA.exe 2017-12-13 Steam). Ordinary NPCs have one
                // main region; the body uses NPC group kind3 for pairs.
                let mut h = merged;
                player_mitigate(&mut h, defender, &tables.game);
                let (dp, dm) = v.statuses.defence_muls();
                h.phys = (h.phys * dp).max(0.0);
                h.mag = (h.mag * dm).max(0.0);
                // As in the existing enemy pass, attacker+0x14 is not
                // yet carried by Fighter: its minimum is assumed 1.
                let total = scale_for_region(&mut h, pool, rates, &tables.game, None, 1.0);
                let hp_before = pool.hp;
                let was_alive = pool.hp > 0.0;
                apply_break(pool, &h, rates);
                let npc_status = StatusHit { element: h.element, element_buildup: h.element_buildup,
                    status: h.status, mag: h.mag };
                build_up(&mut pool.status, &npc_status, &pool.resist.element, 1.0, 1.0, &mut *roll);
                let (slots, frozen) = fire(&mut pool.status);
                add_fired(&mut v.statuses, &slots, &tables.status_player);
                // NoDamage affects HP alone. It does not discard this
                // hit, its status build-up, or the contact history.
                apply_hp(pool, total, rates, h.row.is_no_death);
                if let (Some(trace), Some(contacts), Some(before_mitigation)) =
                    (npc_trace.as_mut(), npc_contacts.take(), npc_unmitigated)
                {
                    trace.push(NpcHitTrace { victim: v.owner, region, contacts,
                        before_mitigation, after_region: h.clone(), defender: *defender,
                        rates: *rates, valued_total: total, hp_before, hp_after: pool.hp });
                }
                HitEvent {
                    attacker,
                    victim: v.owner,
                    region,
                    hits: hits_n,
                    total,
                    reaction: resolve_reaction(pool, frozen),
                    killed: was_alive && pool.hp <= 0.0,
                }
            }
        };
        events.push(event.clone());
        if collision.is_some() {
            policy.after_hit(&event);
        }
        // 0x0076b7e0 runs each linked collision immediately after its local
        // hit, preserving the already-valued payload and original region.
        if let (Some(collision), Some(valued)) = (collision, carried) {
            for owner in &collision.linked_owners {
                let target_config = config.iter().find(|c| c.owner == *owner).expect("validated linked config");
                let target = fighters.iter_mut().find(|f| f.owner == *owner).expect("validated linked owner");
                let Victim::Enemy { regions, .. } = &target.victim else { unreachable!("validated linked enemy") };
                let mapped = target_config.pool_index(event.region, regions, true);
                if let Some(linked) = apply_linked_enemy_hit(target, &event, &valued, mapped, target_config.status_rates, &tables.status_enemy, roll) {
                    policy.after_hit(&linked);
                    events.push(linked);
                }
            }
        }
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
    accepts: bool,
) -> Vec<(usize, u32)> {
    history.begin_frame(dt);
    let mut hits = Vec::new();
    for c in contacts(attacks, hurts) {
        let a = &attacks[c.attack];
        if !history.already_hit(a.side.owner, a.key, a.relation) && accepts {
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
            Victim::Npc { region, .. } => {
                // Ordinary NPCs inherit the same player-side timed
                // status handlers. As for Player above, DoT currently
                // writes HP directly; script gating of DoT is untraced.
                let d = poison_player(&mut f.dots, &f.statuses, dt) + burning(&f.statuses, dt);
                region.hp = (region.hp + d).max(0.0);
                decay(&mut region.status, step);
                d
            }
        };
        if hp != 0.0 || !ended.is_empty() || !f.statuses.works.is_empty() {
            out.push(StatusEvent { owner: f.owner, hp, ended, step_scale: scale });
        }
    }
    out
}
