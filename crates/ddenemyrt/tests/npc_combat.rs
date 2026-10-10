use ddenemyrt::npc_combat::{
    accepts_hit, gauge_fill, ordinary_collision_rates, ordinary_defender, ORDINARY_NPC_HP,
};
use ddcombat::damage::{Adjust, AttackRow, EnemyAttacker, PlayerAttacker, RegionPool};
use ddcombat::volumes::{AttackVolume, HurtVolume, Side, Volume};
use ddenemyrt::combat::{hit_pass, Attacker, Fighter, Tables, Victim};

// Synthetic overlap/pool fixtures test the translated pass. They are not
// claimed as Reynard's installed skeleton, volumes or DP/BP values.
fn pool(hp: f32) -> RegionPool {
    RegionPool::from_element(0, hp, 100.0, 0.3, 100.0, 0.3, 1.0, 1.0, 1, 0, 75.0, 30.0)
}

fn npc(owner: u32, flags: u32, hp: f32) -> Fighter {
    let mut region = pool(hp);
    region.attr = 0;
    Fighter {
        owner,
        attacker: Attacker::Player(PlayerAttacker::new(1.0, 1.0, 0.0, 0.0)),
        victim: Victim::Npc { defender: ordinary_defender(), region, rates: ordinary_collision_rates(flags) },
        attacks: vec![],
        hurts: vec![HurtVolume { side: Side { owner, kind: 3 },
            volume: Volume::Sphere { c: [0.0; 3], r: 1.0 }, region_id: 0, hit_prio: 0 }],
        history: Default::default(), statuses: Default::default(), dots: Default::default(), pools: Default::default(),
    }
}

fn enemy() -> Fighter {
    let owner = 100;
    let mut status = [0.0; 15];
    status[0] = 30.0;
    Fighter {
        owner,
        attacker: Attacker::Enemy(EnemyAttacker::new(100.0, 0.0)),
        victim: Victim::Player { defender: ordinary_defender(), hp: 1000.0, hp_max: 1000.0 },
        attacks: vec![AttackVolume { side: Side { owner, kind: 1 },
            volume: Volume::Sphere { c: [0.0; 3], r: 1.0 },
            row: AttackRow { attack_rate: 1.0, hit_rate: 100.0, shrink_type: 1,
                shrink_attack: 150.0, blow_type: 3, status, ..Default::default() },
            flags: Default::default(), reset_time: 0.0, key: 1 << 16, relation: 0 }],
        hurts: vec![], history: Default::default(), statuses: Default::default(), dots: Default::default(), pools: Default::default(),
    }
}

fn tables() -> Tables {
    let mut game = vec![0.0; 0x40];
    game[5] = 100.0;
    game[6] = 0.5;
    game[0x25] = 1.0;
    game[0x26] = 1.0;
    game[0x27] = 1.0;
    Tables { game: Adjust(game), dlc: Adjust(vec![]), status_enemy: Default::default(), status_player: Default::default() }
}

fn set_flags(f: &mut Fighter, flags: u32) {
    let Victim::Npc { rates, .. } = &mut f.victim else { unreachable!() };
    *rates = ordinary_collision_rates(flags);
}

fn main_pool(f: &Fighter) -> &RegionPool {
    let Victim::Npc { region, .. } = &f.victim else { unreachable!() };
    region
}

#[test]
fn ordinary_npc_constructor_and_defence_chain() {
    assert_eq!(ORDINARY_NPC_HP.to_bits(), 0x447a_0000);
    let defender = ordinary_defender();
    assert_eq!((defender.base_defence, defender.base_mgc_defence), (1.0, 1.0));
    assert_eq!((defender.equip_defence, defender.equip_mgc_defence), (0.0, 0.0));
}

#[test]
fn npc_flags_preserve_distinct_hp_contact_and_reaction_gates() {
    let no_damage = ordinary_collision_rates(0x0800_0000);
    assert_eq!(no_damage.flags, 1);
    assert!(accepts_hit(&no_damage));
    let invincible = ordinary_collision_rates(0x1000_0000);
    assert_eq!(invincible.flags, 4);
    assert!(!accepts_hit(&invincible));
    assert_eq!(ordinary_collision_rates(0x0400_0000).flags, 2);
    assert_eq!(ordinary_collision_rates(0x0200_0000).flags, 8);
    assert_eq!(ordinary_collision_rates(0x0008_0000).flags, 9);
    assert_eq!(ordinary_collision_rates(0x1e00_0000).flags, 15);
    // Unrelated canonical flags must not silently become damage gates.
    assert_eq!(ordinary_collision_rates(0x0000_0210).flags, 0);
    // Updating the script flags replaces the ordinary collision word.
    assert_eq!(ordinary_collision_rates(0).flags, 0);
}

#[test]
fn gauge_uses_live_pool_authored_width_and_original_minimum() {
    assert_eq!(gauge_fill(500.0, 1000.0, false, 120.0), Some(60.0));
    assert_eq!(gauge_fill(1.0, 1000.0, false, 120.0), Some(3.0));
    assert_eq!(gauge_fill(1500.0, 1000.0, false, 120.0), Some(180.0));
    assert_eq!(gauge_fill(0.0, 1000.0, false, 120.0), None);
    assert_eq!(gauge_fill(500.0, 1000.0, true, 120.0), None);
    assert_eq!(gauge_fill(500.0, 0.0, false, 120.0), None);
    assert_eq!(gauge_fill(500.0, f32::EPSILON / 2.0, false, 120.0), None);
    assert!(gauge_fill(1.0, f32::EPSILON, false, 120.0).is_some());
    assert_eq!(gauge_fill(f32::NAN, 1000.0, false, 120.0), None);
}

#[test]
fn npc_no_damage_keeps_hp_but_accepts_status_reaction_and_history() {
    let mut fighters = vec![enemy(), npc(500, 0x0800_0000, 1000.0)];
    let attack = fighters[0].attacks.clone();
    let t = tables();
    let events = hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].attacker, events[0].victim), (100, 500));
    let expected = 100.0 - 100.0 / 101.0;
    assert_eq!(events[0].total, expected);
    assert!(events[0].reaction.stagger);
    assert!(!events[0].killed);
    assert_eq!(main_pool(&fighters[1]).hp, 1000.0);
    assert_eq!(main_pool(&fighters[1]).status.acc[0], 30.0);
    set_flags(&mut fighters[1], 0);
    assert!(hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false).is_empty());
    fighters[0].attacks.clear();
    hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false);
    fighters[0].attacks = attack;
    assert_eq!(hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false).len(), 1);
    assert_eq!(main_pool(&fighters[1]).hp, 1000.0 - expected);
}

#[test]
fn npc_invincible_rejects_valuation_but_records_contact() {
    let mut fighters = vec![enemy(), npc(500, 0x1000_0000, 1000.0)];
    let t = tables();
    assert!(hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false).is_empty());
    let p = main_pool(&fighters[1]);
    assert_eq!((p.hp, p.dp, p.status.acc[0]), (1000.0, 100.0, 0.0));
    assert!(fighters[1].history.already_hit(100, 1 << 16, 0));
    set_flags(&mut fighters[1], 0);
    assert!(hit_pass(&mut fighters, &t, 1.0, &mut || 0.0, &|_| false).is_empty());
}

#[test]
fn npc_owners_keep_independent_hp_floor_and_death() {
    let mut fighters = vec![enemy(), npc(500, 0x0400_0000, 20.0), npc(501, 0, 20.0)];
    let events = hit_pass(&mut fighters, &tables(), 1.0, &mut || 0.0, &|_| false);
    assert_eq!(events.len(), 2);
    assert_eq!(main_pool(&fighters[1]).hp, 1.0);
    assert!(!events[0].killed);
    assert_eq!(main_pool(&fighters[2]).hp, 0.0);
    assert!(events[1].killed);
    assert!(!fighters[1].victim.dead());
    assert!(fighters[2].victim.dead());
}

#[test]
fn installed_npc_main_pool_and_collision_pair_schema() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return; }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let game = Adjust::parse(&ddenemyrt::body::load::read(&vfs, r"param\game", "rAdjustParam").unwrap()).unwrap();
    let p = ddenemyrt::npc_combat::ordinary_region(&vfs, &game, ORDINARY_NPC_HP).unwrap();
    assert_eq!((p.id, p.hp, p.hp_max, p.attr, p.surface), (0, 1000.0, 1000.0, 0, 1));
    assert!(p.dp_max > 0.0 && p.bp_max > 0.0);
    assert_eq!((p.dp_reset_max, p.bp_reset_max), (game.get(0x1bf), game.get(0x1c0)));
    assert!(p.status.threshold[..18].iter().all(|f| *f == 1000.0));
    assert_eq!(p.status.threshold[18], 999.0);
    assert_eq!(p.status.threshold[19].to_bits(), 0x4479_f99a);
    // uNpc's collision table 0x014EAC20, selected by 0x00BAB3A0 and
    // loaded by 0x00B5F0A0. This checks actual group/category metadata;
    // it does not claim to have posed Reynard's authored motion.
    let collision = ddenemyrt::body::load::collision(&vfs, r"collision\npc\m0000", None).unwrap();
    let hurts: Vec<_> = collision.body.groups.iter().filter(|g| g.is_damage).collect();
    assert!(!hurts.is_empty());
    for g in hurts {
        assert_eq!(g.kind, 3);
        assert!(!g.nodes.is_empty());
        assert!(g.nodes.iter().all(|n| n.region_id == 0));
    }
}
