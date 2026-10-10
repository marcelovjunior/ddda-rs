//! Opt-in native directional caster depth, with the same finite plane on
//! CPU cascade lists and GPU shadow-view uniforms. Bevy's receiver projection
//! and shadow-map quality are unchanged; native projection fitting is separate.

use std::collections::HashMap;

use bevy::camera::primitives::{Aabb, CascadesFrusta, Frustum};
use bevy::camera::visibility::{CascadesVisibleEntities, NoFrustumCulling};
use bevy::light::{CascadeShadowConfig, Cascades, SimulationLightSystems};
use bevy::math::primitives::{HalfSpace, ViewFrustum};
use bevy::math::Vec3A;
use bevy::pbr::LightEntity;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderSystems};

type CascadeKey = (Entity, Entity, u32);

#[derive(Resource, Clone, Default, ExtractResource)]
struct FinitePlanes(HashMap<CascadeKey, HalfSpace>);

#[derive(Resource, Default)]
struct CasterSetting {
    stage_weather: Option<(u32, usize)>,
    extension_cm: Option<f32>,
}

impl CasterSetting {
    fn refresh(&mut self, stage: u32, weather: usize, load: impl FnOnce() -> Option<f32>) {
        if self.stage_weather == Some((stage, weather)) { return; }
        self.stage_weather = Some((stage, weather));
        self.extension_cm = load().filter(|v| v.is_finite() && *v >= 0.0);
    }
}

pub struct FiniteShadowPlugin;

impl Plugin for FiniteShadowPlugin {
    fn build(&self, app: &mut App) {
        // Experimental first: absence, empty strings and zero keep the
        // accepted renderer. No process-global mutation is needed in tests.
        if !enabled(std::env::var("DDVIEW_FINITE_SHADOW_EXTENT").ok().as_deref()) {
            return;
        }
        info!("finite shadow extent opt-in: installed stage SDL, CPU + GPU");
        app.init_resource::<FinitePlanes>()
            .init_resource::<CasterSetting>()
            .add_plugins(ExtractResourcePlugin::<FinitePlanes>::default())
            .add_systems(Update, sync_stage_setting
                .after(crate::opening::step).after(crate::stream::update))
            .add_systems(PostUpdate, filter_cpu_cascades
                .after(SimulationLightSystems::CheckLightVisibility));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            // prepare_lights inserts shadow views with an infinite near
            // halfspace. Its deferred insertions must precede this repair;
            // view uniform preparation must consume the repaired frustum.
            render_app.add_systems(Render, repair_gpu_frusta
                .in_set(RenderSystems::CreateViews)
                .after(bevy::pbr::prepare_lights)
                .before(bevy::render::view::prepare_view_uniforms));
        }
    }
}

fn enabled(value: Option<&str>) -> bool { value == Some("1") }

fn sync_stage_setting(args: Res<crate::Args>,
    source: Option<Res<crate::settings::RenderSource>>,
    streamer: Option<Res<crate::stream::Streamer>>,
    mut setting: ResMut<CasterSetting>) {
    let Some(source) = source else { return; };
    // The initial SkyShadowSetting describes Cassardis in opening mode.
    // A restored field checkpoint or JumpArea must use its CURRENT stage.
    let stage = streamer.as_ref().map_or_else(
        || if args.opening { crate::opening::STAGE } else { args.stage }, |s| s.stage);
    let weather = args.weather.unwrap_or(0);
    let changed = setting.stage_weather != Some((stage, weather));
    setting.refresh(stage, weather, || crate::sky::StageSky::load(source.vfs, stage, weather)
        .and_then(|sky| sky.shadow).and_then(|shadow| shadow.backforward_view_distance));
    if changed { info!(stage, weather, extension_cm=?setting.extension_cm, "finite caster extent stage SDL"); }
}

/// Near boundary of the native enclosing sphere plus the authored caster
/// extension. Corners are in CAMERA view space; choosing a world-space AABB
/// would change the radius whenever the camera turns.
// Translated from 0x00EE5CE0 (DDDA.exe 2017-12-13 Steam).
// This adapts its finite depth boundary to existing Bevy cascades only;
// native XY texel snapping and depth projection are not implemented here.
fn caster_plane(corners: [Vec3A; 8], world_from_camera: Mat4,
    toward_light: Vec3, extension: f32) -> Option<HalfSpace> {
    if !extension.is_finite() || extension < 0.0 || !world_from_camera.is_finite()
        || !toward_light.is_finite() || toward_light.length_squared() <= 0.0
        || corners.iter().any(|v| !v.is_finite()) { return None; }
    let mut min = corners[0];
    let mut max = corners[0];
    for corner in corners { min = min.min(corner); max = max.max(corner); }
    let center = (min + max) * 0.5;
    let radius = (max - center).length();
    let center_world = world_from_camera.transform_point3(center.into());
    let normal = -toward_light.normalize();
    let distance = -normal.dot(center_world) + radius + extension;
    distance.is_finite().then(|| HalfSpace::new(normal.extend(distance)))
}

fn with_plane(mut frustum: Frustum, plane: HalfSpace) -> Frustum {
    frustum.half_spaces[ViewFrustum::NEAR_PLANE_IDX] = plane;
    frustum
}

fn filter_cpu_cascades(
    setting: Res<CasterSetting>,
    mut planes: ResMut<FinitePlanes>,
    cameras: Query<(&Projection, &GlobalTransform)>,
    mut lights: Query<(Entity, &Cascades, &CascadeShadowConfig,
        &CascadesFrusta, &mut CascadesVisibleEntities), With<DirectionalLight>>,
    bounds: Query<(&Aabb, &GlobalTransform, Has<NoFrustumCulling>)>,
) {
    planes.0.clear();
    let Some(extension) = setting.extension_cm
        .map(|cm| cm * crate::stage::UNIT) else { return; };
    for (light, cascades, config, frusta, mut visible) in &mut lights {
        for (&camera, view_cascades) in &cascades.cascades {
            let Ok((projection, camera_transform)) = cameras.get(camera) else { continue; };
            let Some(view_frusta) = frusta.frusta.get(&camera) else { continue; };
            for (index, (cascade, frustum)) in view_cascades.iter().zip(view_frusta).enumerate() {
                let Some(&far) = config.bounds.get(index) else { continue; };
                let near = if index == 0 { config.minimum_distance }
                    else { config.bounds[index - 1] * (1.0 - config.overlap_proportion) };
                let corners = projection.get_frustum_corners(-near, -far);
                let Some(plane) = caster_plane(corners, camera_transform.to_matrix(),
                    cascade.world_from_cascade.z_axis.truncate(), extension) else { continue; };
                planes.0.insert((light, camera, index as u32), plane);
                let finite = with_plane(*frustum, plane);
                if let Some(list) = visible.entities.get_mut(&camera).and_then(|v| v.get_mut(index)) {
                    // Preserve Bevy's explicit opt-outs and missing-bound
                    // fallback. NoCpuCulling meshes use the GPU path below.
                    list.entities.retain(|&entity| bounds.get(entity).map_or(true,
                        |(aabb, transform, bypass)| bypass
                            || finite.intersects_obb(aabb, &transform.affine(), true, true)));
                }
            }
        }
    }
}

fn repair_gpu_frusta(planes: Res<FinitePlanes>,
    mut views: Query<(&LightEntity, &ExtractedView, &mut Frustum)>) {
    for (kind, view, mut frustum) in &mut views {
        if !matches!(kind, LightEntity::Directional { .. }) { continue; }
        let retained = view.retained_view_entity;
        let key = (retained.main_entity.into(), retained.auxiliary_entity.into(), retained.subview_index);
        if let Some(&plane) = planes.0.get(&key) { *frustum = with_plane(*frustum, plane); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::render::view::RetainedViewEntity;

    fn box_corners() -> [Vec3A; 8] {
        std::array::from_fn(|i| Vec3A::new(if i & 1 == 0 { -2.0 } else { 2.0 },
            if i & 2 == 0 { -3.0 } else { 3.0 }, if i & 4 == 0 { -4.0 } else { -6.0 }))
    }

    #[test]
    fn finite_shadow_sphere_boundary_tracks_camera_and_light_without_world_aabb() {
        let radius = 14.0f32.sqrt(); // synthetic view AABB half extents2,3,1
        let plane = caster_plane(box_corners(), Mat4::IDENTITY, Vec3::Z, 10.0).unwrap();
        assert_eq!(plane.normal_d(), Vec4::new(0.0, 0.0, -1.0, -5.0 + radius + 10.0));
        let camera = Mat4::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(30.0, 7.0, -11.0));
        let direction = camera.transform_vector3(Vec3::Z);
        let rotated = caster_plane(box_corners(), camera, direction, 10.0).unwrap();
        let boundary = camera.transform_point3(Vec3::new(0.0, 0.0, -5.0 + radius + 10.0));
        assert!(rotated.normal_d().dot(boundary.extend(1.0)).abs() < 0.00001);
        assert!(rotated.normal_d().dot((boundary - direction).extend(1.0)) > 0.0);
        assert!(rotated.normal_d().dot((boundary + direction).extend(1.0)) < 0.0);
        assert!(!enabled(None) && !enabled(Some("")) && !enabled(Some("0")) && enabled(Some("1")));
        assert!(caster_plane(box_corners(), Mat4::IDENTITY, Vec3::Z, f32::NAN).is_none());
        assert!(caster_plane(box_corners(), Mat4::IDENTITY, Vec3::ZERO, 10.0).is_none());
    }

    #[test]
    fn finite_shadow_near_plane_rejects_remote_caster_and_preserves_crossing_obb() {
        let plane = HalfSpace::new(Vec4::new(0.0, 0.0, -1.0, 10.0));
        let mut frustum = Frustum::default();
        // Synthetic unconstraining side/far planes; isolate near behavior.
        frustum.half_spaces = [HalfSpace::new(Vec4::new(1.0, 0.0, 0.0, 1000.0)); 6];
        let frustum = with_plane(frustum, plane);
        let bound = Aabb { center: Vec3A::ZERO, half_extents: Vec3A::ONE };
        let outside = GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 12.0));
        assert!(frustum.intersects_obb(&bound, &outside.affine(), false, true), "Bevy ignored near");
        assert!(!frustum.intersects_obb(&bound, &outside.affine(), true, true));
        let crossing = GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 10.5));
        assert!(frustum.intersects_obb(&bound, &crossing.affine(), true, true));
    }

    #[test]
    fn finite_shadow_gpu_repair_matches_light_camera_and_cascade_and_keeps_other_lights() {
        let mut world = World::new();
        let light = world.spawn_empty().id();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let near1 = HalfSpace::new(Vec4::new(0.0, 0.0, -1.0, 10.0));
        let near2 = HalfSpace::new(Vec4::new(-1.0, 0.0, 0.0, 20.0));
        world.insert_resource(FinitePlanes(HashMap::from([
            ((light, first, 0), near1), ((light, second, 0), near2),
        ])));
        let extracted = |camera: Entity, index: u32| ExtractedView {
            retained_view_entity: RetainedViewEntity::new(light.into(), Some(camera.into()), index),
            clip_from_view: Mat4::IDENTITY, world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: bevy::render::render_resource::TextureFormat::Depth32Float,
            viewport: UVec4::new(0, 0, 16, 16), color_grading: Default::default(),
            invert_culling: false,
        };
        let infinite = with_plane(Frustum::default(), HalfSpace::new(Vec4::new(0.0, 0.0, -1.0, f32::INFINITY)));
        let first_view = world.spawn((LightEntity::Directional { light_entity: light, cascade_index: 0 },
            extracted(first, 0), infinite)).id();
        let second_view = world.spawn((LightEntity::Directional { light_entity: light, cascade_index: 0 },
            extracted(second, 0), infinite)).id();
        let missing_cascade = world.spawn((LightEntity::Directional { light_entity: light, cascade_index: 1 },
            extracted(first, 1), infinite)).id();
        let point = world.spawn((LightEntity::Point { light_entity: light, face_index: 0 },
            extracted(first, 0), infinite)).id();
        world.run_system_once(repair_gpu_frusta).unwrap();
        let plane = |entity| world.get::<Frustum>(entity).unwrap().half_spaces[ViewFrustum::NEAR_PLANE_IDX];
        assert_eq!(plane(first_view).normal_d(), near1.normal_d());
        assert_eq!(plane(second_view).normal_d(), near2.normal_d());
        assert!(plane(missing_cascade).d().is_infinite());
        assert!(plane(point).d().is_infinite());
    }

    #[test]
    fn finite_shadow_cpu_filter_preserves_unknown_bounds_and_explicit_opt_out() {
        use bevy::camera::visibility::VisibleMeshEntities;
        let mut world = World::new();
        world.init_resource::<FinitePlanes>();
        world.insert_resource(CasterSetting { stage_weather: Some((200, 0)), extension_cm: Some(1000.0) });
        let projection = Projection::Perspective(PerspectiveProjection {
            near: 1.0, far: 5.0, ..Default::default()
        });
        let plane = caster_plane(projection.get_frustum_corners(-1.0, -5.0),
            Mat4::IDENTITY, Vec3::Z, 10.0).unwrap();
        let camera = world.spawn((projection, GlobalTransform::IDENTITY)).id();
        let bound = Aabb { center: Vec3A::ZERO, half_extents: Vec3A::ONE };
        let inside = world.spawn((bound, GlobalTransform::from_translation(Vec3::new(0.0, 0.0, -3.0)))).id();
        let outside = world.spawn((bound, GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 100.0)))).id();
        let crossing = world.spawn((bound, GlobalTransform::from_translation(Vec3::new(0.0, 0.0, plane.d() + 0.5)))).id();
        let bypass = world.spawn((bound, GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 100.0)), NoFrustumCulling)).id();
        let unknown = world.spawn_empty().id();
        let mut frustum = Frustum::default();
        frustum.half_spaces = [HalfSpace::new(Vec4::new(1.0, 0.0, 0.0, 1000.0)); 6];
        let mut cascades = Cascades::default();
        cascades.cascades.insert(camera, vec![bevy::light::Cascade {
            world_from_cascade: Mat4::IDENTITY, ..Default::default()
        }]);
        let mut frusta = CascadesFrusta::default();
        frusta.frusta.insert(camera, vec![frustum]);
        let mut visible = CascadesVisibleEntities::default();
        visible.entities.insert(camera, vec![VisibleMeshEntities {
            entities: vec![inside, outside, crossing, bypass, unknown],
        }]);
        let light = world.spawn((DirectionalLight::default(), cascades, frusta, visible,
            CascadeShadowConfig { bounds: vec![5.0], minimum_distance: 1.0, overlap_proportion: 0.0 })).id();
        world.run_system_once(filter_cpu_cascades).unwrap();
        let kept = &world.get::<CascadesVisibleEntities>(light).unwrap().entities[&camera][0].entities;
        assert_eq!(kept, &[inside, crossing, bypass, unknown]);
        assert_eq!(world.resource::<FinitePlanes>().0[&(light, camera, 0)].normal_d(), plane.normal_d());
        // No authored value restores conservative behavior and removes
        // previously extracted planes, without a constructor fallback.
        world.resource_mut::<CasterSetting>().extension_cm = None;
        world.run_system_once(filter_cpu_cascades).unwrap();
        assert!(world.resource::<FinitePlanes>().0.is_empty());
    }

    #[test]
    fn finite_shadow_stage_handoff_loads_once_and_drops_previous_stage_on_missing_data() {
        let mut setting = CasterSetting::default();
        setting.refresh(200, 0, || Some(1000.0)); // synthetic scheduler values
        assert_eq!(setting.extension_cm, Some(1000.0));
        setting.refresh(200, 0, || panic!("no repeated installed IO per frame"));
        setting.refresh(100, 0, || Some(2000.0));
        assert_eq!(setting.extension_cm, Some(2000.0));
        setting.refresh(801, 0, || None);
        assert_eq!(setting.extension_cm, None, "no stale prior-stage caster plane");
        setting.refresh(100, 1, || Some(f32::NAN));
        assert_eq!(setting.extension_cm, None);
    }
}
