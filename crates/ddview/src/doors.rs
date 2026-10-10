//! Thin authored-door presentation; simulation and collision belong to Host.
use crate::{
    character::Playback,
    stage::{StageEntity, UNIT},
};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use dddoor::{
    placement::{self, Id},
    policy::Observed,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct View {
    key: Option<(u32, Vec<(u16, u16)>)>,
    layouts: Option<placement::LoadedLayouts>,
    enemies: Option<ddhost::doors::EnemyLayouts>,
    entities: BTreeMap<Id, (Entity, u64)>,
    admitted: Vec<Id>,
    failure: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    res: Option<Res<crate::opening::OpeningRes>>,
    stream: Option<ResMut<crate::stream::Streamer>>,
    state: Option<ResMut<crate::play::PlayState>>,
    args: Res<crate::Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut roots: Query<&mut Playback>,
    mut view: Local<View>,
) {
    let (Some(res), Some(mut stream), Some(mut state)) = (res, stream, state) else {
        return;
    };
    // Door snapshots are not in the LAN protocol yet. A guest must not
    // invent authority or silently simulate its own door decisions.
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
            let file = ddexe::ExeFile::next_to(&stream.loader.vfs.root).map_err(|e| e.to_string())?;
            let families = placement::Families::read(&file.image().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            view.layouts = Some(placement::LoadedLayouts::read(
                stream.loader.vfs,
                &families,
                stage,
                &key.1,
            )?);
            view.enemies = ddhost::doors::EnemyLayouts::read(stream.loader.vfs, stage, &key.1)
                .map_err(|e| warn!("door enemy coverage: {e}"))
                .ok();
            view.key = Some(key.clone());
        }
        let story = session
            .host
            .story
            .as_ref()
            .ok_or("door adapter needs story world")?;
        let (scene, hour, flag) = ddhost::spawns::world_inputs(&story.world, stage);
        let candidates = view
            .layouts
            .as_ref()
            .unwrap()
            .candidates(&ddhost::ddspawn::World {
                scene,
                hour,
                flag: &flag,
            });
        let ambient_standins = view.enemies.as_ref().and_then(|audit| {
            audit.spawn_standins(&ddhost::ddspawn::World {
                scene,
                hour,
                flag: &flag,
            })
        });
        drop(flag);
        let occupancy_complete = ambient_standins.is_some() && session.door_npc_coverage();
        let mut admitted: Vec<_> = candidates.doors.iter().map(|d| d.id).collect();
        admitted.sort();
        if session.host.doors.as_ref().is_none_or(|d| d.stage != stage) || admitted != view.admitted {
            let mut doors = ddhost::doors::Doors::load(stream.loader.vfs, stage, candidates.doors, |_| {
                Observed::fresh(hour)
            })?;
            // This adapter is only reached by opening's explicit new game.
            // Ordinary saved-game door persistence needs its own reader.
            doors.nearest_interaction_standin = true;
            if let Some(old) = &mut session.host.doors {
                if old.stage == stage {
                    for (&id, state) in &mut doors.states {
                        if let Some(previous) = old.states.remove(&id) {
                            *state = previous;
                            if let Some(entry) = old.entries.remove(&id) {
                                doors.entries.insert(id, entry);
                            }
                            doors.collision.enable(id, state.collision);
                        }
                    }
                }
            }
            for error in &doors.diagnostics {
                warn!("door: {error}");
            }
            info!("doors: stage {stage}, {} authored hinged objects; occupancy covered={occupancy_complete} (ambient spawn-position stand-in)",doors.states.len());
            session.host.install_doors(doors, Box::new(state.terrain.clone()));
            view.admitted = admitted;
        }
        let Some(doors) = &mut session.host.doors else {
            return Ok(());
        };
        doors.occupancy_complete = occupancy_complete;
        doors.ambient_spawn_standins = ambient_standins.unwrap_or_default();
        state.door_collision = doors.collision.clone();
        let removed: Vec<_> = view
            .entities
            .keys()
            .filter(|id| !doors.states.contains_key(id))
            .copied()
            .collect();
        for id in removed {
            if let Some((entity, _)) = view.entities.remove(&id) {
                commands.entity(entity).try_despawn();
            }
        }
        for (&id, door) in &doors.states {
            let entry = &doors.entries[&id];
            if let Some((entity, revision)) = view.entities.get_mut(&id) {
                if let Ok(mut playback) = roots.get_mut(*entity) {
                    if *revision != entry.revision {
                        playback
                            .change_motion(entry.motions.motions[door.motion].as_ref().unwrap().clone(), 5.0);
                        *revision = entry.revision;
                    }
                    playback.hold_frame = Some(entry.frame);
                    continue;
                }
            }
            let paths = placement::resources(door.object);
            let entity = crate::character::spawn(
                &mut commands,
                &mut stream.loader,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut bindposes,
                &paths.model,
                &paths.motion,
                door.motion,
                Vec3::ZERO,
                Some(entry.frame),
            )?;
            let mut transform = Transform::from_matrix(Mat4::from_cols_array(&entry.matrix));
            transform.translation = (transform.translation - state.origin) * UNIT;
            transform.scale *= UNIT;
            commands.entity(entity).insert((transform, StageEntity));
            view.entities.insert(id, (entity, entry.revision));
        }
        Ok(())
    })();
    if let Err(error) = result {
        if view.failure.as_ref() != Some(&error) {
            warn!("doors: {error}");
            view.failure = Some(error);
        }
    } else {
        view.failure = None;
    }
}

/// Static stage inspection: authored closed poses under the caller's
/// explicit preview scene/hour/flags. No player or simulation is created.
#[allow(clippy::too_many_arguments)]
pub fn preview(
    commands: &mut Commands,
    loader: &mut crate::stage::Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    stage: u32,
    cells: &[(u16, u16)],
    world: &ddhost::ddspawn::World<'_>,
    origin: Vec3,
) -> Result<(), String> {
    let exe = ddexe::ExeFile::next_to(&loader.vfs.root).map_err(|e| e.to_string())?;
    let families =
        placement::Families::read(&exe.image().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let layouts = placement::LoadedLayouts::read(loader.vfs, &families, stage, cells)?;
    let doors = ddhost::doors::Doors::load(loader.vfs, stage, layouts.candidates(world).doors, |_| {
        Observed::fresh(world.hour)
    })?;
    for error in doors.diagnostics {
        warn!("door preview: {error}");
    }
    for entry in doors.entries.values() {
        let paths = placement::resources(entry.placement.object);
        let entity = crate::character::spawn(
            commands,
            loader,
            meshes,
            materials,
            images,
            bindposes,
            &paths.model,
            &paths.motion,
            0,
            Vec3::ZERO,
            Some(0.0),
        )?;
        let mut transform = Transform::from_matrix(Mat4::from_cols_array(&entry.matrix));
        transform.translation = (transform.translation - origin) * UNIT;
        transform.scale *= UNIT;
        commands.entity(entity).insert((transform, StageEntity));
    }
    info!(
        "door preview: stage {stage}, {} authored closed objects",
        doors.entries.len()
    );
    Ok(())
}
