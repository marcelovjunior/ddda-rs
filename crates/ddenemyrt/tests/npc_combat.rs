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
fn npc_contact_observer_preserves_pass_and_exposes_scaled_deduplicated_contacts() {
    let mut fixtures = vec![enemy(), npc(500, 0, 1000.0), npc(501, 0x0800_0000, 1000.0)];
    let Attacker::Enemy(attacker) = &mut fixtures[0].attacker else { unreachable!() };
    attacker.attack_mul = Some(0.2); // Synthetic multiplier, not a native placement constant.
    let duplicate_attack = fixtures[0].attacks[0].clone();
    fixtures[0].attacks.push(duplicate_attack);
    let duplicate_hurt = fixtures[1].hurts[0].clone();
    fixtures[1].hurts.push(duplicate_hurt);
    let mut ordinary = fixtures.clone();
    let expected = hit_pass(&mut ordinary, &tables(), 1.0, &mut || 0.0, &|_| true);
    let mut trace = Vec::new();
    let actual = ddenemyrt::combat::hit_pass_with_npc_trace(
        &mut fixtures, &tables(), 1.0, &mut || 0.0, &|_| true, &mut trace);
    assert_eq!(actual, expected);
    assert_eq!(trace.len(), 2);
    for (index, evidence) in trace.iter().enumerate() {
        assert_eq!(evidence.contacts.len(), 1, "repeated volumes must remain one accepted contact");
        assert_eq!(evidence.before_mitigation.hits, 1);
        assert_eq!(evidence.contacts[0].raw.phys, 20.0);
        assert_eq!(evidence.valued_total, actual[index].total);
        assert_eq!(evidence.hp_after, main_pool(&fixtures[index + 1]).hp);
        assert_eq!(fixtures[index + 1].history, ordinary[index + 1].history);
        assert_eq!(main_pool(&fixtures[index + 1]), main_pool(&ordinary[index + 1]));
    }
    assert!(trace[0].hp_after < trace[0].hp_before);
    assert_eq!(trace[1].hp_after, trace[1].hp_before, "NoDamage observation cannot bypass the HP gate");
}

/// Private per-hit evidence for the rapid-HP-loss investigation. The
/// placements/rows/motions/rates are installed data; flat ground, an
/// unattended NPC-only fight and the current default think-table adapter
/// are explicit fixture limits, not the route.
#[test]
#[ignore = "bounded private NPC contact diagnostic; run explicitly with --ignored --nocapture"]
fn installed_reynard_rescue_contact_trace() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let read_layout = |path| mtlot::parse(&vfs.read(path, mtarc::type_hash("rLayout")).unwrap()).unwrap();
    let npc_records = read_layout(r"scr\st100\etc\st100_00m00n_n04");
    let npc_record = npc_records.iter().find(|r| r.get("mNpcId") == Some(&mtlot::Value::Int(304))).unwrap();
    for label in ["HP倍率設定の有無", "HPの倍率", "防御力倍率設定の有無", "防御力の変化倍率", "魔法防御力倍率設定の有無", "魔法防御力の変化倍率"] {
        eprintln!("NPC304_PLACEMENT {label} {:?}", npc_record.get(label));
    }
    let body = std::sync::Arc::new(ddenemyrt::npc_body::NpcBody::load(&vfs, &image, 304, 304, 0, 0, 0).unwrap());
    let data = std::sync::Arc::new(ddenemyrt::fight::FightData::load(&vfs, &image).unwrap());
    let region = ddenemyrt::npc_combat::ordinary_region(&vfs, &data.tables.game, ORDINARY_NPC_HP).unwrap();
    let mut fight = ddenemyrt::fight::Fight::new(data);
    let owner = fight.bind_npc(304, 1, body.clone(), region, body.resolve(3226).unwrap());
    let npc_position = npc_record.placement().unwrap().position;
    fight.npcs.get_mut(&owner).unwrap().position = npc_position;
    fight.npcs.get_mut(&owner).unwrap().yaw = npc_record.placement().unwrap().angle[1];
    let enemy_records = read_layout(r"scr\st100\etc\st100_64m53n_e23");
    for record in enemy_records {
        let enabled = |name| matches!(record.get(name), Some(mtlot::Value::Int(n)) if *n != 0);
        let scalar = |name| match record.get(name) { Some(mtlot::Value::F32(n)) => *n, other => panic!("missing {name}: {other:?}") };
        let placement = record.placement().unwrap();
        let rates = ddenemyrt::goblin::PlacementRates {
            hp: enabled("HP倍率設定の有無").then(|| scalar("HPの倍率")),
            attack: enabled("攻撃力倍率設定の有無").then(|| scalar("攻撃力の変化倍率")),
        };
        let attacker = fight.add_goblin_placed(placement.position, placement.angle[1], rates);
        eprintln!("RESCUE_GOBLIN owner={attacker} record={} rates={rates:?} first_think={:?}", record.id, record.get("mStartTableNo"));
    }
    struct Flat(f32);
    impl ddsim::Ground for Flat { fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= self.0).then_some(self.0) } }
    let ground = Flat(npc_position[1]);
    let sim = ddsim::Sim::new(Default::default());
    let mut trace = Vec::new();
    for frame in 0..1800 {
        trace.clear();
        fight.hits_with_npc_trace(1.0, &mut trace);
        for hit in &trace {
            eprintln!("NPC_HIT frame={frame} seconds={} {hit:?}", frame as f32 / 30.0);
        }
        fight.enemies(&sim, &ground, 1.0);
        let npc = fight.npcs.get_mut(&owner).unwrap();
        npc.frame = body.body.wrap_frame(npc.motion, npc.frame + 1.0);
        fight.volumes(&sim);
        if fight.npcs[&owner].fighter.victim.dead() { eprintln!("NPC_HP_ZERO frame={frame}"); break; }
    }
    for goblin in &fight.goblins { eprintln!("GOBLIN_ACTIONS owner={} log={:?}", goblin.fighter.owner, goblin.log); }
    eprintln!("NPC_FINAL hp={}", fight.npcs[&owner].fighter.victim.hp());
    assert!(fight.npc_errors.is_empty(), "diagnostic must use valid installed poses");
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
