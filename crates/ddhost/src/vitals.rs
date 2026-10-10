//! Native save field retention, with per-profile ownership for co-op.
//! cSAVE_DATA_PARAM reflection at 0x00785790 (2017-12-13 Steam) and the
//! player's save XML define these six fields. No private record is embedded.
//! Damage wound-pool reduction and pawn save vitals remain open.

use crate::save::{Owner, SaveFile, TAG_VITALS};
use crate::{Host, HostError, PlayerId, ProfileId, StoryWorld};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vitals {
    pub hp: f32,
    pub hp_max: f32,
    pub hp_max_white: f32,
    pub stamina: f32,
    pub stamina_base: f32,
    pub stamina_lv: f32,
}

impl Vitals {
    pub fn validate(self) -> Result<Self, HostError> {
        let values = [
            self.hp,
            self.hp_max,
            self.hp_max_white,
            self.stamina,
            self.stamina_base,
            self.stamina_lv,
        ];
        if values.iter().any(|v| !v.is_finite() || *v < 0.0)
            || self.hp > self.hp_max
            || self.hp_max_white > self.hp_max
            || !(self.stamina_base + self.stamina_lv).is_finite()
            || self.stamina > self.stamina_base + self.stamina_lv
        {
            return Err(HostError::Save("invalid VITL values".into()));
        }
        Ok(self)
    }
    pub fn to_bytes(self) -> Vec<u8> {
        let mut w = ddid::Writer::new();
        w.u8(1);
        for value in [
            self.hp,
            self.hp_max,
            self.hp_max_white,
            self.stamina,
            self.stamina_base,
            self.stamina_lv,
        ] {
            w.f32(value);
        }
        w.finish()
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HostError> {
        let mut r = ddid::Reader::new(bytes);
        let err = HostError::Save;
        if r.u8().map_err(err)? != 1 {
            return Err(err("VITL version".into()));
        }
        let value = Self {
            hp: r.f32().map_err(err)?,
            hp_max: r.f32().map_err(err)?,
            hp_max_white: r.f32().map_err(err)?,
            stamina: r.f32().map_err(err)?,
            stamina_base: r.f32().map_err(err)?,
            stamina_lv: r.f32().map_err(err)?,
        };
        r.end().map_err(err)?;
        value.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseMismatch {
    pub profile: ProfileId,
    pub stored: f32,
    pub installed: f32,
    pub body_class: usize,
}

pub(crate) fn read(file: &SaveFile) -> Result<BTreeMap<ProfileId, Vitals>, HostError> {
    let mut loaded = BTreeMap::new();
    for chunk in file.chunks.iter().filter(|c| c.tag == TAG_VITALS) {
        let Owner::Profile(profile) = chunk.owner else {
            return Err(HostError::Save("VITL must belong to a profile".into()));
        };
        if loaded
            .insert(profile, Vitals::from_bytes(&chunk.bytes)?)
            .is_some()
        {
            return Err(HostError::Save("duplicate profile VITL".into()));
        }
    }
    Ok(loaded)
}

impl<D: ddstory::data::Data, W: StoryWorld> Host<D, W> {
    pub fn seat_vitals(&self, seat: PlayerId) -> Option<Vitals> {
        let body = self.sim.player(seat)?;
        let actor = self.fight.as_ref()?.arisen.get(&seat)?;
        let ddenemyrt::combat::Victim::Player { hp, hp_max, .. } = actor.fighter.victim else {
            return None;
        };
        Some(Vitals {
            hp,
            hp_max,
            hp_max_white: actor.hp_max_white,
            stamina: body.stamina,
            stamina_base: body.stamina_base,
            stamina_lv: body.stamina_lv,
        })
    }
    pub(crate) fn parked_vitals(state: &crate::profiles::Parked) -> Option<Vitals> {
        let mut value = state.vitals?;
        if let Some(actor) = &state.fighter {
            if let ddenemyrt::combat::Victim::Player { hp, hp_max, .. } = actor.fighter.victim {
                value.hp = hp;
                value.hp_max = hp_max;
                value.hp_max_white = actor.hp_max_white;
            }
        }
        if let Some((stamina, ..)) = state.endurance {
            value.stamina = stamina;
        }
        Some(value)
    }
    pub(crate) fn save_vitals(&self, file: &mut SaveFile) -> Result<(), HostError> {
        for seat in self.roster.seats() {
            if let Some(value) = self.seat_vitals(seat.id) {
                file.put(
                    TAG_VITALS,
                    Owner::Profile(seat.profile),
                    value.validate()?.to_bytes(),
                );
            }
        }
        for (profile, state) in &self.profiles {
            if let Some(value) = Self::parked_vitals(state) {
                file.put(TAG_VITALS, Owner::Profile(*profile), value.validate()?.to_bytes());
            }
        }
        Ok(())
    }
    pub(crate) fn apply_profile_vitals(&mut self, profile: ProfileId, seat: PlayerId, value: Vitals) {
        if let Some(body) = self.sim.player_mut(seat) {
            if let Some(installed) = self
                .data
                .rift
                .as_ref()
                .and_then(|data| data.stamina.entries.get(body.body_class))
                .map(|row| row.stamina)
            {
                if value.stamina_base != installed {
                    let mismatch = BaseMismatch {
                        profile,
                        stored: value.stamina_base,
                        installed,
                        body_class: body.body_class,
                    };
                    eprintln!("VITL stored base differs from installed weight-class base: {mismatch:?}");
                    self.vitals_diagnostics.push(mismatch);
                }
            }
            body.stamina = value.stamina;
            body.stamina_base = value.stamina_base;
            body.stamina_lv = value.stamina_lv;
            body.stamina_max = value.stamina_base + value.stamina_lv;
            body.ran_out = value.stamina == 0.0;
        }
        if let Some(actor) = self.fight.as_mut().and_then(|fight| fight.arisen.get_mut(&seat)) {
            if let ddenemyrt::combat::Victim::Player { hp, hp_max, .. } = &mut actor.fighter.victim {
                *hp = value.hp;
                *hp_max = value.hp_max;
            }
            actor.hp_max_white = value.hp_max_white;
        }
    }
    pub(crate) fn apply_loaded_vitals(&mut self, loaded: &BTreeMap<ProfileId, Vitals>) {
        for seat in self.roster.seats().to_vec() {
            if let Some(value) = loaded.get(&seat.profile) {
                self.apply_profile_vitals(seat.profile, seat.id, *value);
            }
        }
    }
}
