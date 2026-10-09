//! Messages (ADR 0002 sections 8.3-8.5). Every message is a design
//! decision; the original has no LAN co-op.

use crate::body::{self, Body};
use crate::mirror::{self, Enemy, LookData, Spot, Vital};
use ddhost::PlayerInput;
use ddid::{PlayerId, Reader, Result, Writer};

/// Bumped on any incompatible change.
///
/// - 1: m3-lan (bodies only).
/// - 2: light and heavy attack input, the Attack and Shot states; the
///   guest's look in Hello; the host's spot in Welcome; Look events; HP
///   and enemies in snapshots.
pub const PROTOCOL: u16 = 2;

/// Reliable messages from a client.
#[derive(Debug, Clone, PartialEq)]
pub enum ToHost {
    /// Join request. `build` is the exe's PE timestamp (1513180209 for the
    /// supported build); `data` a fingerprint of the install's archives.
    /// `look`: the guest's own Arisen as its save holds it.
    Hello { protocol: u16, build: u32, data: u64, profile: u64, look: Option<LookData> },
    Bye,
}

/// Reliable messages from the host.
#[derive(Debug, Clone, PartialEq)]
pub enum ToClient {
    /// `spot`: where the host's session is, for the guest to load.
    Welcome { seat: PlayerId, frame: u64, delta: f32, spot: Option<Spot> },
    Refuse { reason: String },
    Joined { seat: PlayerId, profile: u64 },
    Left { seat: PlayerId },
    /// The look of a seat (an Arisen, or a pawn's seat), once per seat.
    Look { seat: PlayerId, look: LookData },
    /// A story request of host frame `frame` (its debug text until the
    /// views perform requests).
    Story { frame: u64, text: String },
}

/// The unreliable input upload: the newest commands, oldest first; the
/// last is for host frame `newest`.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub newest: u64,
    pub cmds: Vec<PlayerInput>,
    /// Newest snapshot frame the client has applied (the delta baseline).
    pub snapshot_ack: Option<u64>,
}

/// The unreliable state upload: every Arisen after host frame `frame`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub frame: u64,
    /// The snapshot frame the bodies are deltas against.
    pub base: Option<u64>,
    /// Newest command frame the host has received from this client (for
    /// the client's input lead).
    pub cmd_newest: Option<u64>,
    pub bodies: Vec<Body>,
    /// HP of every seat in the fight (Arisen and pawns).
    pub vitals: Vec<Vital>,
    /// The fight's enemies.
    pub enemies: Vec<Enemy>,
}

fn opt_u64(w: &mut Writer, v: Option<u64>) {
    w.bool(v.is_some()).u64(v.unwrap_or(0));
}

fn read_opt_u64(r: &mut Reader) -> Result<Option<u64>> {
    let (some, v) = (r.bool()?, r.u64()?);
    Ok(some.then_some(v))
}

fn string(w: &mut Writer, s: &str) {
    w.bytes(s.as_bytes());
}

fn read_string(r: &mut Reader) -> Result<String> {
    String::from_utf8(r.bytes()?).map_err(|e| e.to_string())
}

impl ToHost {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ToHost::Hello { protocol, build, data, profile, look } => {
                w.u8(0).u16(*protocol).u32(*build).u64(*data).u64(*profile).bool(look.is_some());
                if let Some(l) = look {
                    mirror::write_look(&mut w, l);
                }
                &mut w
            }
            ToHost::Bye => w.u8(1),
        };
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<ToHost> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            0 => {
                let (protocol, build, data, profile) = (r.u16()?, r.u32()?, r.u64()?, r.u64()?);
                // A peer of another protocol may end here: answer it with a
                // refusal rather than a decoding error.
                if protocol != PROTOCOL {
                    return Ok(ToHost::Hello { protocol, build, data, profile, look: None });
                }
                let look = if r.bool()? { Some(mirror::read_look(&mut r)?) } else { None };
                ToHost::Hello { protocol, build, data, profile, look }
            }
            1 => ToHost::Bye,
            k => return Err(format!("host message kind {k}")),
        };
        r.end()?;
        Ok(m)
    }
}

impl ToClient {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ToClient::Welcome { seat, frame, delta, spot } => {
                w.u8(0).u8(*seat).u64(*frame).f32(*delta).bool(spot.is_some());
                if let Some(s) = spot {
                    mirror::write_spot(&mut w, s);
                }
                &mut w
            }
            ToClient::Refuse { reason } => {
                w.u8(1);
                string(&mut w, reason);
                &mut w
            }
            ToClient::Joined { seat, profile } => w.u8(2).u8(*seat).u64(*profile),
            ToClient::Left { seat } => w.u8(3).u8(*seat),
            ToClient::Look { seat, look } => {
                w.u8(5).u8(*seat);
                mirror::write_look(&mut w, look);
                &mut w
            }
            ToClient::Story { frame, text } => {
                w.u8(4).u64(*frame);
                string(&mut w, text);
                &mut w
            }
        };
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<ToClient> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            0 => {
                let (seat, frame, delta) = (r.u8()?, r.u64()?, r.f32()?);
                let spot = if r.bool()? { Some(mirror::read_spot(&mut r)?) } else { None };
                ToClient::Welcome { seat, frame, delta, spot }
            }
            1 => ToClient::Refuse { reason: read_string(&mut r)? },
            2 => ToClient::Joined { seat: r.u8()?, profile: r.u64()? },
            3 => ToClient::Left { seat: r.u8()? },
            5 => ToClient::Look { seat: r.u8()?, look: mirror::read_look(&mut r)? },
            4 => ToClient::Story { frame: r.u64()?, text: read_string(&mut r)? },
            k => return Err(format!("client message kind {k}")),
        };
        r.end()?;
        Ok(m)
    }
}

fn input(w: &mut Writer, i: &PlayerInput) {
    let s = &i.sim;
    w.f32(s.stick[0]).f32(s.stick[1]).bool(s.walk).bool(s.sprint).bool(s.jump).f32(s.camera_yaw);
    w.bool(s.light).bool(s.heavy).bool(i.interact);
}

fn read_input(r: &mut Reader) -> Result<PlayerInput> {
    let stick = [r.f32()?, r.f32()?];
    let (walk, sprint, jump, camera_yaw) = (r.bool()?, r.bool()?, r.bool()?, r.f32()?);
    let (light, heavy) = (r.bool()?, r.bool()?);
    let sim = ddsim::Input { stick, walk, sprint, jump, camera_yaw, light, heavy };
    Ok(PlayerInput { sim, interact: r.bool()? })
}

impl Inputs {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.newest).len(self.cmds.len());
        for c in &self.cmds {
            input(&mut w, c);
        }
        opt_u64(&mut w, self.snapshot_ack);
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<Inputs> {
        let mut r = Reader::new(b);
        let newest = r.u64()?;
        let n = r.len(16)?;
        let cmds = (0..n).map(|_| read_input(&mut r)).collect::<Result<_>>()?;
        let snapshot_ack = read_opt_u64(&mut r)?;
        r.end()?;
        Ok(Inputs { newest, cmds, snapshot_ack })
    }
}

impl Snapshot {
    /// Encodes against `base`, the snapshot of frame `self.base`.
    pub fn encode(&self, base: Option<&Snapshot>) -> Vec<u8> {
        let (bodies, enemies) = base.map_or((&[][..], &[][..]), |b| (&b.bodies[..], &b.enemies[..]));
        let mut w = Writer::new();
        w.u64(self.frame);
        opt_u64(&mut w, self.base);
        opt_u64(&mut w, self.cmd_newest);
        w.len(self.bodies.len());
        for b in &self.bodies {
            body::write(&mut w, b, bodies.iter().find(|x| x.id == b.id));
        }
        mirror::write_vitals(&mut w, &self.vitals);
        mirror::write_enemies(&mut w, &self.enemies, enemies);
        w.finish()
    }

    /// Decodes; `baseline` gives a base snapshot the client still holds.
    pub fn decode(b: &[u8], baseline: &dyn Fn(u64) -> Option<Snapshot>) -> Result<Snapshot> {
        let mut r = Reader::new(b);
        let frame = r.u64()?;
        let base = read_opt_u64(&mut r)?;
        let cmd_newest = read_opt_u64(&mut r)?;
        let held = match base {
            Some(f) => Some(baseline(f).ok_or_else(|| format!("snapshot {frame}: baseline {f} not held"))?),
            None => None,
        };
        let bodies_base = held.as_ref().map_or(&[][..], |s| &s.bodies[..]);
        let n = r.len(3)?;
        let bodies = (0..n)
            .map(|_| body::read(&mut r, &|id| bodies_base.iter().find(|x| x.id == id).cloned()))
            .collect::<Result<_>>()?;
        let vitals = mirror::read_vitals(&mut r)?;
        let enemies = mirror::read_enemies(&mut r, held.as_ref().map_or(&[][..], |s| &s.enemies[..]))?;
        r.end()?;
        Ok(Snapshot { frame, base, cmd_newest, bodies, vitals, enemies })
    }
}
