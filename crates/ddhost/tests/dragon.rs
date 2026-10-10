//! Installed entrance: original graph and motion clocks, no fake done bits.
use ddhost::{Control, Host, ProfileId};
use std::{path::PathBuf, sync::Arc};

struct Ground;
impl ddsim::Ground for Ground {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

#[test]
fn opening_dragon_is_owned_and_flies_to_hover_from_real_feedback() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let data = ddhost::dragon::OpeningDragonData::load(&vfs, &exe).unwrap();
    let initial = data.placement.position;
    let body = data.data.clone();
    let fight = ddhost::ddenemyrt::fight::Fight::new(Arc::new(
        ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe).unwrap(),
    ));
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.set_sce_no(1025, true);
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        ddhost::opening::OpeningWorld::new(),
        Box::new(Ground),
    );
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    host.join(ProfileId(2), Control::Local, [500.0, 0.0, 0.0], 0.0)
        .unwrap();
    host.fight = Some(fight);
    host.set_opening_dragon_data(data);
    host.tick(&Default::default()).unwrap();
    assert!(
        host.fight.as_ref().unwrap().dragon.is_none(),
        "closed authored LOT gate"
    );
    host.story.as_mut().unwrap().world.set_lot(200, 0, true);
    let mut visited = vec![];
    let mut owner = None;
    let mut saw_hurt_volume = false;
    for _ in 0..1500 {
        host.tick(&Default::default()).unwrap();
        let dragon = host
            .fight
            .as_ref()
            .unwrap()
            .dragon
            .as_ref()
            .expect("admitted dragon");
        let states = dragon.thinker.current_states();
        if visited.last() != states.first() {
            visited.extend(states);
        }
        assert_eq!(
            *owner.get_or_insert(dragon.actor.fighter.owner),
            dragon.actor.fighter.owner
        );
        assert!(dragon.actor.pos.iter().all(|v| v.is_finite()));
        saw_hurt_volume |= !dragon.actor.fighter.hurts.is_empty();
        if dragon.entrance_complete() {
            break;
        }
    }
    let fight = host.fight.as_ref().unwrap();
    let dragon = fight.dragon.as_ref().unwrap();
    assert_eq!(visited, [4, 5, 1, 3]);
    assert!(
        dragon.entrance_complete(),
        "stalled {:?} pos {:?} target {:?} flight {:?}",
        dragon.thinker.current_states(),
        dragon.actor.pos,
        dragon.target,
        dragon.actor.flight
    );
    assert_ne!(dragon.actor.pos, initial);
    assert_eq!(dragon.actor.motion().unwrap().slot, 121);
    assert!(dragon.actor.motion_ended);
    assert!(host.story.as_ref().unwrap().world.quest_bit(0, 1));
    assert!(!host.story.as_ref().unwrap().world.quest_bit(0, 20));
    // Volume activation belongs to the final posed pass-C boundary.
    let pose = dragon.actor.pose(&body).unwrap();
    let activation = body
        .body
        .activations(dragon.actor.motion().unwrap(), dragon.actor.frame);
    let (_, hurts) = body.body.volumes(&activation, &pose, dragon.actor.fighter.owner);
    assert_eq!(dragon.actor.fighter.hurts.len(), hurts.len());
    assert!(
        saw_hurt_volume,
        "installed rise publishes its authored hurt volumes"
    );
    let position = dragon.actor.pos;
    for _ in 0..5 {
        host.tick(&Default::default()).unwrap();
    }
    assert_eq!(
        host.fight
            .as_ref()
            .unwrap()
            .dragon
            .as_ref()
            .unwrap()
            .thinker
            .current_states(),
        [3]
    );
    assert_eq!(
        host.fight.as_ref().unwrap().dragon.as_ref().unwrap().actor.pos,
        position
    );
}
