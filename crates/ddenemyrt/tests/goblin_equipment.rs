//! Installed kind-5 catalog/model compatibility, not an original-game comparison.
use ddenemyrt::goblin::equipment::{Catalog, NativeRng, Placement};

#[test]
fn installed_body_variants_and_equipment_bind_to_real_model_groups_and_joints() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: DDDA install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let catalog = Catalog::read(&file.image().unwrap()).unwrap();
    let body_bytes = vfs.read(r"model\em\e01\e0100\e0100", mtarc::type_hash("rModel")).unwrap();
    let body = mtmod::parse(&body_bytes).unwrap();
    let admitted = |appearance: &ddenemyrt::goblin::equipment::Appearance| {
        body.meshes.iter().filter(|mesh| mesh.draw_mode & 1 != 0 && mesh.lod & 1 != 0
            && mesh.flags & 1 != 0 && appearance.visible(mesh.group)).count()
    };
    let all = body.meshes.iter().filter(|mesh| mesh.draw_mode & 1 != 0
        && mesh.lod & 1 != 0 && mesh.flags & 1 != 0).count();
    for equip_type in 0..8 {
        for leader in [false, true] {
            for roll in 0..10 {
                let appearance = catalog.resolve(Placement { equip_type, leader, ..Default::default() }, roll);
                assert!(admitted(&appearance) > 0 && admitted(&appearance) < all,
                    "native body mask must admit a proper subset, equip={equip_type} leader={leader} roll={roll}");
                for attachment in catalog.attachments(&appearance) {
                    assert!(body.bones.iter().any(|b| b.anim_map_index == attachment.joint));
                    let bytes = vfs.read(&attachment.model, mtarc::type_hash("rModel")).unwrap();
                    let model = mtmod::parse(&bytes).unwrap();
                    assert!(!model.meshes.is_empty() && !model.bones.is_empty());
                }
            }
        }
    }
    let first = catalog.choose(Placement::default(), &mut NativeRng::default());
    assert_eq!(first.unit_mask, 1);
    assert_eq!(catalog.attachments(&first).len(), 1);
}
