//! The host's network side: a `ddhost::Host` plus one link per client
//! (ADR 0002 sections 8.3-8.5).
//!
//! Call [`Server::pump`] often (it reads every waiting packet), and
//! [`Server::step`] once per host frame with the host's own seats' input.

use crate::body::Body;
use crate::link::Link;
use crate::mirror::{self, LookData, Spot};
use crate::proto::{Inputs, Snapshot, ToClient, ToHost, PROTOCOL};
use crate::transport::Transport;
use ddhost::{Control, FrameInputs, FrameReport, Host, HostError, PlayerInput, ProfileId, StoryWorld};
use ddid::PlayerId;
use ddstory::data::Data;
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;

/// A client silent this long is dropped (ADR 0002 section 8.3).
pub const TIMEOUT_MS: u64 = 2000;
/// Snapshots kept as delta baselines: 64 frames, about 2 s at 30 Hz.
pub const HISTORY: usize = 64;

/// What a joining client must match.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// PE timestamp of the host's exe (1513180209).
    pub build: u32,
    /// Fingerprint of the host's install ([`crate::data_fingerprint`]).
    pub data: u64,
    /// Where joining Arisen appear (stage frame, cm); seat `n` stands
    /// `150 * n` cm along x.
    pub spawn: [f32; 3],
    /// The host's spot, sent in Welcome so a guest loads the same world.
    pub spot: Option<Spot>,
}

#[derive(Clone)]
struct Remote {
    link: Link,
    seat: Option<PlayerId>,
    profile: u64,
    /// Commands by host frame, not yet run.
    queue: BTreeMap<u64, PlayerInput>,
    /// The command used last; repeated while the next one is missing.
    last: PlayerInput,
    newest_cmd: Option<u64>,
    snapshot_ack: Option<u64>,
    /// Frames run without this client's command for them.
    misses: u64,
    /// Refused: close once the refusal is acknowledged.
    refused: bool,
}

pub struct Server<D: Data, W: StoryWorld, T: Transport> {
    pub host: Host<D, W>,
    pub transport: T,
    pub config: ServerConfig,
    clients: BTreeMap<SocketAddr, Remote>,
    /// Snapshots sent, as delta baselines (`cmd_newest` unset).
    history: VecDeque<Snapshot>,
    /// Looks by seat (the host's own, pawns', and joined guests').
    looks: BTreeMap<PlayerId, LookData>,
    /// Join, refusal and leave notes, for logs.
    pub log: Vec<String>,
    /// Current shared cutscene; replay its typed start to guests joining during it.
    active_event: Option<(u64, u32)>,
    /// Enable when the authoritative view reports cutscene completion with event_done.
    pub wait_for_events: bool,
}

/// Already-prepared installed presentation facts for the restored attempt.
/// Resources must be preflighted before taking the short transaction borrow.
pub struct RetryPresentation {
    pub spawn: [f32; 3],
    pub spot: Option<Spot>,
    pub looks: BTreeMap<PlayerId, LookData>,
}

/// Stages exact existing Link sequencing/ack state without constructing a
/// new connection or transport. Dropping it preserves the live host/network.
pub struct PreparedRetry<'a, D: Data, W: StoryWorld> {
    host: ddhost::death::PreparedAttempt<'a, D, W>,
    remotes: &'a mut BTreeMap<SocketAddr, Remote>,
    staged_remotes: BTreeMap<SocketAddr, Remote>,
    history: &'a mut VecDeque<Snapshot>,
    config: &'a mut ServerConfig,
    looks: &'a mut BTreeMap<PlayerId, LookData>,
    active_event: &'a mut Option<(u64, u32)>,
    presentation: RetryPresentation,
}

impl<D: Data, W: StoryWorld> PreparedRetry<'_, D, W> {
    pub fn candidate(&self) -> &Host<D, W> { self.host.candidate() }
    pub fn commit_retry(self) -> ddhost::death::RetryCommit {
        let result = self.host.commit_retry();
        *self.remotes = self.staged_remotes;
        self.history.clear();
        self.config.spawn = self.presentation.spawn;
        self.config.spot = self.presentation.spot;
        *self.looks = self.presentation.looks;
        *self.active_event = None;
        result
    }
}

impl<D: Data, W: StoryWorld, T: Transport> Server<D, W, T> {
    /// All save/world/presentation/notice validation happens before commit.
    /// The Server and its socket stay in place; staged links preserve every
    /// connection's reliable order/acks/sequence identity, retiring only old
    /// attempt commands and baselines. No transport IO occurs during commit.
    pub fn prepare_retry<F>(&mut self, checkpoint: &ddhost::save::SaveFile, data: D,
        factory: F, presentation: RetryPresentation) -> Result<PreparedRetry<'_, D, W>, HostError>
    where F: FnOnce(&ddhost::death::RetrySeed) -> Result<Host<D, W>, HostError> {
        if presentation.spawn.iter().any(|v| !v.is_finite())
            || presentation.spot.is_some_and(|s| !s.yaw.is_finite() || s.pos.iter().any(|v| !v.is_finite())) {
            return Err(HostError::Save("retry nonfinite network spawn".into()));
        }
        let attempt = self.host.prepare_retry(checkpoint, data, factory)?;
        let candidate = attempt.candidate();
        if candidate.frame.checked_add(1 + crate::client::LEAD_MAX as u64).is_none() {
            return Err(HostError::Save("retry frame exceeds input lead range".into()));
        }
        let mut staged_remotes = self.clients.clone();
        for remote in staged_remotes.values_mut() {
            let Some(seat) = remote.seat else { continue };
            if candidate.roster.seat(seat).is_none_or(|s| s.profile.0 != remote.profile) {
                return Err(HostError::Save("retry remote does not match preserved roster".into()));
            }
            let notice = crate::proto::RetryNotice {
                epoch: candidate.death.epoch(), frame: candidate.frame,
                spot: presentation.spot, looks: presentation.looks.clone(),
                dismissals: attempt.notices().iter().filter(|n| n.profile.0 == remote.profile).cloned().collect(),
            };
            let payload = ToClient::Retry(notice).encode();
            // Validate the complete codec while the live attempt is unchanged.
            ToClient::decode(&payload).map_err(HostError::Save)?;
            remote.queue.clear();
            remote.last = PlayerInput::default();
            remote.newest_cmd = None;
            remote.snapshot_ack = None;
            remote.misses = 0;
            remote.link.send_reliable(payload);
        }
        Ok(PreparedRetry { host: attempt, remotes: &mut self.clients, staged_remotes,
            history: &mut self.history, config: &mut self.config, looks: &mut self.looks,
            active_event: &mut self.active_event, presentation })
    }

    pub fn new(host: Host<D, W>, transport: T, config: ServerConfig) -> Self {
        Server {
            host,
            transport,
            config,
            clients: BTreeMap::new(),
            history: VecDeque::new(),
            looks: BTreeMap::new(),
            log: vec![],
            active_event: None,
            wait_for_events: false,
        }
    }

    /// Sets the look of a seat (the host's Arisen, a pawn's seat) and tells
    /// every guest.
    pub fn set_look(&mut self, seat: PlayerId, look: LookData) {
        let m = ToClient::Look {
            seat,
            look: look.clone(),
        }
        .encode();
        for r in self.clients.values_mut().filter(|r| r.seat.is_some()) {
            r.link.send_reliable(m.clone());
        }
        self.looks.insert(seat, look);
    }

    /// The looks known, by seat.
    pub fn looks(&self) -> &BTreeMap<PlayerId, LookData> {
        &self.looks
    }

    /// Seats of the clients, with their misses (frames run on a repeated
    /// command).
    pub fn remotes(&self) -> Vec<(PlayerId, u64)> {
        self.clients
            .values()
            .filter_map(|r| Some((r.seat?, r.misses)))
            .collect()
    }

    /// Reads every waiting packet: joins, leaves, inputs; drops silent
    /// clients.
    pub fn pump(&mut self, now: u64) {
        while let Some((from, bytes)) = self.transport.recv(now) {
            let Some(conn) = Link::peek_conn(&bytes) else {
                continue;
            };
            let fresh = |now| Remote {
                link: Link::new(conn, now),
                seat: None,
                profile: 0,
                queue: BTreeMap::new(),
                last: PlayerInput::default(),
                newest_cmd: None,
                snapshot_ack: None,
                misses: 0,
                refused: false,
            };
            let r = self.clients.entry(from).or_insert_with(|| fresh(now));
            if r.link.conn != conn {
                if r.seat.is_some() {
                    continue;
                }
                *r = fresh(now);
            }
            let Ok(d) = r.link.receive(&bytes, now) else {
                continue;
            };
            for m in d.reliable {
                match ToHost::decode(&m) {
                    Ok(ToHost::Hello {
                        protocol,
                        build,
                        data,
                        profile,
                        look,
                    }) => self.hello(from, protocol, build, data, profile, look, now),
                    Ok(ToHost::Bye) => self.drop_client(from, "left"),
                    Err(_) => {}
                }
            }
            let Some(r) = self.clients.get_mut(&from) else {
                continue;
            };
            if let (Some(u), Some(_)) = (d.unreliable, r.seat) {
                if let Ok(inp) = Inputs::decode(&u) {
                    if inp.epoch != self.host.death.epoch() { continue; }
                    let first = inp.newest + 1 - inp.cmds.len() as u64;
                    for (i, c) in inp.cmds.into_iter().enumerate() {
                        let f = first + i as u64;
                        if f > self.host.frame {
                            r.queue.insert(f, c);
                        }
                    }
                    r.newest_cmd = Some(r.newest_cmd.map_or(inp.newest, |n| n.max(inp.newest)));
                    if let Some(a) = inp.snapshot_ack {
                        r.snapshot_ack = Some(r.snapshot_ack.map_or(a, |x| x.max(a)));
                    }
                }
            }
        }
        let silent: Vec<SocketAddr> = self
            .clients
            .iter()
            .filter(|(_, r)| {
                now.saturating_sub(r.link.last_recv_ms) > TIMEOUT_MS || (r.refused && r.link.unacked() == 0)
            })
            .map(|(a, _)| *a)
            .collect();
        for a in silent {
            self.drop_client(a, "timed out or refused");
        }
    }

    fn hello(
        &mut self,
        from: SocketAddr,
        protocol: u16,
        build: u32,
        data: u64,
        profile: u64,
        look: Option<LookData>,
        now: u64,
    ) {
        let Some(r) = self.clients.get(&from) else { return };
        if r.seat.is_some() {
            return;
        }
        let refuse = if protocol != PROTOCOL {
            Some(format!("protocol {protocol}, host speaks {PROTOCOL}"))
        } else if build != self.config.build {
            Some(format!("exe build {build}, host has {}", self.config.build))
        } else if data != self.config.data {
            Some(format!(
                "game data {data:016x} differs from the host's {:016x}",
                self.config.data
            ))
        } else {
            None
        };
        let seat = match refuse {
            Some(_) => None,
            None => {
                let n = self.host.roster.len() as f32;
                let s = self.config.spawn;
                self.host.join(
                    ProfileId(profile),
                    Control::Remote,
                    [s[0] + 150.0 * n, s[1], s[2]],
                    0.0,
                )
            }
        };
        let r = self.clients.get_mut(&from).unwrap();
        match seat {
            None => {
                let reason =
                    refuse.unwrap_or_else(|| "the session is full, or this profile is already seated".into());
                self.log.push(format!("{from}: refused: {reason}"));
                r.link.send_reliable(ToClient::Refuse { reason }.encode());
                r.refused = true;
            }
            Some(seat) => {
                r.seat = Some(seat);
                r.profile = profile;
                let welcome = ToClient::Welcome {
                    seat,
                    epoch: self.host.death.epoch(),
                    frame: self.host.frame,
                    delta: self.host.clock.delta,
                    spot: self.config.spot,
                };
                r.link.send_reliable(welcome.encode());
                if let Some((frame, row)) = self.active_event {
                    r.link.send_reliable(ToClient::PlayEvent { epoch: self.host.death.epoch(), frame, row }.encode());
                }
                for s in self.host.roster.seats() {
                    r.link.send_reliable(
                        ToClient::Joined {
                            seat: s.id,
                            profile: s.profile.0,
                        }
                        .encode(),
                    );
                }
                for (&s, l) in &self.looks {
                    r.link.send_reliable(
                        ToClient::Look {
                            seat: s,
                            look: l.clone(),
                        }
                        .encode(),
                    );
                }
                self.log.push(format!("{from}: joined as seat {seat}"));
                let joined = ToClient::Joined { seat, profile }.encode();
                let look_msg = look.as_ref().map(|l| {
                    ToClient::Look {
                        seat,
                        look: l.clone(),
                    }
                    .encode()
                });
                for (a, o) in self.clients.iter_mut() {
                    if *a != from && o.seat.is_some() {
                        o.link.send_reliable(joined.clone());
                        if let Some(m) = &look_msg {
                            o.link.send_reliable(m.clone());
                        }
                    }
                }
                if let Some(l) = look {
                    self.looks.insert(seat, l);
                }
                // Late admission observes the CURRENT authoritative clock,
                // even while the host is only servicing heartbeats. Never
                // replay a stored quest-camera start or a cached old snapshot.
                let snapshot = self.current_snapshot();
                let payload = snapshot.encode(None);
                let remote = self.clients.get_mut(&from).unwrap();
                let packet = remote.link.packet(Some(&payload), now);
                self.transport.send(from, &packet, now);
            }
        }
    }

    fn current_snapshot(&self) -> Snapshot {
        let (vitals, enemies) = mirror::from_fight(self.host.fight.as_ref());
        Snapshot {
            epoch: self.host.death.epoch(),
            frame: self.host.frame,
            stage: self.host.story.as_ref().map(|s| s.world.stage_no),
            base: None,
            cmd_newest: None,
            bodies: self.host.sim.players.iter().map(Body::of).collect(),
            vitals, enemies,
            death: self.host.death.report(&self.host.roster),
            npcs: mirror::npcs_of(&self.host),
            quest_camera: self.host.world.quest_camera_active().map(|active|
                crate::quest_camera::QuestCamera::of(active, self.host.world.player_actions_blocked(0))),
        }
    }

    fn drop_client(&mut self, addr: SocketAddr, why: &str) {
        let Some(r) = self.clients.remove(&addr) else {
            return;
        };
        if let Some(seat) = r.seat {
            self.host.leave(seat);
            self.looks.remove(&seat);
            self.log.push(format!("{addr}: seat {seat} {why}"));
            let left = ToClient::Left { seat }.encode();
            for o in self.clients.values_mut() {
                o.link.send_reliable(left.clone());
            }
        }
    }

    /// Runs one host frame: the host's own `local` input plus each
    /// client's command for this frame (the last one again if it has not
    /// arrived; held buttons stay held), then sends every client its
    /// snapshot and the frame's story events.
    pub fn step(&mut self, local: &FrameInputs, now: u64) -> Result<FrameReport, HostError> {
        let frame = self.host.frame + 1;
        let mut inputs = local.clone();
        for r in self.clients.values_mut() {
            let Some(seat) = r.seat else { continue };
            r.queue.retain(|f, _| *f >= frame);
            match r.queue.remove(&frame) {
                Some(c) => r.last = c,
                None => r.misses += 1,
            }
            inputs.insert(seat, r.last);
        }
        if self.active_event.is_some() {
            for input in inputs.values_mut() {
                *input = PlayerInput::default();
            }
        }
        let report = self.host.tick(&inputs)?;
        for request in &report.requests {
            if let ddstory::Request::PlayEvent { row, .. } = request {
                if self.wait_for_events {
                    self.active_event = Some((report.frame, *row as u32));
                }
            }
        }
        let bodies: Vec<Body> = self.host.sim.players.iter().map(Body::of).collect();
        let (vitals, enemies) = mirror::from_fight(self.host.fight.as_ref());
        let npcs = mirror::npcs_of(&self.host);
        self.history.push_back(Snapshot {
            epoch: self.host.death.epoch(),
            frame: report.frame,
            stage: self.host.story.as_ref().map(|s| s.world.stage_no),
            base: None,
            cmd_newest: None,
            bodies,
            death: report.death.clone(),
            vitals,
            enemies,
            npcs,
            quest_camera: self.host.world.quest_camera_active().map(|active|
                crate::quest_camera::QuestCamera::of(active, self.host.world.player_actions_blocked(0))),
        });
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        for (addr, r) in self.clients.iter_mut() {
            if r.seat.is_some() {
                for message in &report.pawn_messages {
                    if message.heard_by(ProfileId(r.profile)) {
                        r.link.send_reliable(ToClient::PawnMessage { epoch: self.host.death.epoch(),
                            message: message.clone() }.encode());
                    }
                }
            }
            for q in &report.requests {
                r.link.send_reliable(
                    ToClient::Story {
                        epoch: self.host.death.epoch(),
                        frame: report.frame,
                        text: format!("{q:?}"),
                    }
                    .encode(),
                );
                if let ddstory::Request::PlayEvent { row, .. } = q {
                    r.link.send_reliable(
                        ToClient::PlayEvent {
                            epoch: self.host.death.epoch(),
                            frame: report.frame,
                            row: *row as u32,
                        }
                        .encode(),
                    );
                }
            }
            let payload = r.seat.map(|_| {
                let base = r
                    .snapshot_ack
                    .and_then(|a| self.history.iter().find(|s| s.frame == a));
                let now = self.history.back().expect("pushed above");
                let snap = Snapshot {
                    base: base.map(|b| b.frame),
                    cmd_newest: r.newest_cmd,
                    ..now.clone()
                };
                snap.encode(base)
            });
            let p = r.link.packet(payload.as_deref(), now);
            self.transport.send(*addr, &p, now);
        }
        Ok(report)
    }

    /// Service reliable acknowledgements while a presentation endpoint holds
    /// simulation time (for example the end of a bounded opening slice).
    pub fn heartbeat(&mut self, now: u64) {
        // Rebuild from the host rather than replaying history: a client that
        // joins while simulation is held still receives current camera/end.
        let payload = self.current_snapshot().encode(None);
        for (addr, remote) in &mut self.clients {
            let packet = remote.link.packet(remote.seat.map(|_| payload.as_slice()), now);
            self.transport.send(*addr, &packet, now);
        }
    }

    /// The authoritative view finished the shared cutscene. Guests cannot
    /// release this gate themselves; their local playback only affects presentation.
    pub fn event_done(&mut self) {
        self.active_event = None;
    }

    /// Hash of the bodies the host sent for `frame`, while it is kept.
    pub fn hash_at(&self, frame: u64) -> Option<u64> {
        self.history
            .iter()
            .find(|s| s.frame == frame)
            .map(crate::snapshot_hash)
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::transport::Udp;
    use ddhost::data::NoStoryData;
    type TestHost = Host<NoStoryData, ddstory::scripted::ScriptedHost>;
    type TestServer = Server<NoStoryData, ddstory::scripted::ScriptedHost, Udp>;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
    }
    fn setup() -> (TestServer, ddhost::save::SaveFile, SocketAddr) {
        let mut host = TestHost::new(ddsim::Sim::new(Default::default()), None,
            Default::default(), Box::new(Flat));
        host.join(ProfileId(100), Control::Local, [0.0; 3], 0.0).unwrap();
        host.join(ProfileId(200), Control::Remote, [400.0, 0.0, 0.0], 0.0).unwrap();
        host.frame = 7;
        let checkpoint = host.save().unwrap();
        host.frame = 900;
        host.death.session.enter_dead(ProfileId(100));
        let mut server = TestServer::new(host, Udp::bind("127.0.0.1:0").unwrap(),
            ServerConfig { build: 1, data: 2, spawn: [0.0; 3], spot: None });
        let remote_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut link = Link::new(42, 100);
        link.send_reliable(ToClient::Story { epoch: 0, frame: 900, text: "old attempt".into() }.encode());
        link.rtt_ms = 17.0;
        link.received = 9;
        let moving = PlayerInput { sim: ddsim::Input { stick: [1.0, 0.0],
            ..Default::default() }, ..Default::default() };
        server.clients.insert(remote_addr, Remote { link, seat: Some(1), profile: 200,
            queue: [(901, moving)].into_iter().collect(), last: moving,
            newest_cmd: Some(901), snapshot_ack: Some(899), misses: 3, refused: false });
        server.history.push_back(server.current_snapshot());
        server.active_event = Some((900, 7));
        (server, checkpoint, remote_addr)
    }
    fn presentation() -> RetryPresentation {
        RetryPresentation { spawn: [0.0; 3], spot: Some(Spot { stage: 100,
            cell: (0, 0), pos: [0.0; 3], yaw: 0.0 }), looks: Default::default() }
    }
    fn factory(seed: &ddhost::death::RetrySeed, template: &ddsim::Sim) -> Result<TestHost, HostError> {
        let mut host = TestHost::new(seed.sim(template, &Default::default())?, None,
            Default::default(), Box::new(Flat));
        host.roster = seed.roster.clone();
        Ok(host)
    }
    #[test]
    fn server_retry_keeps_connection_sequence_and_retires_old_commands_baselines() {
        let (mut server, checkpoint, addr) = setup();
        let socket = server.transport.local_addr();
        let old_link = server.clients[&addr].link.clone();
        let template = server.host.sim.fresh_attempt();
        let result = server.prepare_retry(&checkpoint, NoStoryData,
            |seed| factory(seed, &template), presentation()).unwrap().commit_retry();
        assert_eq!((result.epoch, result.frame), (1, 7));
        assert_eq!(server.transport.local_addr(), socket);
        let remote = server.clients.get_mut(&addr).unwrap();
        assert_eq!((remote.link.conn, remote.link.rtt_ms, remote.link.received),
            (old_link.conn, old_link.rtt_ms, old_link.received));
        assert_eq!(remote.link.unacked(), old_link.unacked() + 1);
        assert!(remote.queue.is_empty());
        assert_eq!(remote.last, PlayerInput::default());
        assert_eq!((remote.newest_cmd, remote.snapshot_ack), (None, None));
        let mut peer = Link::new(42, 100);
        let delivered = peer.receive(&remote.link.packet(None, 101), 101).unwrap();
        assert!(matches!(ToClient::decode(&delivered.reliable[0]).unwrap(), ToClient::Story { frame: 900, .. }));
        let ToClient::Retry(notice) = ToClient::decode(&delivered.reliable[1]).unwrap() else { panic!("retry notice"); };
        assert_eq!((notice.epoch, notice.frame), (1, 7));
        assert!(server.history.is_empty() && server.active_event.is_none());
        let snapshot = server.current_snapshot();
        assert_eq!((snapshot.epoch, snapshot.frame, snapshot.base), (1, 7, None));
        assert!(!server.host.player_actions_blocked(0));
    }
    #[test]
    fn server_failed_or_dropped_retry_preserves_host_connection_and_pending_input() {
        let (mut server, checkpoint, addr) = setup();
        let hash = server.host.state_hash();
        let template = server.host.sim.fresh_attempt();
        let mut invalid = presentation(); invalid.spawn[0] = f32::NAN;
        assert!(server.prepare_retry(&checkpoint, NoStoryData,
            |seed| factory(seed, &template), invalid).is_err());
        {
            let prepared = server.prepare_retry(&checkpoint, NoStoryData,
                |seed| factory(seed, &template), presentation()).unwrap();
            assert_eq!(prepared.candidate().frame, 7);
            drop(prepared);
        }
        assert_eq!(server.host.state_hash(), hash);
        let remote = &server.clients[&addr];
        assert_eq!((remote.link.conn, remote.link.unacked()), (42, 1));
        assert!(remote.queue.contains_key(&901));
        assert_eq!((remote.newest_cmd, remote.snapshot_ack), (Some(901), Some(899)));
        assert_eq!(server.history.back().unwrap().frame, 900);
        assert_eq!(server.active_event, Some((900, 7)));
    }
    #[test]
    fn server_discards_old_epoch_inputs_and_ack_before_they_can_poison_retry() {
        let (mut server, checkpoint, addr) = setup();
        let template = server.host.sim.fresh_attempt();
        server.prepare_retry(&checkpoint, NoStoryData,
            |seed| factory(seed, &template), presentation()).unwrap().commit_retry();
        // Inject through the actual datagram pump, preserving the connection
        // and testing epoch rejection before max-command/max-ack updates.
        let mut sender = Udp::bind("127.0.0.1:0").unwrap();
        let actual_addr = sender.local_addr();
        let remote = server.clients.remove(&addr).unwrap();
        server.clients.insert(actual_addr, remote);
        let mut peer = Link::new(42, 100);
        let payload = Inputs { epoch: 0, newest: 999, cmds: vec![PlayerInput::default()], snapshot_ack: Some(998) }.encode();
        sender.send(server.transport.local_addr(), &peer.packet(Some(&payload), 101), 101);
        server.pump(101);
        let remote = &server.clients[&actual_addr];
        assert!(remote.queue.is_empty());
        assert_eq!((remote.newest_cmd, remote.snapshot_ack), (None, None));
        let payload = Inputs { epoch: 1, newest: 8, cmds: vec![PlayerInput::default()], snapshot_ack: Some(7) }.encode();
        sender.send(server.transport.local_addr(), &peer.packet(Some(&payload), 102), 102);
        server.pump(102);
        let remote = &server.clients[&actual_addr];
        assert!(remote.queue.contains_key(&8));
        assert_eq!((remote.newest_cmd, remote.snapshot_ack), (Some(8), Some(7)));
    }
}
