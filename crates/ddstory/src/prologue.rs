//! New-game party selection, separate from story progression.
//!
//! This consumes the installed-data story stock built by
//! `ddpawn::builder::build_story_stock`. It does not manufacture enemy
//! deaths, complete scripts, or change a session's player identities.

use crate::Error;
use ddpawn::cmc::Cmc;

fn resource(data: &impl crate::data::Data, path: &str, class: &str) -> Result<Vec<u8>, Error> {
    data.resource(path, class)?
        .ok_or_else(|| Error::Data(format!("missing {class} {path}")))
}

/// Read one of the original's 20 story-stock templates from the install.
/// This uses only the resources needed for that record, so scripted
/// recruitment does not require loading all 29,700 offline Rift pawns.
/// `language == 0` keeps the Japanese template name; otherwise the
/// alphabet name table is used before the story-specific name override.
// Translated from 0x00796420 (DDDA.exe 2017-12-13 Steam), same selection
// and overrides as ddpawn::builder::build_story_stock, for one record.
pub fn story_stock(data: &impl crate::data::Data, index: usize, language: u32) -> Result<Cmc, Error> {
    use ddpawn::builder::{equip_plain, STORY_EDIT_OVERRIDES, STORY_EDIT_TARGETS, STORY_SPECIAL};
    use ddpawn::cmc::Equip;
    if index >= 20 {
        return Err(Error::Data(format!("story stock index {index} outside 0..20")));
    }
    let info = ddpawn::RomPawnInfo::parse(&resource(data, r"etc\editPattern\romPawnInfo", "rRomPawnInfo")?)
        .map_err(Error::Data)?;
    let src = info
        .records
        .get(index)
        .ok_or_else(|| Error::Data(format!("missing story stock {index}")))?;
    let mut out = Cmc::initial();
    out.edit = src.edit.clone();
    out.param = src.param.clone();
    out.status = src.status;
    out.study_flag = src.study_flag.clone();
    out.info_value = src.info_value;
    out.info_status = src.info_status;
    out.info_ex_value = src.info_ex_value;
    out.use_scene_talk = src.use_scene_talk;
    out.pawn_type = src.pawn_type;
    out.employ_game_play_time = src.employ_game_play_time;
    out.rom_id = 0x2000_0000 + index as u32;
    if language != 0 {
        let names = ddpawn::PawnNames::parse(&resource(
            data,
            r"etc\editPattern\romPawnNameAlphabet",
            "rRomPawnName",
        )?)
        .map_err(Error::Data)?;
        let number = info.pawns[index].name_no as u8 as i8;
        if let Ok(number) = usize::try_from(number) {
            if let Some(name) = names.name(out.param.job, out.edit.get("mGender").unwrap_or(0), number) {
                out.edit.set_name(name.as_bytes());
            }
        }
    }
    if (10..17).contains(&index) {
        for (byte, fields) in STORY_EDIT_OVERRIDES[index - 10].iter().zip(STORY_EDIT_TARGETS) {
            for field in fields {
                out.edit.set(field, *byte);
            }
        }
        out.edit.set("mPersonality", 0);
        out.pawn_type = if index <= 14 { 2 } else { 0 };
    }
    if let Some((_, npc, items)) = STORY_SPECIAL.iter().find(|entry| entry.0 == index) {
        let ledger_bytes = resource(data, r"etc\item\npclist", "rNpcLedgerList")?;
        let ledger = ddnpc::Ledger::parse(&ledger_bytes).map_err(|e| Error::Data(e.to_string()))?;
        let npc = ledger
            .entries
            .iter()
            .find(|entry| entry.id as u32 == *npc)
            .ok_or_else(|| Error::Data(format!("missing story stock NPC name {npc}")))?;
        out.edit.set_name(npc.name_j);
        let slots = ddpawn::tables::ItemSlots::parse(&resource(data, r"etc\item\itemList", "rItemList")?)
            .map_err(Error::Data)?;
        out.param.equip = [Equip::EMPTY; 12];
        for item in *items {
            equip_plain(&slots, &mut out, *item).map_err(Error::Data)?;
        }
    }
    Ok(out)
}

/// Build the new-game party from runtime resources and a host RNG value.
/// Names come from the ledger, never copied into this repository.
pub fn load_party(data: &impl crate::data::Data, language: u32, random: u32) -> Result<PartySetup, Error> {
    let mut stock = vec![Cmc::initial(); 10];
    for index in 10..14 {
        stock.push(story_stock(data, index, language)?);
    }
    let ledger_bytes = resource(data, r"etc\item\npclist", "rNpcLedgerList")?;
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).map_err(|e| Error::Data(e.to_string()))?;
    party_from_stock(
        &stock,
        &|id| {
            ledger
                .entries
                .iter()
                .find(|n| n.id as u32 == id)
                .map(|n| n.name_j.to_vec())
        },
        random,
    )
}

/// The original's temporary hero and three shared companions. A co-op
/// host may instantiate the hero template for each Arisen seat; companion
/// records remain session-owned. That seat mapping is a co-op policy,
/// not a property of the original game's four-slot party.
#[derive(Debug, Clone, PartialEq)]
pub struct PartySetup {
    pub hero: Cmc,
    pub companions: [Cmc; 3],
    pub gold: u32,
    pub rift_crystals: u32,
    /// Spendable discipline (cPlayerInfo.mJobPoint +0x344), not level XP.
    pub discipline: u32,
}

/// Selects already-built stock entries and applies the new-game names.
/// `name` resolves NPC ledger `mNameJ` by id. `random` is the next value
/// supplied by the host-owned game RNG; this function never seeds an RNG.
///
/// This is a setup description, not the entire player-record conversion:
/// equipment-derived stats (0x00446860), active inventory (0x004460d0),
/// and the three pawn-presence flags still belong to the host adapters.
// Translated from 0x00445ab0 (DDDA.exe 2017-12-13 Steam): stock selection
// at 0x00445c58..0x00445d0c, names at 0x00445d96..0x0044601e, hero
// balances at 0x00445e16 and RNG bonus at 0x00446020..0x0044608c.
pub fn party_from_stock(
    stock: &[Cmc],
    name: &dyn Fn(u32) -> Option<Vec<u8>>,
    random: u32,
) -> Result<PartySetup, Error> {
    let mut members = Vec::with_capacity(4);
    for seat in 0..4 {
        let index = 10 + seat;
        let mut member = stock
            .get(index)
            .ok_or_else(|| Error::Data(format!("prologue: missing story stock {index}")))?
            .clone();
        let npc = 0x380 + seat as u32;
        let bytes = name(npc).ok_or_else(|| Error::Data(format!("prologue: missing NPC name {npc}")))?;
        member.edit.set_name(&bytes);
        members.push(member);
    }
    let [hero, a, b, c]: [Cmc; 4] = members.try_into().unwrap();
    Ok(PartySetup {
        hero,
        companions: [a, b, c],
        gold: 37_000 + random % 5_000 + 1_000,
        rift_crystals: 785,
        discipline: 1068,
    })
}
