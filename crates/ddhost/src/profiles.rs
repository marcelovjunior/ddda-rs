//! Persistent profile state and temporary seat attachment. Co-op policy:
//! leaving parks the Arisen, cancels its in-flight attacks, and retains its
//! own bags/delivery ids/progression. Shared pawns stay in the session;
//! hired ownership follows the hirer's ProfileId, never a reused seat.
//! Rejoining uses the caller's current-stage spawn; a parked position is
//! retained in PLCE and restored when that profile is seated during load.
//! Runtime HP/status/endurance survive rejoin; existing saves do not persist
//! them, so save/load reconstructs combat from installed vocation/stats.

use crate::save::{self, Owner, SaveFile};
use crate::{Host, PlayerId, ProfileId, StoryWorld};
use ddstory::data::Data;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub(crate) struct Parked {
    pub economy: Option<ddeconomy::PlayerState>,
    pub character: Option<ddprogress::Character>,
    pub fighter: Option<ddenemyrt::fight::ArisenFighter>,
    pub place: Option<([f32; 3], f32)>,
    pub endurance: Option<(f32, f32, usize, usize, bool)>,
    pub hired: Vec<ParkedPawn>,
}

#[derive(Debug)]
pub(crate) struct ParkedPawn {
    index: usize,
    rom_id: u32,
    pawn: ddenemyrt::pawn::PawnSeat,
    fighter: ddenemyrt::fight::ArisenFighter,
    body: ddsim::Player,
}

fn cancel_attacks(fight: &mut ddenemyrt::fight::Fight, seat: PlayerId) {
    let owner = u32::from(seat);
    let mut released: BTreeSet<u32> = fight
        .shooters
        .iter()
        .filter_map(|(&id, &shooter)| (shooter == owner).then_some(id))
        .collect();
    released.extend(
        fight
            .shells
            .iter()
            .filter_map(|s| (s.shooter == owner).then_some(s.owner)),
    );
    released.extend(
        fight
            .arrows
            .iter()
            .filter_map(|s| (s.shooter == owner).then_some(s.owner)),
    );
    released.extend(
        fight
            .spells
            .shells
            .iter()
            .filter_map(|s| (s.shooter == owner).then_some(s.owner)),
    );
    released.insert(owner);
    for fighter in fight
        .arisen
        .values_mut()
        .map(|a| &mut a.fighter)
        .chain(fight.goblins.iter_mut().map(|g| &mut g.fighter))
        .chain(fight.npcs.values_mut().map(|n| &mut n.fighter))
        .chain(fight.dragon.iter_mut().map(|d| &mut d.actor.fighter))
    {
        // Unlike ordinary attack release, no relation-0 contact can survive
        // a profile's departure and refer to the seat's next occupant.
        fighter
            .history
            .entries
            .retain(|entry| !released.contains(&entry.attacker));
        fighter
            .history
            .frame
            .retain(|entry| !released.contains(&entry.attacker));
    }
    fight.shells.retain(|s| s.shooter != owner);
    fight.arrows.retain(|s| s.shooter != owner);
    fight.spells.shells.retain(|s| s.shooter != owner);
    fight.shooters.retain(|_, shooter| *shooter != owner);
    fight.positions.retain(|id, _| !released.contains(id));
    fight.bow_contexts.remove(&seat);
    fight.bow_errors.remove(&seat);
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }

    #[test]
    fn parked_health_and_status_change_runtime_hash_but_not_saved_hash() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            eprintln!("SKIP: install absent");
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let exe = ddexe::ExeFile::next_to(native).unwrap();
        let data =
            std::sync::Arc::new(ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap());
        let mut host = Host::new(
            ddsim::Sim::new(Default::default()),
            None::<ddstory::Story<crate::data::NoStoryData>>,
            ddstory::scripted::ScriptedHost::default(),
            Box::new(Flat),
        );
        host.fight = Some(ddenemyrt::fight::Fight::new(data));
        let seat = host
            .join(ProfileId(11), crate::Control::Local, [0.0; 3], 0.0)
            .unwrap();
        host.leave(seat);
        let before = host.state_hash();
        let saved = host.saved_hash();
        let fighter = &mut host
            .profiles
            .get_mut(&ProfileId(11))
            .unwrap()
            .fighter
            .as_mut()
            .unwrap()
            .fighter;
        if let ddenemyrt::combat::Victim::Player { hp, .. } = &mut fighter.victim {
            *hp = 37.0;
        }
        assert_ne!(host.state_hash(), before);
        assert_eq!(host.saved_hash(), saved);
        let health_changed = host.state_hash();
        let fighter = &mut host
            .profiles
            .get_mut(&ProfileId(11))
            .unwrap()
            .fighter
            .as_mut()
            .unwrap()
            .fighter;
        fighter.dots.poison_timer = 17.0;
        assert_ne!(host.state_hash(), health_changed);
        assert_eq!(host.saved_hash(), saved);
    }
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    pub(crate) fn align_arisen_records(&mut self) {
        let arisen: Vec<_> = self
            .progression
            .party
            .iter()
            .flatten()
            .copied()
            .filter(|member| matches!(member.role, ddprogress::Role::Arisen(_)))
            .collect();
        for member in &mut self.progression.party {
            if member.is_some_and(|member| matches!(member.role, ddprogress::Role::Arisen(_))) {
                *member = None;
            }
        }
        for member in arisen {
            let ddprogress::Role::Arisen(seat) = member.role else {
                unreachable!()
            };
            if let Some(slot) = self
                .party
                .slots
                .iter()
                .position(|m| *m == Some(crate::Member::Arisen(seat)))
            {
                self.progression.party[slot] = Some(member);
            }
        }
    }

    pub(crate) fn rebuild_profile_fighters(&mut self) {
        let ids: Vec<_> = self.roster.ids().collect();
        if let Some(fight) = &mut self.fight {
            for &seat in &ids {
                cancel_attacks(fight, seat);
                fight.arisen.remove(&seat);
            }
            for index in 0..2 {
                let seat = Self::hired_pawn_seat(index);
                cancel_attacks(fight, seat);
                fight.pawns.remove(&seat);
                fight.arisen.remove(&seat);
                self.sim.players.retain(|p| p.id != seat);
            }
        }
        // Existing save policy reconstructs combat from installed kit/stats;
        // HP/status and in-flight attacks are not durable save fields.
        for seat in ids {
            self.joined_fighter(seat);
        }
    }

    /// Co-op admission uses this profile's vocation/stats when present,
    /// otherwise the current opening vocation. Existing saves still rebuild
    /// combat from data; they do not persist HP/status or active attacks.
    pub(crate) fn joined_fighter(&mut self, seat: PlayerId) {
        use ddenemyrt::fight::Kit;
        use ddskill::vocation::Vocation;
        let mut character = self.character_of(ddprogress::Role::Arisen(seat)).cloned();
        let vocation = character.as_ref().map(|c| c.vocation).or_else(|| {
            self.story
                .as_ref()
                .and_then(|story| Vocation::from_id(story.world.player_job))
        });
        // A newly joining profile needs its own level-1 record even when
        // the shared vocation has not changed since apply_job_kit last ran.
        if character.is_none() && self.data.tables.is_some() {
            if let Some(vocation) = vocation {
                if self.create_arisen(seat, vocation).is_ok() {
                    character = self.character_of(ddprogress::Role::Arisen(seat)).cloned();
                }
            }
        }
        let Some(fight) = &mut self.fight else { return };
        if fight.arisen.contains_key(&seat) {
            return;
        }
        let kit = match vocation {
            Some(Vocation::Fighter) => Kit::Fighter,
            Some(Vocation::Strider) => Kit::Strider,
            Some(Vocation::Mage) => Kit::Mage,
            // Match the existing Fight::add_arisen admission convention;
            // unsupported vocation kits remain a separate labelled gap.
            _ => Kit::Fighter,
        };
        fight.add_arisen_as(seat, kit);
        if let Some(character) = character {
            let stats = character.progress.stats;
            // Force the existing sourced stat application even at level 1.
            fight.arisen.get_mut(&seat).unwrap().level = 0;
            fight.apply_level(
                seat,
                character.progress.level,
                stats.hp_max,
                stats.attack,
                stats.defend,
                stats.magic_attack,
                stats.magic_defend,
            );
        }
        fight.equip(&mut self.sim, seat);
    }

    pub(crate) fn hash_parked_fighters(&self, hash: &mut crate::hash::StateHasher) {
        for (profile, state) in &self.profiles {
            hash.debug(profile);
            // Fighter/PawnSeat contain runtime scalar state, not asset Arcs.
            hash.debug(&state.fighter);
            hash.debug(&state.endurance);
            hash.debug(&state.hired);
        }
    }

    pub(crate) fn detach_profile(&mut self, profile: ProfileId, seat: PlayerId) {
        let mut hired = Vec::new();
        for index in 0..2 {
            if self.hired_by[index] != Some(profile) {
                continue;
            }
            let pawn_seat = Self::hired_pawn_seat(index);
            if let (Some(fight), Some(record)) = (&mut self.fight, &self.rift.party.hired[index]) {
                cancel_attacks(fight, pawn_seat);
                if let (Some(pawn), Some(mut fighter), Some(body)) = (
                    fight.pawns.remove(&pawn_seat),
                    fight.arisen.remove(&pawn_seat),
                    self.sim.player(pawn_seat).cloned(),
                ) {
                    fighter.fighter.attacks.clear();
                    fighter.fighter.hurts.clear();
                    fighter.fighter.history = Default::default();
                    hired.push(ParkedPawn {
                        index,
                        rom_id: record.pawn.rom_id,
                        pawn,
                        fighter,
                        body,
                    });
                }
                fight.positions.remove(&u32::from(pawn_seat));
            }
            self.sim.players.retain(|p| p.id != pawn_seat);
        }
        // Older release adapters can leave a pawn body after its Rift slot
        // was dismissed. Such an actor is no longer an owned hire to park,
        // but its stale AI owner must never point at the next seat occupant.
        if let Some(fight) = &mut self.fight {
            let detached: Vec<_> = fight
                .pawns
                .iter()
                .filter_map(|(&id, pawn)| (id != Self::main_pawn_seat() && pawn.owner == seat).then_some(id))
                .collect();
            for id in detached {
                cancel_attacks(fight, id);
                fight.pawns.remove(&id);
                fight.arisen.remove(&id);
                self.sim.players.retain(|player| player.id != id);
            }
        }
        let fighter = self.fight.as_mut().and_then(|fight| {
            cancel_attacks(fight, seat);
            let mut fighter = fight.arisen.remove(&seat)?;
            fighter.fighter.attacks.clear();
            fighter.fighter.hurts.clear();
            fighter.fighter.history = Default::default();
            fighter.attacks_seen = 0;
            fighter.volleys_seen = 0;
            Some(fighter)
        });
        self.profiles.insert(
            profile,
            Parked {
                economy: Some(self.economy.detach_player(seat)),
                character: self.progression.detach_arisen(seat),
                fighter,
                place: self.sim.player(seat).map(|p| (p.position, p.yaw)),
                endurance: self
                    .sim
                    .player(seat)
                    .map(|p| (p.stamina, p.stamina_max, p.body_class, p.load_class, p.ran_out)),
                hired,
            },
        );
    }

    pub(crate) fn attach_profile(&mut self, profile: ProfileId, seat: PlayerId) {
        let Some(state) = self.profiles.remove(&profile) else {
            return;
        };
        if let Some(economy) = state.economy {
            self.economy.attach_player(seat, economy);
        }
        if let (Some(body), Some((stamina, maximum, body_class, load_class, ran_out))) =
            (self.sim.player_mut(seat), state.endurance)
        {
            body.stamina = stamina;
            body.stamina_max = maximum;
            body.body_class = body_class;
            body.load_class = load_class;
            body.ran_out = ran_out;
        }
        if let Some(character) = state.character {
            let character = self.progression.add(character);
            let slot = self
                .party
                .slots
                .iter()
                .position(|m| *m == Some(crate::Member::Arisen(seat)))
                .expect("new seat is in party");
            self.progression.party[slot] = Some(ddprogress::Member {
                character,
                role: ddprogress::Role::Arisen(seat),
            });
        }
        if let (Some(fight), Some(mut fighter)) = (&mut self.fight, state.fighter) {
            fighter.fighter.owner = u32::from(seat);
            fight.arisen.insert(seat, fighter);
            fight.equip(&mut self.sim, seat);
        }
        for parked in state.hired {
            let ParkedPawn {
                index,
                rom_id,
                mut pawn,
                mut fighter,
                body,
            } = parked;
            if self.hired_by[index] != Some(profile)
                || !self.rift.party.hired[index]
                    .as_ref()
                    .is_some_and(|record| record.pawn.rom_id == rom_id)
            {
                continue;
            }
            if let Some(fight) = &mut self.fight {
                pawn.owner = seat;
                // Re-enter beside the Arisen, without restoring an old attack.
                let (position, yaw) = self.sim.player(seat).map(|p| (p.position, p.yaw)).unwrap();
                self.sim.players.retain(|p| p.id != body.id);
                self.sim
                    .add_player(body.id, Self::hired_pawn_place(index, position, yaw), yaw);
                let restored = self.sim.player_mut(body.id).unwrap();
                restored.stamina = body.stamina;
                restored.stamina_max = body.stamina_max;
                restored.body_class = body.body_class;
                restored.load_class = body.load_class;
                restored.ran_out = body.ran_out;
                fighter.attacks_seen = 0;
                fighter.volleys_seen = 0;
                fight.arisen.insert(body.id, fighter);
                fight.pawns.insert(body.id, pawn);
                fight.equip(&mut self.sim, body.id);
            }
        }
    }

    pub(crate) fn save_parked_profiles(&self, file: &mut SaveFile) {
        for (&profile, state) in &self.profiles {
            let owner = Owner::Profile(profile);
            if let Some(economy) = &state.economy {
                file.put(save::TAG_ECON, owner, economy.save());
            }
            if let Some(character) = &state.character {
                file.put(save::TAG_PROG, owner, ddprogress::save::save_character(character));
            }
            if let Some((pos, yaw)) = state.place {
                file.put(save::TAG_PLACE, owner, save::place_bytes(pos, yaw));
            }
        }
    }

    pub(crate) fn read_parked_profiles(
        &self,
        file: &SaveFile,
    ) -> Result<BTreeMap<ProfileId, Parked>, crate::HostError> {
        let mut profiles = BTreeSet::new();
        for chunk in &file.chunks {
            if chunk.tag == save::TAG_PLACE && matches!(chunk.owner, Owner::Profile(_)) {
                if save::place_from(&chunk.bytes).is_none_or(|(position, yaw)| {
                    !yaw.is_finite() || position.iter().any(|value| !value.is_finite())
                }) {
                    return Err(crate::HostError::Save("invalid profile PLCE".into()));
                }
            }
            if ![save::TAG_ECON, save::TAG_PROG, save::TAG_PLACE].contains(&chunk.tag) {
                continue;
            }
            if let Owner::Profile(profile) = chunk.owner {
                if !self.roster.seats().iter().any(|seat| seat.profile == profile) {
                    profiles.insert(profile);
                }
            }
        }
        profiles
            .into_iter()
            .map(|profile| {
                let owner = Owner::Profile(profile);
                let economy = match (
                    file.get(save::TAG_ECON, Owner::Shared),
                    file.get(save::TAG_ECON, owner),
                ) {
                    (Some(shared), Some(bytes)) => {
                        Some(ddeconomy::PlayerState::load(shared, bytes).map_err(crate::HostError::Save)?)
                    }
                    (_, Some(_)) => {
                        return Err(crate::HostError::Save("profile ECON without shared ECON".into()))
                    }
                    _ => None,
                };
                if let Some(items) = &self.item_data {
                    if economy
                        .iter()
                        .flat_map(|state| state.bag.iter().chain(&state.storage))
                        .flat_map(|bag| &bag.slots)
                        .any(|slot| items.items.get(slot.item as u32).is_none())
                    {
                        return Err(crate::HostError::Save(
                            "parked ECON item outside installed catalogue".into(),
                        ));
                    }
                }
                let character = file
                    .get(save::TAG_PROG, owner)
                    .map(ddprogress::save::load_character)
                    .transpose()
                    .map_err(crate::HostError::Save)?;
                let place = file
                    .get(save::TAG_PLACE, owner)
                    .map(|bytes| {
                        save::place_from(bytes)
                            .ok_or_else(|| crate::HostError::Save("invalid parked PLCE".into()))
                    })
                    .transpose()?;
                Ok((
                    profile,
                    Parked {
                        economy,
                        character,
                        place,
                        ..Default::default()
                    },
                ))
            })
            .collect()
    }
}
