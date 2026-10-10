//! Presents the host's authored NPC motion frame without reporting UI Done.

use crate::character::Playback;
use ddhost::npc_motion::{Banks, Token};
use ddhost::story_npcs::NpcLifetime;
use std::collections::HashMap;
use std::sync::Arc;

pub struct View {
    banks: Arc<Banks>,
    bound: HashMap<u32, NpcLifetime>,
    shown: HashMap<u32, Token>,
    remote_shown: HashMap<u32, (u64, u64, u64, u8, u16)>,
}

impl View {
    pub fn load(vfs: &ddvfs::Vfs, exe: &std::path::Path) -> Result<Self, String> {
        let file = ddexe::ExeFile::open(exe).map_err(|e| e.0)?;
        // The opening's generic assembled NPC bodies use cNpcInfo's
        // constructor type0 (0x00784880). Specialized body type overrides
        // and the zone-dependent 99 common bank remain unimplemented here.
        let banks = Banks::load(vfs, &file.image().map_err(|e| e.0)?, 0, 0)?;
        Ok(Self { banks: Arc::new(banks), bound: HashMap::new(), shown: HashMap::new(), remote_shown: HashMap::new() })
    }

    /// Bind only actual spawned bodies, not admitted intels with missing models.
    pub fn bind<D: ddstory::data::Data, W: ddhost::StoryWorld>(
        &mut self, host: &mut ddhost::Host<D, W>, bodies: &[u32],
    ) -> Result<(), String> {
        let retired: Vec<_> = self.bound.iter().filter(|(npc, owner)|
            !bodies.contains(npc) || host.story_npc_lifetime(**npc) != Some(**owner))
            .map(|(&npc, &owner)| (npc, owner)).collect();
        for (npc, owner) in retired {
            host.unbind_story_npc_motion_body(owner);
            self.bound.remove(&npc); self.shown.remove(&npc);
        }
        for &npc in bodies {
            let Some(owner) = host.story_npc_lifetime(npc) else { continue };
            if self.bound.get(&npc) == Some(&owner) { continue; }
            if host.bind_story_npc_motion_body(owner, self.banks.clone()).map_err(|e| e.to_string())? {
                self.bound.insert(npc, owner);
            }
        }
        Ok(())
    }

    /// An installed skeleton samples the canonical headless frame; renderer
    /// timing, visibility and a different co-op viewing seat cannot complete it.
    pub fn show<D: ddstory::data::Data, W: ddhost::StoryWorld>(
        &mut self, host: &ddhost::Host<D, W>, npc: u32, playback: &mut Playback,
    ) -> bool {
        let Some(actor) = host.story_npc_motion(npc) else {
            self.shown.remove(&npc); return false;
        };
        if self.bound.get(&npc) != Some(&actor.token.owner) { return false; }
        if self.shown.get(&npc) != Some(&actor.token) {
            let Some(resource) = host.story_npc_motion_resource(npc) else { return false };
            playback.change_motion(resource.clone(), actor.authored.blend_frames);
            self.shown.insert(npc, actor.token);
            bevy::log::info!("opening: NPC {npc} authored motion {} bank {:#x} loop {} target {} fall {} command {} launch {}",
                actor.authored.number, actor.motion >> 8, actor.authored.looping,
                actor.authored.target, actor.authored.fall, actor.token.command, actor.token.launch);
        }
        playback.hold_frame = Some(actor.frame);
        true
    }

    /// Sampling the received token never binds to or advances a guest host.
    pub fn show_remote(&mut self, npc: &ddnet::npc::NpcBody, playback: &mut Playback) -> Result<bool, String> {
        let Some(motion) = npc.motion else {
            self.remote_shown.remove(&npc.id); playback.hold_frame = None; return Ok(false);
        };
        let key = (npc.generation, motion.command, motion.launch, motion.bank, motion.slot);
        if self.remote_shown.get(&npc.id) != Some(&key) {
            let requested = (u16::from(motion.bank) << 8) | motion.slot;
            let (resolved, resource) = self.banks.resolve(i32::from(requested))?;
            if resolved != requested { return Err("host supplied an unresolved NPC motion".into()); }
            playback.change_motion(resource.clone(), motion.blend_frames);
            self.remote_shown.insert(npc.id, key);
        }
        playback.hold_frame = Some(motion.frame);
        Ok(true)
    }

    pub fn forget_remote(&mut self, npc: u32) { self.remote_shown.remove(&npc); }
}
