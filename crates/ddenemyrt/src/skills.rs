//! Equipped-skill data from the player's installation. A skill replaces
//! bank 7 or 9 for its owner (0x00AC65A0), never a shared global motion bank.
use crate::arisen::ArisenData;
use crate::body::{load, Body};
use ddsim::skills::{Definition, Kind};
use ddskill::catalogue::ExeTables;
use ddskill::magic::{Magic, MagicList};
use std::collections::HashMap;
pub mod healing;
pub mod spells;

#[derive(Debug, Clone)]
pub struct SkillData {
    pub id: u32,
    /// Native primary/secondary binding chosen by this skill's category.
    pub bank: u8,
    pub body: Body,
    pub definition: Definition,
    pub magic: Option<Magic>,
    pub spell: Option<spells::SpellData>,
}

impl SkillData {
    /// Pose at the release event. B47890 emits before changing to its end
    /// motion; the simulation's release queue is consumed after that change.
    pub fn release_pose(&self, feet: [f32; 3], yaw: f32) -> crate::body::Posed {
        let motion = match self.definition.kind {
            Kind::Spell { release, .. } => release,
            Kind::Sword { motion } => motion,
            Kind::Blink { end, .. } => end,
            Kind::ShieldProvoke { motion, .. } => motion,
            Kind::DaggerSeries { strikes, .. } => strikes[0],
        };
        let root = &self.definition.motions[&motion];
        let frame = (0..root.frame_count)
            .find(|f| {
                root.event_bits(0, *f as i32, *f as i32)
                    & (ddsim::attack::CHAIN_CLOSED_BIT | ddsim::attack::ACTION_END_BIT)
                    != 0
            })
            .unwrap_or(root.frame_count.saturating_sub(1));
        self.body.pose(
            crate::body::MotionId::new((motion >> 8) as u8, motion & 255),
            frame as f32,
            feet,
            yaw,
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillSet(pub HashMap<u32, SkillData>);

impl SkillSet {
    /// Supported base-level skills. Selection is a runtime support
    /// list, not an unlock grant: equipped slots are supplied separately.
    pub fn load(
        vfs: &ddvfs::Vfs,
        exe: &ddexe::Exe<'_>,
        sword: &ArisenData,
        mage: &ArisenData,
        strider: &ArisenData,
    ) -> Result<Self, String> {
        let tables = ExeTables::from_image(exe).map_err(|e| e.to_string())?;
        let magic = MagicList::parse(&load::read(
            vfs,
            MagicList::STAFF_PATH,
            MagicList::RESOURCE_CLASS,
        )?)
        .map_err(|e| e.to_string())?;
        let stamina = ddparam::Stamina::from_bytes(&load::read(
            vfs,
            r"param\pl\stamina\PlStamina_Sword",
            "rPlStamina",
        )?)
        .map_err(|e| e.to_string())?;
        let ability = ddparam::Ability::from_bytes(&load::read(
            vfs,
            r"param\pl\other\plabilityparam",
            "rPlAbilityParam",
        )?)
        .map_err(|e| e.to_string())?;
        let dagger_stamina = ddparam::Stamina::from_bytes(&load::read(
            vfs,
            r"param\pl\stamina\PlStamina_Dagger",
            "rPlStamina",
        )?)
        .map_err(|e| e.to_string())?;
        let dagger_adjust = ddcombat::damage::Adjust::parse(&load::read(
            vfs,
            r"param\pl\other\m_pl000_Dagger",
            "rAdjustParam",
        )?)
        .ok_or("bad dagger adjustment table")?;
        let shield_stamina = ddparam::Stamina::from_bytes(&load::read(
            vfs,
            r"param\pl\stamina\PlStamina_Shield",
            "rPlStamina",
        )?)
        .map_err(|e| e.to_string())?;
        let shield_adjust = ddcombat::damage::Adjust::parse(&load::read(
            vfs,
            r"param\pl\other\m_pl000_Shield",
            "rAdjustParam",
        )?)
        .ok_or("bad shield adjustment table")?;
        let sword_adjust = ddcombat::damage::Adjust::parse(&load::read(
            vfs,
            r"param\pl\other\m_pl000_Sword",
            "rAdjustParam",
        )?)
        .ok_or("bad sword adjustment table")?;
        let mut out = HashMap::new();
        for id in [40u32, 41, 43, 152, 210, 212, 272] {
            let (category, first, base) = if id < 100 {
                (1, 40, sword)
            } else if id < 210 {
                (4, 150, strider)
            } else if id == 272 {
                (8, 270, sword)
            } else {
                (5, 210, mage)
            };
            let entry = &tables.motion_lists[category][(id - first) as usize];
            let bank = load::bank(vfs, &entry.path)?;
            let mut body = base.body.clone();
            let bank_key = if id == 272 { 9 } else { 7 };
            body.banks.resize(body.banks.len().max(bank_key + 1), None);
            body.banks[bank_key] = Some(bank);
            let mut motions = HashMap::new();
            let bank = body.banks[bank_key].as_ref().unwrap();
            for (slot, motion) in bank.list.motions.iter().enumerate() {
                if let Some(m) = motion {
                    motions.insert(
                        (bank_key as u16) << 8 | slot as u16,
                        ddsim::RootMotion::from_motion(m)
                            .with_curves(bank.list.curves[slot].clone())
                            .with_events(bank.list.events[slot].clone()),
                    );
                }
            }
            let (kind, cost, spell) = if id < 100 {
                // cPlActWpnSwordCstmIchidoriKiri::vf06 B3E290 and
                // cPlActWpnSwordCstmKiriAge::vf06 B3E9A0, base level.
                let kind = if id == 40 {
                    // Translated from B3DBD0/B3DE40, Steam 2017-12-13.
                    // Sword adjust0 is angular rate (7975D0), not velocity.
                    let maximum_distance = *sword_adjust.0.get(1).ok_or("no Blink distance")?;
                    let turn_rate = *sword_adjust.0.get(0).ok_or("no Blink turn rate")?;
                    let motion = bank
                        .list
                        .motions
                        .get(0x29)
                        .and_then(Option::as_ref)
                        .ok_or("no Blink dash motion")?;
                    let loop_frame = motion.loop_frame as f32;
                    if !maximum_distance.is_finite()
                        || maximum_distance < 0.0
                        || !turn_rate.is_finite()
                        || turn_rate < 0.0
                        || loop_frame < 0.0
                        || loop_frame >= motion.frame_count.saturating_sub(1) as f32
                    {
                        return Err("unsupported Blink movement parameters".into());
                    }
                    Kind::Blink {
                        dash: 0x729,
                        end: 0x72a,
                        loop_frame,
                        maximum_distance,
                        turn_rate: turn_rate.to_radians(),
                    }
                } else {
                    Kind::Sword {
                        motion: if id == 41 { 0x73a } else { 0x740 },
                    }
                };
                let row = &stamina.entries[(id - first) as usize];
                if row.frame > 1.0 {
                    return Err("sustained sword cost not supported".into());
                }
                // B60F70/B79500, no Proficiency equipped; resolve the
                // *unequipped* value from the resource (not assumed zero).
                let proficiency = ability.entries[62].float_1;
                (kind, row.stamina * (100. - proficiency) * 0.01, None)
            } else if id == 152 {
                // Translated from 0x00B23F10 (DDDA.exe 2017-12-13 Steam).
                // Base level: adjust 0xC playback rate, 0xA repeat limit.
                let rate = *dagger_adjust.0.get(0xC).ok_or("no dagger motion rate")?;
                let maximum = *dagger_adjust.0.get(0xA).ok_or("no dagger cycle limit")?;
                if !rate.is_finite()
                    || rate <= 0.0
                    || !maximum.is_finite()
                    || maximum < 1.0
                    || maximum > u32::MAX as f32
                {
                    return Err("unsupported dagger motion rate/cycle limit".into());
                }
                let row = &dagger_stamina.entries[(id - first) as usize];
                if row.frame > 1.0 {
                    return Err("sustained dagger cost not supported".into());
                }
                let proficiency = ability.entries[62].float_1;
                (
                    Kind::DaggerSeries {
                        intro: 0x71e,
                        strikes: [0x71f, 0x726, 0x727],
                        end: 0x720,
                        rate,
                        maximum_cycles: maximum as u32,
                    },
                    row.stamina * (100. - proficiency) * 0.01,
                    None,
                )
            } else if id == 272 {
                // Translated from 0x00B38240/0x00B38490 and B81600
                // (DDDA.exe 2017-12-13 Steam), learned base level only.
                let radius = *shield_adjust.0.get(0xA).ok_or("no provoke radius")?;
                let amount = *shield_adjust.0.get(0xB).ok_or("no provoke amount")?;
                if !radius.is_finite()
                    || radius < 0.0
                    || !amount.is_finite()
                    || amount < 0.0
                    || amount >= u32::MAX as f32
                {
                    return Err("unsupported provoke radius/amount".into());
                }
                let row = &shield_stamina.entries[(id - first) as usize];
                if row.frame > 1.0 {
                    return Err("sustained shield cost not supported".into());
                }
                let proficiency = ability.entries[62].float_1;
                (
                    Kind::ShieldProvoke {
                        motion: 0x92f,
                        radius,
                        amount: amount as u32,
                    },
                    row.stamina * (100. - proficiency) * 0.01,
                    None,
                )
            } else {
                let action = &tables.actions[category][(tables.skills[id as usize].action & 0xffff) as usize];
                // cPlActWpnWandBase::vf18 B44AC0: params[1] selects
                // staff/shield table and params[2] the magic row.
                let m = magic
                    .entries
                    .get(action.params[2] as usize)
                    .ok_or("magic row out of range")?
                    .clone();
                let lv = &m.levels[0];
                let motion = |n: i32| (n >= 0).then_some(0x700 | n as u16);
                let kind = Kind::Spell {
                    start: motion(m.motions[0]).ok_or("no chant motion")?,
                    chant_loop: motion(m.motions[1]),
                    release: motion(m.motions[2]).ok_or("no release motion")?,
                    end: motion(m.motions[3]),
                    chant_frames: ddskill::magic::chant_time(
                        lv.chant_frames,
                        ability.entries[81].float_1,
                        1.,
                    ),
                };
                let cost = -ddskill::magic::cast_stamina(lv.stamina, ability.entries[82].float_1);
                (kind, cost, Some(m))
            };
            let spell_data = spell
                .as_ref()
                .map(|m| spells::SpellData::load(vfs, m))
                .transpose()?;
            out.insert(
                id,
                SkillData {
                    id,
                    bank: bank_key as u8,
                    body,
                    definition: Definition {
                        weapon: base.weapon(),
                        kind,
                        cost,
                        motions,
                    },
                    magic: spell,
                    spell: spell_data,
                },
            );
        }
        Ok(Self(out))
    }

    pub fn install(&self, sim: &mut ddsim::Sim) {
        for (&id, skill) in &self.0 {
            sim.skill_definitions
                .entry(id)
                .or_insert_with(|| skill.definition.clone());
        }
    }

    pub fn body<'a>(&'a self, p: &ddsim::Player, fallback: &'a Body) -> &'a Body {
        p.skills
            .active
            .and_then(|a| self.0.get(&a.id))
            .map_or(fallback, |s| &s.body)
    }
}
