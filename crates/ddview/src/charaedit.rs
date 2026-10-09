//! Character creation, first version (`--charaedit`, `--charaedit-female`): the Arisen's
//! 41 edit fields (`cSAVE_DATA_EDIT`, `ddpawn::cmc::EDIT_U8_FIELDS`) with
//! the body rebuilt by the game's own edit-to-look path
//! (`ddlook::save::edit_look`, 0x00780fa0) after every change.
//!
//! Keys: Up/Down picks a field, Left/Right changes it, Enter confirms:
//! the record (the values a save would hold) is printed and stored in
//! [`EditResult`] for the caller (the opening's stage 801).
//!
//! *Not yet the original's screen.* The `uGUIEdit` layout, its pages and
//! each field's allowed values are not read: the ranges below come from
//! the data the fields index (preset files, pattern tables, pallets) where
//! that is known and are otherwise 0..=255, labelled per field. The body
//! wears no equipment (*viewer choice*). Stage 801's set is not loaded.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use ddlook::Source;
use ddpawn::cmc::EDIT_U8_FIELDS;

use crate::stage::Loader;

/// The idle motion the edited body holds (bank 0 slot 0, `m0000_co`).
const MOTION: &str = r"motion\pl\m\m00\m0000_co\m0000_co";

/// The confirmed edit record, `(field, value)` in `EDIT_U8_FIELDS` order.
#[derive(Resource, Default)]
pub struct EditResult(pub Option<Vec<(String, u32)>>);

#[derive(Resource)]
pub struct CharaEdit {
    loader: Loader<'static>,
    tables: ddlook::Tables,
    pattern: ddlook::pattern::EditPattern,
    /// Field values, in `EDIT_U8_FIELDS` order.
    values: [u32; 41],
    /// The values each field steps through, and where they come from.
    allowed: Vec<(Vec<u32>, &'static str)>,
    cursor: usize,
    body: Option<Entity>,
    text: Entity,
    dirty: bool,
}

/// The first built-in pawn of `sex` (`etc\editPattern\romPawnInfo`): the
/// editor's starting values (*viewer choice*: the original's starting
/// character is not traced).
fn start_values(vfs: &ddvfs::Vfs, sex: u8) -> Result<[u32; 41], String> {
    let b = vfs
        .resource(r"etc\editPattern\romPawnInfo", "rRomPawnInfo")
        .map_err(|e| format!("{e:?}"))?
        .ok_or("romPawnInfo missing")?;
    let info = ddpawn::RomPawnInfo::parse(&b).map_err(|e| format!("romPawnInfo: {e}"))?;
    let gender = EDIT_U8_FIELDS.iter().position(|f| *f == "mGender").unwrap();
    let c = info
        .records
        .iter()
        .find(|c| c.edit.fields[gender] == sex)
        .ok_or_else(|| format!("no built-in pawn of sex {sex}"))?;
    Ok(c.edit.fields.map(u32::from))
}

/// The values each field steps through, from the data it indexes.
fn bounds(vfs: &ddvfs::Vfs, pattern: &ddlook::pattern::EditPattern) -> Vec<(Vec<u32>, &'static str)> {
    let upto = |n: usize| (0..n as u32).collect::<Vec<u32>>();
    // The preset numbers NNN under 100 with an edit\parts\<part>\<part>_NNN
    // (0x00780fa0 adds 100 for a female; the sets are not contiguous).
    let files = |part: &str, class: &str| {
        (0..100u32)
            .filter(|n| vfs.locate(&format!("edit\\parts\\{part}\\{part}_{n:03}"), mtarc::type_hash(class)).is_some())
            .collect::<Vec<u32>>()
    };
    let presets = |part: &str| files(part, "rHumanPartsEdit");
    // the face fields index edit\parts\<base|eyeblow|eye|nose|mouth|ear>
    // (rFacePartsEdit, 0x00780fa0; CHARACTERS.md)
    let face = |part: &str| files(part, "rFacePartsEdit");
    EDIT_U8_FIELDS
        .iter()
        .map(|f| match *f {
            "mGender" => (vec![0, 1], "male / female"),
            "mHumanBodyAll" => (presets("bodyall"), "bodyall presets"),
            "mHumanBody" => (presets("body"), "body presets"),
            "mHumanHead" => (presets("head"), "head presets"),
            "mHumanArm" => (presets("arm"), "arm presets"),
            "mHumanLeg" => (presets("leg"), "leg presets"),
            "mFaceBase" => (face("base"), "face base presets"),
            "mFaceEyebrow" => (face("eyeblow"), "eyebrow presets"),
            "mFaceEye" => (face("eye"), "eye presets"),
            "mFaceNose" => (face("nose"), "nose presets"),
            "mFaceMouth" => (face("mouth"), "mouth presets"),
            "mFaceEar" => (face("ear"), "ear presets"),
            "mHairNo" => (upto(pattern.hair_id.len()), "editPattern mHairId"),
            "mBeardNo" => (upto(pattern.beard_id.len()), "editPattern mBeardId"),
            "mColorHair" | "mColorBeard" | "mColorEyebrow" => (upto(pattern.pallets[0].len()), "hair pallet"),
            "mColorBody" => (upto(pattern.pallets[1].len()), "skin pallet"),
            "mColorEyeR" | "mColorEyeL" | "mColorMakeup" => (upto(pattern.pallets[2].len()), "eye pallet"),
            // 0x00780fa0's five-step slider tables
            "mBodySlender" | "mBodyMuscle" | "mBustScale" | "mOfsXFaceEyebrow" | "mOfsYFaceEyebrow" => {
                (upto(5), "five-step slider (0x00780fa0)")
            }
            _ => (upto(256), "range not traced"),
        })
        .collect()
}

/// The next allowed value after (or before) `v`, wrapping.
fn step(allowed: &[u32], v: u32, up: bool) -> u32 {
    let (Some(&first), Some(&last)) = (allowed.first(), allowed.last()) else { return v };
    if up {
        allowed.iter().copied().find(|&a| a > v).unwrap_or(first)
    } else {
        allowed.iter().rev().copied().find(|&a| a < v).unwrap_or(last)
    }
}

/// Sets up the editor (from main's setup for `--charaedit`).
pub fn setup(commands: &mut Commands, loader: Loader<'static>, sex: u8) -> Result<(), String> {
    let vfs = loader.vfs;
    let tables = ddlook::Tables::load(vfs).map_err(|e| format!("{e:?}"))?;
    let pattern = ddlook::pattern::EditPattern::load(vfs).map_err(|e| format!("{e:?}"))?;
    let values = start_values(vfs, sex)?;
    let allowed = bounds(vfs, &pattern);
    let text = commands
        .spawn((
            Text::new(""),
            TextFont { font_size: bevy::text::FontSize::Px(15.0), ..default() },
            Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(12.0), ..default() },
        ))
        .id();
    commands.insert_resource(EditResult(None));
    commands.insert_resource(CharaEdit { loader, tables, pattern, values, allowed, cursor: 0, body: None, text, dirty: true });
    Ok(())
}

/// Keys, the field list, and the rebuilt body.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    edit: Option<ResMut<CharaEdit>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut result: Option<ResMut<EditResult>>,
    mut texts: Query<&mut Text>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(mut e) = edit else { return };
    let e = &mut *e;
    let n = EDIT_U8_FIELDS.len();
    if keys.just_pressed(KeyCode::ArrowDown) {
        e.cursor = (e.cursor + 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        e.cursor = (e.cursor + n - 1) % n;
    }
    for (key, up) in [(KeyCode::ArrowRight, true), (KeyCode::ArrowLeft, false)] {
        if keys.just_pressed(key) {
            let c = e.cursor;
            e.values[c] = step(&e.allowed[c].0, e.values[c], up);
            e.dirty = true;
        }
    }
    if keys.just_pressed(KeyCode::Enter) {
        let record: Vec<String> = EDIT_U8_FIELDS.iter().zip(e.values).map(|(f, v)| format!("{f}={v}")).collect();
        info!("charaedit: {}", record.join(" "));
        if let Some(r) = result.as_mut() {
            r.0 = Some(EDIT_U8_FIELDS.iter().map(|f| f.to_string()).zip(e.values).collect());
        }
    }
    if let Ok(mut t) = texts.get_mut(e.text) {
        let mut s = String::from("Character edit (Up/Down field, Left/Right value, Enter confirms)\n\n");
        let first = e.cursor.saturating_sub(12).min(n.saturating_sub(25));
        for i in first..(first + 25).min(n) {
            let mark = if i == e.cursor { ">" } else { " " };
            let (a, why) = &e.allowed[i];
            let (lo, hi) = (a.first().copied().unwrap_or(0), a.last().copied().unwrap_or(0));
            s.push_str(&format!("{mark} {:<18} {:>3}  [{lo}..{hi}, {} values: {why}]\n", EDIT_U8_FIELDS[i], e.values[i], a.len()));
        }
        t.0 = s;
    }
    if !e.dirty {
        return;
    }
    e.dirty = false;
    let fields: Vec<(String, u32)> = EDIT_U8_FIELDS.iter().map(|f| f.to_string()).zip(e.values).collect();
    let Some(look) = ddlook::save::edit_look(e.loader.vfs, &e.tables, &e.pattern, &[], ddlook::Unit::Player, &fields, &[]) else {
        warn!("charaedit: no look for these values");
        return;
    };
    if let Some(old) = e.body.take() {
        commands.entity(old).despawn();
    }
    match crate::character::spawn_look(
        &mut commands,
        &mut e.loader,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
        &look,
        MOTION,
        0,
        Vec3::ZERO,
        Some(0.0),
    ) {
        Ok(body) => e.body = Some(body),
        Err(err) => error!("charaedit: {err}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn steps_skip_missing_presets_and_wrap() {
        let a = [0, 1, 2, 50, 51];
        assert_eq!(super::step(&a, 2, true), 50);
        assert_eq!(super::step(&a, 51, true), 0);
        assert_eq!(super::step(&a, 50, false), 2);
        assert_eq!(super::step(&a, 0, false), 51);
        // a value outside the set moves to its neighbour
        assert_eq!(super::step(&a, 10, true), 50);
    }
}
