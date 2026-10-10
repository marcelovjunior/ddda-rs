//! Installed FSM/Press clocks and host contact feedback on synthetic terrain.
//! This does not assert the whole fire encounter or an original comparison.
use ddhost::{Control, Host, ProfileId};
use ddenemyrt::dragon::attacks::PressPhase;
use std::sync::Arc;

struct Plane;
impl ddsim::Ground for Plane {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= 0.0).then_some(0.0) }
    fn triangles_in(&self, _: [f32; 3], _: [f32; 3], out: &mut Vec<ddsim::sweep::Face>) -> bool {
        out.push(([[-20000.0, 0.0, -20000.0], [-20000.0, 0.0, 20000.0], [20000.0, 0.0, -20000.0]], 0));
        out.push(([[20000.0, 0.0, -20000.0], [-20000.0, 0.0, 20000.0], [20000.0, 0.0, 20000.0]], 0));
        true
    }
}

#[test]
fn installed_entrance_continues_to_press_using_prior_host_terrain_and_gsf() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let dragon_data = ddhost::dragon::OpeningDragonData::load(&vfs, &image).unwrap();
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.set_sce_no(1025, true);
    story.world.set_lot(200, 0, true);
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story),
        ddhost::opening::OpeningWorld::new(), Box::new(Plane));
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    host.fight = Some(ddenemyrt::fight::Fight::new(Arc::new(ddenemyrt::fight::FightData::load(&vfs, &image).unwrap())));
    host.set_opening_dragon_data(dragon_data);
    let mut entered_press = false;
    let mut landed = false;
    let mut landing_gsf = false;
    let mut completed = false;
    let mut states = vec![];
    for _ in 0..1800 {
        host.tick(&Default::default()).unwrap();
        let dragon = host.fight.as_mut().unwrap().dragon.as_mut().unwrap();
        let current = dragon.thinker.current_states();
        if states.last() != current.first() { states.extend(current); }
        if dragon.entrance_complete() {
            // Explicitly continue this regression past the production gate.
            dragon.entrance_only = false;
        }
        if dragon.actor.current_action() != Some(33) { continue; }
        entered_press = true;
        let phase = dragon.actor.press.as_ref().unwrap().phase;
        if phase == PressPhase::Landed {
            landed = true;
            landing_gsf |= host.story.as_ref().unwrap().world.gsf.get(463);
            assert!(dragon.scenery_result.is_some());
            assert!(dragon.actor.pos.iter().all(|v| v.is_finite()));
            if dragon.actor.done() { completed = true; break; }
        }
    }
    assert_eq!(states, [4, 5, 1, 3, 0]);
    assert!(entered_press && landed && landing_gsf && completed,
        "entered={entered_press}, landed={landed}, gsf={landing_gsf}, completed={completed}");
    assert!(!host.story.as_ref().unwrap().world.gsf.get(463));
    assert!(!host.story.as_ref().unwrap().world.quest_bit(0, 20), "fire was not executed");
    let dragon = host.fight.as_ref().unwrap().dragon.as_ref().unwrap();
    assert_eq!(dragon.actor.motion(), Some(ddenemyrt::body::MotionId::new(1, 43)));
    assert!(dragon.actor.motion_ended);
}
