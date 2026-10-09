//! The saved pawn bookkeeping: favourites, past summons, pawns that know
//! the main pawn, close friends and the summon history
//! (`sSave::networkGameData`, held by sGameSys at run time).

use ddpawn::cmc::Cmc;
use ddsave::xml::Node;

/// A pawn's identity and counters (`cOnlinePawnInfo` /
/// `cSAVE_DATA_ONLINE_PAWN`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PawnRef {
    /// `mRomPawnUniqueID`; `u32::MAX` for a network pawn.
    pub rom_id: u32,
    /// `mNetUniqueId`, 64 bytes (all zero for built-in pawns).
    pub net_id: Vec<u8>,
    pub with_time: u32,
    pub employ: u32,
    pub die: u32,
    pub clear_quest: u32,
    pub lent: u32,
}

impl PawnRef {
    pub fn rom(rom_id: u32) -> PawnRef {
        PawnRef { rom_id, net_id: vec![0; 64], with_time: 0, employ: 0, die: 0, clear_quest: 0, lent: 0 }
    }

    /// The identity test the exe repeats everywhere (`0x00448050`,
    /// `0x00448da0`, `0x0073b880`...): equal built-in IDs, or both network
    /// pawns with equal network IDs; a built-in pawn never equals a network
    /// one.
    pub fn same_pawn(&self, other: &PawnRef) -> bool {
        match (self.rom_id == u32::MAX, other.rom_id == u32::MAX) {
            (false, false) => self.rom_id == other.rom_id,
            (true, true) => self.net_id == other.net_id,
            _ => false,
        }
    }

    fn add_counters(&mut self, o: &PawnRef) {
        self.with_time = self.with_time.wrapping_add(o.with_time);
        self.employ = self.employ.wrapping_add(o.employ);
        self.die = self.die.wrapping_add(o.die);
        self.clear_quest = self.clear_quest.wrapping_add(o.clear_quest);
        self.lent = self.lent.wrapping_add(o.lent);
    }

    fn read(n: Node<'_, '_>, net: &str, employ: &str, die: &str, clear: &str, lent: &str) -> Option<PawnRef> {
        let net_id: Vec<u8> = match n.child(net)? {
            id if id.tag() == "array" => n.array(net)?,
            id => id.array("mData")?,
        };
        Some(PawnRef {
            rom_id: n.get("mRomPawnUniqueID")?,
            net_id,
            with_time: n.get("mWithTime")?,
            employ: n.get(employ)?,
            die: n.get(die)?,
            clear_quest: n.get(clear)?,
            lent: n.get(lent)?,
        })
    }

    /// From a save `cSAVE_DATA_ONLINE_PAWN`.
    pub fn from_save_entry(n: Node<'_, '_>) -> Option<PawnRef> {
        Self::read(n, "mNetworkUniqueID", "mEmployCount", "mDieCount", "mClearQuestCount", "mLentCount")
    }

    /// From a save `cOnlinePawnInfo`.
    pub fn from_save_info(n: Node<'_, '_>) -> Option<PawnRef> {
        Self::read(n, "mNetUniqueId", "mEmployCnt", "mDieCnt", "mClearQuestCnt", "mLentCnt")
    }
}

/// One `cPawnHistoryInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub edit: ddpawn::cmc::Edit,
    pub online: PawnRef,
    /// Play time spent in the party this time (`0x0077bf10`).
    pub employ_game_play_time: u32,
    pub level: u32,
    pub job: i32,
    pub skills: [i32; 6],
    pub skill_levels: [u32; 6],
    pub equip: [i32; 12],
}

/// Capacities from the exe: 100 (past summons `0x00448130`, history
/// `0x00449380`), 4 close friends (`0x00448500`).
pub const PAST_SUMMONS_MAX: usize = 100;
pub const HISTORY_MAX: usize = 100;
pub const CLOSE_FRIENDS_MAX: usize = 4;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PawnLog {
    /// `mFavoritePawn` (sGameSys +0x9e3ec).
    pub favorites: Vec<PawnRef>,
    /// `mBorrowPawn`: Past Summons (+0x9e404), oldest first.
    pub past_summons: Vec<PawnRef>,
    /// `mLentPawn`: Pawns Your Main Pawn Knows (+0x9e41c).
    pub lent: Vec<PawnRef>,
    /// `mCloseFriendPawn`: full snapshots, longest time together first.
    pub close_friends: Vec<(Cmc, PawnRef)>,
    /// `mPawnHistory`, oldest first.
    pub history: Vec<HistoryEntry>,
}

impl PawnLog {
    /// Reads one `sSave::playerData`'s `mNetGameData`.
    pub fn from_save(player: Node<'_, '_>) -> Option<PawnLog> {
        let net = player.child("mNetGameData")?;
        let count = |name: &str| net.get::<i64>(name).map(|v| v.max(0) as usize);
        let list = |arr: &str, num: &str| -> Option<Vec<PawnRef>> {
            net.child(arr)?.children().take(count(num)?).map(PawnRef::from_save_entry).collect()
        };
        let friends = net
            .child("mCloseFriendPawn")?
            .children()
            .take(count("mCloseFriendPawnCount")?)
            .map(|n| Some((Cmc::from_save(n)?, PawnRef::from_save_info(n.child("mOnlinePawnInfo")?)?)))
            .collect::<Option<Vec<_>>>()?;
        let history = net
            .child("mPawnHistory")?
            .children()
            .take(count("mPawnHistoryNum")?)
            .map(|e| {
                let arr6 = |name: &str| -> Option<[i32; 6]> {
                    let v: Vec<i32> = e.array(name)?;
                    v.get(..6)?.try_into().ok()
                };
                let lv: Vec<u32> = e.array("mSkillLevel")?;
                let eq: Vec<i32> = e.array("mEquip")?;
                Some(HistoryEntry {
                    edit: Cmc::from_save_edit(e.child("mPawnEdit")?)?,
                    online: PawnRef::from_save_info(e.child("mOnlinePawnInfo")?)?,
                    employ_game_play_time: e.get("mEmployGamePlayTime")?,
                    level: e.get("mLevel")?,
                    job: e.get("mJob")?,
                    skills: arr6("mSkill")?,
                    skill_levels: lv.get(..6)?.try_into().ok()?,
                    equip: eq.get(..12)?.try_into().ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(PawnLog {
            favorites: list("mFavoritePawn", "mFavoritePawnNum")?,
            past_summons: list("mBorrowPawn", "mBorrowPawnNum")?,
            lent: list("mLentPawn", "mLentPawnNum")?,
            close_friends: friends,
            history,
        })
    }

    /// Records a hired pawn leaving the party (release or death), for party
    /// slots 1 and 2. `online` holds the pawn's identity and lent count;
    /// `with_time` is the play time since the hire, `quests` the quests
    /// cleared meanwhile. Returns the counters the pawn leaves with.
    ///
    /// Translated from `0x00448da0` (DDDA.exe 2017-12-13 Steam), which calls
    /// `0x00448470` (lent entry), `0x00448130` (past summons) and
    /// `0x00448500` (close friends):
    /// 1. the pawn's counters become `(with_time, 1, died, quests)`;
    /// 2. if it is a favourite, the favourite's counters are added and the
    ///    favourite takes the result; otherwise the counters of its
    ///    past-summons entry, if any, are added;
    /// 3. its "knows your pawn" entry, if any, takes the result;
    /// 4. its past-summons entry is removed and the result appended,
    ///    dropping the oldest entries beyond 99 first;
    /// 5. the close friends are updated with the snapshot.
    pub fn record_departure(
        &mut self,
        pawn: &Cmc,
        online: &PawnRef,
        with_time: u32,
        quests: u32,
        died: bool,
    ) -> PawnRef {
        let mut r = online.clone();
        r.with_time = with_time;
        r.employ = 1;
        r.clear_quest = quests;
        r.die = died as u32;
        let borrow = self.past_summons.iter().position(|e| e.same_pawn(&r));
        if let Some(f) = self.favorites.iter().position(|e| e.same_pawn(&r)) {
            let fav = self.favorites[f].clone();
            r.add_counters(&fav);
            self.favorites[f] = r.clone();
        } else if let Some(b) = borrow {
            let prev = self.past_summons[b].clone();
            r.add_counters(&prev);
        }
        if let Some(l) = self.lent.iter().position(|e| e.same_pawn(&r)) {
            self.lent[l] = r.clone();
        }
        if let Some(b) = self.past_summons.iter().position(|e| e.same_pawn(&r)) {
            self.past_summons.remove(b);
        }
        while self.past_summons.len() >= PAST_SUMMONS_MAX {
            self.past_summons.remove(0);
        }
        self.past_summons.push(r.clone());
        self.update_close_friends(pawn, &r);
        r
    }

    /// Translated from `0x00448500` (DDDA.exe 2017-12-13 Steam): a top-4
    /// list sorted by `mWithTime`, longest first. A pawn already listed
    /// moves up past every entry with less time; a new pawn is inserted
    /// below every entry with at least as much time, and only if the list
    /// has room or it beats the fourth entry (which then drops out).
    pub fn update_close_friends(&mut self, pawn: &Cmc, r: &PawnRef) {
        let snap = (pawn.clone(), r.clone());
        let t = r.with_time;
        if let Some(i) = self.close_friends.iter().position(|(_, e)| e.same_pawn(r)) {
            self.close_friends.remove(i);
            let pos = (0..i).rev().find(|&j| t <= self.close_friends[j].1.with_time).map_or(0, |j| j + 1);
            self.close_friends.insert(pos, snap);
            return;
        }
        if self.close_friends.len() >= CLOSE_FRIENDS_MAX {
            if t <= self.close_friends[CLOSE_FRIENDS_MAX - 1].1.with_time {
                return;
            }
            self.close_friends.truncate(CLOSE_FRIENDS_MAX - 1);
        }
        let pos = (0..self.close_friends.len())
            .rev()
            .find(|&j| t <= self.close_friends[j].1.with_time)
            .map_or(0, |j| j + 1);
        self.close_friends.insert(pos, snap);
    }

    /// Translated from `0x00449380` (DDDA.exe 2017-12-13 Steam): append; when
    /// 100 entries are held, first remove the oldest entry whose pawn is not
    /// in the lent list (or the oldest of all if every one is).
    pub fn add_history(&mut self, entry: HistoryEntry) {
        if self.history.len() >= HISTORY_MAX {
            let i = self
                .history
                .iter()
                .position(|h| !self.lent.iter().any(|l| l.same_pawn(&h.online)))
                .unwrap_or(0);
            self.history.remove(i);
        }
        self.history.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: u32, t: u32) -> PawnRef {
        PawnRef { with_time: t, ..PawnRef::rom(id) }
    }

    #[test]
    fn identity() {
        assert!(r(1, 0).same_pawn(&r(1, 9)));
        assert!(!r(1, 0).same_pawn(&r(2, 0)));
        let mut a = PawnRef::rom(u32::MAX);
        let b = a.clone();
        assert!(a.same_pawn(&b));
        a.net_id[0] = 1;
        assert!(!a.same_pawn(&b));
        assert!(!a.same_pawn(&r(1, 0)));
    }

    #[test]
    fn close_friends_order() {
        let c = Cmc::initial();
        let mut log = PawnLog::default();
        for (id, t) in [(1, 50), (2, 80), (3, 50), (4, 10)] {
            log.update_close_friends(&c, &r(id, t));
        }
        let ids = |l: &PawnLog| l.close_friends.iter().map(|f| f.1.rom_id).collect::<Vec<_>>();
        assert_eq!(ids(&log), vec![2, 1, 3, 4]);
        // Full: equal to the last does not enter; more does.
        log.update_close_friends(&c, &r(5, 10));
        assert_eq!(ids(&log), vec![2, 1, 3, 4]);
        log.update_close_friends(&c, &r(5, 60));
        assert_eq!(ids(&log), vec![2, 5, 1, 3]);
        // A listed pawn moves up past entries with less time.
        log.update_close_friends(&c, &r(3, 90));
        assert_eq!(ids(&log), vec![3, 2, 5, 1]);
    }

    #[test]
    fn departure_counters_and_fifo() {
        let c = Cmc::initial();
        let mut log = PawnLog::default();
        for i in 0..100 {
            log.past_summons.push(r(1000 + i, 1));
        }
        log.past_summons[5] = PawnRef { employ: 2, with_time: 100, ..r(7, 0) };
        let out = log.record_departure(&c, &r(7, 0), 30, 2, true);
        assert_eq!((out.with_time, out.employ, out.die, out.clear_quest), (130, 3, 1, 2));
        assert_eq!(log.past_summons.len(), 100);
        assert_eq!(log.past_summons.last(), Some(&out));
        assert!(log.past_summons.iter().filter(|e| e.rom_id == 7).count() == 1);
        // A favourite takes the totals; its own counters are the base.
        log.favorites.push(PawnRef { employ: 5, with_time: 1000, ..r(9, 0) });
        let out = log.record_departure(&c, &r(9, 0), 10, 0, false);
        assert_eq!((out.with_time, out.employ), (1010, 6));
        assert_eq!(log.favorites[0], out);
        assert_eq!(log.past_summons.len(), 100);
    }

    #[test]
    fn history_eviction_spares_lent_pawns() {
        let mut log = PawnLog::default();
        let e = |id| HistoryEntry {
            edit: Cmc::initial().edit,
            online: PawnRef::rom(id),
            employ_game_play_time: 0,
            level: 1,
            job: 1,
            skills: [-1; 6],
            skill_levels: [0; 6],
            equip: [-1; 12],
        };
        for i in 0..100 {
            log.add_history(e(i));
        }
        log.lent.push(PawnRef::rom(0));
        log.add_history(e(500));
        assert_eq!(log.history.len(), 100);
        assert_eq!(log.history[0].online.rom_id, 0);
        assert_eq!(log.history[1].online.rom_id, 2);
        assert_eq!(log.history[99].online.rom_id, 500);
    }
}
