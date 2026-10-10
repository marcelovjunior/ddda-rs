//! Authoritative recovery, save/load and native NPC reward regressions.
use ddeconomy::inventory::Owner;
use ddhost::data::NoStoryData;
use ddhost::items::{ItemData, ItemRequest};
use ddhost::opening::OpeningWorld;
use ddhost::{Control, Host, ProfileId};
use std::sync::Arc;

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

fn install() -> Option<(std::path::PathBuf, ddvfs::Vfs)> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install absent");
        return None;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    Some((native, vfs))
}
fn host(
    data: Arc<ItemData>,
    fight: Arc<ddhost::ddenemyrt::fight::FightData>,
    swapped: bool,
) -> Host<NoStoryData, OpeningWorld> {
    let mut h = Host::new(
        ddsim::Sim::new(Default::default()),
        None,
        OpeningWorld::new(),
        Box::new(Void),
    );
    for profile in if swapped { [2, 1] } else { [1, 2] } {
        h.join(ProfileId(profile), Control::Local, [0.0; 3], 0.0).unwrap();
    }
    let mut f = ddhost::ddenemyrt::fight::Fight::new(fight);
    for seat in h.roster.ids() {
        f.add_arisen(seat);
    }
    h.fight = Some(f);
    h.new_game_items(data, ddeconomy::new_game::Phase::NewlyArisen);
    h
}
fn hurt(h: &mut Host<NoStoryData, OpeningWorld>, seat: u8) {
    if let ddhost::ddenemyrt::combat::Victim::Player { hp, hp_max, .. } = &mut h
        .fight
        .as_mut()
        .unwrap()
        .arisen
        .get_mut(&seat)
        .unwrap()
        .fighter
        .victim
    {
        *hp = 20.0;
        *hp_max = 1000.0;
    }
}

#[test]
fn installed_reynard_reward_checkpoint_restores_real_bag_and_host_quick_use() {
    let Some((native, vfs)) = install() else {
        return;
    };
    let data = Arc::new(ItemData::load(&vfs).unwrap());
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let fight = Arc::new(ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    let mut a = host(data.clone(), fight.clone(), false);
    let mut story = ddstory::Story::new_game(NoStoryData);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1410, false);
    // Explicit rescue prerequisites, not a fabricated route result.
    story.world.set_quest_bit(2, 95, true);
    story.world.set_quest_bit(2, 20, true);
    a.story = Some(story);
    a.data.npc = Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap()));
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let world = ddspawn::World {
        scene: 1410,
        hour: 12,
        flag: &|f| f == 3,
    };
    let placement = layouts
        .candidates(&world)
        .placements
        .into_iter()
        .find(|p| p.npc == 304 && p.target.group == 4 && p.target.index == 0)
        .unwrap();
    a.load_story_npc_intel(placement).unwrap();
    let npc = a.story_npc_lifetime(304).unwrap();
    let motion = ddnpcrt::guard::TalkMotionObservation {
        mode: Some(3),
        ..Default::default()
    };
    a.talk_story_npc(0, npc, true, motion).unwrap().unwrap();
    let mut ended = false;
    for _ in 0..50 {
        let out = a.talk_next(0).unwrap();
        if out
            .events
            .iter()
            .any(|e| matches!(e, ddnpcrt::TalkEvent::End { .. }))
        {
            ended = true;
            break;
        }
    }
    assert!(ended);
    a.tick(&Default::default()).unwrap();
    assert_eq!(a.economy.inventory.bag(Owner::Arisen(0)).unwrap().count(1058), 1);
    assert_eq!(a.economy.inventory.bag(Owner::Arisen(1)).unwrap().count(1058), 0);
    // Recovery fixture uses the installed q1 reward pairs, not guessed item
    // definitions. Native Reynard's 1058 is equipment, not the curative.
    let story_data = ddstory::data::InstallData::open(&native, None).unwrap();
    for request in ddstory::orders::reward(&story_data, 1).unwrap() {
        if let ddstory::Request::Reward { quest, items, .. } = request {
            for seat in a.roster.ids() {
                a.economy
                    .opening_grants
                    .reward(&mut a.economy.inventory, &data.items, seat, quest, &items);
            }
        }
    }
    let save = a.save().unwrap();
    let hash = a.saved_hash();
    let mut b = host(data.clone(), fight.clone(), false);
    b.load(&save, NoStoryData).unwrap();
    assert_eq!(b.saved_hash(), hash);
    assert_eq!(b.economy.inventory.bag(Owner::Arisen(0)).unwrap().count(1058), 1);
    let other = b.economy.inventory.bag(Owner::Arisen(1)).unwrap().clone();
    hurt(&mut b, 0);
    assert_eq!(b.item_request(0, ItemRequest::QuickUse).unwrap(), 43);
    assert!(b.fight.as_ref().unwrap().arisen[&0].fighter.victim.hp() > 20.0);
    assert_eq!(b.economy.inventory.bag(Owner::Arisen(0)).unwrap().count(43), 4);
    assert_eq!(b.economy.inventory.bag(Owner::Arisen(1)).unwrap(), &other);
    let mut remapped = host(data, fight, true);
    remapped.load(&save, NoStoryData).unwrap();
    assert_eq!(
        remapped
            .economy
            .inventory
            .bag(Owner::Arisen(1))
            .unwrap()
            .count(1058),
        1
    );
    assert_eq!(
        remapped
            .economy
            .inventory
            .bag(Owner::Arisen(0))
            .unwrap()
            .count(1058),
        0
    );
}

#[test]
fn installed_item_requests_check_owner_expected_slot_and_failed_use_spends_nothing() {
    let Some((native, vfs)) = install() else {
        return;
    };
    let data = Arc::new(ItemData::load(&vfs).unwrap());
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let fight = Arc::new(ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    let mut h = host(data.clone(), fight, false);
    h.economy
        .inventory
        .add(&data.items, Owner::Arisen(0), 0, 43, 2, 0, &mut h.economy.buyback);
    let index = h
        .economy
        .inventory
        .bag(Owner::Arisen(0))
        .unwrap()
        .slots
        .iter()
        .position(|s| s.item == 43)
        .unwrap();
    let expected = h.economy.inventory.bag(Owner::Arisen(0)).unwrap().slots[index];
    let request = ItemRequest::Use {
        owner: Owner::Arisen(0),
        index,
        expected,
    };
    let before = h.economy.clone();
    assert!(h.item_request(1, request).is_err());
    assert!(h.item_request(9, request).is_err());
    assert!(
        h.item_request(0, request).is_err(),
        "full HP must not spend a curative"
    );
    assert_eq!(h.economy, before);
    hurt(&mut h, 0);
    assert_eq!(h.item_request(0, request).unwrap(), 43);
    let after = h.economy.clone();
    assert!(h.item_request(0, request).is_err());
    assert_eq!(h.economy, after);
    let mut bad = h.save().unwrap();
    let mut host_bytes = bad
        .get(ddhost::save::TAG_HOST, ddhost::save::Owner::Shared)
        .unwrap()
        .to_vec();
    host_bytes[1..9].copy_from_slice(&12345u64.to_le_bytes());
    bad.put(ddhost::save::TAG_HOST, ddhost::save::Owner::Shared, host_bytes);
    let mut economy = bad
        .get(ddhost::save::TAG_ECON, ddhost::save::Owner::Profile(ProfileId(1)))
        .unwrap()
        .to_vec();
    economy.pop();
    bad.put(
        ddhost::save::TAG_ECON,
        ddhost::save::Owner::Profile(ProfileId(1)),
        economy,
    );
    let before = h.saved_hash();
    assert!(h.load(&bad, NoStoryData).is_err());
    assert_eq!(h.saved_hash(), before);
}
