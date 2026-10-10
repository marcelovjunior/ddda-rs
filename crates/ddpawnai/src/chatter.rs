//! Native CMC message selection and fetch queue, without text, audio or input.
//! Bulk phrase lists and bank mappings are read from the player's executable.

use crate::commands::Command;
use ddexe::Exe;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub message: i32,
    pub priority: i32,
    /// Lifetime in main-frame units while waiting for the HUD to accept it.
    pub expires: f32,
    /// Selects the separate immediate NPC-message path in 0x00413C80.
    pub immediate: bool,
}

/// `rCmcMsgConfig`, loaded at 0x00CB4430 and searched at 0x00CB45F0.
#[derive(Debug, Clone, PartialEq)]
pub struct Configs(pub Vec<Config>);
impl Configs {
    // Translated from 0x00CB4430 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 16 || &bytes[..4] != b"CMC\0" {
            return Err("CMC header absent".into());
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word(4) != 110_813 { return Err("unsupported CMC version".into()); }
        let count = word(8) as usize;
        if count > (bytes.len() - 16) / 16 || bytes.len() != 16 + count * 16 {
            return Err("CMC record span invalid".into());
        }
        let mut configs = Vec::with_capacity(count);
        for row in bytes[16..].chunks_exact(16) {
            let value = |at| u32::from_le_bytes(row[at..at + 4].try_into().unwrap());
            let expires = f32::from_bits(value(8));
            if !expires.is_finite() || expires < 0.0 {
                return Err("CMC expiry invalid".into());
            }
            configs.push(Config { message: value(0) as i32, priority: value(4) as i32,
                expires, immediate: value(12) != 0 });
        }
        Ok(Self(configs))
    }

    // Translated from 0x00CB45F0 / 0x00413C80 (DDDA.exe 2017-12-13 Steam).
    pub fn get(&self, message: i32) -> Config {
        self.0.iter().find(|row| row.message == message).copied().unwrap_or(Config {
            message, priority: 1,
            expires: 60.0, // 0x00413C87 -> 0x014EA2D8
            immediate: false,
        })
    }
}

/// Native phrase vocabulary selector. These are GOAP goal classifications,
/// not the pawn's battle-distance heuristic or inclination numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyGroup {
    GoBusy,
    GoCombat,
    GoOther,
    ComeOther,
    ComeCombat,
    ComeBusy,
    HelpCombat,
    HelpSupport,
    HelpBusy,
}
impl ReplyGroup {
    fn table(self) -> u32 {
        match self {
            Self::GoBusy => 0x017F_3C0C, Self::GoCombat => 0x017F_3C28,
            Self::GoOther => 0x017F_3C44, Self::ComeOther => 0x017F_3C60,
            Self::ComeCombat => 0x017F_3C7C, Self::ComeBusy => 0x017F_3C98,
            Self::HelpCombat => 0x017F_3CB4, Self::HelpSupport => 0x017F_3CD0,
            Self::HelpBusy => 0x017F_3CEC,
        }
    }
}

/// All installed order phrase lists, indexed by saved `mUseSceneTalk[0]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTables {
    lists: Vec<Vec<i32>>,
    busy_goals: Vec<u32>,
    combat_goals: Vec<u32>,
    support_goals: Vec<u32>,
    go_combat_goals: Vec<u32>,
}
impl OrderTables {
    // Translated from 0x005CFC20, 0x00599EA0 / 0x00599F20
    // (DDDA.exe 2017-12-13 Steam). All phrase IDs remain installed data.
    pub fn from_exe(exe: &Exe<'_>) -> Result<Self, String> {
        let mut lists = Vec::new();
        for group in [ReplyGroup::GoBusy, ReplyGroup::GoCombat, ReplyGroup::GoOther,
            ReplyGroup::ComeOther, ReplyGroup::ComeCombat, ReplyGroup::ComeBusy,
            ReplyGroup::HelpCombat, ReplyGroup::HelpSupport, ReplyGroup::HelpBusy] {
            for style in 0..7 {
                let address = exe.u32(group.table() + style * 4)?;
                let mut list = Vec::new();
                if address != 0 {
                    // Safety bound for corrupted executable pointers; native
                    // lists end with -1. Never substitute a fabricated phrase.
                    for index in 0..256 {
                        let value = exe.i32(address + index * 4)?;
                        if value == -1 { break; }
                        if value < 0 { return Err("invalid order phrase ID".into()); }
                        list.push(value);
                        if index == 255 { return Err("unterminated order phrases".into()); }
                    }
                }
                lists.push(list);
            }
        }
        fn classified(exe: &Exe<'_>, first: u32, count: u32, bytes: u32,
            targets: u32, target: u32) -> Result<Vec<u32>, String> {
            (0..count).filter_map(|index| {
                match exe.u8(bytes + index).and_then(|branch|
                    exe.u32(targets + u32::from(branch) * 4)) {
                    Ok(address) => (address == target).then_some(Ok(first + index)),
                    Err(e) => Some(Err(e.into())),
                }
            }).collect()
        }
        Ok(Self { lists,
            busy_goals: classified(exe, 4, 76, 0x0059_9ED4, 0x0059_9ECC, 0x0059_9EC6)?,
            combat_goals: classified(exe, 1, 84, 0x0059_9F54, 0x0059_9F4C, 0x0059_9F44)?,
            support_goals: classified(exe, 12, 49, 0x005C_FE2C, 0x005C_FE24, 0x005C_FCCD)?,
            go_combat_goals: classified(exe, 50, 27, 0x005C_FE68, 0x005C_FE60, 0x005C_FD7F)?,
        })
    }

    // Translated from 0x005CFC20 (DDDA.exe 2017-12-13 Steam).
    pub fn group(&self, command: Command, goal: Option<u32>) -> Option<ReplyGroup> {
        let busy = goal.is_some_and(|g| self.busy_goals.contains(&g));
        let combat = goal.is_some_and(|g| self.combat_goals.contains(&g));
        match command {
            Command::None => None,
            Command::Go if busy => Some(ReplyGroup::GoBusy),
            Command::Go if combat || goal.is_some_and(|g| self.go_combat_goals.contains(&g)) => Some(ReplyGroup::GoCombat),
            Command::Go => Some(ReplyGroup::GoOther),
            Command::Come if !busy => Some(ReplyGroup::ComeOther),
            Command::Come if combat => Some(ReplyGroup::ComeCombat),
            Command::Come => Some(ReplyGroup::ComeBusy),
            Command::Help if goal.is_some_and(|g| self.support_goals.contains(&g)) => Some(ReplyGroup::HelpSupport),
            Command::Help if busy => Some(ReplyGroup::HelpBusy),
            Command::Help if combat => Some(ReplyGroup::HelpCombat),
            Command::Help => None,
        }
    }

    /// Uniform modulo pick, exactly one native integer draw when nonempty.
    /// The caller supplies host RNG; stream assignment is a co-op policy.
    // Translated from 0x005CFDD3 (DDDA.exe 2017-12-13 Steam).
    pub fn select(&self, command: Command, goal: Option<u32>, style: u32,
        mut random: impl FnMut() -> u32) -> Option<i32> {
        if style >= 7 { return None; }
        let group = self.group(command, goal)?;
        let list = &self.lists[group as usize * 7 + style as usize];
        (!list.is_empty()).then(|| list[(random() % list.len() as u32) as usize])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyNotice { Defeated, FirstSeen, Additional }

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnemyPhrases {
    kind: u32,
    variant: u32,
    knowledge: u32,
    messages: Vec<i32>,
}

/// Native enemy-kind phrase records. The knowledge bitfield belongs to
/// the selected pawn's saved CMC, not to an Arisen or a global player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyTables { rows: [Vec<EnemyPhrases>; 3] }
impl EnemyTables {
    // Translated from 0x00599C50 / 0x008BC5F0 / 0x008BC630
    // (DDDA.exe 2017-12-13 Steam). Bulk records remain installed data.
    pub fn from_exe(exe: &Exe<'_>) -> Result<Self, String> {
        let mut rows = [Vec::new(), Vec::new(), Vec::new()];
        for (index, (base, stride)) in [(0x0150_5A70, 40),
            (0x0150_5AE8, 44), (0x0150_76F0, 44)].into_iter().enumerate() {
            // Safety bound on malformed input; the native sentinel is 0xBF.
            for row_index in 0..512 {
                let at = base + row_index * stride;
                let kind = exe.u32(at)?;
                if kind == 0xBF { break; }
                if row_index == 511 { return Err("unterminated enemy phrase table".into()); }
                let mut messages = Vec::new();
                let mut terminated = false;
                for index in 0..8 {
                    let message = exe.i32(at + 8 + index * 4)?;
                    if message == -1 { terminated = true; break; }
                    // Native tests only -1: retain other signed values.
                    // Their bank/voice meaning is deliberately unresolved.
                    messages.push(message);
                }
                if !terminated { return Err("unterminated enemy phrases".into()); }
                rows[index].push(EnemyPhrases { kind, variant: exe.u32(at + 4)?,
                    knowledge: if stride == 44 { exe.u32(at + 40)? } else { 0 }, messages });
            }
        }
        Ok(Self { rows })
    }

    // Translated from 0x008BC5F0 / 0x008BC630 and 0x00599CF5
    // (DDDA.exe 2017-12-13 Steam).
    pub fn select(&self, notice: EnemyNotice, kind: u32, variant: u32,
        study_flags: &[u32], mut random: impl FnMut() -> u32) -> Option<i32> {
        let rows = &self.rows[notice as usize];
        let row = rows.iter().find(|row| {
            if row.kind != kind { return false; }
            if (notice == EnemyNotice::Defeated || kind == 0x8E)
                && row.variant != variant { return false; }
            let bit = row.knowledge;
            bit == 0 || (bit < 0x2840 && study_flags.get((bit >> 5) as usize)
                .is_some_and(|word| word & (1u32 << (bit & 31)) != 0))
        })?;
        (!row.messages.is_empty()).then(|| row.messages[(random() % row.messages.len() as u32) as usize])
    }
}

/// Native authored-message selection (0x005C6130). First select a living
/// slot, then check its CMC readiness; a failed check does not try another.
/// `living`/`ready` are party pawn slots, never human player seats.
// Translated from 0x005C6130 (DDDA.exe 2017-12-13 Steam).
pub fn authored_speaker(selector: i32, living: [bool; 3], ready: [bool; 3]) -> Option<u8> {
    let slot = if selector < 0 { living.iter().position(|living| *living)? }
        else { usize::try_from(selector).ok().filter(|slot| *slot < 3 && living[*slot])? };
    ready[slot].then_some(slot as u8)
}

/// Native party-slot shuffle precedes the readiness walk, even if only
/// one pawn is eligible. The adapter supplies the native guard result.
// Translated from 0x005A01B0 (DDDA.exe 2017-12-13 Steam).
pub fn random_speaker(eligible: [bool; 3], mut random: impl FnMut() -> u32) -> Option<u8> {
    let mut slots = [0u8, 1, 2];
    for index in 1..3 { slots.swap(index, (random() % (index as u32 + 1)) as usize); }
    slots.into_iter().find(|slot| eligible[*slot as usize])
}

/// Native unsigned-word to unit-random conversion; x87 retains the
/// exact product until its caller stores the resulting timer.
// Translated from 0x00422FA0 (DDDA.exe 2017-12-13 Steam).
pub fn random_unit(word: u32) -> f64 {
    f64::from(word) * f64::from(f32::from_bits(0x2F80_0000)) // 0x0161C650, 2^-32
}

// Translated from 0x0057EFF0 / 0x005AAF2C (DDDA.exe 2017-12-13 Steam).
pub fn actor_cooldown(talk: Option<f32>) -> f32 {
    let talk = talk.unwrap_or(0.0);
    if talk < 200.0 { 1800.0 } // 0x01516E7C / 0x014F8E08
    else if talk < 400.0 { 360.0 } // 0x014F70DC / 0x0161C300
    else if talk < 600.0 { 210.0 } // 0x0152DCF4 / 0x0161CA08
    else if talk < 800.0 { 150.0 } // 0x017F42E4 / 0x0152CF24
    else { 60.0 } // 0x014EA2D8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorRequest { pub message: i32, pub grouped: bool }

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActorQueue { pub pending: Vec<ActorRequest>, pub remaining: f32 }
impl ActorQueue {
    // Translated from 0x005AB130 (DDDA.exe 2017-12-13 Steam).
    pub fn offer(&mut self, request: ActorRequest) {
        if self.pending.len() < 32 { self.pending.push(request); }
    }

    /// Every eligible phase clears the ordinary request list, including
    /// while cooling. Exact zero does not select until the next update.
    // Translated from 0x005AADC0 (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, frames: f32, eligible: bool, cooldown: f32,
        mut previously_shown: impl FnMut(i32) -> bool, mut random: impl FnMut() -> u32)
        -> Option<ActorRequest> {
        if !eligible { return None; }
        self.remaining -= frames;
        let mut requests = std::mem::take(&mut self.pending);
        if self.remaining >= 0.0 { return None; }
        self.remaining = 0.0;
        requests.retain(|request| !previously_shown(request.message));
        if requests.is_empty() { return None; }
        let selected = requests[(random() % requests.len() as u32) as usize];
        self.remaining = cooldown;
        Some(selected)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub speaker: u8,
    pub message: i32,
    pub priority: i32,
    pub remaining: f32,
    /// Native resource +0x40/+0x3C: replace a grouped pending request
    /// from this party pawn slot, irrespective of message ID.
    pub grouped: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Queue {
    pub pending: Vec<Pending>,
    pub fetch_remaining: f32,
    pub fetch_minimum: f32,
    pub fetch_width: f32,
}
impl Default for Queue {
    // Translated from 0x00454F80 (DDDA.exe 2017-12-13 Steam).
    fn default() -> Self {
        Self { pending: Vec::new(), fetch_remaining: 0.0,
            fetch_minimum: 20.0, // 0x015184E0, constructor +0xE8
            fetch_width: 50.0 } // 0x0150A088, constructor +0xEC
    }
}
impl Queue {
    // Translated from 0x00455C30 (DDDA.exe 2017-12-13 Steam).
    pub fn offer(&mut self, request: Pending) {
        if let Some(index) = self.pending.iter().position(|old|
            if request.grouped { old.grouped && old.speaker == request.speaker }
            else { old.message == request.message }) {
            if request.grouped { self.pending[index] = request; }
            else { self.pending[index].remaining = request.remaining; }
            return;
        }
        let at = self.pending.iter().position(|old| request.priority > old.priority)
            .unwrap_or(self.pending.len());
        self.pending.insert(at, request);
        // The executable drops the FIRST linked-list node when count reaches
        // 32 (0x00455F00), retaining at most 31; preserve this odd boundary.
        if self.pending.len() >= 32 { self.pending.remove(0); }
    }

    /// One native main-frame update. Pause freezes timer subtraction but
    /// already-expired nodes are still removed. HUD rejection keeps the head.
    // Translated from 0x004557B0 (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, frames: f32, clock_runs: bool,
        mut accepts: impl FnMut(&Pending) -> bool, mut random_unit: impl FnMut() -> f64)
        -> Option<Pending> {
        if clock_runs && self.fetch_remaining > 0.0 { self.fetch_remaining -= frames; }
        if clock_runs { for entry in &mut self.pending { entry.remaining -= frames; } }
        self.pending.retain(|entry| entry.remaining > 0.0);
        if self.fetch_remaining > 0.0 || !self.pending.first().is_some_and(&mut accepts) {
            return None;
        }
        let event = self.pending.remove(0);
        // Native x87 draw + multiplication/addition, one final f32 store.
        self.fetch_remaining = (random_unit() * f64::from(self.fetch_width)
            + f64::from(self.fetch_minimum)) as f32;
        Some(event)
    }
}

/// The installed message/text/voice bank and its local stream cue key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    pub text: String,
    pub stream: String,
    pub cue: u16,
}
impl Bank {
    /// Read the native message-band routing tables. Voice 12+ falls back to
    /// zero; `(voice / 3) & 1` chooses recorded actor voice, independently of
    /// the motion-voice pitch variant. Gender is the saved native byte.
    // Translated from 0x00449930 and 0x004B0050
    // (DDDA.exe 2017-12-13 Steam).
    pub fn resolve(exe: &Exe<'_>, message: u32, gender: u8, voice: u8,
        japanese_voice: bool) -> Result<Option<Self>, String> {
        let voice = if voice < 12 { voice } else { 0 };
        let variant = (voice / 3) & 1;
        let gender = if gender == 0 { 'm' } else { 'f' };
        let (name, cue) = match message {
            0..1000 => ("e_base".into(), message),
            1000..23000 => {
                let id = exe.u16(0x0155_D6D4 + ((message - 1000) / 1000) * 2)?;
                if id == u16::MAX { return Ok(None); }
                (format!("em{id:04}"), message % 1000)
            }
            100000..110000 => ("q_base".into(), message % 10000),
            110000..188000 => {
                let id = exe.u16(0x0155_D700 + ((message - 110000) / 1000) * 2)?;
                if id == u16::MAX { return Ok(None); }
                (format!("q{id:04}"), message % 1000)
            }
            200000..201000 => ("st_base".into(), message % 1000),
            201000..264000 => {
                let id = exe.u16(0x0155_D7A0 + ((message - 201000) / 1000) * 2)?;
                if id == u16::MAX { return Ok(None); }
                (format!("st{id:03}"), message % 1000)
            }
            300000..350000 => ("etc".into(), message % 10000),
            // 350000..400000 uses the separate unused resource band; reject
            // until its text and consumer are traced, rather than rerouting.
            _ => return Ok(None),
        };
        let voice_root = if japanese_voice { "pwn_jp" } else { "pwn" };
        let suffix = if japanese_voice { "_jp" } else { "" };
        Ok(Some(Self {
            text: format!("id\\message\\pawn\\{name}"),
            stream: format!("sound\\stream\\{voice_root}\\{name}\\{gender}{variant}\\{name}_{gender}{variant}{suffix}"),
            cue: cue as u16,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(speaker: u8, message: i32, priority: i32, remaining: f32) -> Pending {
        Pending { speaker, message, priority, remaining, grouped: false }
    }

    #[test]
    fn native_cmc_parser_retains_signed_priority_and_first_duplicate() {
        let mut bytes = b"CMC\0".to_vec();
        for value in [110_813u32, 2, 0x1234_5678] { bytes.extend(value.to_le_bytes()); }
        for (message, priority, expiry, immediate) in [(17i32, -3i32, 6f32, 2u32), (17, 5, 9.0, 0)] {
            bytes.extend(message.to_le_bytes()); bytes.extend(priority.to_le_bytes());
            bytes.extend(expiry.to_le_bytes()); bytes.extend(immediate.to_le_bytes());
        }
        let configs = Configs::parse(&bytes).unwrap();
        assert_eq!(configs.get(17), Config { message: 17, priority: -3, expires: 6.0, immediate: true });
        assert_eq!(configs.get(18).expires, 60.0);
        assert!(Configs::parse(&bytes[..bytes.len() - 1]).is_err());
        bytes[24..28].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Configs::parse(&bytes).is_err());
    }

    #[test]
    fn pending_priority_ties_and_duplicate_keep_the_original_speaker() {
        let mut queue = Queue::default();
        queue.offer(request(4, 10, 1, 90.0));
        queue.offer(request(5, 11, 3, 20.0));
        queue.offer(request(6, 12, 3, 30.0));
        queue.offer(request(7, 10, 9, 40.0));
        assert_eq!(queue.pending.iter().map(|r| r.message).collect::<Vec<_>>(), [11, 12, 10]);
        assert_eq!(queue.pending[2], request(4, 10, 1, 40.0));
        let mut grouped = request(4, 20, 5, 80.0); grouped.grouped = true;
        queue.offer(grouped.clone());
        grouped.message = 21; grouped.priority = 0;
        queue.offer(grouped.clone());
        assert_eq!(queue.pending.len(), 4);
        // Group replacement preserves the old queue node position.
        assert_eq!(queue.pending[0], grouped);
    }

    #[test]
    fn queue_expires_before_fetch_freezes_subtraction_and_rechecks_same_frame_zero() {
        let mut queue = Queue::default();
        queue.offer(request(4, 1, 4, 1.0));
        queue.offer(request(5, 2, 1, 100.0));
        assert_eq!(queue.step(1.0, true, |_| true, || 0.5).unwrap().message, 2);
        assert_eq!(queue.fetch_remaining, 45.0);
        queue.offer(request(4, 3, 1, 100.0));
        assert!(queue.step(45.0, false, |_| true, || panic!("paused positive timer draws no RNG")).is_none());
        assert_eq!(queue.pending[0].remaining, 100.0);
        assert_eq!(queue.step(45.0, true, |_| true, || 0.0).unwrap().message, 3);
        assert_eq!(queue.fetch_remaining, 20.0);
        queue.offer(request(4, 4, 1, 100.0));
        assert!(queue.step(20.0, true, |_| false, || panic!("HUD rejection draws no RNG")).is_none());
        assert_eq!(queue.pending[0].remaining, 80.0);
        assert_eq!(queue.step(0.0, false, |_| true, || 1.0).unwrap().message, 4);
        assert_eq!(queue.fetch_remaining, 70.0);
    }

    #[test]
    fn native_queue_limit_discards_head_at_thirty_two() {
        let mut queue = Queue::default();
        for message in 0..32 { queue.offer(request(4, message, message, 100.0)); }
        assert_eq!(queue.pending.len(), 31);
        assert_eq!(queue.pending.first().unwrap().message, 30);
        assert_eq!(queue.pending.last().unwrap().message, 0);
    }

    #[test]
    fn order_goal_precedence_and_empty_selection_do_not_invent_replies() {
        let mut lists = vec![Vec::new(); 63];
        lists[ReplyGroup::GoBusy as usize * 7 + 3] = vec![400, 401, 402];
        let tables = OrderTables { lists, busy_goals: vec![4, 12], combat_goals: vec![4, 1],
            support_goals: vec![12], go_combat_goals: vec![50] };
        assert_eq!(tables.group(Command::Go, Some(4)), Some(ReplyGroup::GoBusy));
        assert_eq!(tables.group(Command::Come, Some(4)), Some(ReplyGroup::ComeCombat));
        assert_eq!(tables.group(Command::Help, Some(12)), Some(ReplyGroup::HelpSupport));
        assert_eq!(tables.group(Command::Help, Some(1)), Some(ReplyGroup::HelpCombat));
        assert_eq!(tables.group(Command::Go, Some(50)), Some(ReplyGroup::GoCombat));
        assert_eq!(tables.select(Command::Go, Some(4), 3, || 5), Some(402));
        assert_eq!(tables.select(Command::None, None, 3, || panic!("no phrase")), None);
        assert_eq!(tables.select(Command::Go, Some(4), 7, || panic!("invalid saved style")), None);
        assert_eq!(tables.select(Command::Come, Some(4), 3, || panic!("empty phrase")), None);
    }

    #[test]
    fn installed_order_tables_and_message_voice_binding_agree() {
        let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !path.is_dir() { return; }
        let file = ddexe::ExeFile::next_to(&path).unwrap();
        let exe = file.image().unwrap();
        let tables = OrderTables::from_exe(&exe).unwrap();
        let vfs = ddvfs::Vfs::open(&path).unwrap();
        for (command, goal) in [(Command::Go, None), (Command::Come, None), (Command::Help, Some(52))] {
            for style in 0..7 {
                if let Some(message) = tables.select(command, goal, style, || 0) {
                    let bank = Bank::resolve(&exe, message as u32, 0, 0, false).unwrap().unwrap();
                    let configs = Configs::parse(&vfs.read(&bank.text, mtarc::type_hash("rCmcMsgConfig")).unwrap()).unwrap();
                    assert!(configs.0.iter().any(|config| config.message == message));
                    assert!(!vfs.read(&bank.stream, mtarc::type_hash("rSoundStreamRequest")).unwrap().is_empty());
                }
            }
        }
        let bank = Bank::resolve(&exe, 1000, 0, 0, false).unwrap().unwrap();
        assert!(bank.text.ends_with("em0100"));
        assert_eq!(bank.cue, 0);
        let configs = Configs::parse(&vfs.read(&bank.text, mtarc::type_hash("rCmcMsgConfig")).unwrap()).unwrap();
        assert!(configs.0.iter().any(|config| config.message == 1000));
        assert_eq!(Bank::resolve(&exe, 1000, 0, 12, false).unwrap(), Some(bank));
        assert!(Bank::resolve(&exe, 23_000, 0, 0, false).unwrap().is_none());
        let enemies = EnemyTables::from_exe(&exe).unwrap();
        let message = enemies.select(EnemyNotice::FirstSeen, 5, 0, &[], || 0).unwrap();
        let bank = Bank::resolve(&exe, message as u32, 0, 0, false).unwrap().unwrap();
        let configs = Configs::parse(&vfs.read(&bank.text, mtarc::type_hash("rCmcMsgConfig")).unwrap()).unwrap();
        assert!(configs.0.iter().any(|config| config.message == message));
    }

    #[test]
    fn enemy_notice_matches_first_knowledge_row_and_special_variant_only() {
        let record = |kind, variant, knowledge, messages| EnemyPhrases { kind, variant, knowledge, messages };
        let tables = EnemyTables { rows: [
            vec![record(7, 2, 0, vec![900])],
            vec![record(7, 8, 33, vec![901, 902]), record(7, 9, 0, vec![903]),
                record(0x8E, 2, 0, vec![904]), record(0x8E, 3, 0, vec![905])],
            vec![record(7, 0, 0x2840, vec![906]), record(7, 0, 0, vec![])],
        ] };
        assert_eq!(tables.select(EnemyNotice::FirstSeen, 7, 99, &[0, 2], || 3), Some(902));
        assert_eq!(tables.select(EnemyNotice::FirstSeen, 7, 99, &[0, 1], || 0), Some(903));
        assert_eq!(tables.select(EnemyNotice::FirstSeen, 0x8E, 3, &[], || 0), Some(905));
        assert_eq!(tables.select(EnemyNotice::Defeated, 7, 3, &[], || panic!("variant mismatch")), None);
        assert_eq!(tables.select(EnemyNotice::Defeated, 7, 2, &[], || 0), Some(900));
        assert_eq!(tables.select(EnemyNotice::Additional, 7, 0, &[u32::MAX; 322],
            || panic!("invalid knowledge and empty fallback")), None);
    }

    #[test]
    fn authored_selector_checks_readiness_after_first_living_slot() {
        assert_eq!(authored_speaker(-1, [false, true, true], [false, false, true]), None);
        assert_eq!(authored_speaker(-9, [false, true, true], [false, true, true]), Some(1));
        assert_eq!(authored_speaker(2, [false, true, true], [false, true, true]), Some(2));
        assert_eq!(authored_speaker(0, [false, true, true], [true; 3]), None);
        assert_eq!(authored_speaker(3, [true; 3], [true; 3]), None);
    }

    #[test]
    fn actor_fetch_discards_cooling_requests_and_preserves_strict_zero_boundary() {
        let mut queue = ActorQueue::default();
        let request = ActorRequest { message: 900, grouped: true };
        queue.offer(request);
        assert_eq!(queue.step(1.0, true, 60.0, |_| false, || 0), Some(request));
        queue.offer(request);
        assert_eq!(queue.step(60.0, true, 60.0, |_| false, || panic!("exact zero")), None);
        assert!(queue.pending.is_empty());
        queue.offer(request);
        assert_eq!(queue.step(1.0, false, 60.0, |_| false, || panic!("guarded")), None);
        assert_eq!(queue.pending.len(), 1);
        assert_eq!(queue.step(1.0, true, 60.0, |_| true, || panic!("shown message removed")), None);
        assert!(queue.pending.is_empty());
        for (talk, cooldown) in [(199.0, 1800.0), (200.0, 360.0), (400.0, 210.0),
            (600.0, 150.0), (800.0, 60.0)] {
            assert_eq!(actor_cooldown(Some(talk)), cooldown);
        }
        assert_eq!(random_unit(0), 0.0);
        assert_eq!(random_unit(u32::MAX), 1.0 - 2.0f64.powi(-32));
    }

    #[test]
    fn native_speaker_shuffle_draws_twice_before_walking_eligible_pawns() {
        let mut draws = [0, 1].into_iter();
        assert_eq!(random_speaker([true; 3], || draws.next().unwrap()), Some(1));
        assert_eq!(draws.next(), None);
        let mut count = 0;
        assert_eq!(random_speaker([false, false, true], || { count += 1; 0 }), Some(2));
        assert_eq!(count, 2);
        assert_eq!(random_speaker([false; 3], || 0), None);
    }
}
