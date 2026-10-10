//! uEm0100 equipment and body masks, read from the player's executable.
//! Supported family: ordinary goblins (kind 5). Other enemy kinds load
//! different tables and must not use this catalogue.

/// cSetInfoEnemy0100 +0x110..0x116, applied by 0x008AEBB0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placement {
    pub equip_type: u32,
    pub leader: bool,
    pub weapon_hand_start: bool,
    pub ballista_license: bool,
}

impl Placement {
    /// Translated from 0x008AEBB0 (DDDA.exe 2017-12-13 Steam), kind 5.
    pub fn effective_type(self) -> u32 {
        if self.leader && !matches!(self.equip_type, 0 | 1 | 4 | 5) { 1 }
        else { self.equip_type }
    }

    /// Translated from 0x008CDD80, kind 5 dispatch at 0x008CE2A5.
    pub fn unit_mask(self) -> u32 {
        match self.effective_type() {
            1 => 5, 2 => 8, 3 => 0x10, 4 => 0x81,
            5 => 0x85, 6 => 0x88, 7 => 0x90, _ => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appearance {
    pub placement: Placement,
    pub effective_type: u32,
    pub unit_mask: u32,
    /// Actor +0x110, 512 bits, selected by native vf124 (0x008CF4F0).
    pub groups: [u32; 16],
}

/// Native global 0x018D017C's generator. The host owns this state; clients
/// receive the chosen body instead of advancing their own generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRng(pub [u32; 4]);

impl Default for NativeRng {
    fn default() -> Self {
        // Native static initializer at 0x0137E5C0 supplies this seed.
        Self::seeded(0x1105)
    }
}

impl NativeRng {
    /// Translated from 0x00D006C0 (DDDA.exe 2017-12-13 Steam).
    pub fn seeded(seed: u32) -> Self {
        let mut state = [0x075b_cd15, 0x159a_55e5, 0x1f12_3bb5, 0x0549_1333];
        let mut mixed = seed ^ 0x00ac_9365;
        let mut last = state[3];
        for _ in 0..20 {
            let mut outputs = [0; 5];
            for output in &mut outputs {
                mixed ^= 0x65ac_9365 >> (mixed & 3);
                mixed ^= ((mixed >> 1) ^ mixed) >> 3
                    ^ (mixed.wrapping_mul(2) ^ mixed).wrapping_mul(8);
                let shifted = (mixed << 15) ^ mixed;
                last = ((last >> 17) ^ shifted) >> 4 ^ last ^ shifted;
                *output = last;
            }
            state.copy_from_slice(&outputs[1..]);
        }
        Self(state)
    }

    /// Translated from 0x00D00690, native vf124 calls it once for a
    /// non-leader body selection and does not consume it for a leader.
    pub fn next_word(&mut self) -> u32 {
        let shifted = (self.0[0] << 15) ^ self.0[0];
        let last = self.0[3];
        let word = ((last >> 17) ^ shifted) >> 4 ^ last ^ shifted;
        self.0 = [self.0[1], self.0[2], last, word];
        word
    }
}

impl Appearance {
    pub fn visible(&self, group: u16) -> bool {
        self.groups.get(usize::from(group / 32))
            .is_some_and(|word| word & (1 << (group % 32)) != 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub model: String,
    pub joint: u8,
    pub rotation_degrees: [f32; 3],
    pub position: [f32; 3],
}

/// Runtime data, never a copy of the executable's body lists in the repo.
#[derive(Debug, Clone)]
pub struct Catalog {
    ordinary: Vec<Vec<u16>>,
    helmet: Vec<Vec<u16>>,
    leader: Vec<u16>,
    /// Models named in vf33's resource descriptors; individual attached units.
    pub weapon: String,
    pub shield: String,
    pub extra: String,
    pub extra_material: String,
}

impl Catalog {
    /// Translated from unit creation 0x008CDD80 and the child units'
    /// vf96 callbacks (0x008EB550, 0x004C6D60, 0x008EBEC0).
    /// This is initial attachment placement; detached equipment and later
    /// actor +0x6020/+0x607C changes require their own runtime adapter.
    pub fn attachments(&self, appearance: &Appearance) -> Vec<Attachment> {
        let mut units = Vec::new();
        if appearance.unit_mask & 1 != 0 {
            let (joint, rotation_degrees, position) = if appearance.placement.weapon_hand_start {
                (8, [0.0, -20.0, 0.0], [-11.0, -2.8, 2.0])
            } else { (13, [15.0, 90.0, 0.0], [-23.0, 8.0, -15.0]) };
            units.push(Attachment { model: self.weapon.clone(), joint, rotation_degrees, position });
        }
        if appearance.unit_mask & 4 != 0 {
            units.push(Attachment { model: self.shield.clone(), joint: 11,
                rotation_degrees: [-35.0, 0.0, 0.0], position: [22.0, 0.0, 0.0] });
        }
        if appearance.unit_mask & 0x80 != 0 {
            units.push(Attachment { model: self.extra.clone(), joint: 8,
                rotation_degrees: [0.0, -20.0, 0.0], position: [-11.0, -2.8, 2.0] });
        }
        units
    }
    pub fn choose(&self, placement: Placement, rng: &mut NativeRng) -> Appearance {
        self.resolve(placement, if placement.leader { 0 } else { rng.next_word() })
    }
    /// Translated from 0x008CF4F0 (DDDA.exe 2017-12-13 Steam).
    /// The weighted selection maps and negative-terminated body lists stay
    /// in the player's executable. Bounds reject malformed/foreign tables.
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let list = |pointer: u32| -> Result<Vec<u16>, String> {
            let mut groups = Vec::new();
            for offset in 0..=512u32 {
                let group = exe.i32(pointer + offset * 4)?;
                if group < 0 { return Ok(groups); }
                if group >= 512 { return Err(format!("goblin body group {group} exceeds native mask")); }
                groups.push(group as u16);
            }
            Err("unterminated goblin body list".into())
        };
        let variants = |map: u32, count: usize| -> Result<Vec<Vec<u16>>, String> {
            exe.u32s(map, count)?.into_iter().map(|index| {
                if index > 14 { return Err(format!("goblin body variant {index} exceeds kind 5 table")); }
                list(exe.u32(0x017c_277c + index * 4)?)
            }).collect()
        };
        Ok(Self {
            ordinary: variants(0x0150_3208, 10)?,
            helmet: variants(0x0150_3230, 4)?,
            leader: list(exe.u32(0x017c_27b4)?)?,
            // vf33 descriptors used by 0x008EB530, 0x008EB7D0, 0x008EB920.
            weapon: exe.cstr(exe.u32(0x0183_8624)?)?,
            shield: exe.cstr(exe.u32(0x0183_8630)?)?,
            extra: exe.cstr(exe.u32(0x0183_8640)?)?,
            // Kind 5 material replacement in uEm0100_20::vf33 0x008EB920.
            extra_material: exe.cstr(exe.u32(0x0183_8664)?)?,
        })
    }

    /// Translated from 0x008CF4F0. `roll` is the host's supplied RNG word;
    /// the caller owns RNG state and must snapshot the resolved appearance.
    pub fn resolve(&self, placement: Placement, roll: u32) -> Appearance {
        let unit_mask = placement.unit_mask();
        let selected = if placement.leader { &self.leader }
            else if unit_mask & 8 != 0 { &self.helmet[(roll & 3) as usize] }
            else { &self.ordinary[(roll % 10) as usize] };
        let mut groups = [0; 16];
        groups[0] = 1;
        for &group in selected {
            groups[usize::from(group / 32)] |= 1 << (group % 32);
        }
        // vf33 bit 0x10 -> actor +0x60A0; leader vf124 also sets it.
        if placement.leader || unit_mask & 0x10 != 0 { groups[0] |= 0x20; }
        Appearance { placement, effective_type: placement.effective_type(), unit_mask, groups }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog() -> Catalog {
        Catalog { ordinary: (0..10).map(|i| vec![32 + i]).collect(),
            helmet: (0..4).map(|i| vec![64 + i]).collect(), leader: vec![96],
            weapon: String::new(), shield: String::new(), extra: String::new(), extra_material: String::new() }
    }
    #[test]
    fn leader_clamps_forbidden_equipment_before_dispatch() {
        for (kind, expected) in [(0,1),(1,5),(2,5),(3,5),(4,0x81),(5,0x85),(6,5),(7,5),(99,5)] {
            assert_eq!(Placement { equip_type: kind, leader: true, ..Default::default() }.unit_mask(), expected);
        }
        assert_eq!(Placement { equip_type: 2, ..Default::default() }.unit_mask(), 8);
    }
    #[test]
    fn native_weighted_selection_and_leader_precedence() {
        let data = catalog();
        let ordinary = data.resolve(Placement::default(), 21);
        assert!(ordinary.visible(0) && ordinary.visible(33));
        assert!(!ordinary.visible(32) && !ordinary.visible(64) && !ordinary.visible(512));
        let helmet = data.resolve(Placement { equip_type: 2, ..Default::default() }, 6);
        assert!(helmet.visible(66) && !helmet.visible(38));
        let leader = data.resolve(Placement { equip_type: 2, leader: true, ..Default::default() }, 6);
        assert!(leader.visible(96) && leader.visible(5) && !leader.visible(66));
        let type_three = data.resolve(Placement { equip_type: 3, ..Default::default() }, 0);
        assert!(type_three.visible(5));
    }
    #[test]
    fn native_rng_matches_executable_instruction_emulation() {
        // Private Unicorn x86 execution of D006C0 then D00690, three seeds.
        // These short regression vectors are algorithm outputs, not game data.
        for (seed, state, outputs) in [
            (0, [26541426,2045168005,148739841,1104137873],
                [957410874,635888367,3935184305,1180768413,210512503]),
            (0x1105, [3400880726,139467079,1548750404,3651730850],
                [3155119132,3098798383,3866542666,1266198273,1356463495]),
            (u32::MAX, [1196233184,2148283408,2847465889,3245563689],
                [37067228,2442260124,1164905899,761703771,4015033457]),
        ] {
            let mut rng = NativeRng::seeded(seed);
            assert_eq!(rng.0, state);
            assert_eq!(std::array::from_fn::<_,5,_>(|_| rng.next_word()), outputs);
        }
    }
    #[test]
    fn leader_selection_does_not_advance_global_random_state() {
        let mut rng = NativeRng::default();
        let before = rng.clone();
        catalog().choose(Placement { leader: true, ..Default::default() }, &mut rng);
        assert_eq!(rng, before);
        catalog().choose(Placement::default(), &mut rng);
        assert_ne!(rng, before);
    }
    #[test]
    fn equipment_units_preserve_native_attachment_states() {
        let data = catalog();
        let plain = data.resolve(Placement::default(), 0);
        assert_eq!(data.attachments(&plain).iter().map(|a| a.joint).collect::<Vec<_>>(), [13]);
        let in_hand = data.resolve(Placement { weapon_hand_start: true, ..Default::default() }, 0);
        assert_eq!(data.attachments(&in_hand)[0].joint, 8);
        let shield = data.resolve(Placement { equip_type: 1, ..Default::default() }, 0);
        assert_eq!(data.attachments(&shield).iter().map(|a| a.joint).collect::<Vec<_>>(), [13,11]);
        let extra = data.resolve(Placement { equip_type: 5, ..Default::default() }, 0);
        assert_eq!(data.attachments(&extra).iter().map(|a| a.joint).collect::<Vec<_>>(), [13,11,8]);
        let helmet = data.resolve(Placement { equip_type: 2, ..Default::default() }, 0);
        assert!(data.attachments(&helmet).is_empty());
    }
}
