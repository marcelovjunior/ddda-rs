//! Present the host's resolved kind-5 equipment and body mask.
use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};
use ddhost::ddenemyrt::goblin::{equipment::Appearance, GoblinData};
use crate::{character::{ExtraPart, Playback}, stage::Loader};

/// Explicit render-only comparison fixture from an authored, disabled
/// roadside layout. This never opens gates or enters the host's fight.
pub(crate) fn comparison_record(vfs: &ddvfs::Vfs) -> Result<mtlot::Record, String> {
    let path = ddhost::ddspawn::layout_path(100,64,53,'e',24);
    let bytes = vfs.read(&path, mtarc::type_hash("rLayout"))?;
    mtlot::parse(&bytes).map_err(|error| error.to_string())?.into_iter()
        .find(|record| record.class == "cSetInfoEnemy0100"
            && ddhost::spawns::placement_equipment(record).equip_type > 0)
        .ok_or_else(|| format!("{path}: no higher-equipment goblin comparison record"))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    commands: &mut Commands, loader: &mut Loader,
    meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>, bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    data: &GoblinData, appearance: &Appearance, translation: Vec3,
) -> Result<Entity, String> {
    let units = data.equipment.attachments(appearance);
    let parts: Vec<_> = units.iter().map(|unit| ExtraPart {
        path: unit.model.clone(),
        material_path: (unit.model == data.equipment.extra).then(|| data.equipment.extra_material.clone()),
        tint: None,
        attach: Some((unit.joint, unit.rotation_degrees, unit.position)),
    }).collect();
    let entity = crate::character::spawn_assembled(
        commands, loader, meshes, materials, images, bindposes, &data.model,
        None, None, &[], &parts, &|group| appearance.visible(group),
        ddhost::ddenemyrt::goblin::BANKS[0].1, 0, translation, Some(0.0),
    )?;
    // Equipment uses uCoord::vf21 0x00DEBBA0, not human uCnsAttach's
    // normalized joint axes. Resolve this before the first animate pass.
    commands.entity(entity).queue(|mut root: EntityWorldMut| {
        if let Some(mut playback) = root.get_mut::<Playback>() {
            if let Some(morph) = &mut playback.morph {
                for attachment in &mut morph.attach { attachment.preserve_joint_scale = true; }
            }
        }
    });
    info!("goblin body owner equipment={} effective={} leader={} unit_mask={:#x} attachments={}",
        appearance.placement.equip_type, appearance.effective_type, appearance.placement.leader,
        appearance.unit_mask, parts.len());
    Ok(entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;
    use ddhost::ddenemyrt::goblin::equipment::Placement;

    #[test]
    fn installed_equipped_body_uses_native_groups_units_and_replacement_material() {
        let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() { eprintln!("SKIP: DDDA install absent"); return; }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let file = ddexe::ExeFile::next_to(&native).unwrap();
        let data = GoblinData::load(&vfs, &file.image().unwrap()).unwrap();
        let comparison = comparison_record(&vfs).unwrap();
        assert_eq!(ddhost::spawns::placement_equipment(&comparison).equip_type, 4);
        let mfx_bytes = std::fs::read(native.join("usershader/BbsShaderPackage.mfx")).unwrap();
        let mfx = mtshader::ShaderPackage::parse(&mfx_bytes).unwrap();
        let mut loader = Loader { vfs: &vfs, mfx: &mfx, textures: Default::default(),
            models: Default::default(), model_names: Default::default(), water: None,
            stats: Default::default(), lod_eye: None, view_range: 1, tag: None };
        let mut meshes = Assets::default();
        let mut materials = Assets::default();
        let mut images = Assets::default();
        let mut bindposes = Assets::default();
        let mut world = World::new();
        let mut state = SystemState::<Commands>::new(&mut world);
        for equip_type in [0,5] {
            let appearance = data.equipment.resolve(Placement { equip_type, ..Default::default() }, 0);
            let expected_units = data.equipment.attachments(&appearance);
            let mut expected_meshes = 0;
            for (model_path, base) in std::iter::once((&data.model, true))
                .chain(expected_units.iter().map(|unit| (&unit.model, false))) {
                let bytes = vfs.read(model_path, crate::stage::RMODEL).unwrap();
                let model = mtmod::parse(&bytes).unwrap();
                expected_meshes += model.meshes.iter().filter(|mesh| mesh.draw_mode & 1 != 0
                    && mesh.flags & 1 != 0 && mesh.lod & 1 != 0
                    && (!base || appearance.visible(mesh.group))).count();
            }
            if equip_type == 5 {
                let bytes = vfs.read(&data.equipment.extra_material, crate::stage::RMATERIAL).unwrap();
                let replacement = mtmrl::parse(&bytes).unwrap();
                let bytes = vfs.read(&data.equipment.extra, crate::stage::RMODEL).unwrap();
                let extra = mtmod::parse(&bytes).unwrap();
                for name in &extra.material_names { assert!(replacement.by_name(name).is_some()); }
            }
            let entity = spawn(&mut state.get_mut(&mut world).unwrap(), &mut loader, &mut meshes,
                &mut materials, &mut images, &mut bindposes, &data, &appearance, Vec3::ZERO).unwrap();
            state.apply(&mut world);
            let playback = world.get::<Playback>(entity).unwrap();
            let attachments = &playback.morph.as_ref().unwrap().attach;
            assert_eq!(attachments.len(), expected_units.len());
            assert!(attachments.iter().all(|unit| unit.preserve_joint_scale));
            let count = world.query::<(&Mesh3d,&ChildOf)>().iter(&world)
                .filter(|(_, parent)| parent.parent() == entity).count();
            assert_eq!(count, expected_meshes);
            world.despawn(entity);
        }
    }
}
