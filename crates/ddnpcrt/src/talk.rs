//! Talking to NPCs: which conversation plays, its lines, the menus their
//! markup opens. Translated from DDDA.exe (2017-12-13 Steam); see
//! docs/NPCS.md, "Talking".

use ddnpc::{Conversation, MessageSet};

/// Per-NPC talk memory (`sNpcManager` NPC record, 0x110 bytes per NPC):
/// - `once` (+0x898): conversations of sequence type 1 already said;
/// - `said` (+0x8a8): conversations said (any type but 2);
/// - `last` (+0x8ba): the last conversation said;
/// - `seq` (+0x8c8): per story band, the highest sequence said. Reset to
///   -1 for every NPC by `sNpcManager::vf10` (0x00475c80).
///
/// World state: saved with the session, shared by every player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkMemory {
    pub once: [u32; 4],
    pub said: [u32; 4],
    pub last: i16,
    pub seq: [i16; 16],
}

impl Default for TalkMemory {
    fn default() -> Self {
        TalkMemory {
            once: [0; 4],
            said: [0; 4],
            last: 0,
            seq: [-1; 16],
        }
    }
}

fn bit(words: &[u32; 4], n: u32) -> bool {
    n < 128 && words[(n >> 5) as usize] & (0x8000_0000 >> (n & 31)) != 0
}

fn set_bit(words: &mut [u32; 4], n: u32) {
    if n < 128 {
        words[(n >> 5) as usize] |= 0x8000_0000 >> (n & 31);
    }
}

/// The story state the chooser reads.
pub trait TalkState {
    /// Quest flag (`0x004356c0`).
    fn quest_flag(&self, quest: i32, flag: i32) -> bool;
    /// Game-state flag (bit at sGameSys +0xac6e4).
    fn gsf(&self, flag: u32) -> bool;
    /// NPC affinity (`sNpcManager` +0x8b8; 0 below 0, 500 from 900 up).
    fn affinity(&self, npc: i32) -> i32;
}

impl TalkState for ddstory::World {
    fn quest_flag(&self, quest: i32, flag: i32) -> bool {
        quest >= 0 && flag >= 0 && self.quest_bit(quest as usize, flag as usize)
    }
    fn gsf(&self, flag: u32) -> bool {
        (flag as usize) < ddstory::world::GSF_COUNT && self.gsf.get(flag as usize)
    }
    fn affinity(&self, npc: i32) -> i32 {
        if npc < 0 {
            0
        } else if npc >= 900 {
            500
        } else {
            self.npc_friend.get(npc as usize).copied().unwrap_or(500) as i32
        }
    }
}

fn band(season: u32) -> usize {
    (season as usize).min(15)
}

/// Pick the conversation for `season` (the story band, sGameSys +0x3c).
///
/// Translated from `0x00bb8cf0` (DDDA.exe 2017-12-13 Steam), for the NPC's
/// base talk set:
/// - rows are tried in order; a row numbered 128 or more is always a
///   candidate, a lower one only if it was never said (neither memory bit)
///   or it is the last conversation said;
/// - a row needs `mSeason == season`; its conditions (quest flag when
///   both `mQuestNo` and `mQuestFlgNo` are set, `mGsfFlgNo`, affinity in
///   `mFriend..=mFriendMax`) must all hold;
/// - with conditions, the band's counter first drops to the row's
///   sequence if higher; then (with or without conditions) the row wins
///   when the counter is below its sequence;
/// - with no winner and a counter of 0 or more, the counter drops by one
///   and the band is tried again; bands above 1 fall back to the band
///   below;
/// - the winner marks memory: sequence type 1 sets `once`; any type but 2
///   sets `said` and `last` and raises the counter to its sequence.
///
/// Not translated: the stage-200 special case for placement id 995
/// (returns 300 or 11).
pub fn choose(set: &MessageSet, mem: &mut TalkMemory, npc: i32, season: u32, st: &dyn TalkState) -> Option<i32> {
    let s = band(season);
    let mut found: Option<&Conversation> = None;
    for c in &set.conversations {
        let no = c.number as u32;
        let candidate = no > 0x7f || (!bit(&mem.once, no) && !bit(&mem.said, no)) || mem.last as i32 == c.number;
        if !candidate || c.season as i32 != season as i32 {
            continue;
        }
        let quest = c.quest_number != -1 && c.quest_flag != -1;
        let gsf = c.global_flag != -1;
        let friend = c.friendship != -1;
        let any = quest || gsf || friend;
        let ok = (!quest || st.quest_flag(c.quest_number as i32, c.quest_flag as i32))
            && (!gsf || (c.global_flag as u32) < 0x600 && st.gsf(c.global_flag as u32))
            && (!friend || {
                let a = st.affinity(npc);
                c.friendship as i32 <= a && a <= c.friendship_max as i32
            });
        let seq = c.season_sequence as i16;
        if any {
            if !ok {
                continue;
            }
            if seq < mem.seq[s] {
                mem.seq[s] = seq;
            }
        }
        if mem.seq[s] < seq {
            found = Some(c);
            break;
        }
    }
    if found.is_none() && mem.seq[s] >= 0 {
        mem.seq[s] -= 1;
        if let Some(n) = choose(set, mem, npc, season, st) {
            return Some(n);
        }
    }
    match found {
        Some(c) => {
            let no = c.number as u32;
            let ty = c.season_sequence_type;
            if ty == 1 && no < 0x80 {
                set_bit(&mut mem.once, no);
            }
            if ty != 2 {
                set_bit(&mut mem.said, no);
                mem.last = c.number as i16;
                if mem.seq[s] < c.season_sequence as i16 {
                    mem.seq[s] = c.season_sequence as i16;
                }
            }
            Some(c.number)
        }
        None if season >= 2 => choose(set, mem, npc, season - 1, st),
        None => None,
    }
}

/// The conversation a plain talk opens (`0x00bb4830`, its general branch):
/// the chooser's pick, or conversation 6 when it finds none; nothing if
/// the set lacks it.
pub fn entry(set: &MessageSet, mem: &mut TalkMemory, npc: i32, season: u32, st: &dyn TalkState) -> Option<i32> {
    let n = choose(set, mem, npc, season, st).unwrap_or(6);
    set.by_number(n).map(|_| n)
}

/// Why the shop menu is being shown (`0x00b9ddc0`'s first argument). 0 is
/// the opening menu; 1 and 2 follow a closed shop screen (their lines,
/// e.g. Caxton's "Take proper care of that." / "Good, good.", suggest
/// after buying / after selling: **inferred**, the GUI results that pick
/// them were not traced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    Open = 0,
    AfterBuy = 1,
    AfterSell = 2,
}

/// The shop-menu conversation of a shop NPC (`0x00b9ddc0`, kinds 0-2).
/// `equip_menus` is sGameSys +0xb8840 bit 0x8000000 (the variants with an
/// equipment option, 110). Returns `None` for Olra (783) on stage 200.
pub fn shop_menu(kind: MenuKind, npc: u32, stage: u32, equip_menus: bool, pawn_ready: bool) -> Option<i32> {
    if npc == 0x30f && stage == 200 {
        return None;
    }
    Some(match kind {
        MenuKind::Open => match npc {
            0x30f | 0x310 => 102,
            // Devyn (153): 110 unless the first pawn slot is filled and
            // ready (`[0x018fa4ec]+0x9a0`, byte +0x31ea).
            0x99 => {
                if pawn_ready {
                    100
                } else {
                    110
                }
            }
            400 => {
                if equip_menus {
                    130
                } else {
                    120
                }
            }
            _ if equip_menus => 110,
            _ => 100,
        },
        MenuKind::AfterBuy => match npc {
            0x30f => 102,
            0x310 => 101,
            _ => 105,
        },
        MenuKind::AfterSell => match npc {
            0x30f => 101,
            0x1db..=0x1dd => 100,
            0x310 => 103,
            _ => 106,
        },
    })
}

/// A command a line's markup gives (`sGUIExt::vf16`, 0x0044f950): the tag
/// sets sIdMessage +0xe4, and when the message window closes
/// `sIdMessage::vf06` (0x00455280) runs that handler (table set up by the
/// constructor 0x00454f80).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handler(pub u8);

/// Tag name to handler. `<FUNC n>` names handler n directly; `<INN>`
/// starts a free rest at once (0x005188b0) and has no handler.
pub const TAGS: [(&str, u8); 18] = [
    ("GSKL", 1),
    ("GILD", 2),
    ("STRG", 3),
    ("SHOP", 4),
    ("TENT", 5),
    ("JUNK", 6),
    ("UPGR", 7),
    ("GV_F", 8),
    ("EQSH", 9),
    ("BABA", 10),
    ("EQUG", 11),
    ("RMSH", 12),
    ("EQRM", 13),
    ("CHK", 14),
    ("EQJK", 15),
    ("RMVC", 16),
    ("UPG2", 17),
    ("GSK2", 18),
];

/// The handler a line's markup sets, if any (the last one wins).
pub fn line_handler(text: &str) -> Option<Handler> {
    let mut out = None;
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        let Some(j) = rest[i..].find('>') else { break };
        let tag = &rest[i + 1..i + j];
        let mut parts = tag.split_whitespace();
        let head = parts.next().unwrap_or("");
        if head == "FUNC" {
            if let Some(n) = parts.next().and_then(|n| n.parse::<u8>().ok()) {
                out = Some(Handler(n));
            }
        } else if let Some(&(_, h)) = TAGS.iter().find(|t| t.0 == head) {
            out = Some(Handler(h));
        }
        rest = &rest[i + j + 1..];
    }
    out
}

/// What a handler does with the player's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the NPC's shop screen (GUI 7) in a mode: 0 buy, 1 sell,
    /// 2 enhance, 3 extended enhance (no shop list), 4/5 equipment.
    Shop { mode: u8 },
    /// Pay for and start a rest (handler 0, 0x004565d0): `option` is the
    /// answer (0 / 1, two rest options, not identified).
    Inn { option: u8 },
    /// Check the inn price (handler 14, 0x00456690).
    InnCheck,
    /// A handler not translated here (storage, guild, Rift shop, ...).
    Other(u8),
    /// The answer selects nothing in this handler (e.g. "Never mind").
    Nothing,
}

/// Choice index to action for the menu handlers, from their branch tables:
/// SHOP 4 (0x004567f0) buy/sell; JUNK 6 and RMSH 12 (0x00456c40) buy;
/// UPGR 7 (0x00456ee0) buy/sell/enhance; EQSH 9 (0x00456900)
/// buy/sell/equipment; EQUG 11 (0x00457060) buy/sell/enhance/equipment;
/// EQRM 13 (0x00457240) buy/equipment; EQJK 15 (0x00456ce0) buy/-/
/// equipment; UPG2 17 (0x00457150) extended enhance.
pub fn action(h: Handler, choice: Option<u8>) -> Action {
    let c = choice;
    let shop = |m| Action::Shop { mode: m };
    match (h.0, c) {
        (0, Some(o @ 0..=1)) => Action::Inn { option: o },
        (14, _) => Action::InnCheck,
        (4, Some(0)) => shop(0),
        (4, Some(1)) => shop(1),
        (6 | 12, _) => shop(0),
        (7, Some(0)) => shop(0),
        (7, Some(1)) => shop(1),
        (7, Some(2)) => shop(2),
        (9, Some(0)) => shop(0),
        (9, Some(1)) => shop(1),
        (9, Some(2)) => shop(4),
        (11, Some(0)) => shop(0),
        (11, Some(1)) => shop(1),
        (11, Some(2)) => shop(2),
        (11, Some(3)) => shop(4),
        (13, Some(0)) => shop(0),
        (13, Some(1)) => shop(4),
        (15, Some(0)) => shop(0),
        (15, Some(2)) => shop(4),
        (17, _) => shop(3),
        (0 | 4 | 7 | 9 | 11 | 13 | 15, _) => Action::Nothing,
        (n, _) => Action::Other(n),
    }
}

/// The lines of conversation `no` that show: each slot's GMD index, with a
/// slot whose `mGfsFlgNo_msg` names a clear flag left out
/// (`0x00cc9ca0` via `MessageSet::loaded_message_index`).
pub fn lines(set: &MessageSet, no: i32, gsf: &dyn Fn(u32) -> bool) -> Vec<u32> {
    let Some(c) = set.by_number(no) else { return vec![] };
    (0..c.active_messages().len())
        .filter_map(|slot| {
            set.loaded_message_index(no, slot, |f| f >= 0 && gsf(f as u32))
                .filter(|&i| i >= 0)
                .map(|i| i as u32)
        })
        .collect()
}
