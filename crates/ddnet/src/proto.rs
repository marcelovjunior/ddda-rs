//! Messages (ADR 0002 sections 8.3-8.5). Every message is a design
//! decision; the original has no LAN co-op.

use crate::body::{self, Body};
use ddhost::PlayerInput;
use ddid::{PlayerId, Reader, Result, Writer};

/// Bumped on any incompatible change.
pub const PROTOCOL: u16 = 1;

/// Reliable messages from a client.
#[derive(Debug, Clone, PartialEq)]
pub enum ToHost {
    /// Join request. `build` is the exe's PE timestamp (1513180209 for the
    /// supported build); `data` a fingerprint of the install's archives.
    Hello { protocol: u16, build: u32, data: u64, profile: u64 },
    Bye,
}

/// Reliable messages from the host.
#[derive(Debug, Clone, PartialEq)]
pub enum ToClient {
    Welcome { seat: PlayerId, frame: u64, delta: f32 },
    Refuse { reason: String },
    Joined { seat: PlayerId, profile: u64 },
    Left { seat: PlayerId },
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
            ToHost::Hello { protocol, build, data, profile } => w.u8(0).u16(*protocol).u32(*build).u64(*data).u64(*profile),
            ToHost::Bye => w.u8(1),
        };
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<ToHost> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            0 => ToHost::Hello { protocol: r.u16()?, build: r.u32()?, data: r.u64()?, profile: r.u64()? },
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
            ToClient::Welcome { seat, frame, delta } => w.u8(0).u8(*seat).u64(*frame).f32(*delta),
            ToClient::Refuse { reason } => {
                w.u8(1);
                string(&mut w, reason);
                &mut w
            }
            ToClient::Joined { seat, profile } => w.u8(2).u8(*seat).u64(*profile),
            ToClient::Left { seat } => w.u8(3).u8(*seat),
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
            0 => ToClient::Welcome { seat: r.u8()?, frame: r.u64()?, delta: r.f32()? },
            1 => ToClient::Refuse { reason: read_string(&mut r)? },
            2 => ToClient::Joined { seat: r.u8()?, profile: r.u64()? },
            3 => ToClient::Left { seat: r.u8()? },
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
    /// Encodes against the baseline bodies of frame `self.base`.
    pub fn encode(&self, base: &[Body]) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.frame);
        opt_u64(&mut w, self.base);
        opt_u64(&mut w, self.cmd_newest);
        w.len(self.bodies.len());
        for b in &self.bodies {
            body::write(&mut w, b, base.iter().find(|x| x.id == b.id));
        }
        w.finish()
    }

    /// Decodes; `baseline` gives the bodies of a base frame the client
    /// still holds.
    pub fn decode(b: &[u8], baseline: &dyn Fn(u64) -> Option<Vec<Body>>) -> Result<Snapshot> {
        let mut r = Reader::new(b);
        let frame = r.u64()?;
        let base = read_opt_u64(&mut r)?;
        let cmd_newest = read_opt_u64(&mut r)?;
        let bodies_base = match base {
            Some(f) => baseline(f).ok_or_else(|| format!("snapshot {frame}: baseline {f} not held"))?,
            None => vec![],
        };
        let n = r.len(3)?;
        let bodies = (0..n)
            .map(|_| body::read(&mut r, &|id| bodies_base.iter().find(|x| x.id == id).cloned()))
            .collect::<Result<_>>()?;
        r.end()?;
        Ok(Snapshot { frame, base, cmd_newest, bodies })
    }
}
