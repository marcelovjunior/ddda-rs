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
        }
    }

    /// Replaces the ground the prediction stands on (a guest that joined
    /// before loading the host's world).
    pub fn set_ground(&mut self, ground: Box<dyn Ground + Send>) {
        self.ground = ground;
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
            if let Some(&(_, p)) = self.predicted.iter().find(|(f, _)| *f == s.frame) {
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
        let inputs = HashMap::from([(id, c.sim)]);
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

    /// The newest snapshot.
    pub fn latest(&self) -> Option<&Snapshot> {
        self.snaps.back()
    }

    /// The other Arisen at host time `t` (fractional frames), interpolated
    /// between the snapshots around it; past the newest, the newest.
    pub fn others_at(&self, t: f64) -> Vec<Body> {
        let own = self.seat;
        let Some(last) = self.snaps.back() else { return vec![] };
        let after = self.snaps.iter().position(|s| s.frame as f64 >= t);
        let pick = |s: &Snapshot| s.bodies.iter().filter(|b| Some(b.id) != own).cloned().collect::<Vec<_>>();
        match after {
            Some(0) | None => pick(if after.is_none() { last } else { &self.snaps[0] }),
            Some(i) => {
                let (a, b) = (&self.snaps[i - 1], &self.snaps[i]);
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
        match self.snaps.iter().position(|s| s.frame as f64 >= t) {
            None => last.enemies.clone(),
            Some(0) => self.snaps[0].enemies.clone(),
            Some(i) => {
                let (a, b) = (&self.snaps[i - 1], &self.snaps[i]);
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
