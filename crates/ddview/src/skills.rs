//! Offline viewer adapter for equipped skill slots. The simulation owns all
//! costs, action phases and effects; these keys are viewer bindings only.
use bevy::prelude::*;
use ddsim::{PlayerId, Sim, Weapon};

/// Each equipped action binds its own bank 7 (0x00AC65A0). The view must
/// select by skill ID as well as motion ID: several spells share 0x77f.
#[derive(Default)]
pub struct Motions(std::collections::HashMap<u32, (mtlmt::MotionList, Vec<f32>)>);

impl Motions {
    pub fn load(vfs: &ddvfs::Vfs, data: &ddhost::ddenemyrt::fight::FightData) -> Self {
        Self(
            data.skills
                .0
                .iter()
                .filter_map(|(&id, skill)| {
                    let bank = skill.body.banks.get(7)?.as_ref()?;
                    let blend = ddhost::install::adjust_table(vfs, &bank.path).unwrap_or_default();
                    Some((id, (bank.list.clone(), blend)))
                })
                .collect(),
        )
    }

    pub fn get(&self, skill: u32, motion: u16) -> Option<(&mtlmt::Motion, f32)> {
        let (bank, blend) = self.0.get(&skill)?;
        let animation = bank.motions.get(usize::from(motion & 255))?.as_ref()?;
        Some((
            animation,
            mtlmt::blend::default_frames(blend, motion).unwrap_or(0.),
        ))
    }
}

/// Read the same manual-slot character parameter as ddlook::save. Signed
/// empty sentinels stay empty; unsupported positive IDs stay equipped and
/// are rejected by the simulation rather than replaced by demo skills.
fn saved_slots(xml: &str, weapon: Weapon) -> Result<[Option<u32>; 6], String> {
    let doc = ddsave::xml::Document::parse(xml).map_err(|e| e.to_string())?;
    let param = doc
        .root()
        .path("mPlayerDataManual/mPlCmcEditAndParam/mPl/mParam")
        .ok_or("save has no Arisen skill parameters")?;
    let category = match weapon {
        Weapon::Sword => "SWORD",
        Weapon::Staff => "WAND",
        _ => return Ok([None; 6]),
    };
    let values = param
        .array::<i16>(&format!("mWeaponSkill[nWeapon::{category}]"))
        .ok_or("save has no equipped skills for this weapon")?;
    if values.len() != 6 {
        return Err("equipped skill array must contain six slots".into());
    }
    Ok(std::array::from_fn(|i| {
        u32::try_from(values[i]).ok().filter(|v| *v > 0)
    }))
}

/// Without a save, equip a documented preview pair for each local seat.
/// A save supplies only its Arisen's slots; extra local seats keep empty slots.
pub fn equip(sim: &mut Sim, locals: &[PlayerId], save: Option<&str>) {
    for (index, &seat) in locals.iter().enumerate() {
        let Some(p) = sim.player_mut(seat) else { continue };
        p.skills.equipped = if let Some(xml) = save {
            if index == 0 {
                saved_slots(xml, p.weapon).unwrap_or_else(|e| {
                    warn!("skills: {e}");
                    [None; 6]
                })
            } else {
                [None; 6]
            }
        } else {
            let pair = match p.weapon {
                Weapon::Sword => [Some(41), Some(43)],
                Weapon::Staff => [Some(210), Some(212)],
                _ => [None; 2],
            };
            [pair[0], pair[1], None, None, None, None]
        };
        info!(
            "seat {seat}: skill slots {:?} (base-level runtime)",
            p.skills.equipped
        );
    }
}

/// Called only in the unpaused offline play state. Requests remain queued
/// across render frames until the next fixed simulation tick consumes them.
pub fn keys(sim: &mut Sim, locals: &[PlayerId], keys: &ButtonInput<KeyCode>) -> bool {
    let bindings = [
        [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
        ],
        [
            KeyCode::Numpad4,
            KeyCode::Numpad5,
            KeyCode::Numpad6,
            KeyCode::Numpad7,
            KeyCode::Numpad8,
            KeyCode::Numpad9,
        ],
    ];
    let mut requested = false;
    for (&seat, row) in locals.iter().zip(bindings) {
        if let Some(slot) = row.iter().position(|key| keys.just_pressed(*key)) {
            sim.request_skill(seat, slot);
            requested = true;
        }
    }
    requested
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_four_skills_run_from_viewer_keys() {
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let data = crate::play::fight_data(&vfs, native).unwrap();
        let view = Motions::load(&vfs, &data);
        struct Flat;
        impl ddsim::Ground for Flat {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
                Some(0.)
            }
        }
        for (id, key) in [
            (41, KeyCode::Digit1),
            (43, KeyCode::Digit2),
            (210, KeyCode::Digit1),
            (212, KeyCode::Digit2),
        ] {
            let mut sim = Sim::new(Default::default());
            data.skills.install(&mut sim);
            for seat in [0, 1] {
                sim.add_player(seat, [seat as f32 * 1000., 0., 0.], 0.);
                sim.player_mut(seat).unwrap().weapon = data.skills.0[&id].definition.weapon;
            }
            equip(&mut sim, &[0, 1], None);
            let initial = sim.player(0).unwrap().stamina;
            let mut input = ButtonInput::default();
            input.press(key);
            assert!(keys(&mut sim, &[0, 1], &input));
            sim.step(1. / 120., &Default::default(), &Flat);
            assert_eq!(sim.player(0).unwrap().skills.active.unwrap().id, id);
            assert!(view.get(id, sim.player(0).unwrap().motion_id()).is_some());
            let mut paid = 0;
            let mut old = initial;
            for _ in 0..1000 {
                sim.step(1. / 120., &Default::default(), &Flat);
                let p = sim.player(0).unwrap();
                if old - p.stamina > 1. {
                    paid += 1;
                    assert!((old - p.stamina - data.skills.0[&id].definition.cost).abs() < 0.01);
                }
                old = p.stamina;
                if p.skills.active.is_none() {
                    break;
                }
            }
            assert_eq!(paid, 1, "skill {id}");
            assert!(sim.player(0).unwrap().skills.active.is_none());
            assert!(sim.player(1).unwrap().skills.active.is_none());
            assert_eq!(sim.player(1).unwrap().stamina, initial);
        }
    }
    #[test]
    fn separate_seats_and_press_edges() {
        let mut sim = Sim::new(Default::default());
        sim.add_player(3, [0.; 3], 0.);
        sim.add_player(7, [0.; 3], 0.);
        let mut input = ButtonInput::default();
        input.press(KeyCode::Digit2);
        input.press(KeyCode::Numpad9);
        assert!(keys(&mut sim, &[3, 7], &input));
        assert_eq!(sim.player(3).unwrap().skills.request, Some(1));
        assert_eq!(sim.player(7).unwrap().skills.request, Some(5));
        input.clear();
        sim.player_mut(3).unwrap().skills.request = None;
        sim.player_mut(7).unwrap().skills.request = None;
        assert!(!keys(&mut sim, &[3, 7], &input));
    }
    #[test]
    fn save_slots_preserve_order_and_unknown_ids() {
        let xml = r#"<root><class name="mPlayerDataManual"><class name="mPlCmcEditAndParam"><class name="mPl"><class name="mParam"><array name="mWeaponSkill[nWeapon::SWORD]"><s16 value="43"/><s16 value="-1"/><s16 value="41"/><s16 value="999"/><s16 value="0"/><s16 value="-1"/></array></class></class></class></class></root>"#;
        assert_eq!(
            saved_slots(xml, Weapon::Sword).unwrap(),
            [Some(43), None, Some(41), Some(999), None, None]
        );
        assert!(saved_slots(xml, Weapon::Staff).is_err());
    }
}
