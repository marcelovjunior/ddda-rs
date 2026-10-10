//! Character edits belong to persistent Arisen profiles, never temporary seats.
//! Native record: cSAVE_DATA_EDIT::vf03 0x00787010; confirmation copies it
//! at 0x006153C3..0x006153E6 (DDDA.exe 2017-12-13 Steam).
//! EDIT is a project save chunk, not the original game's save format.

use crate::ProfileId;
use crate::save::{Owner, SaveFile};
use ddpawn::cmc::Edit;
use std::collections::{BTreeMap, BTreeSet};

pub const TAG_EDIT: [u8; 4] = *b"EDIT";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Characters {
    records: BTreeMap<ProfileId, Edit>,
}

fn validate(edit: &Edit) -> Result<(), String> {
    if edit.get("mGender").is_none_or(|sex| sex > 1) {
        return Err("EDIT gender outside male/female".into());
    }
    let end = edit.name.iter().position(|&b| b == 0).unwrap_or(edit.name.len());
    std::str::from_utf8(&edit.name[..end]).map_err(|_| "EDIT name is not UTF-8".to_owned())?;
    Ok(())
}

impl Characters {
    pub fn get(&self, profile: ProfileId) -> Option<&Edit> {
        self.records.get(&profile)
    }

    pub fn name(&self, profile: ProfileId) -> String {
        self.get(profile).map(Edit::name_str).unwrap_or_default()
    }

    /// Caller presents an edit produced from the installed preset/name rules.
    pub fn confirm(&mut self, profile: ProfileId, edit: Edit) -> Result<(), String> {
        validate(&edit)?;
        self.records.insert(profile, edit);
        Ok(())
    }

    pub fn save(&self, file: &mut SaveFile) {
        for (&profile, edit) in &self.records {
            let mut bytes = 1u32.to_le_bytes().to_vec();
            bytes.extend_from_slice(&edit.nickname.to_le_bytes());
            bytes.extend_from_slice(&edit.name);
            bytes.extend_from_slice(&edit.fields);
            file.put(TAG_EDIT, Owner::Profile(profile), bytes);
        }
    }

    /// Construct a complete candidate before Host::load changes any state.
    /// Old/partial saves without EDIT retain the caller's existing records.
    pub fn loaded(&self, file: &SaveFile) -> Result<Self, String> {
        let mut candidate = self.clone();
        let mut seen = BTreeSet::new();
        for chunk in file.chunks.iter().filter(|chunk| chunk.tag == TAG_EDIT) {
            let Owner::Profile(profile) = chunk.owner else {
                return Err("EDIT chunk must belong to a profile".into());
            };
            if !seen.insert(profile) {
                return Err("duplicate EDIT profile".into());
            }
            let bytes = &chunk.bytes;
            if bytes.len() != 74 || bytes[..4] != 1u32.to_le_bytes() {
                return Err("unsupported or malformed EDIT chunk".into());
            }
            let edit = Edit {
                nickname: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                name: bytes[8..33].try_into().unwrap(),
                fields: bytes[33..74].try_into().unwrap(),
            };
            candidate.confirm(profile, edit)?;
        }
        Ok(candidate)
    }
}

impl<D: ddstory::data::Data, W: crate::StoryWorld> crate::Host<D, W> {
    /// Seat numbers are only a view/input address; records follow profiles.
    pub fn character_edit(&self, player: crate::PlayerId) -> Option<&Edit> {
        self.characters.get(self.roster.seat(player)?.profile)
    }

    pub fn character_name(&self, player: crate::PlayerId) -> String {
        self.character_edit(player)
            .map(Edit::name_str)
            .unwrap_or_default()
    }

    pub fn confirm_character(&mut self, player: crate::PlayerId, edit: Edit) -> Result<(), crate::HostError> {
        let profile = self
            .roster
            .seat(player)
            .ok_or(crate::HostError::UnknownPlayer(player))?
            .profile;
        self.characters
            .confirm(profile, edit)
            .map_err(crate::HostError::Save)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(name: &str, sex: u8) -> Edit {
        let mut edit = Edit {
            name: [0; 25],
            nickname: 71,
            fields: [3; 41],
        };
        edit.set("mGender", sex);
        edit.set_name(name.as_bytes());
        edit
    }

    #[test]
    fn edited_profiles_round_trip_offline_and_partial_saves_preserve_legacy_state() {
        let mut state = Characters::default();
        state.confirm(ProfileId(17), edit("Alba", 1)).unwrap();
        state.confirm(ProfileId(99), edit("Dorian", 0)).unwrap();
        let mut file = SaveFile::default();
        state.save(&mut file);
        let restored = Characters::default()
            .loaded(&SaveFile::from_bytes(&file.to_bytes()).unwrap())
            .unwrap();
        assert_eq!(restored, state);
        assert_eq!(restored.name(ProfileId(17)), "Alba");
        assert_eq!(restored.name(ProfileId(99)), "Dorian");
        assert_eq!(restored.name(ProfileId(123)), "");
        assert_eq!(state.loaded(&SaveFile::default()).unwrap(), state);
        let guest = file.profile_part(ProfileId(99));
        assert_eq!(guest.chunks.len(), 1);
        assert_eq!(
            Characters::default().loaded(&guest).unwrap().get(ProfileId(99)),
            state.get(ProfileId(99))
        );
    }

    #[test]
    fn invalid_offline_edit_chunks_do_not_mutate_existing_records() {
        let mut state = Characters::default();
        state.confirm(ProfileId(17), edit("Alba", 1)).unwrap();
        let before = state.clone();
        let mut file = SaveFile::default();
        state.save(&mut file);
        let original = file.chunks[0].clone();
        file.chunks[0].bytes[33] = 2;
        assert!(state.loaded(&file).is_err());
        assert_eq!(state, before);
        file.chunks[0] = original.clone();
        file.chunks.push(original.clone());
        assert!(state.loaded(&file).is_err());
        assert_eq!(state, before);
        file.chunks = vec![original];
        file.chunks[0].owner = Owner::Shared;
        assert!(state.loaded(&file).is_err());
        assert_eq!(state, before);
        assert!(state.confirm(ProfileId(17), edit("Nope", 2)).is_err());
        assert_eq!(state, before);
    }
}
