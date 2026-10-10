//! Host-authoritative pawn speech. Native selectors and clocks are in
//! ddpawnai; this adapter binds their three pawn slots to admitted records.
//! Co-op policy: every currently seated profile hears shared-party speech
//! once. A reused seat cannot inherit another profile's messages. RNG is
//! a separate deterministic host stream; it is not native global-stream exact.

use crate::{ProfileId, PlayerId};
use ddpawnai::chatter::{self, ActorQueue, ActorRequest, Config, Configs, EnemyNotice,
    EnemyTables, OrderTables, Pending, Queue, SituationTables, UrgentQueue};
use ddpawnai::commands::Command;
use std::collections::BTreeMap;
use std::sync::Arc;
pub use ddpawnai::chatter::Bank;

#[derive(Debug)]
pub struct Data {
    pub orders: OrderTables,
    pub enemies: EnemyTables,
    pub situations: SituationTables,
    configs: BTreeMap<i32, Config>,
}
impl Data {
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let file = ddexe::ExeFile::next_to(&vfs.root)?;
        let exe = file.image()?;
        let mut configs = BTreeMap::new();
        let kind = mtarc::type_hash("rCmcMsgConfig");
        let mut paths: Vec<_> = vfs.paths_of_type(kind).collect();
        paths.sort_unstable();
        for path in paths {
            if !path.starts_with(r"id\message\pawn\") { continue; }
            for config in Configs::parse(&vfs.read(path, kind)?)?.0 {
                // CB45F0 searches only the bank resolved by 449930. A
                // same-number row in another bank cannot override it, and
                // duplicate rows in this bank keep the first native match.
                let Some(bank) = Bank::resolve(&exe, config.message as u32, 0, 0, false)? else { continue; };
                if ddvfs::key(&bank.text) == path { configs.entry(config.message).or_insert(config); }
            }
        }
        Ok(Self { orders: OrderTables::from_exe(&exe)?,
            enemies: EnemyTables::from_exe(&exe)?, situations: SituationTables::from_exe(&exe)?, configs })
    }

    fn config(&self, message: i32) -> Config {
        self.configs.get(&message).copied().unwrap_or_else(|| Configs(Vec::new()).get(message))
    }
}

/// One native party-pawn slot, independently of a human seat. The caller
/// projects actual admission, body vitality and implemented GOAP goals.
pub struct Pawn<'a> {
    pub slot: u8,
    pub seat: PlayerId,
    pub record: &'a ddpawn::cmc::Cmc,
    pub identity: Identity,
    pub living: bool,
    pub ready: bool,
    pub command: Command,
    pub goal: Option<u32>,
}

/// Session identity policy. ROM template alone is not an admission key:
/// a different hirer or hire lifetime must retire old pending requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Main { profile: ProfileId },
    Hired { hirer: ProfileId, rom_id: u32, network_id: Vec<u8>, hired_at: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyEvent { pub notice: EnemyNotice, pub kind: u32, pub variant: u32 }

/// An actor-authored situation, as 599BB0 receives it. Request-list index
/// is independent of CMC configuration priority. Near-death entry uses1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SituationEvent { pub pawn_slot: u8, pub situation: u32, pub list_index: u8 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Monotonically increasing within a host session, including load/reset.
    pub id: u64,
    pub frame: u64,
    pub pawn_slot: u8,
    pub pawn_seat: PlayerId,
    pub pawn_identity: Identity,
    pub message: u32,
    pub name: String,
    pub gender: u8,
    pub voice: u8,
    pub audience: Vec<ProfileId>,
}
impl Message {
    pub fn heard_by(&self, profile: ProfileId) -> bool { self.audience.contains(&profile) }
}

#[derive(Debug, Clone, PartialEq)]
struct Binding { identity: Identity, command: Command, actor: ActorQueue, urgent: UrgentQueue }

#[derive(Debug)]
pub struct Runtime {
    pub data: Option<Arc<Data>>,
    pub queue: Queue,
    bindings: BTreeMap<u8, Binding>,
    rng: ddpawn::rift::XorShift128,
    next_id: u64,
    /// Explicit unsupported paths; never substitute invented text or timing.
    pub errors: Vec<String>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self { data: None, queue: Queue::default(), bindings: BTreeMap::new(),
            // Session policy: isolated chatter stream, native seeded algorithm.
            rng: ddpawn::rift::XorShift128::seeded(0x1105), next_id: 0, errors: Vec::new() }
    }
}
impl Runtime {
    pub fn with_data(data: Arc<Data>) -> Self { Self { data: Some(data), ..Self::default() } }

    pub(crate) fn hash_into(&self, hash: &mut crate::hash::StateHasher) {
        // Installed tables and diagnostics do not belong to simulation state.
        hash.debug(&self.queue).debug(&self.bindings).debug(&self.rng).u64(self.next_id);
    }

    /// Successful load or stage replacement discards transient requests.
    /// Preserve IDs so a presenter never mistakes a new line for a duplicate.
    pub fn clear_pending(&mut self) {
        self.queue = Queue::default();
        self.bindings.clear();
    }

    fn error(&mut self, text: String) {
        if !self.errors.contains(&text) { self.errors.push(text); }
    }

    /// Call after pawn think consumes the prior frame's order mailbox. Changed
    /// active commands produce replies; repeated delivery and expiry do not.
    /// `enemies` must come from authoritative combat observations, never render
    /// distance. Native GUI acceptance/history and camera-target speech still
    /// need their shared adapter; they are not fabricated here.
    pub fn step(&mut self, frame: u64, frames: f32, clock_runs: bool,
        pawns: &[Pawn<'_>], authored: &[ddstory::Request], enemies: &[EnemyEvent], situations: &[SituationEvent],
        audience: &[ProfileId]) -> Vec<Message> {
        use ddpawn::rift::PawnRng;
        let Some(data) = self.data.clone() else { return Vec::new(); };
        self.bindings.retain(|slot, binding| pawns.iter().any(|pawn|
            pawn.slot == *slot && pawn.identity == binding.identity));
        self.queue.pending.retain(|pending| self.bindings.contains_key(&pending.speaker));
        for pawn in pawns.iter().filter(|pawn| pawn.slot < 3) {
            let prior = self.bindings.get(&pawn.slot).map(|binding| binding.command);
            let binding = self.bindings.entry(pawn.slot).or_insert_with(|| Binding {
                identity: pawn.identity.clone(), command: pawn.command, actor: ActorQueue::default(), urgent: UrgentQueue::default() });
            if pawn.ready && prior.is_some_and(|command| command != pawn.command)
                && pawn.command != Command::None {
                if let Some(message) = data.orders.select(pawn.command, pawn.goal,
                    pawn.record.use_scene_talk[0], || self.rng.next_u32()) {
                    binding.actor.offer(ActorRequest { message, grouped: true });
                }
            }
            if pawn.ready { binding.command = pawn.command; }
        }
        let mut living = [false; 3];
        let mut ready = [false; 3];
        for pawn in pawns.iter().filter(|pawn| pawn.slot < 3) {
            living[pawn.slot as usize] = pawn.living;
            ready[pawn.slot as usize] = pawn.ready;
        }
        if clock_runs {
            for event in situations {
                if event.pawn_slot >= 3 || event.list_index >= 2 { continue; }
                let Some(binding) = self.bindings.get_mut(&event.pawn_slot) else { continue; };
                // The producer applies the native actor's request guards.
                // HP0 alone cannot suppress its near-death cry (AFCFBF).
                if let Some(message) = data.situations.select(event.situation, || self.rng.next_u32()) {
                    let request = ActorRequest { message, grouped: false };
                    if event.list_index == 0 { binding.actor.offer(request); }
                    else { binding.urgent.offer(request); }
                }
            }
            for request in authored {
                if let ddstory::Request::PawnMessage { message, pawn_slot } = request {
                    if let Some(slot) = chatter::authored_speaker(*pawn_slot, living, ready) {
                        self.bindings.get_mut(&slot).unwrap().actor.offer(ActorRequest {
                            message: *message as i32, grouped: false });
                    }
                }
            }
            for enemy in enemies {
                if let Some(slot) = chatter::random_speaker(ready, || self.rng.next_u32()) {
                    let pawn = pawns.iter().find(|pawn| pawn.slot == slot).unwrap();
                    if let Some(message) = data.enemies.select(enemy.notice, enemy.kind,
                        enemy.variant, &pawn.record.study_flag, || self.rng.next_u32()) {
                        self.bindings.get_mut(&slot).unwrap().actor.offer(ActorRequest { message, grouped: false });
                    }
                }
            }
        }
        for pawn in pawns.iter().filter(|pawn| pawn.slot < 3) {
            let selected = self.bindings.get_mut(&pawn.slot).unwrap().actor.step(frames,
                clock_runs && pawn.ready, chatter::actor_cooldown(Some(pawn.record.info_ex_value[0])),
                |_| false, || self.rng.next_u32());
            let mut selected_requests: Vec<_> = selected.into_iter().collect();
            selected_requests.extend(self.bindings.get_mut(&pawn.slot).unwrap().urgent.step(clock_runs, |_| false));
            for selected in selected_requests {
                let config = data.config(selected.message);
                if config.immediate {
                    self.error(format!("immediate NPC message path unported: {}", selected.message));
                } else {
                    self.queue.offer(Pending { speaker: pawn.slot, message: selected.message,
                        priority: config.priority, remaining: config.expires, grouped: selected.grouped });
                }
            }
        }
        let Some(selected) = self.queue.step(frames, clock_runs, |_| true,
            || chatter::random_unit(self.rng.next_u32())) else { return Vec::new(); };
        let Some(pawn) = pawns.iter().find(|pawn| pawn.slot == selected.speaker) else { return Vec::new(); };
        self.next_id = self.next_id.checked_add(1).expect("host speech ID exhausted");
        vec![Message { id: self.next_id, frame, pawn_slot: pawn.slot,
            pawn_seat: pawn.seat, pawn_identity: pawn.identity.clone(), message: selected.message as u32,
            name: pawn.record.edit.name_str(), gender: pawn.record.edit.get("mGender").unwrap(),
            voice: pawn.record.edit.get("mVoice").unwrap(), audience: audience.to_vec() }]
    }
}

impl<D: ddstory::data::Data, W: crate::StoryWorld> crate::Host<D, W> {
    pub(crate) fn step_pawn_chatter(&mut self, frames: f32, authored: &[ddstory::Request],
        enemies: &[EnemyEvent], situations: &[SituationEvent]) -> Vec<Message> {
        use ddpawnai::pawn::{self, Goal};
        let audience: Vec<_> = self.roster.seats().iter().map(|seat| seat.profile).collect();
        let mut pawns = Vec::new();
        if let Some(fight) = &self.fight {
            for slot in 0..3u8 {
                let (seat, admission, identity) = if slot == 0 {
                    let seat = Self::main_pawn_seat();
                    let Some(admission) = self.rift.party.main_pawn.as_ref() else { continue; };
                    let Some(body) = fight.pawns.get(&seat) else { continue; };
                    let Some(owner) = self.roster.seat(body.owner) else { continue; };
                    (seat, admission, Identity::Main { profile: owner.profile })
                } else {
                    let index = slot as usize - 1;
                    if self.rift.party.reserved[index] { continue; }
                    let Some(admission) = self.rift.party.hired[index].as_ref() else { continue; };
                    let Some(hirer) = self.hired_by[index] else { continue; };
                    if !audience.contains(&hirer) { continue; }
                    (Self::hired_pawn_seat(index), admission, Identity::Hired {
                        hirer, rom_id: admission.online.rom_id, network_id: admission.online.net_id.clone(),
                        hired_at: admission.hire_time })
                };
                let Some(body) = fight.pawns.get(&seat) else { continue; };
                let Some(fighter) = fight.arisen.get(&seat) else { continue; };
                let living = !fighter.fighter.victim.dead();
                // Implemented readiness subset only: native brain/action flags
                // and GUI history still require their actor adapters.
                let ready = living && self.sim.player(seat).is_some()
                    && !self.world.player_actions_blocked(seat);
                let goal = match body.brain.goal.as_ref() {
                    Some(Goal::Wait) => Some(pawn::GOAL_WAIT),
                    Some(Goal::Follow { .. }) => Some(pawn::GOAL_FOLLOW),
                    Some(Goal::Attack(_)) => pawn::weapon_goal(body.brain.weapon),
                    None => None,
                };
                pawns.push(Pawn { slot, seat, record: &admission.pawn, identity, living, ready,
                    command: body.brain.orders.active, goal });
            }
        }
        // Presentation/simulation policy: a shared quest camera holds this
        // adapter's clocks; per-actor locks gate ordinary actor fetch above.
        // Native global sound/main flags are not represented yet.
        let clock_runs = self.world.quest_camera_active().is_none();
        self.pawn_chatter.step(self.frame, frames, clock_runs, &pawns, authored, enemies, situations, &audience)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
            (below >= 0.0).then_some(0.0)
        }
    }

    #[test]
    fn installed_host_mailbox_reply_waits_for_next_move_phase_and_load_is_atomic() {
        use crate::{Control, FrameInputs, Host, PlayerInput};
        let root = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !root.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(&root).unwrap();
        let file = ddexe::ExeFile::next_to(&root).unwrap();
        let mut host = Host::<crate::data::NoStoryData, ddstory::scripted::ScriptedHost>::new(
            ddsim::Sim::new(Default::default()), None, Default::default(), Box::new(Flat));
        host.join(ProfileId(41), Control::Local, [0.0;3], 0.0).unwrap();
        host.join(ProfileId(42), Control::Remote, [100.0,0.0,0.0], 0.0).unwrap();
        let mut fight = ddenemyrt::fight::Fight::new(Arc::new(
            ddenemyrt::fight::FightData::load(&vfs, &file.image().unwrap()).unwrap()));
        fight.add_arisen(0);
        fight.add_arisen(1);
        fight.add_pawn(&mut host.sim, Host::<crate::data::NoStoryData,
            ddstory::scripted::ScriptedHost>::hired_pawn_seat(0), 0,
            [0.0,0.0,200.0], 0.0, [500.0;9], 7);
        host.fight = Some(fight);
        host.hired_by[0] = Some(ProfileId(41));
        host.rift.party.hired[0] = Some(ddrift::rift::PartyPawn { pawn: ddpawn::cmc::Cmc::initial(),
            online: ddrift::log::PawnRef::rom(1), hire_time: 0, quests_cleared: 0 });
        host.pawn_chatter = Runtime::with_data(Arc::new(Data::load(&vfs).unwrap()));
        assert!(host.tick(&Default::default()).unwrap().pawn_messages.is_empty());
        let inputs = FrameInputs::from([(0, PlayerInput { sim: ddsim::Input {
            pawn_orders: ddsim::PawnOrderButtons { up: true, ..Default::default() }, ..Default::default() },
            ..Default::default() })]);
        assert!(host.tick(&inputs).unwrap().pawn_messages.is_empty(), "pass-C mailbox is not this frame's active command");
        let report = host.tick(&inputs).unwrap();
        assert_eq!(report.pawn_messages.len(), 1);
        let message = &report.pawn_messages[0];
        assert_eq!(message.frame, report.frame);
        assert_eq!(message.pawn_slot, 1);
        assert_eq!(message.audience, [ProfileId(41),ProfileId(42)]);
        assert!(host.tick(&inputs).unwrap().pawn_messages.is_empty());
        host.pawn_chatter.queue.offer(Pending { speaker: 1, message: 901, priority: 1,
            remaining: 90.0, grouped: false });
        let before = host.state_hash();
        let mut saved = host.save().unwrap();
        saved.put(crate::save::TAG_RIFT, crate::save::Owner::Shared, vec![255]);
        assert!(host.load(&saved, crate::data::NoStoryData).is_err());
        assert_eq!(host.state_hash(), before, "failed load preserves simulation and pending speech");
        let saved = host.save().unwrap();
        host.load(&saved, crate::data::NoStoryData).unwrap();
        assert!(host.pawn_chatter.queue.pending.is_empty());
        assert_eq!(host.pawn_chatter.next_id, message.id);
    }

    #[test]
    fn emitted_message_hearing_uses_profiles_not_reused_seats() {
        let message = Message { id: 9, frame: 12, pawn_slot: 1, pawn_seat: 5,
            pawn_identity: Identity::Main { profile: ProfileId(41) }, message: 901, name: String::new(), gender: 0, voice: 0,
            audience: vec![ProfileId(41), ProfileId(42), ProfileId(43)] };
        assert!(message.heard_by(ProfileId(43)));
        assert!(!message.heard_by(ProfileId(99)));
    }

    #[test]
    fn successful_lifecycle_clear_preserves_host_message_identity() {
        let mut runtime = Runtime::default();
        runtime.next_id = 42;
        runtime.queue.offer(Pending { speaker: 1, message: 901, priority: 1,
            remaining: 90.0, grouped: false });
        runtime.clear_pending();
        assert!(runtime.queue.pending.is_empty());
        assert_eq!(runtime.next_id, 42);
    }

    #[test]
    fn installed_order_transition_emits_once_and_rebinding_discards_old_queue() {
        let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !path.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(path).unwrap();
        let data = Arc::new(Data::load(&vfs).unwrap());
        assert!(!data.configs.is_empty());
        let mut record = ddpawn::cmc::Cmc::initial();
        record.rom_id = 1;
        record.info_ex_value[0] = 1000.0;
        fn pawn(command: Command, record: &ddpawn::cmc::Cmc) -> Pawn<'_> {
            Pawn { slot: 1, seat: 6, record, identity: Identity::Hired {
                hirer: ProfileId(41), rom_id: record.rom_id, network_id: vec![], hired_at: 0 },
                living: true, ready: true, command, goal: None }
        }
        let mut runtime = Runtime::with_data(data);
        let audience = [ProfileId(41), ProfileId(42)];
        assert!(runtime.step(1, 1.0, true, &[pawn(Command::None, &record)], &[], &[], &[], &audience).is_empty());
        let messages = runtime.step(2, 1.0, true, &[pawn(Command::Go, &record)], &[], &[], &[], &audience);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].audience, audience);
        assert!(runtime.step(3, 1.0, true, &[pawn(Command::Go, &record)], &[], &[], &[], &audience).is_empty());
        runtime.queue.offer(Pending { speaker: 1, message: 901, priority: 1,
            remaining: 90.0, grouped: false });
        record.rom_id = 2;
        assert!(runtime.step(4, 1.0, true, &[pawn(Command::Go, &record)], &[], &[], &[], &audience).is_empty());
        assert!(runtime.queue.pending.is_empty());
        assert_eq!(runtime.next_id, messages[0].id);
        runtime.queue.offer(Pending { speaker: 1, message: 901, priority: 1,
            remaining: 90.0, grouped: false });
        let mut rebound = pawn(Command::Go, &record);
        rebound.identity = Identity::Hired { hirer: ProfileId(42), rom_id: record.rom_id,
            network_id: vec![], hired_at: 0 };
        assert!(runtime.step(5, 1.0, true, &[rebound], &[], &[], &[], &audience).is_empty());
        assert!(runtime.queue.pending.is_empty(), "same ROM template, different hirer retires pending speech");
    }
}
