//! Original new-game records mapped onto this session's seats.
//!
//! Stock selection is the original's (`ddstory::prologue`). Giving every
//! Arisen a hero template and reserving the final pawn slot for the guest
//! is our co-op policy (COOP_DESIGN.md), not original-game behaviour.

use crate::{Host, HostError, PlayerId, Roster, StoryWorld};
use ddpawn::cmc::Cmc;
use ddstory::data::Data;
use ddstory::prologue::PartySetup;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct HeroSetup {
    pub record: Cmc,
    pub gold: u32,
    pub rift_crystals: u32,
    pub discipline: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrologueSessionSetup {
    /// Owned records for each seat; editing one cannot mutate another.
    pub heroes: BTreeMap<PlayerId, HeroSetup>,
    /// One shared pawn party. The final original companion is omitted
    /// when the guest occupies that slot under the existing co-op policy.
    pub pawns: ddrift::Party,
}

/// Map original data to the roster without changing a running host.
/// Empty sessions are rejected before any state is installed.
pub fn for_roster(roster: &Roster, original: &PartySetup) -> Result<PrologueSessionSetup, HostError> {
    if roster.is_empty() {
        return Err(HostError::NoData("prologue Arisen seats"));
    }
    let heroes = roster
        .ids()
        .map(|id| {
            (
                id,
                HeroSetup {
                    record: original.hero.clone(),
                    gold: original.gold,
                    rift_crystals: original.rift_crystals,
                    discipline: original.discipline,
                },
            )
        })
        .collect();
    let pawn = |record: &Cmc| ddrift::PartyPawn {
        online: ddrift::PawnRef::rom(record.rom_id),
        pawn: record.clone(),
        hire_time: 0,
        quests_cleared: 0,
    };
    let mut pawns = ddrift::Party::new(roster.len());
    pawns.main_pawn = Some(pawn(&original.companions[0]));
    for index in 0..2 {
        if !pawns.reserved[index] {
            pawns.hired[index] = Some(pawn(&original.companions[index + 1]));
        }
    }
    Ok(PrologueSessionSetup { heroes, pawns })
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// Prepare a fresh stage-380 session after seating its Arisen and
    /// before its first tick. Resource errors leave host state untouched.
    /// `random` must be drawn by the host; no local RNG is seeded here.
    ///
    /// Installs wallet and shared pawn records, returning the per-seat
    /// hero templates for actor/appearance construction. This is not
    /// active player conversion: inventory setup, equipment-derived
    /// stats, movement/combat bodies and progression remain caller work.
    /// It must not be called by the Cassardis fast-forward path.
    pub fn prepare_prologue(
        &mut self,
        language: u32,
        random: u32,
    ) -> Result<PrologueSessionSetup, HostError> {
        let story = self.story.as_ref().ok_or(HostError::NoData("story"))?;
        if self.frame != 0 || story.world.stage_no != 380 || story.world.sce_no != 100 {
            return Err(HostError::Rift(
                "prologue setup requires a fresh stage380/Sce100 session".into(),
            ));
        }
        if self.rift.party.main_pawn.is_some() || self.rift.party.hired.iter().any(Option::is_some) {
            return Err(HostError::Rift(
                "prologue setup refuses an occupied pawn party".into(),
            ));
        }
        if self.roster.is_empty() {
            return Err(HostError::NoData("prologue Arisen seats"));
        }
        let original = ddstory::prologue::load_party(&story.data, language, random)?;
        let setup = for_roster(&self.roster, &original)?;
        // The original script getters read cPlayerInfo's vocation/sex.
        // Every seat currently receives the same template; the shared
        // story fields mirror it without creating a global player actor.
        let world = &mut self.story.as_mut().unwrap().world;
        world.player_job = u32::from(original.hero.param.job);
        world.player_sex = original.hero.edit.get("mGender").unwrap_or(0);
        for (&id, hero) in &setup.heroes {
            let wallet = self.economy.wallet(id);
            wallet.gold = hero.gold as i32;
            wallet.rift_crystals = hero.rift_crystals as i32;
        }
        self.rift.party = setup.pawns.clone();
        let owner = self.roster.ids().next().unwrap();
        for index in 0..2 {
            self.hired_by[index] = self.rift.party.hired[index].as_ref()
                .and_then(|_| self.roster.seat(owner).map(|s| s.profile));
        }
        self.refresh_party();
        Ok(setup)
    }
}
