use ddedit::*;
use ddsave::xml::Document;

fn fixture() -> String {
    let fields = EDIT_U8_FIELDS
        .iter()
        .map(|f| format!("<u8 name=\"{f}\" value=\"0\"/>"))
        .collect::<String>();
    format!("<class type=\"cSAVE_DATA_EDIT\"><u32 name=\"mNickname\" value=\"4294967295\"/><array name=\"(u8*)mNameStr\" type=\"u8\" count=\"25\">{}</array>{fields}</class>", "<u8 value=\"0\"/>".repeat(25))
}

#[test]
fn strict_edit_shape_widths_and_missing_fields() {
    let xml = fixture();
    let read = |s: &str| read_edit(Document::parse(s).unwrap().root());
    let edit = read(&xml).unwrap();
    assert_eq!(edit.fields, [0; 41]);
    assert_eq!(edit.nickname, u32::MAX);
    for malformed in [
        xml.replace("cSAVE_DATA_EDIT", "other"),
        xml.replace("name=\"mVoice\" value=\"0\"", "name=\"mVoice\" value=\"256\""),
        xml.replace("<u8 name=\"mVoice\" value=\"0\"/>", ""),
        xml.replace("<u8 name=\"mVoice\"", "<u32 name=\"mVoice\""),
        xml.replace("</class>", "<u8 name=\"mVoice\" value=\"0\"/></class>"),
        xml.replace("count=\"25\"", "count=\"24\""),
        xml.replacen("<u8 value=\"0\"/>", "", 1),
        xml.replace("</array>", "<u8 value=\"0\"/></array>"),
    ] {
        assert!(read(&malformed).is_err());
    }
}

#[test]
fn preset_gender_boundary_and_missing_resource() {
    let xml = fixture();
    let mut e = read_edit(Document::parse(&xml).unwrap().root()).unwrap();
    assert_eq!(preset_offset(&e), 0);
    e.set("mGender", 1);
    e.set("mHumanBodyAll", 199);
    assert_eq!(preset_offset(&e), 100);
    e.set("mHumanBodyAll", 200);
    assert_eq!(preset_offset(&e), 0);
    struct Missing;
    impl ddlook::Source for Missing {
        fn resource(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, ddlook::Error> {
            Ok(None)
        }
    }
    assert!(body_presets(&e, &Missing).is_err());
}

#[test]
fn source_scale_order_and_body_boundaries() {
    assert_eq!(ground_scale(0.5, 2., 3.), Some(3.));
    // Passive original-process observation, 2026-10-09 15:19:56 UTC.
    // owner+64, active records indexed by controller+4e/+50, record+84.
    assert_eq!(
        ground_scale(0.859_359_4, 1.211_538_6, 0.813_630_5),
        Some(0.847_109)
    );
    assert!(ground_scale(f32::NAN, 1., 1.).is_none());
    // Synthetic thresholds, to ensure no dependency on hard-coded game values.
    let rows = [2., 5., 7., 9., 11.];
    for (w, class) in [(0., 0), (4.99, 0), (5., 1), (7., 2), (9., 3), (11., 4)] {
        assert_eq!(body_class(w, &rows), Some(class));
    }
}

#[test]
fn body_weight_resource_gates_and_truncation() {
    assert_eq!(body_weight(None, Some([f32::NAN; 2]), true, f32::NAN), Some(76.));
    assert_eq!(body_weight(Some([1.; 3]), None, true, f32::NAN), Some(76.));
    assert_eq!(body_weight(Some([1.; 3]), Some([0.; 2]), false, 100.), Some(76.));
    assert_eq!(body_weight(Some([1.; 3]), Some([0.; 2]), true, 10.), Some(92.));
    assert!(body_weight(Some([f32::INFINITY; 3]), None, false, 0.).is_none());
}

#[test]
fn private_save_edit_records_and_body_presets() {
    use std::path::PathBuf;
    let path = if let Some(p) = std::env::var_os("DDDA_SAVE") {
        let p = PathBuf::from(p);
        assert!(p.is_file(), "configured private copy absent");
        Some(p)
    } else {
        let mut files: Vec<_> = std::fs::read_dir("E:/ddda-re/save-backups")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path().join("367500/remote/DDDA.sav"))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        files.pop()
    };
    let Some(path) = path else {
        eprintln!("SKIP: no private save copy");
        return;
    };
    let save = ddsave::SaveFile::parse(&std::fs::read(path).unwrap()).unwrap();
    let doc = save.document().unwrap();
    let mut count = 0;
    for n in doc
        .iter()
        .filter(|n| n.ty().as_deref() == Some("cSAVE_DATA_EDIT") && n.tag() == "class")
    {
        read_edit(n).expect("strict saved edit");
        count += 1;
    }
    assert!(count >= 2);
    let player = doc
        .iter()
        .find(|n| n.name().as_deref() == Some("mPlayerDataManual"))
        .unwrap()
        .child("mPlCmcEditAndParam")
        .unwrap();
    let arisen = read_edit(player.path("mPl/mEdit").unwrap()).unwrap();
    let pawn = read_edit(
        player
            .child("mCmc")
            .unwrap()
            .children()
            .next()
            .unwrap()
            .child("mEdit")
            .unwrap(),
    )
    .unwrap();
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.is_dir() {
        assert!(explicit.is_none());
        eprintln!("validated {count} edit records; SKIP presets without install");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for (role, edit) in [("arisen", arisen), ("main pawn", pawn)] {
        let h = body_presets(&edit, &vfs).unwrap();
        let b = ddlook::morph::body_morph(&h, &|_| true);
        let a = b.joints.iter().find(|(id, _)| *id == 13).unwrap().1.scale[1];
        let c = b.joints.iter().find(|(id, _)| *id == 14).unwrap().1.scale[1];
        let scale = ground_scale(b.model_scale[1], a, c).unwrap();
        assert!(scale > 0.);
        eprintln!("{role} derived ground scale {scale:.9}; all five presets resolved");
    }
    eprintln!("strictly decoded {count} edit records; identity values withheld");
}
