//! The Rift: pawn stock, lists, room, party, hiring and dismissal.

use crate::data::RiftData;
use crate::log::{HistoryEntry, PawnLog, PawnRef};
use ddpawn::builder;
use ddpawn::cmc::Cmc;
use ddpawn::rift::{self as rules, PawnRng, POOL_FIRST_STOCK_SLOT, POOL_SLOTS};
use ddpawn::RomPawnId;

/// A player in the session: the seat, 0 = host (`ddid::PlayerId`).
pub use ddid::PlayerId;

/// What the Rift needs to know about the Arisen who acts: level and the
/// Rift Crystal balance (`cSAVE_DATA_PARAM.OnlinePoint` /
/// `sGameSys+0xa7a1c` for player one), which a hire spends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arisen {
    pub id: PlayerId,
    pub level: u32,
    pub rift_crystals: i32,
}

impl Arisen {
    /// Spends Rift Crystals, flooring at 0. Translated from `0x004433f0`.
    pub fn spend(&mut self, cost: i32) {
        self.rift_crystals = (self.rift_crystals - cost).max(0);
    }
}

/// Number of stock slots (`sGameSys+0x16e0`, stride 0x14e8).
pub const STOCK_SLOTS: usize = 120;
/// Story pawns occupy stock slots 0..20 (`0x00796420`).
pub const STORY_SLOTS: usize = 20;

/// A pawn in a party slot (`cCmcInfo` at sGameSys+0xa7ec0 + slot*0x1660).
#[derive(Debug, Clone, PartialEq)]
pub struct PartyPawn {
    pub pawn: Cmc,
    pub online: PawnRef,
    /// Play time when hired (`cCmcInfo+0x1650`).
    pub hire_time: u32,
    /// Quests cleared since the hire (`cCmcInfo+0x1654`).
    pub quests_cleared: u32,
}

/// The party's pawn slots, shared by all players: slot 0 the main pawn,
/// slots 1 and 2 hired pawns. In two-player co-op the second Arisen takes
/// hired slot 2 (docs/COOP_DESIGN.md), which is then `reserved` and never
/// offered to a hire.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Party {
    pub main_pawn: Option<PartyPawn>,
    pub hired: [Option<PartyPawn>; 2],
    pub reserved: [bool; 2],
}

impl Party {
    pub fn new(players: usize) -> Party {
        Party { reserved: [false, players >= 2], ..Party::default() }
    }

    /// Party slots a hire may use (1-based like the exe's slots 1 and 2).
    pub fn hire_capacity(&self) -> usize {
        self.reserved.iter().filter(|r| !**r).count()
    }

    pub fn hired_count(&self) -> usize {
        self.hired.iter().filter(|h| h.is_some()).count()
    }

    /// Whether this pawn is one of the hired pawns (`0x00447d90`).
    pub fn has_hired(&self, r: &PawnRef) -> bool {
        self.hired.iter().flatten().any(|p| p.online.same_pawn(r))
    }
}

/// Rift menu categories (`sPawnManager+0x154c`, set by uRim `0x0073b5c0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// 1: Pawns by Level, at the chosen level (cursor + 1, at most 200).
    ByLevel(u32),
    /// 0xd: Past Summons (`mBorrowPawn`).
    PastSummons,
    /// 0xe: Favorites List.
    Favorites,
    /// 0xf: Pawns Your Main Pawn Knows (network only).
    MainPawnKnows,
    /// 0x10: Friend List (network only).
    Friends,
    /// 3/4: by user name, 6: Top Ranked, 7: Special Pawns, 0x12: Top This
    /// Month (network only).
    Online(u32),
}

/// Row flags of `nRim::cPawnInfoList` (`0x0047a550`) with the message the
/// GUI shows on summon (`0x006d7ba0`, `pawn_search_<lang>`).
pub mod flags {
    /// Already summoned into the room (`0x0073b880`), message 269.
    pub const IN_RIFT: u32 = 0x800;
    /// Summoned from this list (set after a summon).
    pub const SUMMONED: u32 = 0x400;
    /// No pawn data (level 0), message 263.
    pub const NO_DATA: u32 = 0x1000;
    /// Network pawn while offline, message 272.
    pub const OFFLINE: u32 = 0x2000;
}

/// One list row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub flags: u32,
    pub pawn: Cmc,
    pub online: PawnRef,
}

/// Why a list cannot be shown (`sPawnManager+0x1544`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListError {
    /// Status 6: the category needs the network (`0x0047a460`).
    NeedsNetwork,
}

/// The state of one Rift room slot (`uRim+0xf0 + i*0x70`, state at +0x14).
#[derive(Debug, Clone, PartialEq)]
pub enum RoomState {
    Empty,
    /// Summoned and standing in the room (state 1 or 3).
    Present,
    /// Hired into party slot `slot` (state 2).
    Hired { slot: usize },
    /// Released again (state 4).
    Released,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoomSlot {
    pub state: RoomState,
    pub pawn: Option<(Cmc, PawnRef)>,
    /// The player who summoned it.
    pub summoned_by: Option<PlayerId>,
    /// The player talking to it (the co-op interaction lock).
    pub talking: Option<PlayerId>,
}

/// Number of room slots (`uRim+0xf0`, 7 of 0x70 bytes).
pub const ROOM_SLOTS: usize = 7;
/// At most this many summoned pawns stand in the room (`0x0073bae0`
/// refuses when `uRim+0x410 > 4`).
pub const ROOM_PRESENT_MAX: u32 = 5;

/// The Rift room of one visit (uRim).
#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub slots: Vec<RoomSlot>,
    /// `uRim+0x410`: pawns standing in the room.
    pub present: u32,
    /// `uRim+0x40c`: hired pawns.
    pub hired: u32,
    /// `uRim+0x45c`: the party slot freed by the last release, reused by
    /// the next hire.
    pub last_released: Option<usize>,
}

/// Results of [`Rift::summon`], with the exe's codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonError {
    /// 1: five pawns already stand in the room.
    RoomFull,
    /// The row cannot be summoned (its flags, see [`flags`]).
    Refused(u32),
    /// No free room slot (not traced: the exe's `0x0073ef10` path).
    NoSlot,
}

/// Results of [`Rift::hire`], with `0x0073c2c0`'s return codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HireError {
    /// 5: no such pawn in the room, or already hired.
    NoPawn,
    /// 2: not enough Rift Crystals ("You don't have enough rift crystals
    /// to enter this covenant.").
    NotEnoughCrystals { cost: i32 },
    /// 1: no free hired-pawn slot.
    PartyFull,
    /// Another player is talking to this pawn (co-op lock).
    Locked(PlayerId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rift {
    /// The pawn stock (`mCmcStock`); not saved, rebuilt on load.
    pub stock: Vec<Cmc>,
    pub party: Party,
    pub log: PawnLog,
    pub room: Option<Room>,
    /// The hire-cost multiplier (`sNetwork+0x5ab0`): 1.0 offline.
    pub cost_multiplier: f32,
}

impl Rift {
    pub fn new(players: usize, log: PawnLog) -> Rift {
        Rift {
            stock: vec![Cmc::initial(); STOCK_SLOTS],
            party: Party::new(players),
            log,
            room: None,
            cost_multiplier: rules::DEFAULT_COST_MULTIPLIER,
        }
    }

    /// Builds a pawn from its built-in ID (`0x00795010`). Story IDs below
    /// 20 copy the current stock slot.
    pub fn build_from_id(&self, data: &RiftData, id: u32) -> Result<Option<Cmc>, String> {
        Ok(match RomPawnId::decode(id) {
            RomPawnId::Nora { .. } => Some(builder::build_from_nora_id(&data.builder(), &data.nora, id)?),
            RomPawnId::Pool { index } => {
                let i = if index > 99 { 20 } else { index as usize };
                if i < STORY_SLOTS {
                    let mut c = self.stock[i].clone();
                    c.rom_id = id;
                    Some(c)
                } else {
                    let records = &data.info.records;
                    let mut c = records.get(i).ok_or("romPawnInfo index")?.clone();
                    c.rom_id = id;
                    if data.language != 0 {
                        let p = &data.info.pawns[i];
                        if let Some(n) = data.names.name(c.param.job, p.edit.gender, p.name_no as usize) {
                            c.edit.set_name(n.as_bytes());
                        }
                    }
                    Some(c)
                }
            }
            _ => None,
        })
    }

    /// Refills the stock on a load, offline. Translated from `0x0047d330`
    /// with `0x007951c0` and then `0x00796420` (DDDA.exe 2017-12-13
    /// Steam), in that order:
    /// - favourites with a built-in ID, not hired (`0x00447d90`) and not
    ///   already placed (`0x0077b750`), take slots 20, 21, ...;
    /// - generated pawns fill the rest from slot 119 down around `level`
    ///   (the host Arisen's level; 5 while the load mode is 0);
    /// - the 20 story pawns are rebuilt.
    ///
    /// Not modelled: the arrangement of stock slots into stage groups
    /// (`0x0047ddf0`), which only places pawns in towns.
    pub fn refill(
        &mut self,
        data: &RiftData,
        level: u32,
        load_mode: u32,
        rng: &mut impl PawnRng,
    ) -> Result<(), String> {
        let mut n = 0usize;
        let favs: Vec<PawnRef> = self.log.favorites.clone();
        for f in &favs {
            if n >= POOL_SLOTS {
                break;
            }
            if f.rom_id == u32::MAX || self.party.has_hired(f) {
                continue;
            }
            let placed = (0..n).any(|k| self.stock[POOL_FIRST_STOCK_SLOT + k].rom_id == f.rom_id);
            if placed {
                continue;
            }
            if let Some(c) = self.build_from_id(data, f.rom_id)? {
                self.stock[POOL_FIRST_STOCK_SLOT + n] = c;
                n += 1;
            }
        }
        let mut ids = [u32::MAX; POOL_SLOTS];
        for (k, id) in ids.iter_mut().enumerate() {
            *id = self.stock[POOL_FIRST_STOCK_SLOT + k].rom_id;
        }
        let level = if load_mode == 0 { None } else { Some(level) };
        let count = POOL_SLOTS - n;
        rules::fill_offline_pool(&data.nora, level, count, &mut ids, rng)?;
        for k in 0..count {
            let slot = POOL_SLOTS - 1 - k;
            self.stock[POOL_FIRST_STOCK_SLOT + slot] =
                builder::build_from_nora_id(&data.builder(), &data.nora, ids[slot])?;
        }
        let npc = |id: u32| data.npc_names.iter().find(|n| n.0 == id).map(|n| n.1.clone());
        let name_no: Vec<u32> = data.info.pawns.iter().map(|p| p.name_no).collect();
        let story = builder::build_story_stock(&data.builder(), &data.info.records, &name_no, &npc)?;
        for (i, c) in story.into_iter().enumerate() {
            self.stock[i] = c;
        }
        Ok(())
    }

    /// The Rift pool: stock slots 20..120.
    pub fn pool(&self) -> &[Cmc] {
        &self.stock[POOL_FIRST_STOCK_SLOT..]
    }

    /// Builds a menu list, offline. Translated from `0x0047a550` with the
    /// offline gate `0x0047a460` and the knowledge sort `0x0047af50`
    /// (DDDA.exe 2017-12-13 Steam):
    /// - by level: 100 generated pawns at the level (`0x00794f10`), each
    ///   redrawn up to 100 times while its ID is already listed;
    /// - past summons and favourites: one row per entry with a built-in ID;
    ///   network entries have no data offline and are dropped (flag 0x1000);
    ///   duplicates are dropped;
    /// - other categories need the network;
    /// - rows of pawns already in the room get flag 0x800;
    /// - by level with an active quest, rows whose knowledge covers it move
    ///   to the front in order.
    pub fn list(
        &self,
        data: &RiftData,
        world: &ddstory::World,
        category: Category,
        player_level: u32,
        rng: &mut impl PawnRng,
    ) -> Result<Vec<Row>, ListError> {
        let mut rows: Vec<Row> = Vec::new();
        let in_room = |r: &PawnRef| {
            self.room.as_ref().is_some_and(|room| {
                room.slots.iter().any(|s| {
                    matches!(s.state, RoomState::Present | RoomState::Hired { .. })
                        && s.pawn.as_ref().is_some_and(|(_, o)| o.same_pawn(r))
                })
            })
        };
        let push = |rows: &mut Vec<Row>, pawn: Cmc, online: PawnRef| {
            let mut flags = 0;
            if pawn.param.level == 0 {
                flags |= flags::NO_DATA;
            } else if in_room(&online) {
                flags |= flags::IN_RIFT;
            }
            rows.push(Row { flags, pawn, online });
        };
        match category {
            Category::ByLevel(level) => {
                for _ in 0..100 {
                    // Up to 100 draws; a row still duplicated after them
                    // keeps the pawn with its level byte cleared, so it
                    // shows as "no data" (0x0047a7df).
                    let mut id = 0;
                    let mut unique = false;
                    for _ in 0..100 {
                        id = rules::random_nora_id(&data.nora, level as i32, player_level, rng).unwrap_or(0);
                        if !rows.iter().any(|r| r.online.rom_id == id) {
                            unique = true;
                            break;
                        }
                    }
                    let mut pawn = builder::build_from_nora_id(&data.builder(), &data.nora, id)
                        .unwrap_or_else(|_| Cmc::initial_without_data());
                    if !unique {
                        pawn.param.level = 0;
                    }
                    push(&mut rows, pawn, PawnRef::rom(id));
                }
                if world.active_quest != -1 {
                    let q = world.active_quest as u32;
                    let (mut front, back): (Vec<Row>, Vec<Row>) =
                        rows.into_iter().partition(|r| data.knows_quest(&r.pawn.study_flag, q));
                    front.extend(back);
                    rows = front;
                }
            }
            Category::PastSummons | Category::Favorites => {
                let list = if category == Category::PastSummons { &self.log.past_summons } else { &self.log.favorites };
                for e in list {
                    let pawn = match self.build_from_id(data, e.rom_id) {
                        Ok(Some(c)) => c,
                        _ => Cmc::initial_without_data(),
                    };
                    if pawn.param.level == 0 || rows.iter().any(|r| r.online.same_pawn(e)) {
                        continue;
                    }
                    push(&mut rows, pawn, e.clone());
                }
            }
            Category::MainPawnKnows | Category::Friends | Category::Online(_) => return Err(ListError::NeedsNetwork),
        }
        Ok(rows)
    }

    /// Opens the room for a Riftstone visit (uRim): empty slots, the hire
    /// counter taken from the party.
    pub fn open_room(&mut self) {
        self.room = Some(Room {
            slots: (0..ROOM_SLOTS)
                .map(|_| RoomSlot { state: RoomState::Empty, pawn: None, summoned_by: None, talking: None })
                .collect(),
            present: 0,
            hired: self.party.hired_count() as u32,
            last_released: None,
        });
    }

    pub fn close_room(&mut self) {
        self.room = None;
    }

    /// Summons a listed pawn into the room. From `0x0073bae0`: refused
    /// when five pawns stand there (code 1) or when the row is flagged.
    /// The room slot choice is a simplification (first empty or released
    /// slot).
    pub fn summon(&mut self, player: PlayerId, row: &mut Row) -> Result<usize, SummonError> {
        let room = self.room.as_mut().ok_or(SummonError::NoSlot)?;
        if room.present >= ROOM_PRESENT_MAX {
            return Err(SummonError::RoomFull);
        }
        let blocking = flags::IN_RIFT | flags::NO_DATA | flags::OFFLINE | flags::SUMMONED;
        if row.flags & blocking != 0 {
            return Err(SummonError::Refused(row.flags & blocking));
        }
        let i = room
            .slots
            .iter()
            .position(|s| matches!(s.state, RoomState::Empty | RoomState::Released))
            .ok_or(SummonError::NoSlot)?;
        room.slots[i] = RoomSlot {
            state: RoomState::Present,
            pawn: Some((row.pawn.clone(), row.online.clone())),
            summoned_by: Some(player),
            talking: None,
        };
        room.present += 1;
        row.flags |= flags::SUMMONED;
        Ok(i)
    }

    /// Starts talking to a room pawn; it is locked for other players until
    /// [`Rift::stop_talking`] (the co-op NPC interaction lock).
    pub fn talk(&mut self, player: PlayerId, slot: usize) -> Result<(), HireError> {
        let s = self.room.as_mut().and_then(|r| r.slots.get_mut(slot)).ok_or(HireError::NoPawn)?;
        match s.talking {
            Some(p) if p != player => Err(HireError::Locked(p)),
            _ => {
                s.talking = Some(player);
                Ok(())
            }
        }
    }

    pub fn stop_talking(&mut self, player: PlayerId, slot: usize) {
        if let Some(s) = self.room.as_mut().and_then(|r| r.slots.get_mut(slot)) {
            if s.talking == Some(player) {
                s.talking = None;
            }
        }
    }

    /// The cost `arisen` would pay for a pawn (`0x0043eec0`; offline no
    /// owner is a friend).
    pub fn cost(&self, arisen: &Arisen, pawn: &Cmc) -> i32 {
        rules::hire_cost(arisen.level as i32, pawn.param.level as u32, self.cost_multiplier, false)
    }

    /// Hires a room pawn for the party. Translated from `0x0073c2c0`
    /// (DDDA.exe 2017-12-13 Steam), in its order: the pawn must stand in
    /// the room (code 5); the cost (`0x0043eec0`, the hiring Arisen's
    /// level) must not exceed the Arisen's crystals (2); a hired slot must
    /// be free (1). The party slot is the one freed last in this visit,
    /// else 1, else 2 (in co-op the second Arisen's slot is never used);
    /// the crystals are spent with `0x004433f0`.
    pub fn hire(&mut self, arisen: &mut Arisen, slot: usize, play_time: u32) -> Result<usize, HireError> {
        let capacity = self.party.hire_capacity() as u32;
        let free_slot = |party: &Party, last: Option<usize>| -> Option<usize> {
            if let Some(s) = last {
                if party.hired[s - 1].is_none() && !party.reserved[s - 1] {
                    return Some(s);
                }
            }
            (1..=2).find(|&s| party.hired[s - 1].is_none() && !party.reserved[s - 1])
        };
        let room = self.room.as_ref().ok_or(HireError::NoPawn)?;
        let s = room.slots.get(slot).ok_or(HireError::NoPawn)?;
        if let Some(p) = s.talking {
            if p != arisen.id {
                return Err(HireError::Locked(p));
            }
        }
        let (pawn, online) = match (&s.state, &s.pawn) {
            (RoomState::Present, Some(p)) => p.clone(),
            _ => return Err(HireError::NoPawn),
        };
        let cost = self.cost(arisen, &pawn).max(0);
        if arisen.rift_crystals < cost {
            return Err(HireError::NotEnoughCrystals { cost });
        }
        if room.hired >= capacity {
            return Err(HireError::PartyFull);
        }
        let party_slot = free_slot(&self.party, room.last_released).ok_or(HireError::PartyFull)?;
        let room = self.room.as_mut().unwrap();
        room.slots[slot].state = RoomState::Hired { slot: party_slot };
        room.present -= 1;
        room.hired += 1;
        self.party.hired[party_slot - 1] = Some(PartyPawn { pawn, online, hire_time: play_time, quests_cleared: 0 });
        arisen.spend(cost);
        Ok(party_slot)
    }

    /// Counts a cleared quest for every hired pawn (`cCmcInfo+0x1654`).
    pub fn quest_cleared(&mut self) {
        for p in self.party.hired.iter_mut().flatten() {
            p.quests_cleared += 1;
        }
    }

    /// Releases a hired pawn (Riftstone `0x0073c480`, talk `0x00b9cec0`),
    /// or records its death (`0x0043e9b0`), with the bookkeeping of
    /// `0x0043ec60` (DDDA.exe 2017-12-13 Steam). A pawn whose pawn type is 2
    /// (story pawn) is not removed by a death. Returns the departing pawn's
    /// counters, or `None` if the slot was empty or the pawn stays.
    ///
    /// Not modelled: taking back the items tagged with the pawn's owner id
    /// (`0x0045f850`), which belongs to the inventory.
    pub fn release(&mut self, party_slot: usize, play_time: u32, died: bool) -> Option<PawnRef> {
        let p = self.party.hired.get(party_slot.checked_sub(1)?)?.as_ref()?;
        if died && p.pawn.pawn_type == 2 {
            return None;
        }
        let p = self.party.hired[party_slot - 1].take()?;
        let with_time = play_time.wrapping_sub(p.hire_time);
        let out = self.log.record_departure(&p.pawn, &p.online, with_time, p.quests_cleared, died);
        self.log.add_history(history_entry(&p, &out, with_time));
        if let Some(room) = self.room.as_mut() {
            room.hired = room.hired.saturating_sub(1);
            room.last_released = Some(party_slot);
            for s in room.slots.iter_mut() {
                if s.state == (RoomState::Hired { slot: party_slot }) {
                    s.state = RoomState::Released;
                }
            }
        }
        Some(out)
    }
}

/// The history entry `0x0077bf10` makes for a departing pawn: appearance,
/// the updated counters, the time in the party this time, level, vocation,
/// skills and equipment. Its six skills are taken here from the
/// vocation's weapon skill slots in order (guessed: the exe reads them
/// from `cCmcInfo+0x270`; this matches the saved story pawn), with skill
/// level 2 for upgraded skills, 1 otherwise and 0 for empty slots
/// (guessed from the save; the exe asks `0x00780ba0`).
fn history_entry(p: &PartyPawn, out: &PawnRef, with_time: u32) -> HistoryEntry {
    let prm = &p.pawn.param;
    let classes: Vec<usize> = match prm.job {
        2 => vec![4, 10],
        3 => vec![5],
        7 => vec![3, 7],
        8 => vec![4, 11],
        9 => vec![6],
        _ => vec![1, 8],
    };
    let mut skills = [-1i32; 6];
    let mut k = 0;
    for w in &classes {
        let per = if classes.len() == 1 { 6 } else { 3 };
        for i in 0..per {
            skills[k] = prm.weapon_skill[w - 1][i] as i32;
            k += 1;
        }
    }
    let mut skill_levels = [0u32; 6];
    for (i, &s) in skills.iter().enumerate() {
        if s >= 0 {
            let two = prm.skill_lv2[(s >> 5) as usize] & (1 << (s & 31)) != 0;
            skill_levels[i] = if two { 2 } else { 1 };
        }
    }
    HistoryEntry {
        edit: p.pawn.edit.clone(),
        online: out.clone(),
        employ_game_play_time: with_time,
        level: prm.level as u32,
        job: prm.job as i32,
        skills,
        skill_levels,
        equip: std::array::from_fn(|i| prm.equip[i].item_no as i32),
    }
}

/// Lets the ddrift crate build an empty record for rows without data.
trait WithoutData {
    fn initial_without_data() -> Cmc;
}

impl WithoutData for Cmc {
    fn initial_without_data() -> Cmc {
        let mut c = Cmc::initial();
        c.param.level = 0;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u32, level: u8) -> Row {
        let mut pawn = Cmc::initial();
        pawn.param.level = level;
        pawn.rom_id = id;
        Row { flags: 0, pawn, online: PawnRef::rom(id) }
    }

    fn arisen(id: u8, level: u32, rc: i32) -> Arisen {
        Arisen { id: id, level, rift_crystals: rc }
    }

    #[test]
    fn single_player_room_and_hires() {
        let mut rift = Rift::new(1, PawnLog::default());
        rift.open_room();
        let mut rows: Vec<Row> = (0..6).map(|i| row(0x1000_0000 + i, 10)).collect();
        for r in rows.iter_mut().take(5) {
            rift.summon(0, r).unwrap();
        }
        assert_eq!(rift.summon(0, &mut rows[5]), Err(SummonError::RoomFull));
        assert_eq!(rift.summon(0, &mut rows[0]), Err(SummonError::RoomFull));
        let mut p1 = arisen(0, 7, 100);
        // (10+7)*(10-7)^2 + 18 = 171 > 100.
        assert_eq!(rift.hire(&mut p1, 0, 0), Err(HireError::NotEnoughCrystals { cost: 171 }));
        p1.rift_crystals = 400;
        assert_eq!(rift.hire(&mut p1, 0, 1000), Ok(1));
        assert_eq!(p1.rift_crystals, 229);
        assert_eq!(rift.hire(&mut p1, 1, 1000), Ok(2));
        // The exe checks the crystals before the party (0x0073c2c0).
        assert_eq!(rift.hire(&mut p1, 2, 1000), Err(HireError::NotEnoughCrystals { cost: 171 }));
        p1.rift_crystals = 1000;
        assert_eq!(rift.hire(&mut p1, 2, 1000), Err(HireError::PartyFull));
        assert_eq!(rift.hire(&mut p1, 0, 1000), Err(HireError::NoPawn));
        // Release slot 1; the next hire reuses it.
        let out = rift.release(1, 4600, false).unwrap();
        assert_eq!((out.with_time, out.employ, out.die), (3600, 1, 0));
        assert_eq!(rift.log.past_summons.len(), 1);
        assert_eq!(rift.log.history.len(), 1);
        assert_eq!(rift.log.close_friends.len(), 1);
        assert_eq!(rift.hire(&mut p1, 2, 5000), Ok(1));
        // A pawn at or below the Arisen's level is free.
        let mut p_hi = arisen(0, 50, 0);
        rift.release(2, 6000, true);
        rift.summon(0, &mut rows[5]).unwrap();
        let i = rift.room.as_ref().unwrap().slots.iter().position(|s| s.state == RoomState::Present).unwrap();
        assert_eq!(rift.hire(&mut p_hi, i, 6000), Ok(2));
        assert_eq!(p_hi.rift_crystals, 0);
    }

    #[test]
    fn two_players_share_one_hired_slot_and_lock_talks() {
        let mut rift = Rift::new(2, PawnLog::default());
        assert_eq!(rift.party.hire_capacity(), 1);
        rift.open_room();
        let mut rows: Vec<Row> = (0..2).map(|i| row(0x1000_0000 + i, 10)).collect();
        rift.summon(0, &mut rows[0]).unwrap();
        rift.summon(1, &mut rows[1]).unwrap();
        let mut p1 = arisen(0, 7, 1000);
        let mut p2 = arisen(1, 20, 0);
        rift.talk(0, 0).unwrap();
        assert_eq!(rift.talk(1, 0), Err(HireError::Locked(0)));
        assert_eq!(rift.hire(&mut p2, 0, 0), Err(HireError::Locked(0)));
        rift.stop_talking(0, 0);
        // P2 (level 20) pays nothing for a level-10 pawn; P1 would pay 171.
        assert_eq!(rift.hire(&mut p2, 0, 0), Ok(1));
        assert_eq!((p1.rift_crystals, p2.rift_crystals), (1000, 0));
        assert_eq!(rift.hire(&mut p1, 1, 0), Err(HireError::PartyFull));
        assert!(rift.party.hired[1].is_none());
    }

    #[test]
    fn story_pawn_survives_death() {
        let mut rift = Rift::new(1, PawnLog::default());
        let mut pawn = Cmc::initial();
        pawn.pawn_type = 2;
        rift.party.hired[0] =
            Some(PartyPawn { pawn, online: PawnRef::rom(0x2000_000e), hire_time: 0, quests_cleared: 0 });
        assert_eq!(rift.release(1, 100, true), None);
        assert!(rift.party.hired[0].is_some());
        assert!(rift.release(1, 100, false).is_some());
    }
}

#[cfg(test)]
mod live {
    /// 0x0043eec0 called live in the original on a zeroed pawn record with
    /// +0x700 = L and a built-in ID (oracle, claude-4, board #879).
    #[test]
    fn hire_cost_matches_the_original() {
        for (p, l, cost) in [(7, 10, 171), (10, 20, 3018), (1, 200, 7_959_819), (50, 10, 0), (12, 31, 15_541)] {
            assert_eq!(ddpawn::rift::hire_cost(p, l, 1.0, false), cost, "P {p} L {l}");
        }
    }
}
