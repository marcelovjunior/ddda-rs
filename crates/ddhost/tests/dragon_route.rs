//! Installed q0000 exits, independent of combat/presentation acceptance.
use ddhost::{opening::OpeningWorld, StoryWorld};
use ddstory::{data::{Data, InstallData}, script::Script, Env, Request, Target, World};

#[test]
fn challenge_reads_first_resource_pool_even_when_its_id_is_not_zero() {
    use ddhost::ddenemyrt::combat::Victim;
    use ddcombat::damage::{CollisionRates, EnemyDefender, RegionPool};
    let pool = |id, hp| RegionPool::from_element(id, hp, 0.0, 0.0, 0.0, 0.0,
        1.0, 1.0, 0, 0, 0.0, 0.0);
    let victim = |regions| Victim::Enemy { defender: EnemyDefender::new(0.0, 0.0),
        regions, rates: CollisionRates::default() };
    assert_eq!(ddhost::opening::dragon_route::first_life_hp(&victim(vec![pool(9, 123.0), pool(0, 45.0)])), Some(123.0));
    assert_eq!(ddhost::opening::dragon_route::first_life_hp(&victim(vec![])), None);
}

fn install() -> Option<InstallData> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { return None; }
    let exe = ddexe::locate(&native).unwrap();
    Some(InstallData::open(native, Some(&exe)).unwrap())
}

#[test]
fn installed_editor_confirmation_requests_beach_and_waits_for_loaded_stage_before_ev00() {
    use ddhost::{Control, FrameInputs, Host, ProfileId};
    use ddhost::opening::{fast_forward, dragon_route::editor_exit_request};
    use ddstory::{HostEvent, Story};
    struct FixtureGround;
    impl ddsim::Ground for FixtureGround {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
    }
    let Some(data) = install() else { return };
    let vfs = ddvfs::Vfs::open(&data.native_pc).unwrap();
    let starts = ddhost::install::start_positions(&vfs, 200).unwrap();
    let start = starts.entries.get(3).expect("native editor beach start3");
    let position = [start.position[0], start.position[1], start.position[2]];
    let mut host = Host::new(ddsim::Sim::new(Default::default()),
        Some(Story::new_game(data)), OpeningWorld::new(), Box::new(FixtureGround));
    assert_eq!(host.story.as_ref().unwrap().stage.game_flags.b8844 & 0x0002_0000, 0,
        "fresh story does not invent the post-prologue context");
    let a = host.join(ProfileId(801), Control::Local, [0.0; 3], 0.0).unwrap();
    let b = host.join(ProfileId(802), Control::Local, [0.0; 3], 0.0).unwrap();
    // Precondition driver only: existing stand-ins run the prologue/movie.
    // From editor confirmation onward there is no fast-forward or synthetic
    // stage/event completion. This is not whole-route acceptance.
    let prologue: Vec<_> = ddhost::opening::to_newly_arisen().into_iter()
        .filter(|point| point.stage != 200).collect();
    fast_forward(&mut host, &prologue, 240, 30_000, |host|
        host.story.as_ref().unwrap().world.stage_no == 801).unwrap();
    assert_eq!(host.story.as_ref().unwrap().world.sce_no, 1000);
    assert_ne!(host.story.as_ref().unwrap().stage.game_flags.b8844 & 0x0002_0000, 0,
        "actual translated row3E end supplies native editor context");
    let request = |host: &Host<InstallData, OpeningWorld>, confirmed| {
        let story = host.story.as_ref().unwrap();
        editor_exit_request(story.world.stage_no, story.world.sce_no,
            &story.stage.game_flags, confirmed, 0)
    };
    let mut first = ddpawn::cmc::Edit { name: [0; 25], nickname: 0, fields: [0; 41] };
    first.set_name(b"First");
    host.confirm_character(a, first.clone()).unwrap();
    assert_eq!(request(&host, false).unwrap(), None, "one profile is insufficient");
    let mut second = ddpawn::cmc::Edit { name: [0; 25], nickname: 0, fields: [0; 41] };
    second.set("mGender", 1);
    second.set_name(b"Second");
    host.confirm_character(b, second.clone()).unwrap();
    let jump = request(&host, true).unwrap().unwrap();
    assert_eq!(jump, Request::JumpArea { stage: 200, pos: 3, fade: 0 });
    let mut absent = host.story.as_ref().unwrap().stage.game_flags;
    absent.b8844 &= !0x0002_0000;
    assert!(editor_exit_request(801, 1000, &absent, true, 0).is_err());
    assert!(editor_exit_request(801, 1050,
        &host.story.as_ref().unwrap().stage.game_flags, true, 0).is_err());
    assert!(editor_exit_request(802, 1000, &absent, true, 0).is_err());
    let target = Target { kind: 2, group: 0, index: 0 }; // Binding fixture only.
    host.world.enable_dragon_scene(200, target);
    host.world.manual_stages = true;
    host.world.handle(801, &[jump]);
    for _ in 0..40 {
        let report = host.tick(&FrameInputs::new()).unwrap();
        assert_eq!(host.story.as_ref().unwrap().world.stage_no, 801);
        assert!(!report.requests.iter().any(|request| matches!(request,
            Request::PlayEvent { row: 19, .. })), "queued synthetic stage cannot start ev00");
    }
    // Headless stage-adapter fixture: installed start3 is applied to all
    // seats, then the adapter reports its actual completed load.
    for id in [a, b] {
        let player = host.sim.player_mut(id).unwrap();
        player.position = position;
        player.yaw = start.angle;
    }
    host.story.as_mut().unwrap().world.start_pos = 3;
    host.world.report(HostEvent::StageLoaded { stage: 200 });
    let mut ev00 = false;
    for _ in 0..120 {
        let report = host.tick(&FrameInputs::new()).unwrap();
        assert_eq!(host.story.as_ref().unwrap().world.stage_no, 200);
        assert!(host.story.as_ref().unwrap().quest(1).is_none());
        if report.requests.iter().any(|request| matches!(request,
            Request::PlayEvent { row: 19, stage: 200, event: 0 })) {
            ev00 = true;
            break;
        }
    }
    assert!(ev00, "installed q0 must request ev00 after the beach loaded");
    assert_eq!(host.world.inner.awaiting, Some(19));
    assert_eq!(host.character_edit(a), Some(&first));
    assert_eq!(host.character_edit(b), Some(&second));
    let event = &host.story.as_ref().unwrap().stage.event;
    assert!(event.active && event.started);
    assert_eq!(event.playing.as_ref().unwrap().row, 19,
        "ev00 is still playing; request cannot manufacture EventFinished");
}

struct FightPhase {
    data: InstallData,
    script: Script,
    stage: ddstory::story::StageState,
    world: World,
    actors: OpeningWorld,
}

impl FightPhase {
    fn new(state: u32) -> Option<Self> {
        let data = install()?;
        let mut graph = (*data.fsm("quest/q0000_b00").unwrap()).clone();
        // Test fixture: admit the already reached fight state. Processes,
        // conditions, links and all parameter values remain installed data.
        graph.root.as_mut().unwrap().initial_state = state;
        let mut script = Script::new("quest/q0000_b00", std::sync::Arc::new(graph)).unwrap();
        script.owner.quest = Some(0);
        let mut world = World::default();
        world.stage_no = 200;
        world.set_sce_no(1025, false);
        world.area_stay = 0x4000_0000;
        let mut actors = OpeningWorld::new();
        actors.enable_live_encounters();
        let mut out = Self { data, script, world, actors,
            stage: ddstory::story::StageState { stage_no: 200, ..Default::default() } };
        out.step(); // Real enter callbacks; first transition check is skipped.
        assert_eq!(out.script.current_states(), vec![state]);
        Some(out)
    }

    fn step(&mut self) -> Vec<Request> {
        let mut requests = Vec::new();
        let mut commands = Vec::new();
        self.script.step::<ddstory::orders::Orders>(&mut Env {
            world: &mut self.world, stage: &mut self.stage, host: &self.actors,
            time: Default::default(), out: &mut requests, cmds: &mut commands,
            events: self.data.event_table(),
        }).unwrap();
        requests
    }

    fn assert_heart_requested(&self) {
        assert_eq!(self.script.current_states(), vec![1]);
        let event = self.stage.event.pending.as_ref().expect("authored ev01 request");
        assert_eq!((event.stage, event.event, event.sce_no), (200, 1, Some(1050)));
        assert!(!self.stage.event.started, "request is not event completion");
    }
}

#[test]
fn installed_gsf379_and_leaving_area_exit_both_fight_states() {
    for state in [16, 5] {
        let Some(mut hit) = FightPhase::new(state) else { return };
        hit.world.gsf.set(379, true);
        hit.step();
        hit.step(); // State5 setting1 queues its post-callback transition.
        hit.assert_heart_requested();

        let mut left = FightPhase::new(state).unwrap();
        left.world.area_stay = 0;
        left.step();
        left.step();
        left.assert_heart_requested();
    }
}

#[test]
fn installed_enemy_check_consumes_persistent_killed_fact_not_hp_or_absence() {
    for state in [16, 5] {
        let Some(mut phase) = FightPhase::new(state) else { return };
        // Target is the actual installed EnemyCheck order, rather than a
        // hand-picked layout record. Its flag selects persistent death.
        let process = phase.script.graph().root.as_ref().unwrap().states.iter()
            .find(|s| s.id == state).unwrap().processes.iter()
            .find(|p| p.container == b"EnemyCheck").unwrap();
        let parameter = process.parameter.as_ref().unwrap();
        use ddquest::order::Fields;
        assert_eq!(parameter.u("mCheckFlag"), Some(1));
        fn target(record: &mtfsm::Record) -> Option<Target> {
            if record.class_name == Some("cLinkUnit::cTarget") {
                return Some(Target { kind: record.u("mType")? as u32,
                    group: record.u("mNo0")? as u32, index: record.u("mNo1")? as u32 });
            }
            record.fields.iter().flat_map(|(_, fields)| fields).find_map(|field| match field {
                mtfsm::Parameter::Object(Some(child)) => target(child), _ => None,
            })
        }
        let target = target(parameter).expect("installed EnemyCheck target");
        let facts = phase.actors.encounters.as_mut().unwrap();
        facts.report_unit(200, target, ddstory::encounter::UnitFacts {
            present: true, hp: Some(0.0), max_hp: Some(100.0), ..Default::default()
        });
        phase.step();
        phase.step();
        assert_eq!(phase.script.current_states(), vec![state]);
        let facts = phase.actors.encounters.as_mut().unwrap();
        facts.report_unit(200, target, ddstory::encounter::UnitFacts { killed: true, ..Default::default() });
        phase.step(); // EnemyCheck state callback publishes FreeFlag[0].
        phase.step(); // Next condition evaluation consumes that observation.
        phase.assert_heart_requested();
    }
}

#[test]
fn installed_seventeen_then_forty_three_timers_are_strict_and_reset() {
    let Some(mut phase) = FightPhase::new(16) else { return };
    phase.script.owner.cnt_state = 17.0;
    phase.step();
    assert_eq!(phase.script.current_states(), vec![16], "equal17 is not >17");
    phase.step();
    assert_eq!(phase.script.current_states(), vec![5]);
    assert!(phase.world.gsf.get(378));
    assert!(phase.script.owner.cnt_state < 0.04, "state5 owns a fresh timer");
    phase.script.owner.cnt_state = 43.0;
    phase.step();
    assert_eq!(phase.script.current_states(), vec![5], "equal43 is not >43");
    phase.step();
    // State5 checks after callbacks (installed mSetting bit0), then enters
    // the selected destination on the following core update.
    assert_eq!(phase.script.current_states(), vec![5]);
    phase.step();
    phase.assert_heart_requested();
}

#[test]
fn installed_beach_victims_follow_lot1_and_are_demo_actors() {
    let Some(data) = install() else { return };
    let vfs = ddvfs::Vfs::open(&data.native_pc).unwrap();
    let list = ddhost::npc_placements::group_list(&vfs, 200).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 200, &list, &[(0, 0)]).unwrap();
    let mut world = World::default();
    world.stage_no = 200;
    world.set_sce_no(1025, false);
    let victims = |world: &World| {
        let flag = |n: u16| world.lot(200, n as usize);
        layouts.candidates(&ddhost::ddspawn::World { scene: world.sce_no as i32,
            hour: world.hour, flag: &flag }).placements.into_iter()
            .filter(|p| p.target.group == 3).collect::<Vec<_>>()
    };
    assert!(victims(&world).is_empty());
    world.set_lot(200, 1, true);
    let admitted = victims(&world);
    assert_eq!(admitted.len(), 7);
    for (index, placement) in admitted.iter().enumerate() {
        assert_eq!((placement.target.index, placement.npc), (index as u32, 704 + index as i32));
        let graph = data.fsm(&placement.authored_action).unwrap();
        assert_eq!(graph.owner, b"cAIActionFSMDemo");
        assert!(!graph.root.as_ref().unwrap().states.iter()
            .flat_map(|s| &s.processes).any(|p| p.container == b"Attack"));
    }
    world.set_lot(200, 1, false);
    assert!(victims(&world).is_empty());
}

#[test]
fn live_combat_snapshots_drive_strict_stage_hp_and_all_arisen_but_no_pawns() {
    use ddhost::ddenemyrt::{combat::Victim, fight::{Fight, FightData}, dragon::OpeningDragon};
    let Some(data) = install() else { return };
    let vfs = ddvfs::Vfs::open(&data.native_pc).unwrap();
    let file = ddexe::ExeFile::next_to(&vfs.root).unwrap();
    let exe = file.image().unwrap();
    let dragon = ddhost::dragon::OpeningDragonData::load(&vfs, &exe).unwrap();
    let mut fight = Fight::new(std::sync::Arc::new(FightData::load(&vfs, &exe).unwrap()));
    let owner = fight.allocate_enemy_owner();
    fight.dragon = Some(OpeningDragon::new(dragon.data, dragon.graph, owner,
        dragon.placement.position, dragon.placement.angle[1], dragon.placement.scale).unwrap());
    fight.add_arisen(0);
    fight.add_arisen(1);
    let mut sim = ddsim::Sim::new(Default::default());
    fight.add_pawn(&mut sim, 4, 0, [0.0; 3], 0.0, [0.0; 9], 1);
    if let Victim::Player { hp, .. } = &mut fight.arisen.get_mut(&4).unwrap().fighter.victim { *hp = 1.0; }
    // Synthetic binding proves that observations honor the explicit target,
    // without treating owner allocation or enemy-family id as layout identity.
    let target = Target { kind: 2, group: 7, index: 9 };
    let mut actors = OpeningWorld::new();
    actors.enable_dragon_scene(200, target);
    actors.observe_fight(Some(&fight));
    let facts = actors.encounters.as_ref().unwrap();
    assert_eq!(facts.challenge_players.len(), 2);
    assert!(!facts.challenge_players.iter().any(|player| player.life_hp == Some(1.0)));
    assert_eq!(facts.unit(200, &target).present, true);
    assert_eq!(facts.unit(200, &target).killed, false);
    let baseline = facts.challenge_dragon.unwrap().life_hp.unwrap();
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.gsf.set(378, true);
    story.tick(&Default::default(), &actors).unwrap();
    assert_eq!(story.stage.dragon_challenge.baseline_hp, Some(baseline));
    let set_dragon_hp = |fight: &mut Fight, hp| {
        let Victim::Enemy { regions, .. } = &mut fight.dragon.as_mut().unwrap().actor.fighter.victim else { panic!() };
        regions[0].hp = hp; // Explicit scalar fixture, not a normal-input hit.
    };
    set_dragon_hp(&mut fight, baseline - 4.0);
    actors.observe_fight(Some(&fight));
    story.tick(&Default::default(), &actors).unwrap();
    assert!(!story.world.gsf.get(379));
    set_dragon_hp(&mut fight, baseline - 4.125);
    actors.observe_fight(Some(&fight));
    story.tick(&Default::default(), &actors).unwrap();
    assert!(story.world.gsf.get(379));

    story.world.gsf.set(378, false);
    story.world.gsf.set(379, false);
    if let Victim::Player { hp, .. } = &mut fight.arisen.get_mut(&1).unwrap().fighter.victim { *hp = 1.0; }
    actors.observe_fight(Some(&fight));
    story.tick(&Default::default(), &actors).unwrap();
    assert!(story.world.gsf.get(379), "second Arisen independently satisfies native life branch");
    fight.dragon = None;
    actors.observe_fight(Some(&fight));
    let facts = actors.encounters.as_ref().unwrap();
    assert_eq!(facts.challenge_dragon, None);
    assert!(!facts.unit(200, &target).present);
    assert_eq!(facts.unit(200, &target).hp, None);
    assert!(!facts.unit(200, &target).killed, "unload is not persistent death");
    actors.observe_fight(None);
    assert!(actors.encounters.as_ref().unwrap().challenge_players.is_empty());
}
