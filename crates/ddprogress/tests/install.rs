//! Progression runs against the player's install and DDDA.exe
//! (`DDDA_NATIVE_PC`, `DDDA_EXE`); skipped when either is absent.
use ddprogress::change::{self, Refused as ChangeRefused};
use ddprogress::learn::{self, Learnable, Refused};
use ddprogress::{Award, Character, Member, Progression, Role, Tables};
use ddskill::growth::{self, Progress};
use ddskill::vocation::{Vocation, WeaponCategory};
use std::path::PathBuf;
use std::sync::OnceLock;

fn tables() -> Option<&'static Tables> {
    static T: OnceLock<Option<Tables>> = OnceLock::new();
    T.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        let exe = std::env::var_os("DDDA_EXE").map(PathBuf::from).or_else(|| ddexe::locate(&native).ok());
        let Some(exe) = exe.filter(|e| native.is_dir() && e.is_file()) else {
            eprintln!("SKIP progression tests: no install or DDDA.exe for {}", native.display());
            return None;
        };
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let file = ddexe::ExeFile::open(exe).unwrap();
        Some(Tables::load(&vfs, &file.image().unwrap()).unwrap())
    })
    .as_ref()
}

fn stats(c: &Character) -> [f32; 6] {
    let s = c.progress.stats;
    [s.hp_max, s.stamina_lv, s.attack, s.defend, s.magic_attack, s.magic_defend]
}

/// Gives a lone Arisen exactly enough experience for `level`.
fn raise(t: &Tables, p: &mut Progression, id: usize, level: u32) {
    let c = &p.characters[id];
    let need = growth::cumulative_exp(level, &t.exp) - growth::cumulative_exp(c.progress.level, &t.exp) - c.progress.exp;
    p.characters[id].award(t, need);
    p.characters[id].settle(t);
}

#[test]
fn constants_from_data() {
    let Some(t) = tables() else { return };
    assert_eq!(t.pawn_catch_up(), 0.7);
    assert_eq!(t.asura_armor_rate(), 2.0);
    assert_eq!(t.perseverance_rate(), 1.5);
    let c = Character::new(t, Vocation::Mage, false);
    // Weal (augment 93): 1.0 when not equipped.
    assert_eq!(c.augment(t, 0x5d), Some(1.0));
    // Live regression (claude-4 oracle, board #722): a Mage Arisen levelled
    // to 7 in one vocation has these stats and needs 2660 for level 8; the
    // discipline of an award follows the live band values.
    let mut c = c;
    c.award(t, growth::cumulative_exp(7, &t.exp));
    c.settle(t);
    assert_eq!(c.progress.level, 7);
    assert_eq!(stats(&c), [542.0, 120.0, 72.0, 78.0, 104.0, 98.0]);
    assert_eq!(c.progress.next_exp, 2660);
    assert_eq!(c.discipline.points, growth::discipline_from_exp(growth::cumulative_exp(7, &t.exp) as i32));
    assert_eq!(growth::discipline_from_exp(1000), 80);
}

/// Level 1 to 10 as a Fighter, change to Warrior at 10, then level 11 with
/// the Warrior's growth.
#[test]
fn level_run_and_vocation_change() {
    let Some(t) = tables() else { return };
    let mut p = Progression::new();
    let a = p.add(Character::new(t, Vocation::Fighter, false));
    p.party[0] = Some(Member { character: a, role: Role::Arisen(0) });
    let c = &p.characters[a];
    assert_eq!((c.progress.level, c.progress.next_exp), (1, 200));
    // Live regression (board #722): next-exp after level 1 is 485.
    p.characters[a].award(t, 200);
    assert!(p.characters[a].step(t).0);
    assert_eq!(p.characters[a].progress.next_exp, 485);
    // One level per step, as the per-frame update does.
    p.characters[a].award(t, 485 + 770);
    assert_eq!(p.characters[a].step(t), (true, false));
    assert!(p.characters[a].step(t).0);
    assert!(!p.characters[a].step(t).0);
    raise(t, &mut p, a, 10);
    let c = &p.characters[a];
    assert_eq!(c.progress.level, 10);
    assert_eq!(stats(c), stats_at(t, Vocation::Fighter, 10));
    // A level-9 character only sees the basic vocations.
    let mut low = Character::new(t, Vocation::Fighter, false);
    low.discipline.points = 5000;
    assert_eq!(change::check(&low, Vocation::Warrior, false), Err(ChangeRefused::NotOffered));
    // At 10 the Arisen may take Warrior for 1000 discipline.
    p.characters[a].discipline.points = 999;
    assert_eq!(
        change::check(&p.characters[a], Vocation::Warrior, false),
        Err(ChangeRefused::Discipline { need: 1000, have: 999 })
    );
    p.characters[a].discipline.points = 1500;
    let before = p.characters[a].clone();
    let done = change::change(t, &mut p.characters[a], Vocation::Warrior, false, false).unwrap();
    assert_eq!((done.cost, done.items.len()), (1000, 0));
    let c = &p.characters[a];
    assert_eq!((c.vocation, c.discipline.points, c.changes), (Vocation::Warrior, 500, 1));
    assert!(c.paid[Vocation::Warrior as usize]);
    // Level, stats and ranks carry over.
    assert_eq!((c.progress, c.discipline.rank), (before.progress, before.discipline.rank));
    // Back to Fighter costs nothing (paid at creation).
    assert_eq!(change::check(c, Vocation::Fighter, false), Ok(0));
    // Level 11 adds the Warrior's row for 11..100.
    raise(t, &mut p, a, 11);
    let row = growth::level_row(11, t.growth(Vocation::Warrior)).clone();
    let after = stats(&p.characters[a]);
    let was = stats_at(t, Vocation::Fighter, 10);
    assert_eq!(after[0] - was[0], f32::from(row.hp_max));
    assert_eq!(after[2] - was[2], f32::from(row.attack));
}

fn stats_at(t: &Tables, v: Vocation, level: u32) -> [f32; 6] {
    let s = Progress::at_level(level, t.growth(v), &t.exp).stats;
    [s.hp_max, s.stamina_lv, s.attack, s.defend, s.magic_attack, s.magic_defend]
}

#[test]
fn learning_and_equipping() {
    let Some(t) = tables() else { return };
    let mut c = Character::new(t, Vocation::Fighter, false);
    let none = |_| false;
    // Starting skills (cost 0, rank 0): Blink Strike level 1 is known.
    assert_eq!(c.learned.weapon_level(40), 1);
    assert_eq!(learn::check(t, &c, Learnable::Core(30), none), Err(Refused::Discipline { need: 800, have: 0 }));
    c.discipline.points = 3000;
    assert_eq!(learn::learn(t, &mut c, Learnable::Core(30), none), Ok(800));
    assert_eq!(learn::learn(t, &mut c, Learnable::Core(30), none), Err(Refused::AlreadyKnown));
    assert_eq!(learn::check(t, &c, Learnable::Weapon { id: 41, level: 2 }, none), Err(Refused::NeedsLevel1));
    assert_eq!(learn::learn(t, &mut c, Learnable::Weapon { id: 41, level: 1 }, none), Ok(200));
    // Flesh Skewer needs rank 3; Fitness rank 2; a Strider augment is not offered.
    assert_eq!(
        learn::check(t, &c, Learnable::Weapon { id: 46, level: 1 }, none),
        Err(Refused::Rank { need: 3, have: 1 })
    );
    assert_eq!(learn::check(t, &c, Learnable::Augment(0), none), Err(Refused::Rank { need: 2, have: 1 }));
    assert_eq!(learn::check(t, &c, Learnable::Augment(16), none), Err(Refused::WrongVocation));
    assert_eq!(learn::check(t, &c, Learnable::Augment(100), none), Err(Refused::Locked));
    // Earn rank 2 through discipline experience (500 at rank 1).
    c.discipline.exp[Vocation::Fighter as usize] = 500;
    assert_eq!(c.step(t), (false, true));
    assert_eq!(c.discipline.rank[Vocation::Fighter as usize], 2);
    assert_eq!(learn::learn(t, &mut c, Learnable::Augment(6), none), Ok(700));
    assert_eq!(c.discipline.points, 3000 - 800 - 200 - 700);
    // Equip: augment slots and the sword's skill set.
    learn::equip_augment(&mut c, 0, Some(6)).unwrap();
    assert_eq!(learn::equip_augment(&mut c, 1, Some(6)), Err(Refused::Duplicate));
    assert_eq!(learn::equip_augment(&mut c, 1, Some(0)), Err(Refused::NotLearned));
    assert_eq!(c.augment(t, 6), Some(t.abilities[6].float_0));
    learn::equip_skill(t, &mut c, WeaponCategory::Sword, 0, Some(40)).unwrap();
    learn::equip_skill(t, &mut c, WeaponCategory::Sword, 1, Some(41)).unwrap();
    assert_eq!(learn::equip_skill(t, &mut c, WeaponCategory::Sword, 3, Some(40)), Err(Refused::WrongSlot));
    assert_eq!(learn::equip_skill(t, &mut c, WeaponCategory::Shield, 0, Some(40)), Err(Refused::WrongSlot));
    assert_eq!(learn::equip_skill(t, &mut c, WeaponCategory::Sword, 2, Some(45)), Err(Refused::NotLearned));
    let active = learn::active_skills(&c, WeaponCategory::Sword, Some(WeaponCategory::Shield));
    assert_eq!(active, [Some(40), Some(41), None, None, None, None]);
    // Vocation change to Mystic Knight at level 10 keeps the shared sword
    // skills and the augment, and empties the active slots.
    c.progress = Progress::at_level(10, t.growth(Vocation::Fighter), &t.exp);
    c.active = active;
    c.discipline.points = 1500;
    learn::equip_skill(t, &mut c, WeaponCategory::Sword, 2, Some(40)).unwrap();
    c.learned.level1.set(45, true);
    learn::equip_skill(t, &mut c, WeaponCategory::Sword, 2, Some(45)).unwrap();
    change::change(t, &mut c, Vocation::MysticKnight, false, false).unwrap();
    assert_eq!(c.discipline.points, 0);
    assert_eq!(c.active, [None; 6]);
    assert_eq!(&c.weapon_sets[WeaponCategory::Sword as usize][..3], [Some(40), Some(41), None]);
    assert_eq!(c.augments[0], Some(6));
}

#[test]
fn first_choice_and_pawn_limits() {
    let Some(t) = tables() else { return };
    let mut c = Character::new(t, Vocation::Fighter, false);
    let done = change::change(t, &mut c, Vocation::Strider, true, false).unwrap();
    assert_eq!((done.cost, done.items), (0, vec![0x195, 0x1f8, 0x227, 0x228]));
    assert_eq!(c.changes, 0);
    let mut pawn = Character::new(t, Vocation::Mage, true);
    pawn.progress = Progress::at_level(20, t.growth(Vocation::Mage), &t.exp);
    pawn.discipline.points = 5000;
    assert_eq!(change::check(&pawn, Vocation::Assassin, false), Err(ChangeRefused::NotOffered));
    assert_eq!(change::check(&pawn, Vocation::Sorcerer, false), Ok(1000));
}

#[test]
fn installed_first_vocation_grants_exact_original_slots_without_cross_class_unlocks() {
    let Some(t) = tables() else { return };
    use WeaponCategory::*;
    // Expectations are source-table observations at 0x015827e8 and the
    // 0x00674a60 writers; production reads the rows from the installation.
    for (vocation, primary, secondary, slots, ids) in [
        (Vocation::Fighter, Sword, Some(Shield), [Some(40), None, None, Some(272), None, None], vec![40, 272]),
        (Vocation::Strider, Dagger, Some(Bow), [Some(152), None, None, Some(350), None, None], vec![152, 350]),
        (Vocation::Mage, Staff, None, [Some(210), None, None, Some(220), None, None], vec![210, 220]),
    ] {
        let mut character = Character::unselected(t, false);
        assert!(character.learned.level1.words().iter().all(|word| *word == 0));
        assert!(character.weapon_sets.iter().flatten().all(Option::is_none));
        change::change(t, &mut character, vocation, true, false).unwrap();
        let learned: Vec<_> = (0..ddprogress::character::SKILL_IDS)
            .filter(|id| character.learned.level1.get(*id)).map(|id| id as u16).collect();
        assert_eq!(learned, ids, "{vocation:?}");
        assert!(character.learned.level2.words().iter().all(|word| *word == 0));
        assert!(character.learned.augments.words().iter().all(|word| *word == 0));
        assert_eq!(learn::active_skills(&character, primary, secondary), slots);
        assert_eq!(character.discipline.points, 0);
        assert_eq!(character.changes, 0);
        if vocation == Vocation::Fighter { assert_eq!(character.weapon_sets[Mace as usize][0], Some(40)); }
        if vocation == Vocation::Mage { assert_eq!(character.weapon_sets[Archistaff as usize][0], Some(210)); }
    }
}

#[test]
fn installed_free_skill_grants_preserve_custom_empty_slots_and_independent_characters() {
    let Some(t) = tables() else { return };
    let mut fighter = Character::new(t, Vocation::Fighter, false);
    let mut mage = Character::new(t, Vocation::Mage, false);
    fighter.weapon_sets[WeaponCategory::Sword as usize][0] = None;
    fighter.weapon_sets[WeaponCategory::Mace as usize][0] = None;
    fighter.weapon_sets[WeaponCategory::Shield as usize][0] = None;
    let before_mage = mage.clone();
    t.starting.grant(&mut fighter);
    assert_eq!(fighter.weapon_sets[WeaponCategory::Sword as usize][0], None);
    assert_eq!(fighter.weapon_sets[WeaponCategory::Shield as usize][0], None);
    assert_eq!(mage, before_mage);
    fighter.progress.level = 10;
    fighter.discipline.points = 5000;
    change::change(t, &mut fighter, Vocation::MysticKnight, false, false).unwrap();
    assert_eq!(fighter.learned.weapon_level(210), 1);
    assert_eq!(fighter.learned.weapon_level(310), 1);
    assert_eq!(fighter.learned.weapon_level(220), 0, "hybrid staff excludes Anodyne");
    assert_eq!(fighter.weapon_sets[WeaponCategory::Sword as usize][0], None);
    mage.progress.level = 10;
    mage.discipline.points = 5000;
    mage.weapon_sets[WeaponCategory::Staff as usize][0] = None;
    mage.weapon_sets[WeaponCategory::Archistaff as usize][0] = None;
    change::change(t, &mut mage, Vocation::Sorcerer, false, false).unwrap();
    assert_eq!(mage.weapon_sets[WeaponCategory::Archistaff as usize][0], None);
    assert_eq!(mage.weapon_sets[WeaponCategory::Archistaff as usize][3], Some(234));
    assert_eq!(mage.learned.weapon_level(234), 1);
    assert_eq!(fighter.learned.weapon_level(234), 0);
}

#[test]
fn installed_active_skill_refresh_preserves_primary_tail_under_empty_secondary_slots() {
    let Some(t) = tables() else { return };
    let mut character = Character::unselected(t, false);
    character.weapon_sets[WeaponCategory::Staff as usize] = [Some(210), Some(211), None, Some(220), Some(222), Some(223)];
    character.weapon_sets[WeaponCategory::Shield as usize] = [Some(272), None, Some(271), None, None, None];
    assert_eq!(learn::active_skills(&character, WeaponCategory::Staff, Some(WeaponCategory::Shield)),
        [Some(210), Some(211), None, Some(272), Some(222), Some(271)]);
    assert_eq!(learn::active_skills(&character, WeaponCategory::Unarmed, Some(WeaponCategory::Shield)),
        [None, None, None, Some(272), None, Some(271)]);
    assert_eq!(learn::active_skills(&character, WeaponCategory::Staff, Some(WeaponCategory::Unarmed)),
        character.weapon_sets[WeaponCategory::Staff as usize]);
}

/// Two players, the shared main pawn and a hired pawn share one award.
#[test]
fn two_players_share_with_pawns() {
    let Some(t) = tables() else { return };
    let mut p = Progression::new();
    let p1 = p.add(Character::new(t, Vocation::Fighter, false));
    let pawn = p.add(Character::new(t, Vocation::Mage, true));
    let hired = p.add(Character::new(t, Vocation::Strider, true));
    let p2 = p.add(Character::new(t, Vocation::Ranger, false));
    p.party = [
        Some(Member { character: p1, role: Role::Arisen(0) }),
        Some(Member { character: pawn, role: Role::MainPawn { owner: 0 } }),
        Some(Member { character: hired, role: Role::Support { owner: 1 } }),
        Some(Member { character: p2, role: Role::Arisen(1) }),
    ];
    raise(t, &mut p, p1, 5);
    raise(t, &mut p, p2, 3);
    p.characters[pawn].progress = Progress::at_level(2, t.growth(Vocation::Mage), &t.exp);
    p.characters[hired].progress = Progress::at_level(4, t.growth(Vocation::Strider), &t.exp);
    let levels = [Some(5), Some(2), Some(4), Some(3)];
    let totals: Vec<f32> = p.characters.iter().map(|c| c.total_exp(t)).collect();
    let before: Vec<u32> = p.characters.iter().map(|c| c.progress.exp).collect();
    let out = p.award(t, Award::new(1000));
    let share = |slot: usize| {
        let m = growth::party_share(levels, slot);
        let f = ((f64::from(m) * 1000.0 + 0.5) as i64) as f32;
        (f as i64, f)
    };
    // Both Arisen earn their share (the second one sits in slot 3).
    assert_eq!(out[0].unwrap().exp as i64, share(0).0);
    assert_eq!(out[3].unwrap().exp as i64, share(3).0);
    assert_eq!(p.characters[p1].progress.exp, before[p1] + share(0).0 as u32);
    assert_eq!(p.characters[p2].progress.exp, before[p2] + share(3).0 as u32);
    // The main pawn catches up toward player 0's Arisen.
    let catch = ((totals[p1] - totals[pawn]) * 0.7 * 0.01f32 + share(1).0 as f32) as u32;
    assert_eq!(out[1].unwrap().exp, catch);
    assert!(catch > share(1).0 as u32);
    // The hired pawn earns no experience, 10 % as Rift Crystals.
    let rc = (f64::from(share(2).1) * f64::from(0.1f32)) as i32;
    assert_eq!(out[2].unwrap(), ddprogress::party::Share { exp: 0, rift_crystals: rc });
    assert_eq!(p.characters[hired].progress.exp, before[hired]);
    assert_eq!(p.characters[hired].rift_crystals, rc);
    // Discipline follows each Arisen's experience.
    assert!(p.characters[p2].discipline.points > 0);
}

/// One player and the main pawn: the original's slot layout.
#[test]
fn single_player_matches_original_layout() {
    let Some(t) = tables() else { return };
    let mut p = Progression::new();
    let a = p.add(Character::new(t, Vocation::Mage, false));
    let pawn = p.add(Character::new(t, Vocation::Fighter, true));
    p.party[0] = Some(Member { character: a, role: Role::Arisen(0) });
    p.party[1] = Some(Member { character: pawn, role: Role::MainPawn { owner: 0 } });
    let out = p.award(t, Award { exp: 300, rate: 1.0, fixed: true, double: false });
    // Fixed awards skip the share: both get 300 (the pawn has the same total).
    assert_eq!((out[0].unwrap().exp, out[1].unwrap().exp), (300, 300));
    let mut world = ddstory::World::default();
    p.sync_to_world(&mut world);
    assert_eq!(world.player_job, Vocation::Mage as u32);
    assert!(world.gsf.get(542) && world.gsf.get(549));
    assert!(!world.gsf.get(540));
    world.gsf.set(543, true);
    p.sync_from_world(&world);
    assert!(p.characters[a].paid[Vocation::Warrior as usize]);
}

#[test]
fn save_parts_round_trip_and_seat_change() {
    let Some(t) = tables() else { return };
    let mut p = Progression::new();
    let a0 = p.add(Character::new(t, Vocation::Mage, false));
    let pawn = p.add(Character::new(t, Vocation::Fighter, true));
    let a1 = p.add(Character::new(t, Vocation::Strider, false));
    p.party = [
        Some(Member { character: a0, role: Role::Arisen(0) }),
        Some(Member { character: pawn, role: Role::MainPawn { owner: 0 } }),
        None,
        Some(Member { character: a1, role: Role::Arisen(1) }),
    ];
    p.award(t, Award::new(3000));
    p.step(t);
    p.characters[a1].augments[2] = Some(0x5d);
    p.characters[pawn].rift_crystals = 12;
    let parts = p.save();
    assert_eq!(parts.players.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
    let same = Progression::load(&parts, &|s| Some(s)).unwrap();
    assert_eq!(same, p);
    // Next session the guest sits on seat 1 again but the host seat moved
    // nowhere; with the guest absent its Arisen and slot are left out.
    let mut alone = parts.clone();
    alone.players.remove(&1);
    let q = Progression::load(&alone, &|s| (s == 0).then_some(0)).unwrap();
    assert_eq!(q.characters.len(), 2);
    assert_eq!(q.party[3], None);
    assert_eq!(q.characters[0], p.characters[a0]);
    // Seats swapped: the old seat 1 is now seat 0 and the other way round.
    let mut swapped = parts.clone();
    swapped.players = [(0, parts.players[&1].clone()), (1, parts.players[&0].clone())].into();
    let s = Progression::load(&swapped, &|x| Some(1 - x)).unwrap();
    assert_eq!(s.party[0].unwrap().role, Role::Arisen(1));
    assert_eq!(s.party[3].unwrap().role, Role::Arisen(0));
    assert_eq!(s.characters[s.party[3].unwrap().character], p.characters[a1]);
}
