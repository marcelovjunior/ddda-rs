//! Cockpit presentation from the installed rGUI. Constant geometry and atlas
//! rectangles are authored data; this is not a general GUI animation VM.
use crate::{character::Playback, play, stage};
use bevy::math::Affine2;
use bevy::prelude::*;
use std::collections::HashMap;

const COCKPIT: &str = "id/DDN/cockpit/cockpit0_1"; // 0x006781c0
const NPC_GAUGE: &str = "id/cockpit/npc_gauge"; // 0x0069F540

struct NpcTemplate {
    image: Handle<Image>,
    position: Vec2,
    size: Vec2,
    rect: Rect,
    scale: Vec2,
    anchor: u32,
    priority: i32,
    crop: bool,
    color: Color,
}

#[derive(Resource, Default)]
pub(crate) struct NpcGauges {
    templates: Vec<NpcTemplate>,
    owners: HashMap<(u32, Entity), Vec<Entity>>,
    poses: HashMap<u32, NpcCachedBounds>,
}

#[derive(Clone, Copy, PartialEq)]
struct NpcPoseKey {
    body: usize,
    motion: ddhost::ddenemyrt::body::MotionId,
    frame: f32,
    position: [f32; 3],
    yaw: f32,
    scale: [f32; 3],
}

struct NpcCachedBounds {
    key: NpcPoseKey,
    model: [[f32; 4]; 4],
    bounds: ddhost::ddenemyrt::npc_body::NpcBounds,
}

#[derive(Default)]
pub(crate) struct NpcGaugeCapture {
    checked: bool,
    path: Option<std::path::PathBuf>,
    requested: bool,
}

#[derive(Component)]
pub(crate) struct NpcGaugeQuad {
    owner: u32,
    camera: Entity,
    template: usize,
}

/// The normal bar and full background of the installed layout. Its status
/// icons, name text and animated delayed-HP trail are not a GUI VM port.
fn npc_templates(vfs: &ddvfs::Vfs, images: &mut Assets<Image>) -> Result<Vec<NpcTemplate>, String> {
    let bytes = vfs.read(NPC_GAUGE, mtarc::type_hash("rGUI"))?;
    let gui = mtgui::Gui::parse(&bytes)?;
    let regions = gui.texture_regions()?;
    let mut textures = HashMap::new();
    let mut templates = Vec::new();
    for (name, crop) in [("CMC_gaugeback0", false), ("Npc_gauge", true)] {
        let index = gui
            .nodes
            .iter()
            .enumerate()
            .find(|(i, _)| gui.local_animation(*i).is_some_and(|a| a.name == name))
            .map(|(i, _)| i)
            .ok_or("NPC gauge node absent")?;
        let scalar = |key: &str, default| {
            gui.node_property(index, key)
                .and_then(|p| p.word)
                .map(f32::from_bits)
                .unwrap_or(default)
        };
        let scale = Vec2::new(scalar("SclX", 1.0), scalar("SclY", 1.0));
        let node_pos = node_position(&gui, index)?;
        // 0x0069F760 selects control 5 for an ordinary living NPC.
        let frame = if crop {
            gui.control_range(index, 5)?.map_or(0, |r| r[0] as u32)
        } else {
            0
        };
        for object in gui.animation_objects(name)? {
            let Some(texture) = object.integer("Texture").filter(|&id| id != u32::MAX) else {
                continue;
            };
            let region = regions
                .iter()
                .find(|r| r.id == texture)
                .ok_or("NPC gauge atlas absent")?;
            let image = if let Some(image) = textures.get(region.path) {
                Handle::<Image>::clone(image)
            } else {
                let image = images.add(stage::tex_to_image(
                    &vfs.read(region.path, mtarc::type_hash("rTexture"))?,
                )?);
                textures.insert(region.path, image.clone());
                image
            };
            let size = object
                .property("Size")
                .and_then(|p| gui.vector(p))
                .ok_or("NPC gauge size absent")?;
            let color = object
                .vector_keys
                .iter()
                .filter(|(name, key, _)| *name == "VertexColor" && *key <= frame)
                .last()
                .map(|(_, _, value)| *value)
                .or_else(|| {
                    object
                        .property("VertexColor")
                        .or_else(|| object.property("VertexColorLT"))
                        .and_then(|p| gui.vector(p))
                })
                .unwrap_or([255.0; 4]);
            templates.push(NpcTemplate {
                image,
                position: node_pos
                    + scale
                        * Vec2::new(
                            object.scalar("PosX").unwrap_or(0.0),
                            object.scalar("PosY").unwrap_or(0.0),
                        ),
                size: Vec2::new(size[0], size[1]),
                scale,
                rect: Rect::new(region.rect[0], region.rect[1], region.rect[2], region.rect[3]),
                anchor: object.integer("ControlPoint").unwrap_or(0),
                priority: gui
                    .node_property(index, "Priority")
                    .and_then(|p| p.word)
                    .unwrap_or(0) as i32,
                crop,
                color: Color::srgba(
                    color[0] / 255.0,
                    color[1] / 255.0,
                    color[2] / 255.0,
                    color[3] / 255.0,
                ),
            });
        }
    }
    if !templates.iter().any(|t| t.crop) {
        return Err("NPC gauge fill absent".into());
    }
    Ok(templates)
}

#[derive(Clone, Copy)]
enum Binding {
    Health,
    Stamina,
    Pawn,
    Digit(usize),
    /// The small-enemy gauge (`play::Hud::enemy`).
    Enemy,
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

fn update_npc_gauges(
    commands: &mut Commands,
    gauges: &mut NpcGauges,
    opening: Option<&crate::opening::OpeningRes>,
    cameras: &Query<(Entity, &Camera, &GlobalTransform, &Projection), With<Camera3d>>,
    transform_helper: &bevy::transform::helper::TransformHelper,
    quads: &mut Query<(&NpcGaugeQuad, &mut Node, &mut ImageNode), (Without<Quad>, Without<Prompt>)>,
    origin: Option<Vec3>,
    event_active: bool,
    capture: &mut NpcGaugeCapture,
) {
    use ddhost::ddenemyrt::{combat::Victim, npc_body::ORDINARY_NPC_SCENERY_RADIUS, npc_combat::gauge_fill};
    if !capture.checked {
        capture.checked = true;
        capture.path = std::env::var_os("DDVIEW_NPC_GAUGE_CAPTURE")
            .map(std::path::PathBuf::from)
            .filter(|path| {
                path.starts_with("E:/ddda-re")
                    && !path.components().any(|component| matches!(component, std::path::Component::ParentDir))
            });
    }
    // Diagnostic selection only: observe the ordinary camera12 close-up
    // halfway through its real resource. Never select or advance a camera.
    let capture_closeup = capture.path.is_some() && opening.is_some_and(|opening| {
        let session = opening.0.lock().unwrap();
        session.host.world.cameras.as_ref().and_then(|c| c.active.as_ref())
            .is_some_and(|c| c.stage == 100 && c.camera_id == 12 && c.seconds >= c.camera.duration() * 0.5)
    });
    let snapshots: Vec<_> = opening
        .filter(|_| origin.is_some() && !event_active)
        .into_iter()
        .flat_map(|opening| {
            let session = opening.0.lock().unwrap();
            let Some(fight) = session.host.fight.as_ref() else {
                return Vec::new();
            };
            fight
                .npcs
                .values()
                .filter_map(|npc| {
                    if fight.npc_errors.contains_key(&npc.fighter.owner) {
                        return None;
                    }
                    if session.host.story_npc_gauge_visible(npc.npc) != Some(true) {
                        return None;
                    }
                    let Victim::Npc { region, .. } = &npc.fighter.victim else {
                        return None;
                    };
                    if region.hp <= 0.0 {
                        return None;
                    }
                    let key = NpcPoseKey {
                        body: std::sync::Arc::as_ptr(&npc.body) as usize,
                        motion: npc.motion,
                        frame: npc.frame,
                        position: npc.position,
                        yaw: npc.yaw,
                        scale: npc.scale,
                    };
                    if !gauges
                        .poses
                        .get(&npc.fighter.owner)
                        .is_some_and(|pose| pose.key == key)
                    {
                        // Cache a host pose between render frames; each view still
                        // applies its own live camera pull and projection below.
                        let pose = npc
                            .body
                            .pose(npc.motion, npc.frame, npc.position, npc.yaw, npc.scale)
                            .ok()?;
                        // Explicit opening detail3 adapter: native dynamic LOD choice and
                        // constraint-mask overrides are not yet represented by the host.
                        let bounds = npc.body.bounds(&pose, 3).ok()?;
                        gauges.poses.insert(
                            npc.fighter.owner,
                            NpcCachedBounds {
                                key,
                                model: pose.model,
                                bounds,
                            },
                        );
                    }
                    let pose = &gauges.poses[&npc.fighter.owner];
                    Some((
                        npc.fighter.owner,
                        npc.position,
                        pose.model,
                        pose.bounds,
                        region.hp,
                        region.hp_max,
                        session.npc_body_entity(npc.npc),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    gauges
        .poses
        .retain(|owner, _| snapshots.iter().any(|(candidate, ..)| candidate == owner));
    let bar_width = gauges.templates.iter().find(|t| t.crop).map_or(0.0, |t| t.size.x);
    let mut desired = HashMap::new();
    let mut evidence = HashMap::new();
    if let Some(origin) = origin.filter(|_| !event_active) {
        for (owner, position, model, bounds, hp, max, body_entity) in snapshots {
            let Some(width) = gauge_fill(hp, max, hp <= 0.0, bar_width) else {
                continue;
            };
            for (entity, stored_camera, stored_transform, projection) in cameras {
                if !stored_camera.is_active {
                    continue;
                }
                // HUD runs before PostUpdate propagation/CameraUpdateSystems.
                // Resolve the current hierarchy and lens to match this frame's
                // rendered camera, including an authored quest-camera change.
                let Some(camera) = current_projection_camera(stored_camera, projection) else { continue };
                let Ok(transform) = transform_helper.compute_global_transform(entity) else { continue };
                let point = bounds.gauge_anchor(
                    &model,
                    [0.0; 3],
                    position,
                    Some((transform.translation() / stage::UNIT + origin).to_array()),
                    ORDINARY_NPC_SCENERY_RADIUS,
                );
                let Some((screen, scale)) =
                    npc_gauge_projection(&camera, &transform, origin, Vec3::from_array(point))
                else {
                    continue;
                };
                desired.insert((owner, entity), (screen, scale, width));
                if capture_closeup && !capture.requested {
                    let body_transform = body_entity.and_then(|e| transform_helper.compute_global_transform(e).ok());
                    if let Some(body_transform) = body_transform {
                        let body_root = body_transform.translation() / stage::UNIT + origin;
                        let projected = |camera: &Camera, transform: &GlobalTransform, point: Vec3| {
                            npc_gauge_projection(camera, transform, origin, point).map(|(p, _)| p + Vec2::new(0.0, 40.0))
                        };
                        let old_root = projected(stored_camera, stored_transform, body_root);
                        let current_root = projected(&camera, &transform, body_root);
                        let current_top = projected(&camera, &transform, Vec3::from_array(bounds.gauge_top()));
                        let current_anchor = projected(&camera, &transform, Vec3::from_array(point));
                        evidence.insert((owner, entity), format!(
                            "actor_position={position:?} model_root={:?} body_root={body_root:?} bounds={bounds:?} anchor={point:?} old_root_px={old_root:?} current_root_px={current_root:?} current_top_px={current_top:?} current_anchor_px={current_anchor:?} old_eye={:?} current_eye={:?}",
                            ddcombat::volumes::transform_point(&model, [0.0; 3]), stored_transform.translation(), transform.translation()));
                    }
                }
            }
        }
    }
    reconcile_npc_gauges(commands, gauges, quads, &desired);
    // Optional evidence follows production visibility; it never forces HP,
    // flags, camera, placement or graph completion. Private output only.
    if capture_closeup && !capture.requested {
        if let Some(path) = capture.path.as_ref() {
            let candidate = desired.iter().find(|(_, (point, _, _))| {
                point.x >= 0.0 && point.x <= 1280.0 && point.y >= 0.0 && point.y <= 720.0
            });
            if let Some((&(owner, camera), &(point, _, width))) = candidate {
                if quads.iter().any(|(quad, node, _)| {
                    quad.owner == owner && quad.camera == camera && node.display == Display::Flex
                }) {
                    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
                    commands
                        .spawn(Screenshot::primary_window())
                        .observe(save_to_disk(path.clone()));
                    info!("NPC gauge capture: owner={owner} camera={camera:?} authored_point={point:?} fill_width={width} path={} same_frame={:?}", path.display(), evidence.get(&(owner, camera)));
                    capture.requested = true;
                }
            }
        }
    }
}

// Match this frame's Projection before camera_system refreshes
// Camera::computed in PostUpdate. This clone never writes the render camera.
fn current_projection_camera(camera: &Camera, projection: &Projection) -> Option<Camera> {
    let viewport = camera.logical_viewport_size()?;
    let mut current = camera.clone();
    let mut projection = projection.clone();
    projection.update(viewport.x, viewport.y);
    current.computed.clip_from_view = projection.get_clip_from_view();
    Some(current)
}

fn npc_gauge_projection(
    camera: &Camera,
    transform: &GlobalTransform,
    origin: Vec3,
    game_point: Vec3,
) -> Option<(Vec2, Vec2)> {
    let viewport = camera.logical_viewport_rect()?;
    let scale = viewport.size() / Vec2::new(1280.0, 720.0);
    if !scale.is_finite() || scale.min_element() <= 0.0 {
        return None;
    }
    let screen = camera
        .world_to_viewport(transform, (game_point - origin) * stage::UNIT)
        .ok()?;
    // vf11 / 0x0069FA10: projection, then 40 authored screen px up.
    Some(((screen - viewport.min) / scale - Vec2::new(0.0, 40.0), scale))
}

fn reconcile_npc_gauges(
    commands: &mut Commands,
    gauges: &mut NpcGauges,
    quads: &mut Query<(&NpcGaugeQuad, &mut Node, &mut ImageNode), (Without<Quad>, Without<Prompt>)>,
    desired: &HashMap<(u32, Entity), (Vec2, Vec2, f32)>,
) {
    let retired: Vec<_> = gauges
        .owners
        .keys()
        .copied()
        .filter(|key| !desired.contains_key(key))
        .collect();
    for key in retired {
        if let Some(entities) = gauges.owners.remove(&key) {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
    }
    for &(owner, camera) in desired.keys() {
        gauges.owners.entry((owner, camera)).or_insert_with(|| {
            gauges
                .templates
                .iter()
                .enumerate()
                .map(|(index, t)| {
                    commands
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                display: Display::None,
                                ..default()
                            },
                            ImageNode {
                                image: t.image.clone(),
                                rect: Some(t.rect),
                                color: t.color,
                                ..default()
                            },
                            GlobalZIndex(t.priority),
                            UiTargetCamera(camera),
                            NpcGaugeQuad {
                                owner,
                                camera,
                                template: index,
                            },
                        ))
                        .id()
                })
                .collect()
        });
    }
    for (quad, mut node, mut image) in quads {
        let Some(&(point, screen_scale, width)) = desired.get(&(quad.owner, quad.camera)) else {
            node.display = Display::None;
            continue;
        };
        let t = &gauges.templates[quad.template];
        let (position, size, rect) = npc_quad_layout(t, point, screen_scale, width);
        node.display = Display::Flex;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        image.rect = Some(rect);
        image.color = t.color;
    }
}

fn npc_quad_layout(t: &NpcTemplate, point: Vec2, screen_scale: Vec2, width: f32) -> (Vec2, Vec2, Rect) {
    let mut size = t.size;
    let mut rect = t.rect;
    // 0x0069F5A0 changes the drawable width and texture width; retaining
    // its authored full-width anchor keeps the fill's left edge fixed.
    let anchor = Vec2::from_array(mtgui::layout::anchor_offset(t.size.to_array(), t.anchor));
    if t.crop {
        size.x = width;
        rect.max.x = rect.min.x + width;
    }
    (
        (point + t.position + t.scale * anchor) * screen_scale,
        size * t.scale * screen_scale,
        rect,
    )
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
        // uGUILifeGauge's small-enemy artwork (0x0067A340 / 0x0067AC40):
        // enmy_gauge_back (node 34, id 164) and enmy_gauge (node 19, id 142)
        ("enmy_gauge_back", Binding::Enemy, false),
        ("enmy_gauge", Binding::Enemy, true),
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
    commands.insert_resource(NpcGauges {
        templates: npc_templates(vfs, images)?,
        ..default()
    });
    Ok(())
}

pub fn update(
    mut commands: Commands,
    hud: Res<play::Hud>,
    state: Option<Res<play::PlayState>>,
    opening: Option<Res<crate::opening::OpeningRes>>,
    windows: Query<&Window>,
    cameras: Query<(Entity, &Camera, &GlobalTransform, &Projection), With<Camera3d>>,
    transform_helper: bevy::transform::helper::TransformHelper,
    playback: Query<&Playback>,
    transforms: Query<&GlobalTransform>,
    mut quads: Query<(&Quad, &mut Node, &mut ImageNode), (Without<Prompt>, Without<NpcGaugeQuad>)>,
    mut prompts: Query<(&Prompt, &mut Node), (Without<Quad>, Without<NpcGaugeQuad>)>,
    mut npc_gauges: Option<ResMut<NpcGauges>>,
    mut npc_quads: Query<(&NpcGaugeQuad, &mut Node, &mut ImageNode), (Without<Quad>, Without<Prompt>)>,
    mut npc_capture: Local<NpcGaugeCapture>,
) {
    let _timing = crate::perf::Section::start("hud::update");
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
    if let Some(gauges) = npc_gauges.as_mut() {
        update_npc_gauges(
            &mut commands,
            gauges,
            opening.as_deref(),
            &cameras,
            &transform_helper,
            &mut npc_quads,
            state.as_ref().map(|s| s.origin),
            event_active,
            &mut npc_capture,
        );
    }
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
        // 0x007325d0: joint-map entry 4 +25cm Y. 0x00ffddb0 maps
        // actor+368 to rModel+110; 0x00fa8fe0 loads the MOD remap there.
        let joint = transforms.get(*p.joints.get(&4)?).ok()?.translation();
        let (_, camera, transform, _) = cameras.single().ok()?;
        let point = pawn_anchor(joint, transform.translation());
        let projected = camera.world_to_viewport(transform, point).ok()?;
        Some(projected / scale - Vec2::new(0.0, 40.0)) // original screen-space offset
    })();
    // The small enemy's anchor: *viewer choice* until 0x0067C970's mode-0
    // point (actor +0x1A0, +0x1B4 on Y) is traced: joint-map entry 4 plus
    // 25 cm, pulled toward the camera, as the pawn gauge's 0x007325D0.
    let enemy_position = (|| {
        let (entity, ..) = hud.enemy?;
        let p = playback.get(entity).ok()?;
        let joint = transforms.get(*p.joints.get(&4)?).ok()?.translation();
        let (_, camera, transform, _) = cameras.single().ok()?;
        let point = pawn_anchor(joint, transform.translation());
        let projected = camera.world_to_viewport(transform, point).ok()?;
        Some(projected / scale - Vec2::new(0.0, 40.0))
    })();
    let enemy_row = hud
        .enemy
        .map(|(_, hp, max)| (String::new(), hp, max, Color::WHITE));
    for (quad, mut node, mut image) in &mut quads {
        if event_active {
            node.display = Display::None;
            continue;
        }
        let data = match quad.binding {
            Binding::Health | Binding::Digit(_) => row("Arisen HP"),
            Binding::Stamina => row("Arisen stamina"),
            Binding::Pawn => row("Pawn HP"),
            Binding::Enemy => enemy_row.as_ref(),
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
        if matches!(quad.binding, Binding::Enemy) {
            let Some(point) = enemy_position else {
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

// Translated from 0x007325d0 and 0x00679a10 (2017-12-13 Steam).
fn pawn_anchor(joint: Vec3, camera: Vec3) -> Vec3 {
    let point = joint + Vec3::Y * 0.25;
    point + (camera - point).normalize_or_zero() * 0.7
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn npc_capture_observes_current_lens_without_overwriting_render_camera() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        let old_clip = Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1);
        let camera = Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1600, 900), scale_factor: 1.0 }),
                clip_from_view: old_clip,
                ..default()
            },
            ..default()
        };
        let projection = Projection::Perspective(PerspectiveProjection { fov: 0.5, aspect_ratio: 1.0, ..default() });
        let current = current_projection_camera(&camera, &projection).unwrap();
        let expected = Mat4::perspective_infinite_reverse_rh(0.5, 16.0 / 9.0, 0.1);
        assert!(current.computed.clip_from_view.abs_diff_eq(expected, 1e-6));
        assert_eq!(camera.computed.clip_from_view, old_clip);
        let transform = GlobalTransform::IDENTITY;
        let point = Vec3::new(50.0, 10.0, -300.0);
        let old = npc_gauge_projection(&camera, &transform, Vec3::ZERO, point).unwrap().0;
        let fresh = npc_gauge_projection(&current, &transform, Vec3::ZERO, point).unwrap().0;
        assert!((old - fresh).length() > 100.0, "changed lens must produce a distinct evidence projection");
        assert!(current_projection_camera(&Camera::default(), &projection).is_none());
    }
    #[test]
    fn npc_gauge_projection_uses_viewport_local_coordinates_and_origin() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
        let camera = Camera {
            viewport: Some(Viewport {
                physical_position: UVec2::new(320, 180),
                physical_size: UVec2::new(640, 360),
                ..default()
            }),
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1280, 720),
                    scale_factor: 1.0,
                }),
                clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1),
                ..default()
            },
            ..default()
        };
        let point = Vec3::new(10000.0, 20000.0, 30000.0);
        let eye = point + Vec3::Z * 300.0;
        let camera_transform = GlobalTransform::from_translation(eye * stage::UNIT);
        let (full, scale) = npc_gauge_projection(&camera, &camera_transform, Vec3::ZERO, point).unwrap();
        assert!((full - Vec2::new(640.0, 320.0)).length() < 0.001);
        assert_eq!(scale, Vec2::splat(0.5));
        let origin = point;
        let relative_transform = GlobalTransform::from_translation((eye - origin) * stage::UNIT);
        let (relative, relative_scale) =
            npc_gauge_projection(&camera, &relative_transform, origin, point).unwrap();
        assert!((relative - full).length() < 0.001);
        assert_eq!(relative_scale, scale);
    }
    #[test]
    fn npc_gauge_owners_have_independent_camera_quads() {
        #[derive(Resource, Default)]
        struct Desired(HashMap<(u32, Entity), (Vec2, Vec2, f32)>);
        let mut app = App::new();
        app.init_resource::<Desired>();
        app.insert_resource(NpcGauges {
            templates: vec![NpcTemplate {
                image: Handle::default(),
                position: Vec2::ZERO,
                size: Vec2::new(100.0, 10.0),
                rect: Rect::new(0.0, 0.0, 100.0, 10.0),
                scale: Vec2::ONE,
                anchor: 0,
                priority: 0,
                crop: true,
                color: Color::WHITE,
            }],
            ..default()
        });
        app.add_systems(
            Update,
            |mut commands: Commands,
             desired: Res<Desired>,
             mut gauges: ResMut<NpcGauges>,
             mut quads: Query<
                (&NpcGaugeQuad, &mut Node, &mut ImageNode),
                (Without<Quad>, Without<Prompt>),
            >| {
                reconcile_npc_gauges(&mut commands, &mut gauges, &mut quads, &desired.0);
            },
        );
        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Desired>().0.extend([
            ((123, first), (Vec2::new(100.0, 50.0), Vec2::ONE, 40.0)),
            ((123, second), (Vec2::new(300.0, 80.0), Vec2::ONE, 40.0)),
        ]);
        app.update();
        app.update();
        let gauges = app.world().resource::<NpcGauges>();
        assert_eq!(gauges.owners.len(), 2);
        for (camera, left) in [(first, 100.0), (second, 300.0)] {
            let entity = gauges.owners[&(123, camera)][0];
            assert_eq!(app.world().get::<UiTargetCamera>(entity).unwrap().0, camera);
            assert_eq!(app.world().get::<Node>(entity).unwrap().left, Val::Px(left));
            assert_eq!(app.world().get::<Node>(entity).unwrap().width, Val::Px(40.0));
        }
        let retired = gauges.owners[&(123, first)][0];
        let retained = gauges.owners[&(123, second)][0];
        app.world_mut().resource_mut::<Desired>().0.remove(&(123, first));
        app.update();
        assert!(app.world().get_entity(retired).is_err());
        assert!(app.world().get_entity(retained).is_ok());
        app.world_mut().resource_mut::<Desired>().0.clear();
        app.update();
        assert!(app.world().get_entity(retained).is_err());
    }
    #[test]
    fn npc_gauge_retirement_is_disjoint_from_other_hud_queries() {
        let mut app = App::new();
        app.init_resource::<play::Hud>().init_resource::<NpcGauges>();
        app.add_systems(Update, update);
        let camera = app.world_mut().spawn_empty().id();
        let quad = app
            .world_mut()
            .spawn((
                Node::default(),
                ImageNode::default(),
                NpcGaugeQuad {
                    owner: 123,
                    camera,
                    template: 0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<NpcGauges>()
            .owners
            .insert((123, camera), vec![quad]);
        app.world_mut().spawn(Window::default());
        app.update();
        assert!(app.world().resource::<NpcGauges>().owners.is_empty());
        assert!(app.world().get_entity(quad).is_err());
        app.update(); // Idempotent once the actor/view is absent.
    }
    #[test]
    fn installed_npc_gauge_fill_preserves_background_and_left_anchor() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let templates = npc_templates(&vfs, &mut Assets::default()).unwrap();
        let fill = templates.iter().find(|t| t.crop).unwrap();
        let background = templates.iter().find(|t| !t.crop).unwrap();
        assert_eq!(fill.size, Vec2::new(88.0, 8.0));
        assert_eq!(fill.scale, Vec2::new(0.8, 1.0));
        assert!(fill.priority > background.priority);
        let point = Vec2::new(640.0, 360.0);
        let (left, full, full_rect) = npc_quad_layout(fill, point, Vec2::ONE, fill.size.x);
        let (half_left, half, half_rect) = npc_quad_layout(fill, point, Vec2::ONE, fill.size.x / 2.0);
        assert_eq!(left, half_left);
        assert_eq!(half, Vec2::new(full.x / 2.0, full.y));
        assert_eq!(half_rect.min, full_rect.min);
        assert_eq!(half_rect.width(), full_rect.width() / 2.0);
        assert_eq!(
            npc_quad_layout(background, point, Vec2::ONE, 3.0),
            npc_quad_layout(background, point, Vec2::ONE, fill.size.x)
        );
        let screen_scale = Vec2::new(0.5, 0.75);
        let (scaled_left, scaled_size, _) = npc_quad_layout(fill, point, screen_scale, fill.size.x / 2.0);
        assert_eq!(scaled_left, half_left * screen_scale);
        assert_eq!(scaled_size, half * screen_scale);
    }
    #[test]
    fn pawn_anchor_uses_authored_joint_and_camera_offset() {
        let point = pawn_anchor(Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 2.25, 10.0));
        assert!((point - Vec3::new(1.0, 2.25, 3.7)).length() < 1e-6);
    }
    #[test]
    #[ignore = "requires DDDA_NATIVE_PC"]
    fn human_joint_remap_matches_playback_id() {
        let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").unwrap()).unwrap();
        for path in [play::BODY, r"model\pl\f\f_base\f000\f000"] {
            let bytes = vfs.read(path, mtarc::type_hash("rModel")).unwrap();
            let model = mtmod::parse(&bytes).unwrap();
            assert_eq!(model.bones[model.bone_map.unwrap()[4] as usize].anim_map_index, 4);
        }
    }
    #[test]
    fn hp_digits_pack_left() {
        assert_eq!(number(455.9), "455");
        assert_eq!(number(0.1), "1");
        assert_eq!(number(0.0), "0");
        assert_eq!(number(10001.0), "1");
    }
}
