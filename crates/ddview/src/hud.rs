//! Cockpit presentation from the installed rGUI. Constant geometry and atlas
//! rectangles are authored data; this is not a general GUI animation VM.
use crate::{character::Playback, play, stage};
use bevy::math::Affine2;
use bevy::prelude::*;
use std::collections::HashMap;

const COCKPIT: &str = "id/DDN/cockpit/cockpit0_1"; // 0x006781c0

#[derive(Clone, Copy)]
enum Binding {
    Health,
    Stamina,
    Pawn,
    Digit(usize),
}

#[derive(Component)]
pub(crate) struct Prompt {
    position: Vec2,
    size: Vec2,
    local_binding: Option<usize>,
    converse: bool,
}

fn node_position(gui: &mtgui::Gui<'_>, wanted: usize) -> Result<Vec2, String> {
    let mut stack: Vec<Affine2> = Vec::new();
    for (index, depth) in gui.preorder()? {
        stack.truncate(depth);
        let value = |name: &str, default: f32| {
            gui.node_property(index, name)
                .and_then(|p| p.word)
                .map(f32::from_bits)
                .unwrap_or(default)
        };
        let local = Affine2::from_scale_angle_translation(
            Vec2::new(value("SclX", 1.0), value("SclY", 1.0)),
            value("Rotation", 0.0).to_radians(),
            Vec2::new(value("PosX", 0.0), value("PosY", 0.0)),
        );
        let world = stack.last().copied().unwrap_or(Affine2::IDENTITY) * local;
        if index == wanted {
            return Ok(world.transform_point2(Vec2::ZERO));
        }
        stack.push(world);
    }
    Err("HUD node unreachable".into())
}

fn spawn_prompts(
    commands: &mut Commands,
    vfs: &ddvfs::Vfs,
    images: &mut Assets<Image>,
) -> Result<(), String> {
    // uGUIActionPallet::vf58, 0x005ee480. Language selection currently English,
    // as elsewhere in the viewer.
    let bytes = vfs.read("id/common/custskl0", mtarc::type_hash("rGUI"))?;
    let gui = mtgui::Gui::parse(&bytes)?;
    let bytes = vfs.read(
        "id/DDN/message/actionpallet/actionpallet_eng",
        mtarc::type_hash("rGUIMessage"),
    )?;
    let messages = mtgmd::MessageFile::parse(&bytes).map_err(|e| e.to_string())?;
    let objects = gui.animation_objects("360_buttons")?;
    let object = objects
        .iter()
        .find(|o| !o.texture_keys.is_empty())
        .ok_or("button texture track")?;
    let regions = gui.texture_regions()?;
    let font_path = gui
        .resources
        .iter()
        .find(|r| r.class == "rGUIFont")
        .and_then(|r| r.path)
        .ok_or("prompt font resource")?;
    let font_bytes = vfs.read(font_path, mtarc::type_hash("rGUIFont"))?;
    let bitmap_font = mtgui::font::Font::parse(&font_bytes)?;
    let mut font_images: HashMap<u8, Handle<Image>> = HashMap::new();
    // The keyboard group in custskl0; role positions cross-checked with ref001.
    // Virtual-key codes follow the viewer bindings J/K/E/Space. Original selector is
    // input binding +0x8c, uGUIActionPallet::vf05, 0x005ed3b0.
    for (text_id, key_id, label, keys, converse) in [
        (175, 178, "action_lightattack", [0x4a, 0x61], false),
        (190, 193, "action_strongattack", [0x4b, 0x62], false),
        (197, 200, "touch_action_talk", [0x45, 0x45], true),
        (204, 207, "action_jump", [0x20, 0x0d], false),
    ] {
        let index = gui
            .nodes
            .iter()
            .position(|n| n.id == text_id)
            .ok_or("prompt text node")?;
        let position = node_position(&gui, index)?;
        let font = gui
            .node_property(index, "FontSize")
            .and_then(|p| gui.vector(p))
            .ok_or("prompt font size")?;
        let text = messages.by_label(label).ok_or("prompt message label")?;
        for placed in bitmap_font.line(text, [font[0], font[1]])? {
            let glyph = placed.glyph;
            let image = if let Some(image) = font_images.get(&glyph.page) {
                image.clone()
            } else {
                let path = bitmap_font.texture_path(glyph.page)?;
                let image = images.add(stage::tex_to_image(
                    &vfs.read(&path, mtarc::type_hash("rTexture"))?,
                )?);
                font_images.insert(glyph.page, image.clone());
                image
            };
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode {
                    image,
                    rect: Some(Rect::new(
                        glyph.x as f32 + 0.5,
                        glyph.y as f32 + 0.5,
                        (glyph.x + glyph.width) as f32 - 0.5,
                        (glyph.y + glyph.height) as f32 - 0.5,
                    )),
                    ..default()
                },
                GlobalZIndex(10),
                Prompt {
                    position: position + Vec2::from_array(placed.position),
                    size: Vec2::from_array(placed.size),
                    local_binding: None,
                    converse,
                },
            ));
        }
        let index = gui
            .nodes
            .iter()
            .position(|n| n.id == key_id)
            .ok_or("prompt key node")?;
        for (local_binding, scan) in keys.into_iter().enumerate() {
            let frame = gui.control_range(index, scan + 0x8c)?.ok_or("prompt selector")?[0] as u32;
            let id = object
                .texture_keys
                .iter()
                .find(|(f, _)| *f == frame)
                .ok_or("prompt key frame")?
                .1;
            let region = regions.iter().find(|r| r.id == id).ok_or("prompt atlas region")?;
            let size = object
                .vector_keys
                .iter()
                .find(|(n, f, _)| *n == "Size" && *f == frame)
                .map(|(_, _, v)| *v)
                .ok_or("prompt key size frame")?;
            let size = Vec2::new(size[0], size[1]);
            let position = node_position(&gui, index)?
                + Vec2::from_array(mtgui::layout::anchor_offset(
                    size.to_array(),
                    object.integer("ControlPoint").unwrap_or(0),
                ));
            let image = images.add(stage::tex_to_image(
                &vfs.read(region.path, mtarc::type_hash("rTexture"))?,
            )?);
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode {
                    image,
                    rect: Some(Rect::new(
                        region.rect[0],
                        region.rect[1],
                        region.rect[2],
                        region.rect[3],
                    )),
                    ..default()
                },
                GlobalZIndex(10),
                Prompt {
                    position,
                    size,
                    local_binding: Some(local_binding),
                    converse,
                },
            ));
        }
    }
    Ok(())
}

#[derive(Component)]
pub(crate) struct Quad {
    position: Vec2,
    size: Vec2,
    rect: Rect,
    crop: bool,
    binding: Binding,
    digits: Vec<Rect>,
    color: Color,
}

fn number(value: f32) -> String {
    // uGUILifeGauge::vf09, 0x00678210: positive fractions show one HP.
    let hp = if value > 0.0 {
        value.floor().max(1.0) as u32
    } else {
        0
    };
    format!("{}", hp % 10000)
}

/// Startup receives the existing VFS and image assets; no separate game index.
pub fn spawn(commands: &mut Commands, vfs: &ddvfs::Vfs, images: &mut Assets<Image>) -> Result<(), String> {
    let bytes = vfs.read(COCKPIT, mtarc::type_hash("rGUI"))?;
    let gui = mtgui::Gui::parse(&bytes)?;
    let regions = gui.texture_regions()?;
    let mut textures: HashMap<&str, Handle<Image>> = HashMap::new();
    for (name, binding, crop) in [
        ("PL_Hp_gaugeback", Binding::Health, false),
        ("PL_Hp_gauge_under", Binding::Health, false),
        ("PL_Hp_gauge", Binding::Health, true),
        ("PL_Ap_gaugeback", Binding::Stamina, false),
        ("PL_Ap_gauge_under", Binding::Stamina, false),
        ("PL_Ap_gauge", Binding::Stamina, true),
        ("CMC_gaugeback", Binding::Pawn, false),
        ("CMC_gauge_under", Binding::Pawn, false),
        ("CMC_gauge", Binding::Pawn, true),
        ("life_counter", Binding::Digit(0), false),
    ] {
        let objects = gui.animation_objects(name)?;
        let nodes: Vec<_> = gui
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| gui.local_animation(*i).is_some_and(|a| a.name == name))
            .collect();
        for (ordinal, (node_index, _)) in nodes.into_iter().enumerate() {
            if name == "life_counter" && ordinal >= 4 {
                break;
            }
            let binding = if name == "life_counter" {
                Binding::Digit(ordinal)
            } else {
                binding
            };
            let prop = |n: &str| gui.node_property(node_index, n);
            let scalar = |n: &str| prop(n).and_then(|p| p.word).map(f32::from_bits).unwrap_or(0.0);
            let node_pos = Vec2::new(scalar("PosX"), scalar("PosY"));
            for object in &objects {
                let Some(texture) = object
                    .integer("Texture")
                    .filter(|&id| id != u32::MAX)
                    .or_else(|| object.texture_keys.first().map(|k| k.1))
                else {
                    continue;
                };
                let region = regions
                    .iter()
                    .find(|r| r.id == texture)
                    .ok_or("HUD texture region absent")?;
                let image = if let Some(h) = textures.get(region.path) {
                    h.clone()
                } else {
                    let image = images.add(stage::tex_to_image(
                        &vfs.read(region.path, mtarc::type_hash("rTexture"))?,
                    )?);
                    textures.insert(region.path, image.clone());
                    image
                };
                let Some(size) = object.property("Size").and_then(|p| gui.vector(p)) else {
                    continue;
                };
                let size = Vec2::new(size[0], size[1]);
                let anchor = mtgui::layout::anchor_offset(
                    size.to_array(),
                    object.integer("ControlPoint").unwrap_or(0),
                );
                let position = node_pos
                    + Vec2::new(
                        object.scalar("PosX").unwrap_or(0.0),
                        object.scalar("PosY").unwrap_or(0.0),
                    )
                    + Vec2::from_array(anchor);
                // Normal living/non-afflicted selector is 5 in 0x00678ec0
                // / 0x00679070. Its adjacent colour keys have equal values.
                let frame = if crop {
                    gui.control_range(node_index, 5)?
                        .map(|r| r[0] as u32)
                        .unwrap_or(0)
                } else {
                    0
                };
                let color = object
                    .vector_keys
                    .iter()
                    .filter(|(n, f, _)| *n == "VertexColor" && *f <= frame)
                    .last()
                    .map(|(_, _, v)| *v)
                    .or_else(|| {
                        object
                            .property("VertexColor")
                            .or_else(|| object.property("VertexColorLT"))
                            .and_then(|p| gui.vector(p))
                    })
                    .unwrap_or([255.0; 4]);
                let color = Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                );
                let rect = Rect::new(region.rect[0], region.rect[1], region.rect[2], region.rect[3]);
                let digits = object
                    .texture_keys
                    .iter()
                    .filter_map(|(_, id)| regions.iter().find(|r| r.id == *id))
                    .map(|r| Rect::new(r.rect[0], r.rect[1], r.rect[2], r.rect[3]))
                    .collect();
                // Rotation 180 authored on the stamina background, handled as
                // both flips with its opposite anchor (the same rectangle).
                let rotate = object.scalar("Rotation").unwrap_or(0.0);
                let position = if rotate == 180.0 {
                    node_pos - Vec2::from_array(anchor) - size
                } else {
                    position
                };
                commands.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    ImageNode {
                        image,
                        rect: Some(rect),
                        color,
                        flip_x: rotate == 180.0,
                        flip_y: rotate == 180.0,
                        ..default()
                    },
                    GlobalZIndex(prop("Priority").and_then(|p| p.word).unwrap_or(0) as i32),
                    Quad {
                        position,
                        size,
                        rect,
                        crop,
                        binding,
                        digits,
                        color,
                    },
                ));
            }
        }
    }
    info!("HUD: installed cockpit geometry/textures loaded");
    spawn_prompts(commands, vfs, images)?;
    Ok(())
}

pub fn update(
    hud: Res<play::Hud>,
    state: Option<Res<play::PlayState>>,
    opening: Option<Res<crate::opening::OpeningRes>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    playback: Query<&Playback>,
    transforms: Query<&GlobalTransform>,
    mut quads: Query<(&Quad, &mut Node, &mut ImageNode), Without<Prompt>>,
    mut prompts: Query<(&Prompt, &mut Node), Without<Quad>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let scale = Vec2::new(window.width() / 1280.0, window.height() / 720.0);
    let row = |label: &str| hud.rows.iter().find(|r| r.0 == label);
    let (event_active, can_talk) = opening
        .as_ref()
        .map(|opening| {
            let session = opening.0.lock().unwrap();
            (
                session.event_active(),
                state.as_ref().is_some_and(|s| session.can_talk(s.follow)),
            )
        })
        .unwrap_or((false, false));
    let local_binding = state
        .as_ref()
        .and_then(|s| s.locals.iter().position(|seat| *seat == s.follow));
    for (prompt, mut node) in &mut prompts {
        node.display = if (prompt.converse && !can_talk)
            || local_binding.is_none()
            || prompt
                .local_binding
                .is_some_and(|binding| Some(binding) != local_binding)
            || event_active
        {
            Display::None
        } else {
            Display::Flex
        };
        node.left = Val::Px(prompt.position.x * scale.x);
        node.top = Val::Px(prompt.position.y * scale.y);
        node.width = Val::Px(prompt.size.x * scale.x);
        node.height = Val::Px(prompt.size.y * scale.y);
    }
    let pawn_position = (|| {
        let st = state.as_ref()?;
        let entity = *st.entities.get(&play::pawn_seat(st.follow))?;
        let p = playback.get(entity).ok()?;
        // Provisional: highest animated joint until the engine's source joint
        // used to populate +0x2e0 is traced. No fixed guessed body height.
        let point = p
            .joints
            .values()
            .filter_map(|e| transforms.get(*e).ok())
            .map(|t| t.translation())
            .max_by(|a, b| a.y.total_cmp(&b.y))?;
        let (camera, transform) = cameras.single().ok()?;
        // 0x00679a10 moves the anchor 70 cm toward the camera before projecting.
        let point = point + (transform.translation() - point).normalize_or_zero() * 0.7;
        let projected = camera.world_to_viewport(transform, point).ok()?;
        Some(projected / scale - Vec2::new(0.0, 40.0)) // original screen-space offset
    })();
    for (quad, mut node, mut image) in &mut quads {
        if event_active {
            node.display = Display::None;
            continue;
        }
        let data = match quad.binding {
            Binding::Health | Binding::Digit(_) => row("Arisen HP"),
            Binding::Stamina => row("Arisen stamina"),
            Binding::Pawn => row("Pawn HP"),
        };
        let Some(data) = data else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        let mut pos = quad.position;
        let mut size = quad.size;
        let mut rect = quad.rect;
        if matches!(quad.binding, Binding::Pawn) {
            let Some(point) = pawn_position else {
                node.display = Display::None;
                continue;
            };
            pos += point;
        }
        if let Binding::Digit(i) = quad.binding {
            let digits = number(data.1);
            let Some(digit) = digits.as_bytes().get(i) else {
                node.display = Display::None;
                continue;
            };
            let Some(&digit_rect) = quad.digits.get((digit - b'0') as usize) else {
                node.display = Display::None;
                continue;
            };
            rect = digit_rect;
        }
        if quad.crop {
            size.x =
                mtgui::layout::gauge_width(quad.size.x, (data.1 / data.2.max(f32::EPSILON)).clamp(0.0, 1.0));
            rect.max.x = rect.min.x + size.x;
        }
        node.left = Val::Px(pos.x * scale.x);
        node.top = Val::Px(pos.y * scale.y);
        node.width = Val::Px(size.x * scale.x);
        node.height = Val::Px(size.y * scale.y);
        image.rect = Some(rect);
        image.color = quad.color;
        // 0x00679070: full stamina fades the entire bar to 30 percent alpha.
        if matches!(quad.binding, Binding::Stamina) && data.1 == data.2 {
            image.color = image.color.with_alpha(image.color.alpha() * 0.3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hp_digits_pack_left() {
        assert_eq!(number(455.9), "455");
        assert_eq!(number(0.1), "1");
        assert_eq!(number(0.0), "0");
        assert_eq!(number(10001.0), "1");
    }
}
