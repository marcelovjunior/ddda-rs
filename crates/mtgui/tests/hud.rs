#[test]
#[ignore = "requires DDDA_NATIVE_PC, reads player's install"]
fn cockpit_authored_geometry() {
    let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").unwrap()).unwrap();
    let bytes = vfs
        .read("id/DDN/cockpit/cockpit0_1", mtarc::type_hash("rGUI"))
        .unwrap();
    let gui = mtgui::Gui::parse(&bytes).unwrap();
    // Public section storage can be edited by callers; missing or truncated
    // key/value pools must return errors rather than panic.
    for header in [0xf0, 0xf4, 0xfc] {
        let mut damaged = gui.clone();
        damaged.sections.retain(|s| s.header_offset != header);
        assert!(damaged.animation_objects("life_counter").is_err());
    }
    let mut damaged = gui.clone();
    damaged
        .sections
        .iter_mut()
        .find(|s| s.header_offset == 0xf4)
        .unwrap()
        .bytes = &[];
    assert!(damaged.animation_objects("life_counter").is_err());
    let mut damaged = gui.clone();
    damaged.sections.retain(|s| s.header_offset != 0x100);
    assert!(damaged.animation_objects("PL_Hp_gauge").is_err());
    let textures = gui.texture_regions().unwrap();
    let counter = gui.animation_objects("life_counter").unwrap();
    let digits = counter.iter().find(|o| !o.texture_keys.is_empty()).unwrap();
    assert_eq!(
        digits.texture_keys.iter().map(|k| k.0).collect::<Vec<_>>(),
        (0..10).collect::<Vec<_>>()
    );
    assert!(digits.texture_keys.iter().all(|(_, id)| textures
        .iter()
        .any(|r| r.id == *id && r.path.ends_with("suji_ID"))));
    for a in &gui.animations {
        gui.animation_objects(a.name).unwrap();
    }
    for (name, position, size) in [
        ("PL_Hp_gauge", [243.0, 643.0], [152.0, 8.0]),
        ("PL_Ap_gauge", [243.0, 655.0], [152.0, 8.0]),
        ("CMC_gauge", [2.0, 0.0], [88.0, 8.0]),
    ] {
        let index = gui
            .nodes
            .iter()
            .enumerate()
            .find(|(i, _)| gui.local_animation(*i).is_some_and(|a| a.name == name))
            .unwrap()
            .0;
        assert_eq!(
            [
                f32::from_bits(gui.node_property(index, "PosX").unwrap().word.unwrap()),
                f32::from_bits(gui.node_property(index, "PosY").unwrap().word.unwrap())
            ],
            position
        );
        let objects = gui.animation_objects(name).unwrap();
        let object = objects.iter().find(|o| o.integer("Texture").is_some()).unwrap();
        let s = gui.vector(object.property("Size").unwrap()).unwrap();
        assert_eq!([s[0], s[1]], size);
        let region = textures
            .iter()
            .find(|r| Some(r.id) == object.integer("Texture"))
            .unwrap();
        assert!(region.rect[2] > region.rect[0] && region.rect[3] > region.rect[1]);
        assert!(vfs.locate(region.path, mtarc::type_hash("rTexture")).is_some());
    }
}

#[test]
#[ignore = "requires DDDA_NATIVE_PC, reads player's install"]
fn action_palette_control_and_keys() {
    let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").unwrap()).unwrap();
    let bytes = vfs.read("id/common/custskl0", mtarc::type_hash("rGUI")).unwrap();
    let gui = mtgui::Gui::parse(&bytes).unwrap();
    for a in &gui.animations {
        gui.animation_objects(a.name).unwrap();
    }
    let node = gui.nodes.iter().position(|n| n.id == 178).unwrap();
    let range = gui.control_range(node, 140 + 0x24).unwrap().unwrap(); // VK_HOME
    assert_eq!(range, [380.0, 385.0]);
    let objects = gui.animation_objects("360_buttons").unwrap();
    // Colour parent and textured child must keep disjoint track spans.
    assert!(objects[1].texture_keys.is_empty());
    assert!(objects[2]
        .texture_keys
        .iter()
        .any(|(frame, _)| *frame == range[0] as u32));
    assert!(objects[2]
        .vector_keys
        .iter()
        .any(|(name, frame, _)| *name == "Size" && *frame == 380));
}

#[test]
#[ignore = "requires DDDA_NATIVE_PC, reads player's install"]
fn font_glyphs_and_texture_pages() {
    let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").unwrap()).unwrap();
    let bytes = vfs
        .read("id/common/font/ddfont_jpn", mtarc::type_hash("rGUIFont"))
        .unwrap();
    let font = mtgui::font::Font::parse(&bytes).unwrap();
    assert_eq!(font.glyphs.len(), 4055);
    assert_eq!(font.nominal_size, 26);
    assert_eq!(font.pages, 4);
    for page in 0..font.pages {
        assert!(vfs
            .locate(
                &font.texture_path(page as u8).unwrap(),
                mtarc::type_hash("rTexture")
            )
            .is_some());
    }
    for text in ["Heavy Attack", "Light Attack", "Converse", "Jump"] {
        let line = font.line(text, [15.0, 17.0]).unwrap();
        assert_eq!(line.len(), text.chars().count());
        assert!(line.windows(2).all(|w| w[1].position[0] >= w[0].position[0]));
    }
}
