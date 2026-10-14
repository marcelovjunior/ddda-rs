//! Messages (ADR 0002 sections 8.3-8.5). Every message is a design
//! decision; the original has no LAN co-op.

use crate::body::{self, Body};
use crate::mirror::{self, Enemy, LookData, NpcBody, Spot, Vital};
use ddhost::PlayerInput;
use ddid::{PlayerId, Reader, Result, Writer};
use std::collections::BTreeMap;

/// Bumped on any incompatible change.
///
/// - 1: m3-lan (bodies only).
/// - 2: light and heavy attack input, the Attack and Shot states; the
///   guest's look in Hello; the host's spot in Welcome; Look events; HP
///   and enemies in snapshots.
/// - 3: NPC bodies in snapshots; PlayEvent (cutscenes).
/// - 6: per-seat bow held/toggle input and bow layers/secondary slot in bodies.
/// - 7: enemy resource identity, full rotation/scale and field-group deltas.
/// - 8: four held pawn-order direction bits in each per-seat input.
/// - 9: loaded story stage; authored quest camera clock/explicit end/input lock; NPC lifetime,
///   pose/motion, gauge and health/death state with masked deltas.
/// - 10: reliable host-selected pawn speech with persistent-profile audiences.
/// - 11: grounded death bodies and transactional retry attempt epochs.
/// - 12: the shield as a secondary weapon (S57 Shield Summons).
pub const PROTOCOL: u16 = 12;

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
    Welcome { seat: PlayerId, epoch: u64, frame: u64, delta: f32, spot: Option<Spot> },
    Refuse { reason: String },
    Joined { seat: PlayerId, profile: u64 },
    Left { seat: PlayerId },
    /// The look of a seat (an Arisen, or a pawn's seat), once per seat.
    Look { seat: PlayerId, look: LookData },
    /// A story request of host frame `frame` (its debug text until the
    /// views perform requests).
    Story { epoch: u64, frame: u64, text: String },
    /// The story asked for cutscene `row` (ddstory `Request::PlayEvent`) on
    /// host frame `frame`: every view plays it from the data; the host's
    /// own playback reports it over.
    PlayEvent { epoch: u64, frame: u64, row: u32 },
    PawnMessage { epoch: u64, message: ddhost::pawn_chatter::Message },
    Retry(RetryNotice),
}

/// Project co-op policy: one reliable attempt replacement notice. A whole
/// epoch-tagged snapshot follows with no prior-attempt delta baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryNotice {
    pub epoch: u64,
    pub frame: u64,
    pub spot: Option<Spot>,
    pub looks: BTreeMap<PlayerId, LookData>,
    pub dismissals: Vec<ddhost::death::ProfileNotice>,
}

/// The unreliable input upload: the newest commands, oldest first; the
/// last is for host frame `newest`.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub epoch: u64,
    pub newest: u64,
    pub cmds: Vec<PlayerInput>,
    /// Newest snapshot frame the client has applied (the delta baseline).
    pub snapshot_ack: Option<u64>,
}

/// The unreliable state upload: every Arisen after host frame `frame`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub epoch: u64,
    pub frame: u64,
    /// Loaded story stage; no story hosts leave this absent.
    pub stage: Option<u32>,
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
    /// The NPCs on their schedules (`Host::npcs`).
    pub npcs: Vec<NpcBody>,
    /// None explicitly releases the host-owned camera on the guest.
    pub quest_camera: Option<crate::quest_camera::QuestCamera>,
    pub death: ddhost::death::Report,
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
            ToClient::Welcome { seat, epoch, frame, delta, spot } => {
                w.u8(0).u8(*seat).u64(*epoch).u64(*frame).f32(*delta).bool(spot.is_some());
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
            ToClient::Story { epoch, frame, text } => {
                w.u8(4).u64(*epoch).u64(*frame);
                string(&mut w, text);
                &mut w
            }
            ToClient::PlayEvent { epoch, frame, row } => w.u8(6).u64(*epoch).u64(*frame).u32(*row),
            ToClient::PawnMessage { epoch, message } => {
                use ddhost::pawn_chatter::Identity;
                w.u8(7).u64(*epoch).u64(message.id).u64(message.frame).u8(message.pawn_slot).u8(message.pawn_seat);
                match &message.pawn_identity {
                    Identity::Main { profile } => { w.u8(0).u64(profile.0); }
                    Identity::Hired { hirer, rom_id, network_id, hired_at } => {
                        w.u8(1).u64(hirer.0).u32(*rom_id).bytes(network_id).u32(*hired_at);
                    }
                }
                w.u32(message.message);
                string(&mut w, &message.name);
                w.u8(message.gender).u8(message.voice).len(message.audience.len());
                for profile in &message.audience { w.u64(profile.0); }
                &mut w
            }
            ToClient::Retry(notice) => {
                w.u8(8).u64(notice.epoch).u64(notice.frame).bool(notice.spot.is_some());
                if let Some(spot) = notice.spot { mirror::write_spot(&mut w, &spot); }
                w.len(notice.looks.len());
                for (seat, look) in &notice.looks { w.u8(*seat); mirror::write_look(&mut w, look); }
                w.len(notice.dismissals.len());
                for dismissal in &notice.dismissals {
                    let pawn = &dismissal.released;
                    w.u64(dismissal.profile.0).u8(dismissal.slot).u32(pawn.rom_id).bytes(&pawn.net_id);
                    for count in [pawn.with_time, pawn.employ, pawn.die, pawn.clear_quest, pawn.lent] { w.u32(count); }
                }
                &mut w
            }
        };
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<ToClient> {
        let mut r = Reader::new(b);
        let m = match r.u8()? {
            0 => {
                let (seat, epoch, frame, delta) = (r.u8()?, r.u64()?, r.u64()?, r.f32()?);
                let spot = if r.bool()? { Some(mirror::read_spot(&mut r)?) } else { None };
                ToClient::Welcome { seat, epoch, frame, delta, spot }
            }
            1 => ToClient::Refuse { reason: read_string(&mut r)? },
            2 => ToClient::Joined { seat: r.u8()?, profile: r.u64()? },
            3 => ToClient::Left { seat: r.u8()? },
            5 => ToClient::Look { seat: r.u8()?, look: mirror::read_look(&mut r)? },
            4 => ToClient::Story { epoch: r.u64()?, frame: r.u64()?, text: read_string(&mut r)? },
            6 => ToClient::PlayEvent { epoch: r.u64()?, frame: r.u64()?, row: r.u32()? },
            7 => {
                use ddhost::{pawn_chatter::{Identity, Message}, ProfileId};
                let epoch = r.u64()?;
                let (id, frame, pawn_slot, pawn_seat) = (r.u64()?, r.u64()?, r.u8()?, r.u8()?);
                if id == 0 || pawn_slot >= 3 { return Err("invalid pawn message identity/slot".into()); }
                let pawn_identity = match r.u8()? {
                    0 => Identity::Main { profile: ProfileId(r.u64()?) },
                    1 => Identity::Hired { hirer: ProfileId(r.u64()?), rom_id: r.u32()?,
                        network_id: r.bytes()?, hired_at: r.u32()? },
                    _ => return Err("invalid pawn admission identity".into()),
                };
                let message = r.u32()?;
                let name = read_string(&mut r)?;
                let (gender, voice) = (r.u8()?, r.u8()?);
                let count = r.len(8)?;
                let audience = (0..count).map(|_| r.u64().map(ProfileId)).collect::<Result<_>>()?;
                ToClient::PawnMessage { epoch, message: Message { id, frame, pawn_slot, pawn_seat, pawn_identity,
                    message, name, gender, voice, audience } }
            }
            8 => {
                let epoch = r.u64()?;
                let frame = r.u64()?;
                if epoch == 0 { return Err("retry epoch must advance".into()); }
                let spot = if r.bool()? { Some(mirror::read_spot(&mut r)?) } else { None };
                if spot.is_some_and(|s| !s.yaw.is_finite() || s.pos.iter().any(|v| !v.is_finite())) {
                    return Err("invalid retry spot".into());
                }
                let mut looks = BTreeMap::new();
                for _ in 0..r.len(1)? {
                    if looks.insert(r.u8()?, mirror::read_look(&mut r)?).is_some() {
                        return Err("duplicate retry look seat".into());
                    }
                }
                let mut dismissals = Vec::new();
                for _ in 0..r.len(33)? {
                    let profile = ddhost::ProfileId(r.u64()?);
                    let slot = r.u8()?;
                    let rom_id = r.u32()?;
                    let net_id = r.bytes()?;
                    if !(1..=2).contains(&slot) || net_id.len() != 64 {
                        return Err("invalid retry dismissal slot/identity".into());
                    }
                    let released = ddhost::death::PawnRef { rom_id, net_id,
                        with_time: r.u32()?, employ: r.u32()?, die: r.u32()?,
                        clear_quest: r.u32()?, lent: r.u32()? };
                    dismissals.push(ddhost::death::ProfileNotice { profile, slot, released });
                }
                ToClient::Retry(RetryNotice { epoch, frame, spot, looks, dismissals })
            }
            k => return Err(format!("client message kind {k}")),
        };
        r.end()?;
        Ok(m)
    }
}

fn input(w: &mut Writer, i: &PlayerInput) {
    let s = &i.sim;
    w.f32(s.stick[0]).f32(s.stick[1]).bool(s.walk).bool(s.sprint).bool(s.jump).f32(s.camera_yaw);
    w.bool(s.light).bool(s.heavy).bool(s.bow).bool(s.bow_toggle).bool(i.interact);
    w.u8((s.pawn_orders.native_bits() >> 19) as u8);
}

fn read_input(r: &mut Reader) -> Result<PlayerInput> {
    let stick = [r.f32()?, r.f32()?];
    let (walk, sprint, jump, camera_yaw) = (r.bool()?, r.bool()?, r.bool()?, r.f32()?);
    let (light, heavy) = (r.bool()?, r.bool()?);
    let (bow, bow_toggle) = (r.bool()?, r.bool()?);
    let interact = r.bool()?;
    let bits = r.u8()?;
    if bits & 0xf0 != 0 { return Err(format!("unknown pawn order direction bits {bits:#x}")); }
    let pawn_orders = ddsim::PawnOrderButtons {
        up: bits & 1 != 0, down: bits & 2 != 0,
        left: bits & 4 != 0, right: bits & 8 != 0,
    };
    let sim = ddsim::Input { stick, walk, sprint, jump, camera_yaw, light, heavy, bow, bow_toggle, pawn_orders };
    Ok(PlayerInput { sim, interact })
}

#[cfg(test)]
mod bow_input_tests {
    use super::*;

    #[test]
    fn bow_commands_round_trip_per_seat_without_changing_primary_buttons() {
        let first = PlayerInput { sim: ddsim::Input { bow: true, light: true,
            ..Default::default() }, interact: false };
        let second = PlayerInput { sim: ddsim::Input { bow_toggle: true, heavy: true,
            ..Default::default() }, interact: true };
        let mut w = Writer::new();
        input(&mut w, &first);
        input(&mut w, &second);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(read_input(&mut r).unwrap(), first);
        assert_eq!(read_input(&mut r).unwrap(), second);
        r.end().unwrap();
    }

    #[test]
    fn order_directions_round_trip_and_unknown_bits_are_rejected() {
        for bits in 0..16 {
            let command = PlayerInput { sim: ddsim::Input {
                pawn_orders: ddsim::PawnOrderButtons { up: bits & 1 != 0, down: bits & 2 != 0,
                    left: bits & 4 != 0, right: bits & 8 != 0 }, ..Default::default()
            }, ..Default::default() };
            let upload = Inputs { epoch: 0, newest: 42, cmds: vec![command, PlayerInput::default()], snapshot_ack: Some(37) };
            assert_eq!(Inputs::decode(&upload.encode()).unwrap(), upload);
            let mut writer = Writer::new();
            input(&mut writer, &command);
            let mut malformed = writer.finish();
            *malformed.last_mut().unwrap() |= 0x80;
            assert!(read_input(&mut Reader::new(&malformed)).unwrap_err().contains("unknown pawn order"));
        }
    }
}

impl Inputs {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.epoch).u64(self.newest).len(self.cmds.len());
        for c in &self.cmds {
            input(&mut w, c);
        }
        opt_u64(&mut w, self.snapshot_ack);
        w.finish()
    }
    pub fn decode(b: &[u8]) -> Result<Inputs> {
        let mut r = Reader::new(b);
        let epoch = r.u64()?;
        let newest = r.u64()?;
        let n = r.len(16)?;
        if newest.checked_add(1).is_none_or(|frames| frames < n as u64) {
            return Err("input command frame range overflow".into());
        }
        let cmds = (0..n).map(|_| read_input(&mut r)).collect::<Result<_>>()?;
        let snapshot_ack = read_opt_u64(&mut r)?;
        r.end()?;
        Ok(Inputs { epoch, newest, cmds, snapshot_ack })
    }
}

impl Snapshot {
    /// Encodes against `base`, the snapshot of frame `self.base`.
    pub fn encode(&self, base: Option<&Snapshot>) -> Vec<u8> {
        // Never reuse another attempt's actor/NPC baseline. A missing or
        // mismatched supplied baseline produces an explicit whole snapshot.
        let base = base.filter(|b| self.base == Some(b.frame) && b.epoch == self.epoch);
        let (bodies, enemies) = base.map_or((&[][..], &[][..]), |b| (&b.bodies[..], &b.enemies[..]));
        let mut w = Writer::new();
        w.u64(self.epoch).u64(self.frame);
        w.bool(self.stage.is_some()).u32(self.stage.unwrap_or(0));
        opt_u64(&mut w, base.map(|b| b.frame));
        opt_u64(&mut w, self.cmd_newest);
        w.len(self.bodies.len());
        for b in &self.bodies {
            body::write(&mut w, b, bodies.iter().find(|x| x.id == b.id));
        }
        mirror::write_vitals(&mut w, &self.vitals);
        mirror::write_enemies(&mut w, &self.enemies, enemies);
        mirror::write_npcs(&mut w, &self.npcs, base.map_or(&[], |s| &s.npcs));
        crate::quest_camera::write(&mut w, self.quest_camera);
        crate::death::write(&mut w, &self.death, base.map(|b| &b.death));
        w.finish()
    }

    /// Decodes; `baseline` gives a base snapshot the client still holds.
    pub fn decode(b: &[u8], baseline: &dyn Fn(u64) -> Option<Snapshot>) -> Result<Snapshot> {
        let mut r = Reader::new(b);
        let epoch = r.u64()?;
        let frame = r.u64()?;
        let stage = if r.bool()? { Some(r.u32()?) } else { r.u32()?; None };
        let base = read_opt_u64(&mut r)?;
        let cmd_newest = read_opt_u64(&mut r)?;
        let held = match base {
            Some(f) => Some(baseline(f).ok_or_else(|| format!("snapshot {frame}: baseline {f} not held"))?),
            None => None,
        };
        if held.as_ref().is_some_and(|s| s.epoch != epoch) {
            return Err("snapshot baseline belongs to another attempt".into());
        }
        let bodies_base = held.as_ref().map_or(&[][..], |s| &s.bodies[..]);
        let n = r.len(3)?;
        let bodies = (0..n)
            .map(|_| body::read(&mut r, &|id| bodies_base.iter().find(|x| x.id == id).cloned()))
            .collect::<Result<_>>()?;
        let vitals = mirror::read_vitals(&mut r)?;
        let enemies = mirror::read_enemies(&mut r, held.as_ref().map_or(&[][..], |s| &s.enemies[..]))?;
        let npcs = mirror::read_npcs(&mut r, held.as_ref().map_or(&[], |s| &s.npcs))?;
        let quest_camera = crate::quest_camera::read(&mut r)?;
        let death = crate::death::read(&mut r, held.as_ref().map(|s| &s.death))?;
        r.end()?;
        Ok(Snapshot { epoch, frame, stage, base, cmd_newest, bodies, vitals, enemies, npcs, quest_camera, death })
    }
}

#[cfg(test)]
mod pawn_message_tests {
    use super::*;
    use ddhost::{pawn_chatter::{Identity, Message}, ProfileId};

    #[test]
    fn reliable_pawn_message_preserves_admission_and_n_profile_audiences() {
        for pawn_identity in [Identity::Main { profile: ProfileId(41) },
            Identity::Hired { hirer: ProfileId(42), rom_id: u32::MAX,
                network_id: vec![7; 64], hired_at: 99 }] {
            let event = ToClient::PawnMessage { epoch: 9, message: Message { id: u64::MAX, frame: 110,
                pawn_slot: 1, pawn_seat: 6, pawn_identity, message: 300_123,
                name: "Synthetic pawn".into(), gender: 1, voice: 11,
                audience: vec![ProfileId(41), ProfileId(42), ProfileId(43)] } };
            let bytes = event.encode();
            assert_eq!(ToClient::decode(&bytes).unwrap(), event);
            for cut in [0, 1, bytes.len() - 1] { assert!(ToClient::decode(&bytes[..cut]).is_err()); }
            let mut invalid = bytes.clone();
            invalid[25] = 3; // opcode + epoch + id + frame = pawn slot offset.
            assert!(ToClient::decode(&invalid).is_err());
        }
    }
}

#[cfg(test)]
mod retry_epoch_tests {
    use super::*;
    fn snapshot(epoch: u64, frame: u64) -> Snapshot {
        Snapshot { epoch, frame, stage: Some(100), base: None, cmd_newest: None,
            bodies: Vec::new(), vitals: Vec::new(), enemies: Vec::new(), npcs: Vec::new(), quest_camera: None, death: Default::default() }
    }
    #[test]
    fn retry_notice_preserves_epoch_restored_looks_and_typed_owner_dismissal() {
        let notice = RetryNotice { epoch: 2, frame: 7, spot: Some(Spot { stage: 100,
            cell: (0, 0), pos: [1.0, 2.0, 3.0], yaw: 0.5 }),
            looks: [(1, LookData { edit: vec![("fixture".into(), 3)], items: vec![7] })].into_iter().collect(),
            dismissals: vec![ddhost::death::ProfileNotice { profile: ddhost::ProfileId(200),
                slot: 1, released: ddhost::death::PawnRef::rom(7) }] };
        let message = ToClient::Retry(notice.clone());
        let bytes = message.encode();
        assert_eq!(ToClient::decode(&bytes).unwrap(), message);
        for cut in 0..bytes.len() { assert!(ToClient::decode(&bytes[..cut]).is_err()); }
        let mut trailing = bytes; trailing.push(0);
        assert!(ToClient::decode(&trailing).is_err());
        let mut invalid = notice.clone(); invalid.epoch = 0;
        assert!(ToClient::decode(&ToClient::Retry(invalid).encode()).is_err());
        let mut invalid = notice.clone(); invalid.dismissals[0].slot = 0;
        assert!(ToClient::decode(&ToClient::Retry(invalid).encode()).is_err());
        let mut invalid = notice; invalid.dismissals[0].released.net_id.pop();
        assert!(ToClient::decode(&ToClient::Retry(invalid).encode()).is_err());
    }
    #[test]
    fn snapshot_epoch_separates_whole_and_delta_baselines_after_frame_rollback() {
        let old = snapshot(0, 900);
        let mut new = snapshot(1, 7); new.base = Some(900);
        let decoded = Snapshot::decode(&new.encode(Some(&old)), &|_| Some(old.clone())).unwrap();
        assert_eq!((decoded.epoch, decoded.frame, decoded.base), (1, 7, None));
        let mut old_delta = snapshot(0, 901); old_delta.base = Some(900);
        let bytes = old_delta.encode(Some(&old));
        let mut wrong_epoch = old.clone(); wrong_epoch.epoch = 1;
        assert!(Snapshot::decode(&bytes, &|_| Some(wrong_epoch.clone())).unwrap_err()
            .contains("another attempt"));
        assert_eq!(Snapshot::decode(&bytes, &|_| Some(old.clone())).unwrap(), old_delta);
    }
    #[test]
    fn input_epoch_roundtrip_rejects_command_frame_underflow_and_overflow() {
        let input = Inputs { epoch: 2, newest: 8, cmds: vec![PlayerInput::default(); 2], snapshot_ack: Some(7) };
        assert_eq!(Inputs::decode(&input.encode()).unwrap(), input);
        for newest in [0, u64::MAX] {
            let invalid = Inputs { newest, ..input.clone() };
            assert!(Inputs::decode(&invalid.encode()).unwrap_err().contains("overflow"));
        }
    }
}
