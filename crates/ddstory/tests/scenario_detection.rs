use ddstory::{data::{Data, InstallData}, scenario::{Scenarios, Zone}, scripted::ScriptedHost,
    story::StageState, Env, Inputs, Player, World};

#[test]
fn installed_field_q2_volume_detects_without_bypassing_its_geometry() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { return; }
    let data = InstallData::open(native, None).unwrap();
    let mut scenarios = Scenarios::load(&data.scenarios(100).unwrap()).unwrap();
    let (task, index, min, max) = scenarios.tasks.iter().enumerate().find_map(|(task, entries)| {
        entries.iter().enumerate().find_map(|(index, entry)| {
            if !entry.name.eq_ignore_ascii_case("q0002_event1") { return None; }
            match &entry.shape.as_ref()?.zone {
                Zone::Aabb { min, max } => Some((task, index, *min, *max)),
                Zone::Area { v, bottom, height, .. } => {
                    let min_x = v.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
                    let max_x = v.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
                    let min_z = v.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min);
                    let max_z = v.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max);
                    Some((task, index, [min_x, *bottom, min_z], [max_x, bottom + height, max_z]))
                }
                other => panic!("q0002_event1 shape changed: {other:?}"),
            }
        })
    }).expect("installed q0002 field entry");
    // Exercise detection with real installed shapes. Task callbacks are
    // separately tested; suppress them here so no quest completion is faked.
    for entries in &mut scenarios.tasks {
        for entry in entries { entry.status = 0x22; }
    }
    let host = ScriptedHost::default();
    let mut world = World::default();
    let mut stage = StageState { stage_no: 100, ..Default::default() };
    let mut requests = vec![];
    let mut commands = vec![];
    let mut env = Env { world: &mut world, stage: &mut stage, host: &host,
        time: Default::default(), out: &mut requests, cmds: &mut commands, events: None };
    let inside = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5 - 25.0, (min[2] + max[2]) * 0.5];
    let mut inputs = Inputs { players: vec![Player { pos: inside, yaw: 0.0 }], ..Default::default() };
    scenarios.frame(&mut env, &inputs).unwrap();
    assert_ne!(scenarios.tasks[task][index].mask & 1, 0);
    inputs.players[0].pos[0] = max[0] + 1.0;
    scenarios.frame(&mut env, &inputs).unwrap();
    assert_eq!(scenarios.tasks[task][index].mask & 1, 0);
    assert!(requests.is_empty());
}
