//! A look-at probe for reporting rendering problems (run with `--probe`,
//! which keeps the meshes the ray needs in main memory): F8 toggles a
//! crosshair, F9 casts a ray from the centre of the screen and logs every
//! surface it crosses near the first hit (name, distance, material
//! settings, drawn or hidden), passing through characters. Surfaces within a few millimetres of each other are what
//! z-fighting is made of, so the log names the culprits.

use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::stage::{DdMaterial, UNIT};

/// Hits farther than this past the first are not reported (viewer units:
/// 0.5 m).
const REPORT_DEPTH: f32 = 0.5;

#[derive(Component)]
pub struct Crosshair;

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    crosshairs: Query<Entity, With<Crosshair>>,
    mut ray_cast: MeshRayCast,
    (names, lods, visibility): (Query<&Name>, Query<&crate::event::Lod>, Query<&ViewVisibility>),
    skinned: Query<(), With<bevy::mesh::skinning::SkinnedMesh>>,
    parents: Query<&ChildOf>,
    dd: Query<&MeshMaterial3d<DdMaterial>>,
    standard: Query<&MeshMaterial3d<StandardMaterial>>,
    dd_assets: Res<Assets<DdMaterial>>,
    std_assets: Res<Assets<StandardMaterial>>,
    mut count: Local<u32>,
    time: Res<Time<Real>>,
    mut auto_done: Local<bool>,
) {
    // DDVIEW_PROBE_AUTO=<seconds>: one cast by itself (unattended checks)
    let auto = !*auto_done
        && std::env::var("DDVIEW_PROBE_AUTO").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| time.elapsed_secs() >= t);
    if auto {
        *auto_done = true;
    }
    if keys.just_pressed(KeyCode::F8) {
        if let Some(e) = crosshairs.iter().next() {
            commands.entity(e).despawn();
        } else {
            commands.spawn((
                Crosshair,
                Text::new("+"),
                TextColor(Color::srgb(1.0, 0.2, 0.2)),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    top: Val::Percent(50.0),
                    margin: UiRect { left: Val::Px(-5.0), top: Val::Px(-12.0), ..default() },
                    ..default()
                },
            ));
        }
    }
    if !keys.just_pressed(KeyCode::F9) && !auto {
        return;
    }
    let (Ok(window), Some((cam, at))) = (windows.single(), cams.iter().find(|(c, _)| c.is_active)) else { return };
    let centre = Vec2::new(window.width(), window.height()) * 0.5;
    let Ok(ray) = cam.viewport_to_world(at, centre) else { return };
    *count += 1;
    // characters (skinned) are passed through: the follow camera keeps the
    // Arisen in the middle of the screen
    let not_character = |e: Entity| !skinned.contains(e);
    let settings = MeshRayCastSettings::default()
        .with_visibility(RayCastVisibility::Any)
        .with_filter(&not_character)
        .never_early_exit();
    let hits = ray_cast.cast_ray(ray, &settings);
    let Some(first) = hits.first().map(|(_, h)| h.distance) else {
        info!("probe #{}: nothing under the crosshair (was ddview started with --probe?)", *count);
        return;
    };
    info!("probe #{}: ray from {:?} along {:?}; surfaces within {REPORT_DEPTH} m of the first:", *count, ray.origin, *ray.direction);
    for (e, h) in hits.iter().filter(|(_, h)| h.distance <= first + REPORT_DEPTH) {
        // the entity's own name, else its parent's
        let name = names
            .get(*e)
            .ok()
            .map(|n| n.as_str().to_string())
            .or_else(|| parents.get(*e).ok().and_then(|p| names.get(p.parent()).ok()).map(|n| n.as_str().to_string()))
            .unwrap_or_default();
        let material = if let Some(m) = dd.get(*e).ok().and_then(|h| dd_assets.get(&h.0)) {
            format!(
                "dd material: alpha {:?}, depth_bias {}, depth state {:?}, cull {:?}",
                m.base.alpha_mode, m.base.depth_bias, m.extension.depth, m.base.cull_mode
            )
        } else if let Some(m) = standard.get(*e).ok().and_then(|h| std_assets.get(&h.0)) {
            format!("standard material: alpha {:?}, depth_bias {}, cull {:?}", m.alpha_mode, m.depth_bias, m.cull_mode)
        } else {
            "no material".to_string()
        };
        let lod = lods.get(*e).map_or("lod all".to_string(), |l| format!("lod mask {:#x} (mid {} low {})", l.mask, l.mid, l.low));
        let shown = if visibility.get(*e).is_ok_and(|v| v.get()) { "drawn" } else { "HIDDEN" };
        info!(
            "  {:+.4} m  {shown} {name}  ({e:?}) {lod} at {:?} cm, normal {:?}, triangle {:?}; {material}",
            h.distance - first,
            h.point / UNIT,
            h.normal,
            h.triangle_index
        );
    }
}
