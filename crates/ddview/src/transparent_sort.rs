//! Original transparent primitive ordering. Runtime center extraction is
//! deliberately separate from the integer key calculation.

use std::collections::HashMap;
use bevy::prelude::*;
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::render::render_phase::{sort_phase_system, ViewSortedRenderPhases};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::ExtractedView;

#[derive(Component)]
pub struct ModelSortCenter(pub Vec3);

#[derive(Component)]
pub struct PrimitiveSort {
    pub model: Entity,
    pub radius_cm: f32,
    pub priority: u8,
    /// Present for sort-bit primitives: first weight-bound's bone and center.
    pub bound: Option<(Entity, Vec3)>,
}

#[derive(Resource, Default)]
struct SortSamples(HashMap<MainEntity, (Vec3, f32, u8)>);

pub struct OriginalSortPlugin;
impl Plugin for OriginalSortPlugin {
    fn build(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return; };
        render.init_resource::<SortSamples>()
            .add_systems(ExtractSchedule, extract_sort_samples)
            .add_systems(Render, sort_primitives
                .in_set(RenderSystems::PhaseSort)
                .after(sort_phase_system::<Transparent3d>));
    }
}

fn extract_sort_samples(
    mut samples: ResMut<SortSamples>,
    primitives: Extract<Query<(Entity, &PrimitiveSort)>>,
    transforms: Extract<Query<&GlobalTransform>>,
    centers: Extract<Query<&ModelSortCenter>>,
) {
    samples.0.clear();
    for (entity, primitive) in &primitives {
        let Ok(model) = transforms.get(primitive.model) else { continue; };
        let (point, radius) = if let Some((bone, center)) = primitive.bound {
            let Ok(bone) = transforms.get(bone) else { continue; };
            (bone.transform_point(center), 0.0)
        } else {
            let Ok(center) = centers.get(primitive.model) else { continue; };
            // 0x00ea1f40 selects the outermost model's sphere center;
            // 0x00ea3d00 scales the current model's sphere radius.
            let scale = model.to_scale_rotation_translation().0.max_element();
            (model.transform_point(center.0), primitive.radius_cm * scale)
        };
        samples.0.insert(entity.into(), (point, radius, primitive.priority));
    }
}

fn sort_primitives(
    views: Query<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    samples: Res<SortSamples>,
) {
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else { continue; };
        let range = view.rangefinder3d();
        // Stable sorting retains existing order for equal original keys.
        // Use f64: the packed 27-bit key cannot preserve all priorities in f32.
        phase.items.sort_by(|_, a, _, b| {
            let key = |item: &Transparent3d| {
                if let Some(&(point, radius, priority)) = samples.0.get(&item.entity.1) {
                    // Unresolved: runtime +0x154 model sort-bias overrides are
                    // not imported; the viewer currently assumes zero.
                    let depth_cm = (-range.distance(&point) + radius) / super::UNIT;
                    f64::from(packed_key(depth_cm, priority)) / 4096.0 - 32767.0
                } else {
                    f64::from(item.distance) / f64::from(super::UNIT)
                }
            };
            key(a).total_cmp(&key(b))
        });
    }
}

/// `depth_cm` already includes the shared model sphere radius (unless the
/// primitive's sort bit selects its own bound) and the model's integer bias.
// Translated from 0x00ea5320 and 0x00ffc350 (DDDA.exe 2017-12-13 Steam).
fn packed_key(depth_cm: f32, alpha_priority: u8) -> u32 {
    let depth = (depth_cm as i32).clamp(0, 0x7fff) as u32;
    (0x7fff - depth) * 0x1000 + u32::from(alpha_priority)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_transparent_key_clamps_truncates_and_orders_priority() {
        assert_eq!(packed_key(-20.0, 4), 0x7fff * 4096 + 4);
        assert_eq!(packed_key(32768.0, 255), 255);
        assert_eq!(packed_key(321.99, 7), (32767 - 321) * 4096 + 7);
        assert!(packed_key(322.0, 255) < packed_key(321.0, 0));
        assert!(packed_key(321.0, 3) < packed_key(321.0, 4));
        // Packing into f32 loses priority at the far end of the key range.
        // A render integration must retain integer/f64 precision instead.
        assert_ne!(packed_key(0.0, 3), packed_key(0.0, 4));
        assert_eq!(packed_key(0.0, 3) as f32, packed_key(0.0, 4) as f32);
    }
}
