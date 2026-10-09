//! Equipped-skill data from the player's installation. A skill replaces
//! bank 7 for its owner (0x00AC65A0), never a shared global motion bank.
use crate::arisen::ArisenData;
use crate::body::{load, Body};
use ddsim::skills::{Definition, Kind};
use ddskill::catalogue::ExeTables;
use ddskill::magic::{Magic, MagicList};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SkillData {
    pub id: u32,
    pub body: Body,
    pub definition: Definition,
    pub magic: Option<Magic>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillSet(pub HashMap<u32, SkillData>);

impl SkillSet {
    /// The four supported base-level skills. Selection is a runtime support
    /// list, not an unlock grant: equipped slots are supplied separately.
    pub fn load(
        vfs: &ddvfs::Vfs,
        exe: &ddexe::Exe<'_>,
        sword: &ArisenData,
        mage: &ArisenData,
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
        let mut out = HashMap::new();
        for id in [41u32, 43, 210, 212] {
            let (category, first, base) = if id < 100 { (1, 40, sword) } else { (5, 210, mage) };
            let entry = &tables.motion_lists[category][(id - first) as usize];
            let bank = load::bank(vfs, &entry.path)?;
            let mut body = base.body.clone();
            body.banks[7] = Some(bank);
            let mut motions = HashMap::new();
            let bank = body.banks[7].as_ref().unwrap();
            for (slot, motion) in bank.list.motions.iter().enumerate() {
                if let Some(m) = motion {
                    motions.insert(
                        0x700 | slot as u16,
                        ddsim::RootMotion::from_motion(m)
                            .with_curves(bank.list.curves[slot].clone())
                            .with_events(bank.list.events[slot].clone()),
                    );
                }
            }
            let (kind, cost, spell) = if id < 100 {
                // cPlActWpnSwordCstmIchidoriKiri::vf06 B3E290 and
                // cPlActWpnSwordCstmKiriAge::vf06 B3E9A0, base level.
                let motion = if id == 41 { 0x73a } else { 0x740 };
                let row = &stamina.entries[(id - first) as usize];
                if row.frame > 1.0 {
                    return Err("sustained sword cost not supported".into());
                }
                // B60F70/B79500, no Proficiency equipped; resolve the
                // *unequipped* value from the resource (not assumed zero).
                let proficiency = ability.entries[62].float_1;
                (
                    Kind::Sword { motion },
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
            out.insert(
                id,
                SkillData {
                    id,
                    body,
                    definition: Definition {
                        weapon: base.weapon(),
                        kind,
                        cost,
                        motions,
                    },
                    magic: spell,
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
