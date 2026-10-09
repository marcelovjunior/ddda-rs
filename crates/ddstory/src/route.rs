//! Waypoints for driving the opening in tests: "stand inside scenario entry
//! (task, no) of stage S". Positions come from the loaded scenario data.

use crate::scenario::{Scenarios, Zone};

#[derive(Debug, Clone)]
pub struct Waypoint {
    pub name: &'static str,
    pub stage: u32,
    /// Scenario entry to stand in.
    pub entry: (u32, u32),
    /// Stay until SceNo reaches this value (else a fixed time).
    pub until: Option<u32>,
    /// Press interact while there.
    pub interact: bool,
    /// NPC ids standing there too (for volumes that detect NPCs).
    pub npcs: Vec<u32>,
}

fn wp(name: &'static str, stage: u32, task: u32, no: u32) -> Waypoint {
    Waypoint { name, stage, entry: (task, no), until: None, interact: false, npcs: vec![] }
}

impl Waypoint {
    pub fn until(mut self, s: u32) -> Self {
        self.until = Some(s);
        self
    }
    pub fn interact(mut self) -> Self {
        self.interact = true;
        self
    }
    pub fn with_npcs(mut self, ids: &[u32]) -> Self {
        self.npcs = ids.to_vec();
        self
    }

    /// A unit position (and yaw) inside the entry's volume: its centre,
    /// lowered by the 25 units the hit test adds (0x007435b0), facing the
    /// angle filter's direction when it has one.
    pub fn place(&self, sc: &Scenarios) -> Option<([f32; 3], f32)> {
        let e = sc.entry(self.entry.0, self.entry.1)?;
        let s = e.shape.as_ref()?;
        let c = match &s.zone {
            Zone::Area { v, bottom, height, .. } => [
                (v[0][0] + v[1][0] + v[2][0] + v[3][0]) * 0.25,
                bottom + height * 0.5,
                (v[0][2] + v[1][2] + v[2][2] + v[3][2]) * 0.25,
            ],
            Zone::Sphere { c, .. } => *c,
            Zone::Cylinder { p0, p1, .. } => [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5, (p0[2] + p1[2]) * 0.5],
            Zone::Obb { m, .. } => [m[3][0], m[3][1], m[3][2]],
            Zone::Aabb { min, max } => [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5],
        };
        let yaw = s.angle.map(|(a, _)| a).unwrap_or(0.0);
        Some(([c[0], c[1] - 25.0, c[2]], yaw))
    }
}

/// The opening route: prologue (stage 380) to the boot of q0002 (stage
/// 200). Entry numbers are scenario entries (task 4 = fsm, 10 = etc).
pub fn opening() -> Vec<Waypoint> {
    vec![
        wp("prologue: pawn", 380, 4, 1).until(200),
        wp("prologue: dragon appears", 380, 4, 4),
        wp("prologue: goblins", 380, 4, 18),
        wp("prologue: tutorial", 380, 4, 2),
        wp("prologue: tutorial 2", 380, 4, 12),
        wp("prologue: 350", 380, 4, 3).until(350),
        wp("prologue: tutorial 3", 380, 4, 16),
        wp("prologue: tutorial 4", 380, 4, 11),
        wp("prologue: prison", 380, 4, 9),
        wp("prologue: 370", 380, 4, 19).until(370),
        wp("prologue: call for help", 380, 4, 17),
        wp("prologue: hall", 380, 4, 5),
        wp("prologue: harpy", 380, 4, 13),
        wp("prologue: hall reset", 380, 4, 6),
        wp("prologue: 650", 380, 4, 8).until(650),
        wp("prologue: carried", 380, 4, 21).with_npcs(&[505, 512, 510]),
        wp("prologue: chimera", 380, 4, 7).until(1000),
        wp("cassardis: dragon attack", 200, 1, 1).until(1100),
        wp("cassardis: weapon table", 200, 10, 0).interact(),
        wp("cassardis: room exit", 200, 4, 0).until(1200),
        wp("cassardis: 1300", 200, 4, 7).until(1300),
        wp("cassardis: q0002 event", 200, 4, 1).until(1400),
    ]
}

/// What [`run`] observed.
#[derive(Debug, Default)]
pub struct Trace {
    /// SceNo values in the order they appeared.
    pub sce_ladder: Vec<u32>,
    /// The world when each waypoint was done, by waypoint name.
    pub checkpoints: Vec<(&'static str, crate::World)>,
    /// Quests booted, in order.
    pub boots: Vec<u32>,
    /// Requests the story made (frame, request).
    pub requests: Vec<(u64, crate::Request)>,
    pub frames: u64,
}

/// Walk `route` with one Arisen standing in each waypoint's volume (frames
/// per waypoint `stay`, or until its SceNo), answering requests with
/// `host`, until the route ends and `done` holds or `max_frames` pass.
pub fn run<D: crate::data::Data>(
    story: &mut crate::Story<D>,
    host: &mut crate::scripted::ScriptedHost,
    route: &[Waypoint],
    stay: u64,
    max_frames: u64,
    done: impl Fn(&crate::Story<D>) -> bool,
) -> Result<Trace, crate::Error> {
    use crate::{HostEvent, Inputs, NpcUnit, Player};
    let mut t = Trace { sce_ladder: vec![story.world.sce_no], ..Default::default() };
    let mut step = 0;
    let mut at_step = 0;
    let mut boot = vec![HostEvent::StageLoaded { stage: story.world.stage_no }];
    let mut known: Vec<u32> = vec![];
    for frame in 0..max_frames {
        t.frames = frame;
        let wp = route.get(step);
        let mut events = host.events();
        events.append(&mut boot);
        let (pos, yaw) = match wp {
            Some(w) if w.stage == story.stage.stage_no => {
                w.place(&story.stage.scenarios).unwrap_or(([0.0, -100_000.0, 0.0], 0.0))
            }
            _ => ([0.0, -100_000.0, 0.0], 0.0),
        };
        let npcs = wp
            .map(|w| w.npcs.iter().map(|&id| NpcUnit { id, unit: Player { pos, yaw: 0.0 } }).collect())
            .unwrap_or_default();
        let inputs = Inputs {
            players: vec![Player { pos, yaw }],
            npcs,
            interact: wp.filter(|w| w.interact).map(|_| 0),
            events,
            ..Default::default()
        };
        let reqs = story.tick(&inputs, &*host)?;
        host.handle(story.stage.stage_no, &reqs);
        t.requests.extend(reqs.into_iter().map(|r| (frame, r)));
        if t.sce_ladder.last() != Some(&story.world.sce_no) {
            t.sce_ladder.push(story.world.sce_no);
        }
        for q in &story.quests {
            if !known.contains(&q.id) {
                known.push(q.id);
                t.boots.push(q.number());
            }
        }
        at_step += 1;
        match wp {
            Some(w) => {
                let reached = match w.until {
                    Some(s) => story.world.sce_no >= s && at_step >= 2,
                    None => at_step >= stay,
                };
                if reached && w.stage == story.stage.stage_no {
                    t.checkpoints.push((w.name, story.world.clone()));
                    step += 1;
                    at_step = 0;
                }
            }
            None => {
                if done(story) {
                    return Ok(t);
                }
            }
        }
    }
    Err(crate::Error::Data(format!("route stalled at waypoint {step} after {max_frames} frames")))
}
