//! Presents the host's authored NPC motion frame without reporting UI Done.

use crate::character::Playback;
use ddhost::npc_motion::{Banks, Token};
use ddhost::story_npcs::NpcLifetime;
use std::collections::HashMap;
use std::sync::Arc;

pub fn remote_transform(npc: &ddnet::npc::NpcBody, origin: bevy::prelude::Vec3) -> bevy::prelude::Transform {
    use bevy::prelude::*;
    let unit = crate::stage::UNIT;
    // Character skeleton vertices remain centimetres; UNIT lives on the
    // root (character::spawn_assembled_hands), separate from the body morph.
    Transform::from_translation((Vec3::from(npc.pos) - origin) * unit)
        .with_rotation(Quat::from_rotation_y(npc.yaw))
        .with_scale(Vec3::from(npc.scale) * unit)
}

pub struct View {
    banks: Arc<Banks>,
    bound: HashMap<u32, NpcLifetime>,
    shown: HashMap<u32, Token>,
    remote_shown: HashMap<u32, (u64, u64, u64, u8, u16)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_actor_root_keeps_centimetre_skeleton_conversion_and_origin() {
        use bevy::prelude::*;
        let mut npc = ddnet::npc::NpcBody::position(304, [1100.0, 200.0, 300.0]);
        npc.scale = [1.0, 1.2, 0.8];
        let transform = remote_transform(&npc, Vec3::new(1000.0, 0.0, 0.0));
        assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
        let top = transform.transform_point(Vec3::new(0.0, 100.0, 0.0));
        assert!((top.y - 3.2).abs() < 1e-6, "100cm local joint becomes 1.2m above root");
        assert_eq!(transform.scale, Vec3::from(npc.scale) * crate::stage::UNIT);
    }

    #[test]
    fn installed_remote_token_controls_blend_and_holds_exact_host_frame() {
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let exe = ddexe::locate(native).unwrap();
        let mut view = View::load(&vfs, &exe).unwrap();
        let (resolved, resource) = view.banks.resolve(3226).unwrap();
        let mut playback = Playback { motion: resource.clone(), joints: HashMap::new(), hold_frame: None,
            event_frame: None, blend_request: None, blend: None, morph: None, drawn: false, upper: None };
        let mut npc = ddnet::npc::NpcBody::position(304, [0.0; 3]);
        npc.generation = 17;
        npc.motion = Some(ddnet::npc::Motion { command: 1, launch: 2, bank: (resolved >> 8) as u8,
            slot: resolved & 255, frame: 8.25, blend_frames: 15.0 });
        assert!(view.show_remote(&npc, &mut playback).unwrap());
        assert_eq!(playback.hold_frame, Some(8.25));
        assert_eq!(playback.blend_request.take(), Some(15.0));
        npc.motion.as_mut().unwrap().frame = 9.5;
        view.show_remote(&npc, &mut playback).unwrap();
        assert_eq!(playback.hold_frame, Some(9.5));
        assert!(playback.blend_request.is_none(), "a new snapshot frame does not restart its motion");
        npc.motion.as_mut().unwrap().launch += 1;
        view.show_remote(&npc, &mut playback).unwrap();
        assert_eq!(playback.blend_request.take(), Some(15.0), "same-bank command replacement restarts blend");
        view.forget_remote(npc.id);
        view.show_remote(&npc, &mut playback).unwrap();
        assert_eq!(playback.blend_request.take(), Some(15.0), "a recreated body binds its resource again");
        npc.motion = None;
        assert!(!view.show_remote(&npc, &mut playback).unwrap());
        assert!(playback.hold_frame.is_none());
    }
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
