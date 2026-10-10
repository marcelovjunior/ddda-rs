//! Installed opening script restoration, independent of game/player saves.
use ddhost::opening::{OpeningWorld, AWAY};
use ddhost::{Control, Host, ProfileId};
use ddsim::{Ground, Sim};
use ddstory::{data::InstallData, HostEvent, Story};

struct Void;
impl Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}
fn install() -> Option<InstallData> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe".into());
    if !np.is_dir() || !exe.is_file() {
        eprintln!("SKIP: DDDA install absent");
        return None;
    }
    Some(InstallData::open(np, Some(&exe)).unwrap())
}
fn host(data: InstallData, seats: usize) -> Host<InstallData, OpeningWorld> {
    let mut h = Host::new(
        Sim::new(Default::default()),
        Some(Story::new_game(data)),
        OpeningWorld::new(),
        Box::new(Void),
    );
    for p in 0..seats {
        h.join(ProfileId(p as u64 + 1), Control::Local, AWAY, 0.0)
            .unwrap();
    }
    h
}
fn park(h: &mut Host<InstallData, OpeningWorld>) {
    for p in h.roster.ids().collect::<Vec<_>>() {
        h.sim.player_mut(p).unwrap().position = AWAY;
    }
}

#[test]
fn installed_opening_q0002_restores_quest_owners_and_continues_its_rescue_camera() {
    let Some(data) = install() else { return };
    let mut first_data = Some(data);
    for seats in [1, 2] {
        let mut a = host(first_data.take().unwrap_or_else(|| install().unwrap()), seats);
        // Existing opening fixture supplies presentation/prerequisite observations
        // until q0002. It does not claim the full real dragon route acceptance.
        ddhost::opening::fast_forward(&mut a, &ddstory::route::opening(), 240, 40_000, |h| {
            h.story.as_ref().unwrap().quest(2).is_some()
        })
        .unwrap();
        a.world.report(HostEvent::StageLoaded { stage: 100 });
        for _ in 0..60 {
            park(&mut a);
            a.tick(&Default::default()).unwrap();
        }
        let s = a.story.as_ref().unwrap();
        assert_eq!(s.stage.stage_no, 100);
        assert!(s.world.quest_bit(2, 95));
        assert!(!s.world.quest_bit(2, 5));
        let old: Vec<_> = s
            .quests
            .iter()
            .map(|q| {
                (
                    q.id,
                    q.flags,
                    q.log.clone(),
                    q.script.as_ref().map(|s| {
                        (
                            s.state_path(),
                            s.owner.free_ins,
                            s.owner.free_real,
                            s.owner.timer_index,
                        )
                    }),
                    q.ctrl.as_ref().map(|c| c.disabled),
                )
            })
            .collect();
        let file = a.save().unwrap();
        let mut b = host(install().unwrap(), seats);
        b.load(&file, install().unwrap()).unwrap();
        let restored: Vec<_> = b
            .story
            .as_ref()
            .unwrap()
            .quests
            .iter()
            .map(|q| {
                (
                    q.id,
                    q.flags,
                    q.log.clone(),
                    q.script.as_ref().map(|s| {
                        (
                            s.state_path(),
                            s.owner.free_ins,
                            s.owner.free_real,
                            s.owner.timer_index,
                        )
                    }),
                    q.ctrl.as_ref().map(|c| c.disabled),
                )
            })
            .collect();
        assert_eq!(old, restored);
        assert_eq!(
            s.script_states(),
            b.story.as_ref().unwrap().script_states(),
            "continuation keeps clocks/flags"
        );
        let old_tasks: Vec<_> = s.tasks.iter().map(|s| (s.path.clone(), s.state_path())).collect();
        assert_eq!(
            old_tasks,
            b.story
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .map(|s| (s.path.clone(), s.state_path()))
                .collect::<Vec<_>>()
        );
        for h in [&mut a, &mut b] {
            // Fight state is outside STOR; no kill or success is supplied. Camera
            // completion comes from the actual installed camera clock below.
            h.world.enable_live_encounters();
            let vfs = ddvfs::Vfs::open(&h.story.as_ref().unwrap().data.native_pc).unwrap();
            let mut cameras = ddhost::quest_camera::Cameras::default();
            cameras.load(&vfs, 100).unwrap();
            h.world.cameras = Some(cameras);
        }
        // Let the fresh host request/reload its stage. Native scripts re-enter;
        // the continuing host is allowed to remain at its existing timers.
        b.world.report(HostEvent::StageLoaded { stage: 100 });
        for _ in 0..3 {
            park(&mut b);
            b.tick(&Default::default()).unwrap();
        }
        let story = a.story.as_ref().unwrap();
        let (task, row) = story
            .stage
            .scenarios
            .tasks
            .iter()
            .enumerate()
            .find_map(|(t, rows)| {
                rows.iter()
                    .position(|e| e.name.eq_ignore_ascii_case("q0002_event1"))
                    .map(|r| (t, r))
            })
            .unwrap();
        let waypoint = ddstory::route::Waypoint {
            name: "installed rescue",
            stage: 100,
            entry: (task as u32, row as u32),
            until: None,
            interact: false,
            npcs: vec![],
        };
        let (pos, yaw) = waypoint.place(&story.stage.scenarios).unwrap();
        for h in [&mut a, &mut b] {
            for _ in 0..600 {
                park(h);
                let seat = (seats - 1) as u8; // Either seat may admit the volume.
                let p = h.sim.player_mut(seat).unwrap();
                p.position = pos;
                p.yaw = yaw;
                h.tick(&Default::default()).unwrap();
                if h.story.as_ref().unwrap().world.quest_bit(2, 5) {
                    break;
                }
            }
            let s = h.story.as_ref().unwrap();
            assert!(
                s.world.quest_bit(2, 25) && s.world.quest_bit(2, 5),
                "{:?}",
                s.script_states()
            );
            assert!(
                !s.world.quest_bit(2, 20) && !s.world.quest_bit(2, 94),
                "camera progression cannot invent rescue victory"
            );
            assert!(h.world.cameras.as_ref().unwrap().error.is_none());
        }
        assert_eq!(
            a.story.as_ref().unwrap().world.quest_bits(2),
            b.story.as_ref().unwrap().world.quest_bits(2)
        );
        for h in [&mut a, &mut b] {
            // Explicit combat-result fixture for the save test, NOT a combat
            // acceptance claim: the installed script, never this test, writes
            // q2:20 after all three authored enemy targets are defeated while
            // its NPC intel remains alive. Elapsed time alone cannot do so.
            let facts = h.world.encounters.as_mut().unwrap();
            for index in 0..3 {
                facts.report_unit(
                    100,
                    ddstory::Target {
                        kind: 2,
                        group: 23,
                        index,
                    },
                    ddstory::encounter::UnitFacts {
                        killed: true,
                        ..Default::default()
                    },
                );
            }
            facts.report_unit(
                100,
                ddstory::Target {
                    kind: 5,
                    group: 4,
                    index: 0,
                },
                ddstory::encounter::UnitFacts {
                    present: true,
                    npc: Some(ddstory::NpcIntel {
                        id: 304,
                        hp_positive: true,
                    }),
                    ..Default::default()
                },
            );
            for _ in 0..600 {
                park(h);
                h.tick(&Default::default()).unwrap();
                if h.story.as_ref().unwrap().world.quest_bit(2, 20) {
                    break;
                }
            }
            assert!(
                h.story.as_ref().unwrap().world.quest_bit(2, 20),
                "{:?}",
                h.story.as_ref().unwrap().script_states()
            );
            // The authored exit-volume task is a separate route operation,
            // already covered geometrically by scenario tests. Start that
            // installed graph explicitly here to compare quest cleanup/reward.
            use ddstory::data::Data;
            let s = h.story.as_mut().unwrap();
            let path = r"scr\st100\fsm\q0002_event2";
            s.tasks
                .push(ddstory::script::Script::new(path, s.data.fsm(path).unwrap()).unwrap());
            for _ in 0..120 {
                park(h);
                h.tick(&Default::default()).unwrap();
                if h.story.as_ref().unwrap().world.quest_bit(2, 94) {
                    break;
                }
            }
            assert!(h.story.as_ref().unwrap().world.quest_bit(2, 94));
        }
        assert_eq!(
            a.story.as_ref().unwrap().world.quest_bits(2),
            b.story.as_ref().unwrap().world.quest_bits(2)
        );
    }
}

#[test]
fn malformed_story_chunk_is_rejected_before_any_host_change_and_legacy_is_supported() {
    use ddhost::{
        data::NoStoryData,
        save::{Owner, SaveFile, TAG_STORY, TAG_STORY_CONTINUATION, TAG_WORLD},
    };
    let world = ddstory::scripted::ScriptedHost::default();
    let mut h = Host::new(
        Sim::new(Default::default()),
        Some(Story::new_game(NoStoryData)),
        world,
        Box::new(Void),
    );
    h.join(ProfileId(9), Control::Local, [1.0, 2.0, 3.0], 0.5)
        .unwrap();
    let before = h.state_hash();
    let mut f = h.save().unwrap();
    let chunk = f
        .chunks
        .iter_mut()
        .find(|c| c.tag == ddhost::save::TAG_HOST)
        .unwrap();
    chunk.bytes[1..9].copy_from_slice(&999u64.to_le_bytes());
    f.put(
        ddhost::save::TAG_PLACE,
        Owner::Profile(ProfileId(9)),
        ddhost::save::place_bytes([900.0, 800.0, 700.0], 2.0),
    );
    f.put(TAG_STORY, Owner::Shared, vec![255]);
    assert!(h.load(&f, NoStoryData).is_err());
    assert_eq!(h.state_hash(), before);
    let mut orphan = SaveFile::default();
    orphan.put(TAG_STORY, Owner::Shared, vec![1]);
    assert!(h.load(&orphan, NoStoryData).is_err());
    assert_eq!(h.state_hash(), before);
    let mut malformed_extension = h.save().unwrap();
    malformed_extension.put(TAG_STORY_CONTINUATION, Owner::Shared, vec![255]);
    assert!(h.load(&malformed_extension, NoStoryData).is_err());
    assert_eq!(h.state_hash(), before);
    f.chunks
        .retain(|c| c.tag != TAG_STORY && c.tag != TAG_STORY_CONTINUATION);
    assert!(f.get(TAG_WORLD, Owner::Shared).is_some());
    h.load(&f, NoStoryData).unwrap();
    assert!(h.story.as_ref().unwrap().quests.is_empty());
}
