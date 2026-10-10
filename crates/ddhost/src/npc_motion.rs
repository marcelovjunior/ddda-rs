//! Authored NPC motion action, independent of rendering and player seats.
//! Observations name both the materialized owner and the current command.

use crate::story_npcs::NpcLifetime;
use ddnpcrt::schedule::Motion;
use std::collections::HashMap;

/// NPC bank resources loaded from the player's install, never embedded.
pub struct Banks { lists: HashMap<u16, mtlmt::MotionList> }

impl Banks {
    /// Translated from 0x00B5B260 -> 0x008282E0 and 0x00bb6060.
    /// `npc_type` is cNpcInfo.mNpcType
    /// (body+0x6ed8); zero is its constructor value (0x00784880), not
    /// a ledger NPC id. The caller must supply actual specialized types.
    /// `common_variant` names the game's regular 00 or alternate 99 set.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, npc_type: u32,
        common_variant: u32) -> Result<Self, String> {
        if npc_type > 32 || ![0, 99].contains(&common_variant) {
            return Err("unsupported NPC motion bank family".into());
        }
        let common = format!(r"motion\npc\{common_variant:02}m000_c\{common_variant:02}m000_c");
        let read = |path: &str| -> Result<mtlmt::MotionList, String> {
            mtlmt::parse(&vfs.read(path, mtarc::type_hash("rMotionList"))?)
                .map_err(|e| format!("{path}: {e}"))
        };
        let mut lists = HashMap::new();
        // uPlayerBase ctor0x00B574D0 sets actor+0x4C24 to this table;
        // uNpc vf1920x00BAE100 calls inherited0x00B5B260 before NPC banks.
        // Native0x008282E0 reads bank/path pairs until packed==-1, loads
        // rMotionList and registers bank(packed>>8)&15 with0x01057670.
        // This is not the NPC collision survey table0x014EAC20.
        // Translated from 0x008282E0 (DDDA.exe 2017-12-13 Steam).
        let mut address = 0x014f_1d30;
        loop {
            let packed = exe.u32(address).map_err(|e| e.0)?;
            if packed == u32::MAX { break; }
            let bank = ((packed >> 8) & 15) as u16;
            let pointer = exe.u32(address + 4).map_err(|e| e.0)?;
            if pointer == 0 { lists.remove(&bank); }
            else { lists.insert(bank, read(&exe.cstr(pointer).map_err(|e| e.0)?)?); }
            address += 8;
        }
        lists.insert(11, read(&common)?);
        let table = exe.u32(0x0163_c980 + 4 * npc_type).map_err(|e| e.0)?;
        for (index, suffix) in ['c', 'j'].into_iter().enumerate() {
            let bank = ((exe.u32(table + index as u32 * 8).map_err(|e| e.0)? >> 8) & 15) as u16;
            let family = exe.u32(table + index as u32 * 8 + 4).map_err(|e| e.0)?;
            let stem = match family {
                9 => "09f000".into(), 25 => "25e008".into(),
                26 => "26e009".into(), 29 => "29e007".into(),
                _ => format!("{family:02}m000"),
            };
            let path = if index == 0 && family == 0 { common.clone() }
                else { format!(r"motion\npc\{stem}_{suffix}\{stem}_{suffix}") };
            lists.insert(bank, read(&path)?);
        }
        Ok(Self { lists })
    }

    /// cEventActMotion::vf05 0x0080d750 falls back from bank12 to11
    /// when the requested bank12 slot is absent. No missing motion is done.
    pub fn resolve(&self, authored: i32) -> Result<(u16, &mtlmt::Motion), String> {
        let requested = (authored as u32 & 0xfff) as u16; // 0x008500a0
        let find = |id: u16| self.lists.get(&(id >> 8))
            .and_then(|list| list.motions.get((id & 255) as usize)).and_then(Option::as_ref);
        if let Some(motion) = find(requested) { return Ok((requested, motion)); }
        if requested >> 8 == 12 {
            let fallback = 0xb00 | (requested & 255);
            if let Some(motion) = find(fallback) { return Ok((fallback, motion)); }
        }
        Err(format!("NPC motion {requested:#05x} missing from loaded banks"))
    }
}

/// Host protocol token, replacing original actor/interface pointer identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub owner: NpcLifetime,
    pub command: u64,
    /// Unique host launch serial also distinguishes replacement graphs whose
    /// local interface counters start over within the same intel lifetime.
    pub launch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub token: Token,
    pub motion: u16,
    pub event_action: bool,
    pub action_terminal: bool,
    pub target_angle_reached: bool,
    pub target_arrived: bool,
}

/// The supported positive-speed motion layer, followed by cEventActMotion.
/// Root displacement, target steering and ground integration are separate
/// actor systems; this state does not invent their observations.
#[derive(Debug, Clone)]
pub struct Actor {
    pub token: Token,
    pub authored: Motion,
    pub motion: u16,
    pub frame: f32,
    pub loop_count: u32,
    frame_max: f32,
    nonloop_layer: bool,
    warm: bool,
    action_terminal: bool,
    completed: bool,
    target_angle_reached: bool,
    target_arrived: bool,
}

impl Actor {
    pub fn new(token: Token, authored: Motion, motion: u16,
        resource: &mtlmt::Motion) -> Result<Self, String> {
        if authored.speed < 0.0 || !authored.speed.is_finite()
            || !authored.start_frame.is_finite() || resource.frame_count == 0 {
            return Err("NPC reverse/invalid motion playback is not implemented".into());
        }
        // Translated from uModel::vf30 0x0105b480. A negative resource
        // loop frame forces the layer's non-loop attribute, even when the
        // interface's Loop flag causes cEventActMotion to replay it.
        let frame_max = resource.frame_count.saturating_sub(1) as f32;
        Ok(Self { token, frame: authored.start_frame.min(frame_max), motion,
            nonloop_layer: !authored.looping || resource.loop_frame < 0,
            authored, frame_max, loop_count: 0, warm: false,
            action_terminal: false, completed: false,
            target_angle_reached: false, target_arrived: false })
    }

    /// Advance the actual loaded layer by original 1/30-second frame units.
    /// Translated scalar positive-speed branch of 0x01060730; terminal is
    /// the motion+6 bit4 read by 0x010570f0, not an estimated duration.
    pub fn advance(&mut self, delta: f32) -> Result<(), String> {
        if !delta.is_finite() || delta < 0.0 {
            return Err("invalid NPC motion frame delta".into());
        }
        if self.action_terminal || self.completed { return Ok(()); }
        if !self.warm {
            // cEventActMotion::vf06 0x0080db80 state0 initializes its
            // counters before state1 reads the layer terminal flag.
            self.warm = true;
            return Ok(());
        }
        let next = self.frame + self.authored.speed * delta;
        let snapped = (next + 0.0005).trunc(); // 0x01060730 positive branch
        self.frame = if next.trunc() != snapped { snapped } else { next };
        let terminal = snapped >= self.frame_max.trunc()
            && (self.nonloop_layer || self.frame_max > 0.0);
        if !terminal { return Ok(()); }
        if self.nonloop_layer { self.frame = self.frame_max; }
        else if self.frame_max > 0.0 { self.frame %= self.frame_max; }
        // Translated from 0x0080db80. Authored loops restart the layer
        // and increment +0x80; only the non-loop branch sets action+0x50.
        if self.authored.looping {
            self.loop_count = self.loop_count.saturating_add(1);
            self.frame = self.authored.start_frame.min(self.frame_max);
        } else {
            // The headless action owns body+0x2e90 = 0 (0x008500a0).
            self.action_terminal = true;
        }
        Ok(())
    }

    pub fn observation(&self) -> Observation {
        Observation { token: self.token, motion: self.motion, event_action: true,
            action_terminal: self.action_terminal, target_angle_reached: self.target_angle_reached,
            target_arrived: self.target_arrived }
    }

    /// Source target terminal flags in 0x0057e015..0x0057e0de. The
    /// caller resolves the authored target type from live actor identities.
    /// Missing targets do not satisfy either latch. This does not implement
    /// the distinct look-time steering/body orientation controller.
    pub fn target(&mut self, position: [f32; 3], yaw: f32, target: Option<[f32; 3]>) {
        if !self.authored.target { return; }
        let Some(target) = target.filter(|p| p.iter().all(|f| f.is_finite())) else { return };
        if !yaw.is_finite() || !position.iter().all(|f| f.is_finite()) { return; }
        let distance = std::array::from_fn::<_, 3, _>(|i| target[i] - position[i]);
        if self.authored.arrive_length <= 0.0 {
            // 0x007ad9d0 computes atan2(dx,dz); 0x007ad870 returns the
            // shortest signed difference, checked against RotSpeed radians.
            let angle = distance[0].atan2(distance[2]);
            let difference = (angle - yaw + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            if difference.abs() <= self.authored.rotation_speed * 0.017453292 {
                self.target_angle_reached = true;
            }
        } else if distance.iter().map(|d| d * d).sum::<f32>()
            < self.authored.arrive_length * self.authored.arrive_length {
            self.target_arrived = true;
        }
    }

    /// Translated terminal predicate 0x0057e0e4 in 0x0057db10. The source
    /// compares low motion bytes so the NPC bank12->11 fallback is valid.
    pub fn observe(&mut self, observation: Observation) -> bool {
        if self.completed || observation.token != self.token || !observation.event_action
            || observation.motion as u8 != self.authored.number as u8 {
            return false;
        }
        let terminal = (!self.authored.target || observation.target_angle_reached)
            && !self.authored.looping && observation.action_terminal;
        if !terminal && !observation.target_arrived { return false; }
        self.completed = true;
        true
    }
}
