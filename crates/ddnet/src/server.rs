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

impl<D: Data, W: StoryWorld, T: Transport> Server<D, W, T> {
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
                    }) => self.hello(from, protocol, build, data, profile, look),
                    Ok(ToHost::Bye) => self.drop_client(from, "left"),
                    Err(_) => {}
                }
            }
            let Some(r) = self.clients.get_mut(&from) else {
                continue;
            };
            if let (Some(u), Some(_)) = (d.unreliable, r.seat) {
                if let Ok(inp) = Inputs::decode(&u) {
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
                    frame: self.host.frame,
                    delta: self.host.clock.delta,
                    spot: self.config.spot,
                };
                r.link.send_reliable(welcome.encode());
                if let Some((frame, row)) = self.active_event {
                    r.link.send_reliable(ToClient::PlayEvent { frame, row }.encode());
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
            }
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
            frame: report.frame,
            stage: self.host.story.as_ref().map(|s| s.world.stage_no),
            base: None,
            cmd_newest: None,
            bodies,
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
            for q in &report.requests {
                r.link.send_reliable(
                    ToClient::Story {
                        frame: report.frame,
                        text: format!("{q:?}"),
                    }
                    .encode(),
                );
                if let ddstory::Request::PlayEvent { row, .. } = q {
                    r.link.send_reliable(
                        ToClient::PlayEvent {
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
        for (addr, remote) in &mut self.clients {
            let packet = remote.link.packet(None, now);
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
