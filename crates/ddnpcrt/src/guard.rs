//! Authored NPC guard dialogue resources. Actor eligibility, reservation,
//! activation and cleanup remain separate from message presentation.

use crate::{schedule::Guard, TalkSet};
use std::{collections::BTreeMap, sync::Arc};

/// Resource keys used by 0x00bb47d0 / 0x00baddf0. Quest identifiers are
/// runtime data; the two negative keys are embedded selector constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKey {
    Quest(u32),
    Base,
    Override,
}

/// Loaded resources for one NPC, preserving explicit absence. No fallback
/// is invented when a requested quest resource has not been loaded.
#[derive(Default)]
pub struct Resources {
    sets: BTreeMap<ResourceKey, Arc<TalkSet>>,
}

impl Resources {
    pub fn insert(&mut self, key: ResourceKey, set: Arc<TalkSet>) {
        self.sets.insert(key, set);
    }

    pub fn remove(&mut self, key: ResourceKey) {
        self.sets.remove(&key);
    }

    /// Translated from 0x00bb47d0 and 0x00baddf0
    /// (DDDA.exe 2017-12-13 Steam). Kind 4 checks key -1 for the
    /// authored conversation, then uses key -2; kind 5 uses key -2.
    pub fn select(&self, guard: &Guard) -> Option<Arc<TalkSet>> {
        let key = match guard.message_kind {
            2 => ResourceKey::Quest(guard.quest),
            4 if self.sets.get(&ResourceKey::Override).is_some_and(|set|
                guard.message >= 0 && set.set.by_number(guard.message).is_some()) => ResourceKey::Override,
            4 | 5 => ResourceKey::Base,
            _ => return None,
        };
        self.sets.get(&key).cloned()
    }
}

/// Quest text preload list for a resolved NPC id. Translated from
/// 0x00bace50 (DDDA.exe 2017-12-13 Steam): pointer table 0x0163cdf0,
/// u16 entries until 0xffff, only identifiers below 128 are loaded.
/// The actor adapter supplies a valid NPC id; this does not establish
/// placement existence or execute the special actor branches of vf32.
pub fn quest_preloads(exe: &ddexe::Exe<'_>, npc: u32) -> Result<Vec<u16>, ddexe::Error> {
    let entry = npc.checked_mul(4).and_then(|v| v.checked_add(0x0163_cdf0))
        .ok_or_else(|| ddexe::Error("NPC preload table address overflow".into()))?;
    let mut address = exe.u32(entry)?;
    if address == 0 { return Ok(vec![]); }
    let mut quests = Vec::new();
    loop {
        let quest = exe.u16(address)?;
        if quest == 0xffff { return Ok(quests); }
        if quest < 128 { quests.push(quest); }
        address = address.checked_add(2)
            .ok_or_else(|| ddexe::Error("NPC preload list address overflow".into()))?;
    }
}

impl crate::InstallData {
    /// Load exactly one authored message resource from the player's install.
    /// Preload membership and actor-specific vf32 branches are the caller's
    /// responsibility; loading a resource alone does not start a conversation.
    pub fn authored_talk_set(&self, path: &str) -> Result<Arc<TalkSet>, ddai::Error> {
        let bytes = self.vfs.read(path, mtarc::type_hash("rMsgSet")).map_err(ddai::Error::Data)?;
        let set = ddnpc::MessageSet::parse(&bytes).map_err(|e| ddai::Error::Data(e.0))?;
        let bytes = self.vfs.read(&format!("{path}_{}", self.language), mtarc::type_hash("rGUIMessage"))
            .map_err(ddai::Error::Data)?;
        let text = mtgmd::MessageFile::parse(&bytes).map_err(|e| ddai::Error::Data(e.0))?
            .messages.iter().map(ToString::to_string).collect();
        Ok(Arc::new(TalkSet { set, text }))
    }
}

/// Path authored by 0x00bace50 for a quest text resource.
pub fn quest_path(npc: u32, quest: u16) -> String {
    format!(r"id\npc_wind\npc_quest_talk\q{quest:04}_n{npc:03}")
}
