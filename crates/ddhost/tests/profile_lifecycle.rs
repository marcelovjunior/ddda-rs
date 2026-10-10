//! Profile/seat lifecycle is project co-op policy, tested without UI.
use ddeconomy::inventory::{Owner, Slot};
use ddhost::data::NoStoryData;
use ddhost::{Control, Host, ProfileId};
use ddstory::scripted::ScriptedHost;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}
type Session = Host<NoStoryData, ScriptedHost>;
fn host() -> Session {
    Host::new(
        ddsim::Sim::new(Default::default()),
        None,
        ScriptedHost::default(),
        Box::new(Flat),
    )
}
fn join(host: &mut Session, profile: u64) -> u8 {
    host.join(
        ProfileId(profile),
        Control::Local,
        [profile as f32, 0.0, 0.0],
        0.5,
    )
    .unwrap()
}
fn bag(host: &mut Session, seat: u8, item: i16, num: i16) {
    host.economy
        .inventory
        .bag_mut(Owner::Arisen(seat))
        .slots
        .push(Slot { item, num, flags: 1 });
}

#[test]
fn recycled_seat_and_new_seat_restore_only_the_matching_profile() {
    let mut h = host();
    let a = join(&mut h, 11);
    h.economy.wallet(a).gold = 1234;
    bag(&mut h, a, 43, 5);
    h.economy.inventory.bag_mut(Owner::Storage(a)).slots.push(Slot {
        item: 44,
        num: 2,
        flags: 1,
    });
    h.leave(a);
    assert!(h.economy.wallets.is_empty());
    assert!(h.economy.inventory.bags.is_empty());
    let b = join(&mut h, 22);
    assert_eq!(b, a);
    assert_eq!(h.economy.wallets[&b].gold, 0);
    assert!(h.economy.inventory.bag(Owner::Arisen(b)).is_none());
    h.economy.wallet(b).gold = 987;
    bag(&mut h, b, 45, 3);
    let a_new = join(&mut h, 11);
    assert_ne!(a_new, a);
    assert_eq!(h.economy.wallets[&a_new].gold, 1234);
    assert_eq!(
        h.economy.inventory.bag(Owner::Arisen(a_new)).unwrap().count(43),
        5
    );
    assert_eq!(
        h.economy.inventory.bag(Owner::Storage(a_new)).unwrap().count(44),
        2
    );
    assert_eq!(h.economy.wallets[&b].gold, 987);
    assert_eq!(h.economy.inventory.bag(Owner::Arisen(b)).unwrap().count(45), 3);
    let before = h.save().unwrap().to_bytes();
    assert!(h.join(ProfileId(11), Control::Local, [0.0; 3], 0.0).is_none());
    assert_eq!(h.save().unwrap().to_bytes(), before);
}

#[test]
fn parked_profile_survives_save_load_without_becoming_the_current_occupant() {
    let mut h = host();
    let a = join(&mut h, 11);
    h.economy.wallet(a).gold = 1234;
    bag(&mut h, a, 43, 5);
    h.sim.player_mut(a).unwrap().position = [456.0, 0.0, 78.0];
    h.leave(a);
    let b = join(&mut h, 22);
    h.economy.wallet(b).gold = 987;
    let bytes = h.save().unwrap().to_bytes();
    let file = ddhost::save::SaveFile::from_bytes(&bytes).unwrap();
    assert_eq!(
        ddhost::save::place_from(
            file.get(
                ddhost::save::TAG_PLACE,
                ddhost::save::Owner::Profile(ProfileId(11))
            )
            .unwrap()
        )
        .unwrap()
        .0,
        [456.0, 0.0, 78.0]
    );
    let mut restored = host();
    let rb = join(&mut restored, 22);
    restored.load(&file, NoStoryData).unwrap();
    assert_eq!(restored.saved_hash(), h.saved_hash());
    assert_eq!(restored.save().unwrap().to_bytes(), bytes);
    assert_eq!(restored.economy.wallets[&rb].gold, 987);
    let ra = join(&mut restored, 11);
    assert_eq!(restored.economy.wallets[&ra].gold, 1234);
    assert_eq!(
        restored
            .economy
            .inventory
            .bag(Owner::Arisen(ra))
            .unwrap()
            .count(43),
        5
    );
    // A may also already be seated while its saved PROG order is absent.
    let mut swapped = host();
    let sa = join(&mut swapped, 11);
    swapped.load(&file, NoStoryData).unwrap();
    assert_eq!(swapped.economy.wallets[&sa].gold, 1234);
    assert_eq!(swapped.sim.player(sa).unwrap().position, [456.0, 0.0, 78.0]);
}

#[test]
fn malformed_unseated_profile_is_rejected_before_state_changes() {
    let mut h = host();
    let a = join(&mut h, 11);
    bag(&mut h, a, 43, 5);
    h.leave(a);
    join(&mut h, 22);
    let before = h.save().unwrap().to_bytes();
    for (tag, bytes) in [
        (ddhost::save::TAG_ECON, vec![2]),
        (ddhost::save::TAG_PROG, vec![1]),
        (ddhost::save::TAG_PLACE, vec![0]),
        (
            ddhost::save::TAG_PLACE,
            ddhost::save::place_bytes([f32::NAN, 0.0, 0.0], 0.0),
        ),
    ] {
        let mut file = h.save().unwrap();
        file.put(tag, ddhost::save::Owner::Profile(ProfileId(11)), bytes);
        assert!(h.load(&file, NoStoryData).is_err());
        assert_eq!(h.save().unwrap().to_bytes(), before);
    }
}

#[test]
fn host_v1_roster_and_hirer_seats_migrate_to_profiles() {
    let mut h = host();
    join(&mut h, 11);
    join(&mut h, 22);
    let mut file = h.save().unwrap();
    let mut legacy = ddid::Writer::new();
    legacy
        .u8(1)
        .u64(7)
        .len(2)
        .u8(0)
        .u64(11)
        .u8(1)
        .u64(22)
        .bool(true)
        .u8(1)
        .bool(false)
        .u8(0);
    file.put(
        ddhost::save::TAG_HOST,
        ddhost::save::Owner::Shared,
        legacy.finish(),
    );
    let mut restored = host();
    join(&mut restored, 22);
    join(&mut restored, 11);
    restored.load(&file, NoStoryData).unwrap();
    assert_eq!(restored.frame, 7);
    let upgraded = restored.save().unwrap();
    let mut reader = ddid::Reader::new(
        upgraded
            .get(ddhost::save::TAG_HOST, ddhost::save::Owner::Shared)
            .unwrap(),
    );
    assert_eq!(reader.u8().unwrap(), 2);
    assert_eq!(reader.u64().unwrap(), 7);
    assert_eq!(reader.len(9).unwrap(), 2);
    for _ in 0..2 {
        reader.u8().unwrap();
        reader.u64().unwrap();
    }
    assert!(reader.bool().unwrap());
    assert_eq!(
        reader.u64().unwrap(),
        22,
        "hirer is profile22, never its old seat1"
    );
}

#[test]
fn partial_and_unknown_profile_chunks_do_not_replace_a_fresh_bag_with_empty_state() {
    let mut h = host();
    join(&mut h, 22);
    let mut file = h.save().unwrap();
    file.put(
        ddhost::save::TAG_PLACE,
        ddhost::save::Owner::Profile(ProfileId(11)),
        ddhost::save::place_bytes([456.0, 0.0, 78.0], 0.0),
    );
    file.put(*b"XTRA", ddhost::save::Owner::Profile(ProfileId(33)), vec![7]);
    h.load(&file, NoStoryData).unwrap();
    assert!(h
        .save()
        .unwrap()
        .get(
            ddhost::save::TAG_ECON,
            ddhost::save::Owner::Profile(ProfileId(33))
        )
        .is_none());
    // Adapter-created/imported fresh state must survive missing ECON.
    bag(&mut h, 1, 43, 5);
    let partial = join(&mut h, 11);
    assert_eq!(
        h.economy.inventory.bag(Owner::Arisen(partial)).unwrap().count(43),
        5
    );
    h.leave(partial);
    bag(&mut h, 1, 44, 2);
    let unknown = join(&mut h, 33);
    assert_eq!(
        h.economy.inventory.bag(Owner::Arisen(unknown)).unwrap().count(44),
        2
    );
}

#[test]
fn installed_progress_delivery_fighter_and_hirer_follow_profile_after_leave_and_save() {
    use ddhost::ddenemyrt::combat::Victim;
    use ddhost::ddenemyrt::fight::{Fight, FightData, Kit};
    use ddskill::vocation::Vocation;
    use std::sync::Arc;
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let tables = Arc::new(ddhost::install::tables(&vfs, &exe.path).unwrap());
    let items = Arc::new(ddhost::items::ItemData::load(&vfs).unwrap());
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    let mut h = host();
    h.data.tables = Some(tables.clone());
    let a = join(&mut h, 11);
    let character = h.create_arisen(a, Vocation::Mage).unwrap();
    let mut story = ddstory::Story::new_game(NoStoryData);
    story.world.player_job = Vocation::Mage as u32;
    h.story = Some(story);
    h.progression.characters[character].discipline.points = 912;
    h.create_main_pawn(Vocation::Fighter).unwrap();
    h.new_game_items(items.clone(), ddeconomy::new_game::Phase::NewlyArisen);
    h.economy
        .opening_grants
        .reward(&mut h.economy.inventory, &items.items, a, 1, &[(1058, 1)]);
    let mut fight = Fight::new(data.clone());
    fight.add_arisen_as(a, Kit::Mage);
    if let Victim::Player { hp, .. } = &mut fight.arisen.get_mut(&a).unwrap().fighter.victim {
        *hp = 37.0;
    }
    fight.shooters.insert(1001, u32::from(a));
    h.fight = Some(fight);
    let enemy = h.fight.as_mut().unwrap().add_goblin([100.0, 0.0, 100.0], 0.0);
    let history = &mut h
        .fight
        .as_mut()
        .unwrap()
        .goblins
        .iter_mut()
        .find(|g| g.fighter.owner == enemy)
        .unwrap()
        .fighter
        .history;
    for attacker in [u32::from(a), 1001, 999] {
        let entry = ddcombat::volumes::HistoryEntry {
            attacker,
            key: 1,
            relation: 0,
            timer: None,
        };
        history.entries.push(entry.clone());
        history.frame.push(entry);
    }
    let mut pawn = ddpawn::cmc::Cmc::initial();
    pawn.param.level = 1;
    pawn.rom_id = 0x1000_0007;
    let mut row = ddrift::Row {
        flags: 0,
        pawn,
        online: ddrift::PawnRef::rom(0x1000_0007),
    };
    h.rift_enter(a).unwrap();
    let room = h.rift_summon(a, &mut row).unwrap();
    assert_eq!(h.rift_hire(a, room).unwrap(), 1);
    h.rift_leave(a);
    let pawn_seat = Session::hired_pawn_seat(0);
    h.fight.as_mut().unwrap().add_pawn_as(
        &mut h.sim,
        pawn_seat,
        a,
        [10.0, 0.0, 20.0],
        0.0,
        [500.0; 9],
        17,
        Kit::Fighter,
    );
    h.sim.player_mut(pawn_seat).unwrap().stamina = 321.0;
    if let Victim::Player { hp, .. } = &mut h
        .fight
        .as_mut()
        .unwrap()
        .arisen
        .get_mut(&pawn_seat)
        .unwrap()
        .fighter
        .victim
    {
        *hp = 23.0;
    }
    // Leave also clears contact ownership and detaches the old combat actor.
    h.leave(a);
    assert!(!h.fight.as_ref().unwrap().arisen.contains_key(&a));
    assert!(!h.fight.as_ref().unwrap().shooters.contains_key(&1001));
    assert!(!h.fight.as_ref().unwrap().pawns.contains_key(&pawn_seat));
    assert!(h.sim.player(pawn_seat).is_none());
    let history = &h
        .fight
        .as_ref()
        .unwrap()
        .goblins
        .iter()
        .find(|g| g.fighter.owner == enemy)
        .unwrap()
        .fighter
        .history;
    assert_eq!(
        history.entries.iter().map(|e| e.attacker).collect::<Vec<_>>(),
        vec![999]
    );
    assert_eq!(
        history.frame.iter().map(|e| e.attacker).collect::<Vec<_>>(),
        vec![999]
    );
    let b = join(&mut h, 22);
    assert_eq!(a, b);
    assert_eq!(h.fight.as_ref().unwrap().arisen[&b].fighter.owner, u32::from(b));
    assert_ne!(h.fight.as_ref().unwrap().arisen[&b].fighter.victim.hp(), 37.0);
    let fresh = h
        .progression
        .party
        .iter()
        .flatten()
        .find(|m| m.role == ddprogress::Role::Arisen(b))
        .unwrap()
        .character;
    assert_eq!(h.progression.characters[fresh].vocation, Vocation::Mage);
    assert_eq!(h.progression.characters[fresh].progress.level, 1);
    assert_eq!(h.progression.characters[fresh].discipline.points, 0);
    assert!(h
        .party
        .slots
        .iter()
        .all(|m| !matches!(m, Some(ddhost::Member::Hired { .. }))));
    assert_eq!(h.economy.inventory.bag(Owner::Arisen(b)).unwrap().count(1058), 0);
    let saved = h.save().unwrap();
    let a2 = join(&mut h, 11);
    let character = h
        .progression
        .party
        .iter()
        .flatten()
        .find(|m| m.role == ddprogress::Role::Arisen(a2))
        .unwrap()
        .character;
    assert_eq!(h.progression.characters[character].discipline.points, 912);
    assert_eq!(h.fight.as_ref().unwrap().arisen[&a2].fighter.owner, u32::from(a2));
    assert_eq!(h.fight.as_ref().unwrap().arisen[&a2].fighter.victim.hp(), 37.0);
    assert_eq!(h.fight.as_ref().unwrap().pawns[&pawn_seat].owner, a2);
    assert_eq!(h.fight.as_ref().unwrap().pawns[&pawn_seat].brain.rng, 17);
    assert_eq!(
        h.fight.as_ref().unwrap().arisen[&pawn_seat].fighter.victim.hp(),
        23.0
    );
    assert_eq!(h.sim.player(pawn_seat).unwrap().stamina, 321.0);
    assert_eq!(
        h.party.slots[2],
        Some(ddhost::Member::Hired { owner: a2, slot: 1 })
    );
    h.economy
        .opening_grants
        .reward(&mut h.economy.inventory, &items.items, a2, 1, &[(1058, 1)]);
    assert_eq!(
        h.economy.inventory.bag(Owner::Arisen(a2)).unwrap().count(1058),
        1,
        "delivery protection follows A"
    );
    // Swapped-seat load and then another departure must also repair active
    // progression slots before a new profile is admitted.
    h.progression.characters[fresh].discipline.points = 135;
    let both = h.save().unwrap();
    let mut swapped = host();
    swapped.data.tables = Some(tables.clone());
    swapped.item_data = Some(items.clone());
    swapped.fight = Some(Fight::new(data.clone()));
    let sa = join(&mut swapped, 11);
    let sb = join(&mut swapped, 22);
    swapped.load(&both, NoStoryData).unwrap();
    assert_eq!(
        swapped.progression.party[0].unwrap().role,
        ddprogress::Role::Arisen(sa)
    );
    assert_eq!(
        swapped.progression.party[3].unwrap().role,
        ddprogress::Role::Arisen(sb)
    );
    swapped.leave(sb);
    let sc = join(&mut swapped, 33);
    assert_eq!(sc, sb);
    assert_eq!(
        swapped.progression.party[0].unwrap().role,
        ddprogress::Role::Arisen(sa)
    );
    assert_eq!(
        swapped.progression.characters[swapped.progression.party[0].unwrap().character]
            .discipline
            .points,
        912
    );
    assert_eq!(
        swapped.progression.characters[swapped.progression.party[3].unwrap().character]
            .discipline
            .points,
        0
    );
    // VITL keeps saved actor health. Legacy checkpoints without that chunk
    // retain the original installed/progression reconstruction contract.
    for (a_seated_first, legacy) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut checkpoint = saved.clone();
        if legacy {
            checkpoint.chunks.retain(|chunk| chunk.tag != ddhost::save::TAG_VITALS);
        }
        let mut restored = host();
        restored.data.tables = Some(tables.clone());
        restored.item_data = Some(items.clone());
        restored.fight = Some(Fight::new(data.clone()));
        let first = join(&mut restored, if a_seated_first { 11 } else { 22 });
        restored.load(&checkpoint, NoStoryData).unwrap();
        let a3 = if a_seated_first {
            first
        } else {
            join(&mut restored, 11)
        };
        let c = restored
            .progression
            .party
            .iter()
            .flatten()
            .find(|m| m.role == ddprogress::Role::Arisen(a3))
            .unwrap()
            .character;
        assert_eq!(restored.progression.characters[c].vocation, Vocation::Mage);
        assert_eq!(restored.progression.characters[c].discipline.points, 912);
        assert_eq!(restored.fight.as_ref().unwrap().arisen[&a3].kit, Kit::Mage);
        assert_eq!(
            restored.fight.as_ref().unwrap().arisen[&a3].fighter.owner,
            u32::from(a3)
        );
        assert_eq!(
            restored.fight.as_ref().unwrap().arisen[&a3].fighter.victim.hp(),
            if legacy {
                restored.progression.characters[c].progress.stats.hp_max
            } else {
                ddhost::vitals::Vitals::from_bytes(saved.get(ddhost::save::TAG_VITALS,
                    ddhost::save::Owner::Profile(ProfileId(11))).unwrap()).unwrap().hp
            }
        );
        assert_eq!(
            restored.party.slots[2],
            Some(ddhost::Member::Hired { owner: a3, slot: 1 })
        );
        restored.economy.opening_grants.reward(
            &mut restored.economy.inventory,
            &items.items,
            a3,
            1,
            &[(1058, 1)],
        );
        assert_eq!(
            restored
                .economy
                .inventory
                .bag(Owner::Arisen(a3))
                .unwrap()
                .count(1058),
            1
        );
    }
    // The old Rift-release adapter may leave a body after dismissal. It
    // must still lose its stale seat owner before the next occupant joins.
    assert!(h.rift_release(1).is_some());
    h.leave(a2);
    assert!(!h.fight.as_ref().unwrap().pawns.contains_key(&pawn_seat));
    assert!(h.sim.player(pawn_seat).is_none());
}
