use ddnpcrt::schedule::{Clock, Npc, Request};

#[test]
fn installed_applied_npc_waits_for_talk_then_finishes_once() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = ddnpcrt::InstallData::open(native, "eng").unwrap();
    let mut npc = Npc::applied(1, &data, r"ai\fsm\npc\st200\quest\q0002_n001_ai").unwrap();
    let clock = Clock {
        hour: 12,
        minute: 0,
        dt: 1.0 / 30.0,
    };
    // A pulse before the talk state cannot stay latched until that state.
    npc.observe_talk_end(true);
    let mut guard = None;
    for _ in 0..300 {
        for request in npc.step(&data, clock).unwrap() {
            match request {
                Request::Guard { config, .. } if config.kind != 0 => guard = Some(config),
                Request::Gsf { .. } | Request::Finished { .. } => {
                    panic!("progress without talk: {request:?}")
                }
                _ => {}
            }
        }
    }
    let guard = guard.expect("installed graph configures the talk sensor");
    assert_eq!(
        (guard.kind, guard.message_kind, guard.message, guard.quest),
        (1, 2, 1, 2)
    );
    assert_eq!((guard.radius, guard.height), (300.0, 100.0));
    assert!(!npc.applied_finished());
    npc.observe_talk_end(true);
    let mut wrote_start = false;
    let mut finishes = 0;
    for _ in 0..600 {
        for request in npc.step(&data, clock).unwrap() {
            match request {
                Request::Gsf { no: 381, value: true } => wrote_start = true,
                // Explicit test actor arrival; production must supply real movement.
                Request::Goto { pos, .. } => npc.done(Some(pos)),
                Request::Finished { .. } => finishes += 1,
                _ => {}
            }
        }
    }
    assert!(wrote_start);
    assert_eq!(finishes, 1);
    assert!(npc.applied_finished());
}
