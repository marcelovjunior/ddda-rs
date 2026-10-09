//! Installed-template consistency. No appearance records are embedded here.
use std::{cell::RefCell, path::PathBuf};

use ddlook::{Error, Source};

struct Observed<'a> {
    vfs: &'a ddvfs::Vfs,
    presets: RefCell<Vec<(String, String)>>,
}

impl Source for Observed<'_> {
    fn resource(&self, path: &str, class: &str) -> Result<Option<Vec<u8>>, Error> {
        let bytes = self.vfs.resource(path, class)?;
        if matches!(class, "rHumanPartsEdit" | "rFacePartsEdit") {
            assert!(bytes.is_some(), "missing requested preset: {path} ({class})");
            self.presets.borrow_mut().push((path.into(), class.into()));
        }
        Ok(bytes)
    }
}

#[test]
fn installed_templates_keep_record_sex_models_and_presets_together() {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !path.is_dir() {
        eprintln!("SKIP: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&path).unwrap();
    let bytes = vfs.resource(r"etc\editPattern\romPawnInfo", "rRomPawnInfo").unwrap().unwrap();
    let records = ddpawn::RomPawnInfo::parse(&bytes).unwrap();
    let file = ddexe::ExeFile::next_to(&path).unwrap();
    let body_tables = ddlook::body::Tables::read(&file.image().unwrap()).unwrap();
    let field = |record: &ddpawn::cmc::Cmc, name: &str| {
        let index = ddpawn::cmc::EDIT_U8_FIELDS.iter().position(|&n| n == name).unwrap();
        u32::from(record.edit.fields[index])
    };
    for sex in 0..=1 {
        let (index, record) = records.records.iter().enumerate().find(|(_, c)| field(c, "mGender") == sex).unwrap();
        let source = Observed { vfs: &vfs, presets: RefCell::new(Vec::new()) };
        let look = ddlook::save::template_look(&source, sex as u8).unwrap();
        assert_eq!(u32::from(look.sex), sex);
        // Base selection: 0x0083fa60. The base contains both head and body.
        let sex_path = if sex == 1 { "f" } else { "m" };
        assert_eq!(look.base, format!(r"model\pl\{sex_path}\{sex_path}_base\{sex_path}000\{sex_path}000"));
        for part in &look.parts {
            assert!(vfs.resource(&part.path, "rModel").unwrap().is_some(), "{}", part.path);
        }
        // Save conversion 0x00780fa0; property offsets independently
        // confirmed by cSAVE_DATA_EDIT::vf03 0x00787010. XML enumeration
        // order is NOT memory order: Body +0x3f, Head +0x40.
        let offset = if sex == 1 && field(record, "mHumanBodyAll") < 200 { 100 } else { 0 };
        let expected = [
            ("bodyall", "mHumanBodyAll"), ("body", "mHumanBody"),
            ("head", "mHumanHead"), ("arm", "mHumanArm"), ("leg", "mHumanLeg"),
        ];
        for (part, saved) in expected {
            let number = field(record, saved) + offset;
            assert!(source.presets.borrow().contains(&(format!(r"edit\parts\{part}\{part}_{number:03}"), "rHumanPartsEdit".into())));
        }
        assert_eq!(source.presets.borrow().len(), 11);
        let shape = look.body_shape.as_ref().unwrap();
        let joints = body_tables.joints(look.sex, shape).unwrap();
        let bytes = vfs.resource(&look.base, "rModel").unwrap().unwrap();
        let model = mtmod::parse(&bytes).unwrap();
        assert!(joints.iter().all(|(id, joint)| (0xaa..=0xce).contains(id)
            && joint.scale.iter().all(|n| n.is_finite())));
        assert!(look.face.iter().all(|f| !(0xaa..=0xce).contains(&f.id)));
        let mapped = joints.iter().filter(|(id, _)| model.bones.iter().any(|b| b.anim_map_index == *id)).count();
        assert!(mapped > 0, "shape joints must bind to the rendered base skeleton");
        eprintln!("body sliders: {}/{}/{}, {} changed joints, {mapped} mapped", shape.slender, shape.muscle, shape.bust, joints.len());
        eprintln!("sex {sex}, record {index}, base {}, parts {:?}, presets {:?}", look.base,
            look.parts.iter().map(|p| &p.path).collect::<Vec<_>>(), source.presets.borrow());
    }
}
