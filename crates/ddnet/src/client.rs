//! The client side (ADR 0002 sections 8.4-8.6).
//!
//! The client stamps each command with the host frame it is meant for,
//! a little ahead of the host (the *lead*). It predicts its own Arisen
//! with `ddsim` from its own commands. On every snapshot it resets that
//! Arisen to the host's state and replays the commands the host has not
//! run yet. Other Arisen are shown [`INTERP_FRAMES`] behind the newest
//! snapshot, interpolated.

use crate::body::Body;
use crate::link::Link;
use crate::mirror::{Enemy, LookData, Spot, Vital};
use crate::proto::{Inputs, Snapshot, ToClient, ToHost, PROTOCOL};
use crate::transport::Transport;
use ddhost::{FrameClock, PlayerInput};
use ddid::PlayerId;
use ddsim::{Ground, Sim};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::SocketAddr;

/// Commands carried in each input packet: the newest and seven before it.
pub const REDUNDANCY: usize = 8;
/// Others are shown this many host frames behind the newest snapshot:
/// 100 ms at 30 Hz.
pub const INTERP_FRAMES: f64 = 3.0;
/// The lead the client keeps: its commands should reach the host 1-4
/// frames before the host runs them.
pub const LEAD_MIN: i64 = 1;
pub const LEAD_MAX: i64 = 4;
const SNAPSHOTS: usize = 64;

pub struct Client<T: Transport> {
    pub transport: T,
    pub server: SocketAddr,
    link: Link,
    pub seat: Option<PlayerId>,
    pub refused: Option<String>,
    /// Prediction: a simulation holding only this client's Arisen.
    pub sim: Sim,
    ground: Box<dyn Ground + Send>,
    clock: FrameClock,
    /// Commands by host frame, not yet confirmed by a snapshot.
    cmds: VecDeque<(u64, PlayerInput)>,
    next_cmd: u64,
    /// Snapshots applied, oldest first.
    snaps: VecDeque<Snapshot>,
    /// Predicted position after each commanded frame.
    predicted: VecDeque<(u64, [f32; 3])>,
    /// Distance between the prediction and the host's state for the same
    /// frame, at each snapshot (cm).
    pub last_error: f32,
    pub max_error: f32,
    pub snapshots: u64,
    /// Lead adjustments made (frames skipped ahead or held).
    pub lead_changes: i64,
    hold: bool,
    pub events: Vec<ToClient>,
    /// The host's spot, from Welcome.
    pub spot: Option<Spot>,
    /// Looks by seat, from Look events.
    pub looks: BTreeMap<PlayerId, LookData>,
    quest_camera: Option<crate::quest_camera::QuestCamera>,
    ground_pending: bool,
    ground_stage: Option<u32>,
    stage_start_frame: u64,
}

#[cfg(test)]
mod camera_prediction_tests {
    use super::*;
    use crate::transport::{Transport, Udp};
    struct Flat;
    impl Ground for Flat {
        fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { Some(below.min(0.0)) }
    }
    #[test]
    fn replicated_lock_applies_before_replay_and_next_prediction() {
        let mut sim = Sim::new(Default::default()); sim.add_player(1, [0.0; 3], 0.0);
        let own = Body::of(sim.player(1).unwrap());
        let server = Udp::bind("127.0.0.1:0").unwrap().local_addr();
        let mut client = Client::new(Udp::bind("127.0.0.1:0").unwrap(), server,
            2, 1, 2, None, sim, Box::new(Flat), 0);
        client.seat = Some(1);
        client.spot = Some(Spot { stage: 100, cell: (50,50), pos: [0.0;3], yaw: 0.0 });
        client.set_ground(Box::new(Flat));
        let moving = PlayerInput { sim: ddsim::Input { stick: [1.0, 0.0], jump: true, light: true,
            ..Default::default() }, interact: true };
        client.cmds.push_back((11, moving));
        client.apply(Snapshot { frame: 10, stage: Some(100), base: None, cmd_newest: None,
            bodies: vec![own.clone()], vitals: vec![], enemies: vec![], npcs: vec![],
            quest_camera: Some(crate::quest_camera::QuestCamera { stage: 100, camera_id: 12,
                generation: 1, seconds: 0.5, input_locked: true }) });
        let replayed = client.own().unwrap();
        assert_eq!((replayed.position[0], replayed.position[2]), (0.0, 0.0));
        assert_eq!(replayed.state, ddsim::State::Wait);
        client.predict(1, 12, moving);
        assert_eq!(client.own().unwrap().position, replayed.position);
        client.apply(Snapshot { frame: 12, stage: Some(100), base: None, cmd_newest: None,
            bodies: vec![own], vitals: vec![], enemies: vec![], npcs: vec![], quest_camera: None });
        assert!(!client.actions_blocked());
        client.predict(1, 13, moving);
        assert_ne!(client.own().unwrap().state, replayed.state, "explicit host end admits movement/jump input before prediction");
    }

    fn snap(frame: u64, stage: u32, bodies: Vec<Body>, enemies: Vec<Enemy>) -> Snapshot {
        Snapshot { frame, stage: Some(stage), base: None, cmd_newest: None,
            bodies, enemies, vitals: vec![], npcs: vec![], quest_camera: None }
    }
    #[test]
    fn stage_change_suspends_old_ground_then_replays_each_command_once() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        struct CountGround { floor: f32, calls: Arc<AtomicUsize> }
        impl Ground for CountGround {
            fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
                self.calls.fetch_add(1, Ordering::Relaxed); (self.floor <= below).then_some(self.floor)
            }
        }
        let mut sim = Sim::new(Default::default()); sim.add_player(1, [0.0; 3], 0.0);
        let mut own = Body::of(sim.player(1).unwrap());
        let old_calls = Arc::new(AtomicUsize::new(0));
        let mut client = Client::new(Udp::bind("127.0.0.1:0").unwrap(),
            Udp::bind("127.0.0.1:0").unwrap().local_addr(), 2, 1, 2, None, sim,
            Box::new(CountGround { floor: 0.0, calls: old_calls.clone() }), 0);
        client.seat = Some(1);
        client.apply(snap(10, 100, vec![own.clone()], vec![]));
        let moving = PlayerInput { sim: ddsim::Input { stick: [1.0, 0.0], ..Default::default() }, ..Default::default() };
        client.cmds.push_back((21, moving));
        own.position = [3000.0, 0.0, 0.0];
        client.apply(snap(20, 200, vec![own.clone()], vec![]));
        assert!(client.ground_pending);
        assert_eq!(old_calls.load(Ordering::Relaxed), 0, "apply never replays on old stage collision");
        client.predict(1, 21, moving);
        assert_eq!(client.own().unwrap(), own, "new command waits with authoritative body");
        let new_calls = Arc::new(AtomicUsize::new(0));
        client.set_ground(Box::new(CountGround { floor: -100.0, calls: new_calls.clone() }));
        assert!(!client.ground_pending);
        assert!(new_calls.load(Ordering::Relaxed) > 0);
        assert_eq!(client.predicted.iter().filter(|(f, _)| *f == 21).count(), 1);
        assert_eq!(client.cmds.len(), 1, "reconciliation does not upload or duplicate a command");
        let predicted = client.own().unwrap();
        assert_eq!(predicted.state, ddsim::State::Fall, "new lower ground controls the replayed state");
        let calls = new_calls.load(Ordering::Relaxed);
        client.set_ground(Box::new(CountGround { floor: -100.0, calls: new_calls.clone() }));
        assert_eq!(new_calls.load(Ordering::Relaxed), calls, "ordinary terrain refresh does not replay twice");
        assert_eq!(client.own().unwrap(), predicted);
    }
    #[test]
    fn initial_field_snapshot_waits_for_field_ground_even_with_stale_welcome() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        struct Plane { height: f32, calls: Arc<AtomicUsize> }
        impl Ground for Plane {
            fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
                self.calls.fetch_add(1, Ordering::Relaxed);
                (self.height <= below).then_some(self.height)
            }
        }
        let mut sim = Sim::new(Default::default()); sim.add_player(1, [0.0, 500.0, 0.0], 0.0);
        let own = Body::of(sim.player(1).unwrap());
        let old = Arc::new(AtomicUsize::new(0));
        let field = Arc::new(AtomicUsize::new(0));
        let mut client = Client::new(Udp::bind("127.0.0.1:0").unwrap(),
            Udp::bind("127.0.0.1:0").unwrap().local_addr(), 2, 1, 2, None, sim,
            Box::new(Plane { height: 500.0, calls: old.clone() }), 0);
        client.seat = Some(1);
        client.spot = Some(Spot { stage: 200, cell: (50,50), pos: own.position, yaw: 0.0 });
        let moving = PlayerInput { sim: ddsim::Input { stick: [1.0, 0.0], ..Default::default() }, ..Default::default() };
        client.cmds.push_back((21, moving));
        client.apply(snap(20, 100, vec![own.clone()], vec![]));
        assert!(!client.ground_ready(), "the first stage snapshot must establish collision identity");
        assert_eq!(old.load(Ordering::Relaxed), 0);
        assert_eq!(client.own().unwrap(), own);
        client.set_ground(Box::new(Plane { height: 100.0, calls: field.clone() }));
        assert_eq!(client.ground_stage, Some(100), "snapshot stage takes precedence over stale Welcome");
        assert!(client.ground_ready());
        assert!(field.load(Ordering::Relaxed) > 0);
        assert_eq!(old.load(Ordering::Relaxed), 0);
        assert_eq!(client.own().unwrap().state, ddsim::State::Fall);
        assert_eq!(client.predicted.iter().filter(|(f, _)| *f == 21).count(), 1);
    }
    #[test]
    fn remote_players_and_enemies_never_interpolate_across_stage_boundary() {
        use crate::mirror::{EnemyIdentity, EnemyFamily, EnemyRotation};
        let mut sim = Sim::new(Default::default()); sim.add_player(1, [0.0; 3], 0.0);
        let mut remote = Body::of(sim.player(1).unwrap()); remote.id = 2;
        let mut enemy = Enemy { id: 256, identity: EnemyIdentity { family: EnemyFamily::Goblin,
            model: "fixture/model".into(), banks: vec![] }, pos: [0.0; 3], rotation: EnemyRotation::Yaw(0.0),
            scale: [1.0; 3], action: 0, bank: 0, slot: 0, frame: 0.0, hp: 1.0, hp_max: 1.0, dead: false };
        let mut client = Client::new(Udp::bind("127.0.0.1:0").unwrap(),
            Udp::bind("127.0.0.1:0").unwrap().local_addr(), 2, 1, 2, None, sim, Box::new(Flat), 0);
        client.seat = Some(1);
        client.apply(snap(10, 100, vec![remote.clone()], vec![enemy.clone()]));
        remote.position = [10000.0; 3]; enemy.pos = [10000.0; 3];
        client.apply(snap(12, 200, vec![remote.clone()], vec![enemy.clone()]));
        assert_eq!(client.others_at(11.0)[0].position, remote.position);
        assert_eq!(client.enemies_at(11.0)[0].pos, enemy.pos);
        // Previous-stage snapshots remain available as wire delta baselines.
        assert_eq!(client.snaps.front().unwrap().frame, 10);
        remote.position = [20000.0; 3]; enemy.pos = [20000.0; 3];
        client.apply(snap(14, 200, vec![remote], vec![enemy]));
        assert_eq!(client.others_at(13.0)[0].position, [15000.0; 3]);
        assert_eq!(client.enemies_at(13.0)[0].pos, [15000.0; 3]);
    }
}

impl<T: Transport> Client<T> {
    /// Starts joining `server`. `sim` is configured like the host's (motions
    /// and parameters, `ddhost::install::sim`) and holds no players.
    /// `look` is this player's own Arisen as its save holds it.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transport: T,
        server: SocketAddr,
        profile: u64,
        build: u32,
        data: u64,
        look: Option<LookData>,
        sim: Sim,
        ground: Box<dyn Ground + Send>,
        now: u64,
    ) -> Self {
        let conn = (profile ^ now.rotate_left(17) ^ 0x5bd1_e995) as u32 | 1;
        let mut link = Link::new(conn, now);
        link.send_reliable(ToHost::Hello { protocol: PROTOCOL, build, data, profile, look }.encode());
        Client {
            transport,
            server,
            link,
            seat: None,
            refused: None,
            sim,
            ground,
            clock: FrameClock::default(),
            cmds: VecDeque::new(),
            next_cmd: 0,
            snaps: VecDeque::new(),
            predicted: VecDeque::new(),
            last_error: 0.0,
            max_error: 0.0,
            snapshots: 0,
            lead_changes: 0,
            hold: false,
            events: vec![],
            spot: None,
            looks: BTreeMap::new(),
            quest_camera: None,
            ground_pending: false,
            ground_stage: None,
            stage_start_frame: 0,
        }
    }

    /// Replaces the ground the prediction stands on (a guest that joined
    /// before loading the host's world).
    pub fn set_ground(&mut self, ground: Box<dyn Ground + Send>) {
        self.ground = ground;
        self.ground_stage = self.stage().or_else(|| self.spot.map(|spot| spot.stage));
        if !std::mem::take(&mut self.ground_pending) { return; }
        // Commands were suspended at the stage boundary. Reapply the latest
        // host body and replay them exactly once on the newly loaded ground.
        // Ordinary collision refreshes never replay already predicted commands.
        if let Some((frame, own)) = self.snaps.back().and_then(|s|
            self.seat.and_then(|id| s.bodies.iter().find(|b| b.id == id)).map(|b| (s.frame, b.clone()))) {
            if let Some(player) = self.sim.player_mut(own.id) { own.apply(player); }
            self.predicted.retain(|(f, _)| *f <= frame);
            let cmds: Vec<_> = self.cmds.iter().copied().filter(|(f, _)| *f > frame).collect();
            for (f, c) in cmds { self.predict(own.id, f, c); }
        }
    }

    pub fn rtt_ms(&self) -> f32 {
        self.link.rtt_ms
    }

    /// Reads every waiting packet from the host.
    pub fn pump(&mut self, now: u64) {
        while let Some((from, bytes)) = self.transport.recv(now) {
            if from != self.server {
                continue;
            }
            let Ok(d) = self.link.receive(&bytes, now) else { continue };
            for m in d.reliable {
                match ToClient::decode(&m) {
                    Ok(ToClient::Welcome { seat, frame, delta, spot }) => {
                        self.spot = spot;
                        self.seat = Some(seat);
                        self.clock = FrameClock::new(delta).unwrap_or_default();
                        self.next_cmd = frame + 1 + LEAD_MAX as u64;
                    }
                    Ok(ToClient::Refuse { reason }) => self.refused = Some(reason),
                    Ok(ToClient::Look { seat, look }) => {
                        self.looks.insert(seat, look);
                    }
                    Ok(ToClient::Left { seat }) => {
                        self.looks.remove(&seat);
                        self.events.push(ToClient::Left { seat });
                    }
                    Ok(e) => self.events.push(e),
                    Err(_) => {}
                }
            }
            if let Some(u) = d.unreliable {
                let snaps = &self.snaps;
                let base = |f: u64| snaps.iter().find(|s| s.frame == f).cloned();
                if let Ok(s) = Snapshot::decode(&u, &base) {
                    if self.snaps.back().is_none_or(|b| s.frame > b.frame) {
                        self.apply(s);
                    }
                }
            }
        }
    }

    fn apply(&mut self, s: Snapshot) {
        self.snapshots += 1;
        let stage_changed = self.snaps.back().is_some_and(|last| last.stage != s.stage);
        if stage_changed {
            // Until the view loads the new installed collision, show the host
            // body and keep commands queued rather than run them on old ground.
            self.stage_start_frame = s.frame;
        }
        self.ground_pending = s.stage.is_some_and(|stage| self.ground_stage != Some(stage));
        // Apply before replay as well as the next prediction step.
        self.quest_camera = s.quest_camera;
        if let (Some(seat), Some(n)) = (self.seat, s.cmd_newest) {
            let lead = n as i64 - s.frame as i64;
            // At most one step per snapshot.
            if lead < LEAD_MIN {
                self.next_cmd += 1;
                self.lead_changes += 1;
            } else if lead > LEAD_MAX + 2 {
                self.hold = true;
                self.lead_changes -= 1;
            }
            let _ = seat;
        }
        if let Some(own) = self.seat.and_then(|id| s.bodies.iter().find(|b| b.id == id)).cloned() {
            if let Some(&(_, p)) = self.predicted.iter().find(|(f, _)| *f == s.frame).filter(|_| !stage_changed) {
                let d: f32 = (0..3).map(|i| (p[i] - own.position[i]).powi(2)).sum::<f32>().sqrt();
                self.last_error = d;
                self.max_error = self.max_error.max(d);
            }
            if self.sim.player(own.id).is_none() {
                self.sim.add_player(own.id, own.position, own.yaw);
            }
            own.apply(self.sim.player_mut(own.id).unwrap());
            // Replay what the host has not run yet.
            while self.cmds.front().is_some_and(|(f, _)| *f <= s.frame) {
                self.cmds.pop_front();
            }
            self.predicted.retain(|(f, _)| *f <= s.frame);
            let cmds: Vec<(u64, PlayerInput)> = self.cmds.iter().copied().collect();
            for (f, c) in cmds {
                self.predict(own.id, f, c);
            }
        }
        self.snaps.push_back(s);
        if self.snaps.len() > SNAPSHOTS {
            self.snaps.pop_front();
        }
    }

    fn predict(&mut self, id: PlayerId, frame: u64, c: PlayerInput) {
        if self.ground_pending { return; }
        let input = if self.actions_blocked() { ddsim::Input::default() } else { c.sim };
        let inputs = HashMap::from([(id, input)]);
        self.sim.step(self.clock.seconds(), &inputs, self.ground.as_ref());
        if let Some(p) = self.sim.player(id) {
            self.predicted.push_back((frame, p.position));
            if self.predicted.len() > 256 {
                self.predicted.pop_front();
            }
        }
    }

    /// One client frame: stamp `input` for the next host frame, predict it
    /// and send the newest commands. Before the welcome it only resends
    /// the join request.
    pub fn frame(&mut self, input: PlayerInput, now: u64) {
        let payload = match self.seat {
            Some(seat) if !std::mem::take(&mut self.hold) => {
                let f = self.next_cmd;
                // A skip ahead leaves a gap: fill it with this command.
                let first = self.cmds.back().map_or(f, |(l, _)| l + 1);
                for g in first..=f {
                    self.cmds.push_back((g, input));
                    if self.sim.player(seat).is_some() {
                        self.predict(seat, g, input);
                    }
                }
                while self.cmds.len() > 128 {
                    self.cmds.pop_front();
                }
                self.next_cmd = f + 1;
                Some(self.inputs())
            }
            Some(_) => Some(self.inputs()),
            None => None,
        };
        let p = self.link.packet(payload.as_deref(), now);
        self.transport.send(self.server, &p, now);
    }

    fn inputs(&self) -> Vec<u8> {
        let n = self.cmds.len().min(REDUNDANCY);
        let tail: Vec<PlayerInput> = self.cmds.iter().skip(self.cmds.len() - n).map(|(_, c)| *c).collect();
        let newest = self.cmds.back().map_or(0, |(f, _)| *f);
        Inputs { newest, cmds: tail, snapshot_ack: self.snaps.back().map(|s| s.frame) }.encode()
    }

    /// Says goodbye (best effort).
    pub fn leave(&mut self, now: u64) {
        self.link.send_reliable(ToHost::Bye.encode());
        let p = self.link.packet(None, now);
        self.transport.send(self.server, &p, now);
    }

    /// The predicted body of this client's Arisen.
    pub fn own(&self) -> Option<Body> {
        self.sim.player(self.seat?).map(Body::of)
    }

    pub fn ground_ready(&self) -> bool { !self.ground_pending }

    pub fn stage(&self) -> Option<u32> { self.snaps.back().and_then(|s| s.stage) }

    pub fn quest_camera(&self) -> Option<crate::quest_camera::QuestCamera> { self.quest_camera }

    pub fn actions_blocked(&self) -> bool { self.quest_camera.is_some_and(|c| c.input_locked) }

    /// The newest snapshot.
    pub fn latest(&self) -> Option<&Snapshot> {
        self.snaps.back()
    }

    /// The other Arisen at host time `t` (fractional frames), interpolated
    /// between the snapshots around it; past the newest, the newest.
    pub fn others_at(&self, t: f64) -> Vec<Body> {
        let own = self.seat;
        let Some(last) = self.snaps.back() else { return vec![] };
        let t = t.max(self.stage_start_frame as f64);
        let after = self.snaps.iter().position(|s| s.frame as f64 >= t);
        let pick = |s: &Snapshot| s.bodies.iter().filter(|b| Some(b.id) != own).cloned().collect::<Vec<_>>();
        match after {
            Some(0) | None => pick(if after.is_none() { last } else { &self.snaps[0] }),
            Some(i) => {
                let (a, b) = (&self.snaps[i - 1], &self.snaps[i]);
                if a.frame < self.stage_start_frame { return pick(b); }
                let k = ((t - a.frame as f64) / (b.frame - a.frame) as f64) as f32;
                pick(b)
                    .into_iter()
                    .map(|bb| match a.bodies.iter().find(|x| x.id == bb.id) {
                        Some(ab) => Body::lerp(ab, &bb, k),
                        None => bb,
                    })
                    .collect()
            }
        }
    }

    /// HP of every seat in the fight, from the newest snapshot.
    pub fn vitals(&self) -> &[Vital] {
        self.snaps.back().map_or(&[], |s| &s.vitals)
    }

    /// The fight's enemies at host time `t` (fractional frames),
    /// interpolated like the other Arisen.
    pub fn enemies_at(&self, t: f64) -> Vec<Enemy> {
        let Some(last) = self.snaps.back() else { return vec![] };
        let t = t.max(self.stage_start_frame as f64);
        match self.snaps.iter().position(|s| s.frame as f64 >= t) {
            None => last.enemies.clone(),
            Some(0) => self.snaps[0].enemies.clone(),
            Some(i) => {
                let (a, b) = (&self.snaps[i - 1], &self.snaps[i]);
                if a.frame < self.stage_start_frame { return b.enemies.clone(); }
                let k = ((t - a.frame as f64) / (b.frame - a.frame) as f64) as f32;
                b.enemies
                    .iter()
                    .map(|e| match a.enemies.iter().find(|x| x.id == e.id) {
                        Some(p) => Enemy::lerp(p, e, k),
                        None => e.clone(),
                    })
                    .collect()
            }
        }
    }

    /// The NPCs of the newest snapshot (the views walk them there).
    pub fn npcs(&self) -> Vec<crate::mirror::NpcBody> {
        self.snaps.back().map(|s| s.npcs.clone()).unwrap_or_default()
    }

    /// The enemies where the views show them (as [`Client::others`]).
    pub fn enemies(&self, since_ms: f64) -> Vec<Enemy> {
        let Some(last) = self.snaps.back() else { return vec![] };
        let frame_ms = 1000.0 * f64::from(self.clock.seconds());
        self.enemies_at(last.frame as f64 - INTERP_FRAMES + (since_ms / frame_ms).min(1.0))
    }

    /// The others where the views show them: [`INTERP_FRAMES`] behind the
    /// newest snapshot, `since_ms` after it arrived.
    pub fn others(&self, since_ms: f64) -> Vec<Body> {
        let Some(last) = self.snaps.back() else { return vec![] };
        let frame_ms = 1000.0 * f64::from(self.clock.seconds());
        self.others_at(last.frame as f64 - INTERP_FRAMES + (since_ms / frame_ms).min(1.0))
    }
}
