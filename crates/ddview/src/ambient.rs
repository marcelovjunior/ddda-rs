//! Ambient model presentation. Host owns admission, actor state and clocks.
use crate::{
    character::Playback,
    stage::{StageEntity, UNIT},
};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use ddhost::ambient::PlacementId;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct View {
    key: Option<(u32, Vec<(u16, u16)>)>,
    groups: Option<ddhost::ddspawn::gpl::GroupList>,
    entities: BTreeMap<PlacementId, (Entity, u64, u32)>,
    failure: Option<String>,
    admission: Option<(i32, u32, Vec<(u16, bool)>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    res: Option<Res<crate::opening::OpeningRes>>,
    stream: Option<ResMut<crate::stream::Streamer>>,
    state: Option<Res<crate::play::PlayState>>,
    args: Res<crate::Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut roots: Query<(&mut Playback, &mut Transform)>,
    mut view: Local<View>,
) {
    let (Some(res), Some(mut stream), Some(state)) = (res, stream, state) else {
        return;
    };
    // Actor snapshots are not in the guest protocol yet.
    if args.join.is_some() {
        return;
    }
    let mut session = res.0.lock().unwrap();
    let stage = stream.stage;
    let mut cells: Vec<_> = if stage == 100 {
        stream
            .loaded_layout_cells()
            .iter()
            .filter_map(|&(m, n)| Some((u16::try_from(m).ok()?, u16::try_from(n).ok()?)))
            .collect()
    } else {
        vec![(0, 0)]
    };
    cells.sort();
    cells.dedup();
    let key = (stage, cells);
    let result = (|| -> Result<(), String> {
        if view.key.as_ref() != Some(&key) {
            if let Some((old_stage, old_cells)) = &view.key {
                if *old_stage != stage {
                    session.host.ambient.retire_stage(*old_stage);
                }
                for &cell in old_cells {
                    if *old_stage != stage || !key.1.contains(&cell) {
                        session.host.ambient.retire_cell(*old_stage, cell);
                    }
                }
            }
            view.groups = Some(ddhost::spawns::group_list(stream.loader.vfs, stage)?);
            view.key = Some(key.clone());
            view.admission = None;
        }
        let groups = view.groups.as_ref().unwrap();
        let story = session
            .host
            .story
            .as_ref()
            .ok_or("ambient adapter requires story world")?;
        let (scene, hour, flag) = ddhost::spawns::world_inputs(&story.world, stage);
        let mut flags: Vec<_> = groups
            .groups
            .iter()
            .flat_map(|g| [g.layout_flag, g.layout_flag2, Some(g.layout_flags as u16)])
            .flatten()
            .map(|f| (f, flag(f)))
            .collect();
        flags.sort();
        flags.dedup();
        drop(flag);
        let admission = (scene, hour, flags);
        if view.admission.as_ref() != Some(&admission) {
            let flag = |f| {
                admission
                    .2
                    .binary_search_by_key(&f, |&(n, _)| n)
                    .ok()
                    .is_some_and(|i| admission.2[i].1)
            };
            let world = ddhost::ddspawn::World {
                scene,
                hour,
                flag: &flag,
            };
            session.host.ambient.remove_groups(stage, groups, &world);
            let file = ddexe::ExeFile::next_to(&stream.loader.vfs.root).map_err(|e| e.to_string())?;
            // This adapter serves opening's explicit fresh game. Saved enemy
            // kill records need the existing spawn_from_save path before reuse.
            let unsupported = session.host.ambient.admit_cells(
                stream.loader.vfs,
                &file.image().map_err(|e| e.to_string())?,
                stage,
                groups,
                &key.1,
                &world,
                &[],
            )?;
            if !unsupported.is_empty() {
                warn!(
                    "ambient: {} placements belong to other enemy runtimes",
                    unsupported.len()
                );
            }
            info!(
                "ambient: stage {stage}, {} canonical actors; unresolved controllers retain initial poses",
                session.host.ambient.iter().count()
            );
            view.admission = Some(admission);
        }
        let live: Vec<_> = session.host.ambient.iter().map(|a| (a.id, a.lifetime)).collect();
        view.entities.retain(|id, (entity, lifetime, _)| {
            if live.contains(&(*id, *lifetime)) {
                true
            } else {
                commands.entity(*entity).despawn();
                false
            }
        });
        for actor in session.host.ambient.iter().filter(|a| a.id.stage == stage) {
            // Generic enemy placement convention, matching ddhost::spawns;
            // full uCoord Euler-order equivalence is still unverified.
            let transform = Transform {
                translation: (Vec3::from_array(actor.position) - state.origin) * UNIT,
                rotation: Quat::from_euler(EulerRot::XYZ, actor.angle[0], actor.angle[1], actor.angle[2]),
                scale: Vec3::from_array(actor.scale) * UNIT,
            };
            let motion = actor.runtime.motion;
            if let Some((entity, _, last_motion)) = view.entities.get_mut(&actor.id) {
                if let Ok((mut playback, mut root)) = roots.get_mut(*entity) {
                    if *last_motion != motion {
                        playback.change_motion(
                            actor.data.motion(motion).ok_or("ambient motion missing")?.clone(),
                            5.0,
                        );
                        *last_motion = motion;
                    }
                    playback.hold_frame = Some(actor.runtime.frame);
                    *root = transform;
                    continue;
                }
            }
            let bank = actor
                .data
                .banks
                .get((motion >> 8) as usize)
                .ok_or("ambient motion bank missing")?;
            let entity = crate::character::spawn(
                &mut commands,
                &mut stream.loader,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut bindposes,
                &actor.data.model,
                &bank.path,
                (motion & 255) as usize,
                Vec3::ZERO,
                Some(actor.runtime.frame),
            )?;
            commands.entity(entity).insert((transform, StageEntity));
            view.entities.insert(actor.id, (entity, actor.lifetime, motion));
        }
        Ok(())
    })();
    if let Err(error) = result {
        if view.failure.as_ref() != Some(&error) {
            warn!("ambient: {error}");
            view.failure = Some(error);
        }
    } else {
        view.failure = None;
    }
}
