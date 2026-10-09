//! The Rift's save part (the host's `RIFT` chunk, ADR 0002 section 6).
//!
//! **Shared** (the Rift is shared by both Arisen):
//! - the party's pawn slots (main pawn, hired pawns, the slot reserved
//!   for the second Arisen);
//! - the pawn log;
//! - the hire-cost multiplier.
//!
//! No part per seat: the Rift keeps nothing per player beyond the visit.
//!
//! **Not saved:**
//! - **The stock.** The original rebuilds it on every load
//!   (`Rift::refill`, 0x0047D330), so a loaded Rift starts from initial
//!   records and the host refills it.
//! - **The room** of an open visit. The host saves only while no one is at
//!   a Riftstone.
//!
//! Our own format, not `cSAVE_DATA_*`.

use crate::log::{HistoryEntry, PawnLog, PawnRef};
use crate::rift::{Party, PartyPawn, Rift};
use ddid::{Parts, Reader, Result, Writer};
use ddpawn::cmc::{Cmc, Edit, Equip, Param};

const VERSION: u8 = 1;

fn edit(w: &mut Writer, e: &Edit) {
    for &b in &e.name {
        w.u8(b);
    }
    w.u32(e.nickname);
    for &b in &e.fields {
        w.u8(b);
    }
}

fn read_edit(r: &mut Reader) -> Result<Edit> {
    let mut name = [0u8; 25];
    for b in &mut name {
        *b = r.u8()?;
    }
    let nickname = r.u32()?;
    let mut fields = [0u8; 41];
    for b in &mut fields {
        *b = r.u8()?;
    }
    Ok(Edit { name, nickname, fields })
}

fn cmc(w: &mut Writer, c: &Cmc) {
    edit(w, &c.edit);
    let p = &c.param;
    w.u8(p.level).u8(p.job);
    for &v in &p.job_level {
        w.u8(v);
    }
    for set in &p.weapon_skill {
        for &v in set {
            w.i16(v);
        }
    }
    for &v in p.skill_lv1.iter().chain(&p.skill_lv2) {
        w.u32(v);
    }
    for &v in &p.ability {
        w.i8(v);
    }
    for &v in &p.ability_info {
        w.u32(v);
    }
    for e in &p.equip {
        w.i16(e.num).i16(e.item_no).u32(e.flag);
        for &x in &e.extra {
            w.u16(x);
        }
    }
    for v in [
        p.hp,
        p.hp_max,
        p.hp_max_white,
        p.stamina,
        p.stamina_base,
        p.stamina_lv,
        p.attack,
        p.defend,
        p.mgc_attack,
        p.mgc_defend,
    ] {
        w.f32(v);
    }
    w.i32(c.status).len(c.study_flag.len());
    for &v in &c.study_flag {
        w.u32(v);
    }
    for &v in &c.info_value {
        w.f32(v);
    }
    for &v in &c.info_status {
        w.u32(v);
    }
    for &v in &c.info_ex_value {
        w.f32(v);
    }
    for &v in &c.use_scene_talk {
        w.u32(v);
    }
    w.i32(c.pawn_type).u32(c.employ_game_play_time).u32(c.rom_id);
}

fn read_cmc(r: &mut Reader) -> Result<Cmc> {
    let edit = read_edit(r)?;
    let (level, job) = (r.u8()?, r.u8()?);
    let mut job_level = [0u8; 10];
    for v in &mut job_level {
        *v = r.u8()?;
    }
    let mut weapon_skill = [[0i16; 6]; 12];
    for set in &mut weapon_skill {
        for v in set.iter_mut() {
            *v = r.i16()?;
        }
    }
    let mut skill_lv1 = [0u32; 14];
    let mut skill_lv2 = [0u32; 14];
    for v in skill_lv1.iter_mut().chain(skill_lv2.iter_mut()) {
        *v = r.u32()?;
    }
    let mut ability = [0i8; 6];
    for v in &mut ability {
        *v = r.i8()?;
    }
    let mut ability_info = [0u32; 4];
    for v in &mut ability_info {
        *v = r.u32()?;
    }
    let mut equip = [Equip::EMPTY; 12];
    for e in &mut equip {
        *e = Equip { num: r.i16()?, item_no: r.i16()?, flag: r.u32()?, extra: [r.u16()?, r.u16()?, r.u16()?, r.u16()?] };
    }
    let param = Param {
        level,
        job,
        job_level,
        weapon_skill,
        skill_lv1,
        skill_lv2,
        ability,
        ability_info,
        equip,
        hp: r.f32()?,
        hp_max: r.f32()?,
        hp_max_white: r.f32()?,
        stamina: r.f32()?,
        stamina_base: r.f32()?,
        stamina_lv: r.f32()?,
        attack: r.f32()?,
        defend: r.f32()?,
        mgc_attack: r.f32()?,
        mgc_defend: r.f32()?,
    };
    let status = r.i32()?;
    let n = r.len(4)?;
    let study_flag = (0..n).map(|_| r.u32()).collect::<Result<_>>()?;
    let mut info_value = [0f32; 9];
    for v in &mut info_value {
        *v = r.f32()?;
    }
    let mut info_status = [0u32; 9];
    for v in &mut info_status {
        *v = r.u32()?;
    }
    let mut info_ex_value = [0f32; 2];
    for v in &mut info_ex_value {
        *v = r.f32()?;
    }
    let mut use_scene_talk = [0u32; 6];
    for v in &mut use_scene_talk {
        *v = r.u32()?;
    }
    Ok(Cmc {
        edit,
        param,
        status,
        study_flag,
        info_value,
        info_status,
        info_ex_value,
        use_scene_talk,
        pawn_type: r.i32()?,
        employ_game_play_time: r.u32()?,
        rom_id: r.u32()?,
    })
}

fn pawn_ref(w: &mut Writer, p: &PawnRef) {
    w.u32(p.rom_id).bytes(&p.net_id).u32(p.with_time).u32(p.employ).u32(p.die).u32(p.clear_quest).u32(p.lent);
}

fn read_pawn_ref(r: &mut Reader) -> Result<PawnRef> {
    Ok(PawnRef {
        rom_id: r.u32()?,
        net_id: r.bytes()?,
        with_time: r.u32()?,
        employ: r.u32()?,
        die: r.u32()?,
        clear_quest: r.u32()?,
        lent: r.u32()?,
    })
}

fn refs(w: &mut Writer, v: &[PawnRef]) {
    w.len(v.len());
    for p in v {
        pawn_ref(w, p);
    }
}

fn read_refs(r: &mut Reader) -> Result<Vec<PawnRef>> {
    let n = r.len(28)?;
    (0..n).map(|_| read_pawn_ref(r)).collect()
}

fn party_pawn(w: &mut Writer, p: &Option<PartyPawn>) {
    w.bool(p.is_some());
    if let Some(p) = p {
        cmc(w, &p.pawn);
        pawn_ref(w, &p.online);
        w.u32(p.hire_time).u32(p.quests_cleared);
    }
}

fn read_party_pawn(r: &mut Reader) -> Result<Option<PartyPawn>> {
    if !r.bool()? {
        return Ok(None);
    }
    Ok(Some(PartyPawn { pawn: read_cmc(r)?, online: read_pawn_ref(r)?, hire_time: r.u32()?, quests_cleared: r.u32()? }))
}

fn history(w: &mut Writer, h: &HistoryEntry) {
    edit(w, &h.edit);
    pawn_ref(w, &h.online);
    w.u32(h.employ_game_play_time).u32(h.level).i32(h.job);
    for &v in &h.skills {
        w.i32(v);
    }
    for &v in &h.skill_levels {
        w.u32(v);
    }
    for &v in &h.equip {
        w.i32(v);
    }
}

fn read_history(r: &mut Reader) -> Result<HistoryEntry> {
    let edit = read_edit(r)?;
    let online = read_pawn_ref(r)?;
    let (employ_game_play_time, level, job) = (r.u32()?, r.u32()?, r.i32()?);
    let mut skills = [0i32; 6];
    for v in &mut skills {
        *v = r.i32()?;
    }
    let mut skill_levels = [0u32; 6];
    for v in &mut skill_levels {
        *v = r.u32()?;
    }
    let mut equip = [0i32; 12];
    for v in &mut equip {
        *v = r.i32()?;
    }
    Ok(HistoryEntry { edit, online, employ_game_play_time, level, job, skills, skill_levels, equip })
}

impl Rift {
    pub fn save(&self) -> Parts {
        let mut w = Writer::new();
        w.u8(VERSION);
        party_pawn(&mut w, &self.party.main_pawn);
        for h in &self.party.hired {
            party_pawn(&mut w, h);
        }
        w.bool(self.party.reserved[0]).bool(self.party.reserved[1]);
        let l = &self.log;
        refs(&mut w, &l.favorites);
        refs(&mut w, &l.past_summons);
        refs(&mut w, &l.lent);
        w.len(l.close_friends.len());
        for (c, p) in &l.close_friends {
            cmc(&mut w, c);
            pawn_ref(&mut w, p);
        }
        w.len(l.history.len());
        for h in &l.history {
            history(&mut w, h);
        }
        w.f32(self.cost_multiplier);
        Parts { shared: w.finish(), players: Default::default() }
    }

    /// A Rift from [`Rift::save`]'s part. The stock is the initial records
    /// until the host refills it (as the original does on a load), and no
    /// room is open.
    pub fn load(parts: &Parts) -> Result<Rift> {
        let mut r = Reader::new(&parts.shared);
        r.version("RIFT", VERSION)?;
        let main_pawn = read_party_pawn(&mut r)?;
        let hired = [read_party_pawn(&mut r)?, read_party_pawn(&mut r)?];
        let reserved = [r.bool()?, r.bool()?];
        let favorites = read_refs(&mut r)?;
        let past_summons = read_refs(&mut r)?;
        let lent = read_refs(&mut r)?;
        let n = r.len(1)?;
        let close_friends = (0..n).map(|_| Ok((read_cmc(&mut r)?, read_pawn_ref(&mut r)?))).collect::<Result<_>>()?;
        let n = r.len(1)?;
        let history = (0..n).map(|_| read_history(&mut r)).collect::<Result<_>>()?;
        let cost_multiplier = r.f32()?;
        r.end()?;
        let mut rift = Rift::new(1, PawnLog { favorites, past_summons, lent, close_friends, history });
        rift.party = Party { main_pawn, hired, reserved };
        rift.cost_multiplier = cost_multiplier;
        Ok(rift)
    }

    /// Someone is at a Riftstone (the room is open).
    pub fn visit_open(&self) -> bool {
        self.room.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rift::{Arisen, Row, STOCK_SLOTS};

    fn row(id: u32, level: u8) -> Row {
        let mut pawn = Cmc::initial();
        pawn.param.level = level;
        pawn.param.hp = 412.5;
        pawn.study_flag[7] = 0xdead_beef;
        pawn.edit.set_name(b"Rook");
        pawn.rom_id = id;
        Row { flags: 0, pawn, online: PawnRef::rom(id) }
    }

    #[test]
    fn round_trip_without_stock_or_room() {
        let mut rift = Rift::new(2, PawnLog::default());
        rift.log.favorites.push(PawnRef::rom(0x1000_0001));
        rift.open_room();
        let mut rows: Vec<Row> = (0..3).map(|i| row(0x1000_0000 + i, 10)).collect();
        for r in &mut rows {
            rift.summon(0, r).unwrap();
        }
        let mut host = Arisen { id: 0, level: 9, rift_crystals: 5000 };
        assert_eq!(rift.hire(&mut host, 0, 100), Ok(1));
        rift.release(1, 4000, false).unwrap();
        assert_eq!(rift.hire(&mut host, 1, 4100), Ok(1));
        rift.quest_cleared();
        assert!(rift.visit_open());
        rift.close_room();
        let parts = rift.save();
        assert!(parts.players.is_empty());
        let back = Rift::load(&parts).unwrap();
        assert_eq!(back.party, rift.party);
        assert_eq!(back.log, rift.log);
        assert_eq!(back.cost_multiplier, rift.cost_multiplier);
        assert!(!back.visit_open());
        assert_eq!(back.stock, vec![Cmc::initial(); STOCK_SLOTS]);
        // Everything saved survives exactly: saving again gives the same bytes.
        assert_eq!(back.save(), parts);
        let mut bad = parts.clone();
        bad.shared.truncate(bad.shared.len() - 1);
        assert!(Rift::load(&bad).is_err());
    }
}
