//! The opening (new game to the boot of q0002) run through the host with
//! two seated Arisen, twice: both runs must end in the same state, and the
//! story must climb the same SceNo ladder as ddstory's own driver
//! (crates/ddstory/tests/opening.rs). Player 1 is placed in each
//! waypoint's trigger volume, player 2 stands far away. Skips when the
//! install is absent (DDDA_NATIVE_PC, DDDA_EXE).

use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId, StoryWorld};
use ddsim::{Ground, Sim};
use ddstory::data::InstallData;
use ddstory::scripted::ScriptedHost;
use ddstory::{HostEvent, NpcIntel, NpcUnit, Request, Story, Target};
use std::collections::HashMap;

fn install() -> Option<InstallData> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    if !np.exists() || !exe.exists() {
        eprintln!("SKIP: DDDA install absent");
        return None;
    }
    Some(InstallData::open(np, Some(&exe)).unwrap())
}

/// No ground: the test places the Arisen itself.
struct Void;
impl Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

/// The scripted stand-in plus the NPCs a waypoint puts in a volume.
struct World {
    inner: ScriptedHost,
    npcs: Vec<NpcUnit>,
}

impl StoryWorld for World {
    fn handle(&mut self, stage: u32, requests: &[Request]) {
        self.inner.handle(stage, requests)
    }
    fn events(&mut self) -> Vec<HostEvent> {
        self.inner.events()
    }
    fn npcs(&self) -> Vec<NpcUnit> {
        self.npcs.clone()
    }
}

impl ddstory::Host for World {
    fn layout_loaded(&self, s: u32, t: &Target) -> bool {
        self.inner.layout_loaded(s, t)
    }
    fn unit_exists(&self, s: u32, t: &Target) -> bool {
        self.inner.unit_exists(s, t)
    }
    fn group_loaded(&self, s: u32, k: u32, g: u32) -> bool {
        self.inner.group_loaded(s, k, g)
    }
    fn enemy_group_done(&self, s: u32, g: u32) -> bool {
        self.inner.enemy_group_done(s, g)
    }
    fn enemy_group_active(&self, s: u32, g: u32) -> bool {
        self.inner.enemy_group_active(s, g)
    }
    fn enemy_check(&self, s: u32, t: &Target, f: u32) -> bool {
        self.inner.enemy_check(s, t, f)
    }
    fn npc_intel(&self, s: u32, t: &Target) -> Option<NpcIntel> {
        self.inner.npc_intel(s, t)
    }
    fn npc_unit(&self, n: u32) -> bool {
        self.inner.npc_unit(n)
    }
    fn apply_fsm_ready(&self, s: u32, t: &Target) -> bool {
        self.inner.apply_fsm_ready(s, t)
    }
    fn unit_hp(&self, s: u32, t: &Target, p: bool) -> Option<f32> {
        self.inner.unit_hp(s, t, p)
    }
    fn tutorials_enabled(&self) -> bool {
        self.inner.tutorials_enabled()
    }
    fn pl_check(&self, k: u32) -> i32 {
        self.inner.pl_check(k)
    }
    fn player_damaged(&self) -> bool {
        self.inner.player_damaged()
    }
    fn pawn_lost(&self) -> bool {
        self.inner.pawn_lost()
    }
    fn has_item(&self, i: u32, c: u32) -> bool {
        self.inner.has_item(i, c)
    }
}

const AWAY: [f32; 3] = [0.0, -100_000.0, 0.0];

struct Outcome {
    ladder: Vec<u32>,
    hash: u64,
    frames: u64,
}

fn run(data: InstallData) -> Outcome {
    let story = Story::new_game(data);
    let world = World { inner: ScriptedHost::opening(), npcs: vec![] };
    let mut h = Host::new(Sim::new(HashMap::new()), Some(story), world, Box::new(Void));
    h.join(ProfileId(1), Control::Local, AWAY, 0.0).unwrap();
    h.join(ProfileId(2), Control::Remote, AWAY, 0.0).unwrap();
    let route = ddstory::route::opening();
    let (mut step, mut at_step) = (0usize, 0u64);
    let mut ladder = vec![h.story.as_ref().unwrap().world.sce_no];
    for frame in 0..40_000u64 {
        let s = h.story.as_ref().unwrap();
        let wp = route.get(step);
        let (pos, yaw) = match wp {
            Some(w) if w.stage == s.stage.stage_no => w.place(&s.stage.scenarios).unwrap_or((AWAY, 0.0)),
            _ => (AWAY, 0.0),
        };
        // The host is the authority: it may place an Arisen directly.
        let p = h.sim.player_mut(0).unwrap();
        p.position = pos;
        p.yaw = yaw;
        h.world.npcs = wp
            .map(|w| w.npcs.iter().map(|&id| NpcUnit { id, unit: ddstory::Player { pos, yaw: 0.0 } }).collect())
            .unwrap_or_default();
        let mut inputs = FrameInputs::new();
        // Press interact every other frame while the waypoint asks for it.
        let interact = wp.is_some_and(|w| w.interact) && frame % 2 == 0;
        inputs.insert(0, PlayerInput { interact, ..Default::default() });
        inputs.insert(1, PlayerInput::default());
        h.tick(&inputs).unwrap();
        let s = h.story.as_ref().unwrap();
        if ladder.last() != Some(&s.world.sce_no) {
            ladder.push(s.world.sce_no);
        }
        at_step += 1;
        match wp {
            Some(w) => {
                let reached = match w.until {
                    Some(n) => s.world.sce_no >= n && at_step >= 2,
                    None => at_step >= 240,
                };
                if reached && w.stage == s.stage.stage_no {
                    step += 1;
                    at_step = 0;
                }
            }
            None if s.quest(2).is_some() => return Outcome { ladder, hash: h.state_hash(), frames: frame },
            None => {}
        }
    }
    panic!("opening stalled at waypoint {step}; ladder {ladder:?}");
}

#[test]
fn opening_through_the_host_replays() {
    let Some(a) = install() else { return };
    let b = install().unwrap();
    let x = run(a);
    let y = run(b);
    assert_eq!(
        x.ladder,
        vec![100, 200, 350, 370, 650, 750, 800, 850, 1000, 1025, 1050, 1100, 1200, 1300, 1400]
    );
    assert_eq!((x.frames, x.hash), (y.frames, y.hash));
}
