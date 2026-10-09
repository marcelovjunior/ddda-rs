use mtgui::Gui;
fn put(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn file(parts: &[(usize, Option<usize>, Vec<u8>, u32)]) -> Vec<u8> {
    let mut b = vec![0; 0x110];
    b[..4].copy_from_slice(b"GUI\0");
    put(&mut b, 4, 0x21209);
    // Empty tables point at EOF; pools are bounded by the next distinct offset.
    for (h, c, data, count) in parts {
        let off = b.len() as u32;
        put(&mut b, *h, off);
        if let Some(c) = c {
            put(&mut b, *c, *count);
        }
        b.extend(data);
    }
    let end = b.len() as u32;
    for h in (0x9c..=0x104).step_by(4).chain(std::iter::once(0x10c)) {
        if u32::from_le_bytes(b[h..h + 4].try_into().unwrap()) == 0 {
            put(&mut b, h, end);
        }
    }
    put(&mut b, 8, end);
    b
}
fn node(id: u32, sibling: u32, child: u32) -> Vec<u8> {
    words(&[id, 0, sibling, child, 0, 0x419b2827, 0, u32::MAX])
}
#[test]
fn header_and_table_bounds() {
    let b = file(&[]);
    assert!(Gui::parse(&b).is_ok());
    for size in 0..0x110 {
        assert!(Gui::parse(&b[..size]).is_err());
    }
    for (p, v) in [
        (0, 0),
        (4, 1),
        (8, 2),
        (0x9c, 1),
        (0x9c, u32::MAX),
        (0x28, u32::MAX),
    ] {
        let mut bad = b.clone();
        put(&mut bad, p, v);
        assert!(Gui::parse(&bad).is_err(), "{p:x}");
    }
}
#[test]
fn child_sibling_graph_and_invalid_indices() {
    let mut nodes = node(40, u32::MAX, 2);
    nodes.extend(node(80, u32::MAX, u32::MAX));
    nodes.extend(node(90, 1, u32::MAX));
    let b = file(&[(0xb4, Some(0x44), nodes, 3)]);
    let g = Gui::parse(&b).unwrap();
    assert_eq!(g.preorder().unwrap(), vec![(0, 0), (2, 1), (1, 1)]);
    for (offset, v) in [
        (0x110 + 12, 0),
        (0x110 + 12, 3),
        (0x110 + 12, u32::MAX),
        (0x110 + 32 + 8, 2),
        (0x24, u32::MAX),
    ] {
        let mut bad = b.clone();
        put(&mut bad, offset, v);
        assert!(Gui::parse(&bad).is_err());
    }
}
#[test]
fn resource_sentinel_strings_and_filter_texture() {
    let b = file(&[
        (0xe8, Some(0x78), words(&[17, 0, 0, 18, 0, u32::MAX]), 2),
        (0xe4, Some(0x74), words(&[0x77214222, 4, 0x241f5deb, 6]), 1),
        (0xf0, None, b"a\\msg\0a\\tex\0".to_vec(), 0),
    ]);
    let g = Gui::parse(&b).unwrap();
    assert_eq!(g.resources.len(), 3);
    assert_eq!(g.resources[0].path, Some("a\\msg"));
    assert_eq!(g.resources[1].path, None);
    assert_eq!(g.resources[2].path, Some("a\\tex"));
    assert_eq!(g.resources[2].source_table, 0xe4);
    let mut bad = b.clone();
    put(&mut bad, 0x110 + 8, u32::MAX - 1);
    assert!(Gui::parse(&bad).is_err());
    let mut bad = b.clone();
    *bad.last_mut().unwrap() = b'x';
    assert!(Gui::parse(&bad).is_err());
}
#[test]
fn local_animation_joins_by_animation_id_not_instance_id() {
    let b = file(&[
        (
            0xb4,
            Some(0x44),
            words(&[99, 0, u32::MAX, u32::MAX, 2, 0x1daa79db, 0, u32::MAX]),
            1,
        ),
        (0xac, Some(0x38), words(&[13, 0, 0, 11, 11, 4]), 2),
        (0xfc, None, words(&[u32::MAX, 7]), 0),
        (0x9c, Some(0x28), words(&[7, 0, 0, 0, 23, 0]), 1),
        (0xf0, None, b"ResourceId\0AnimationId\0Named\0".to_vec(), 0),
    ]);
    let g = Gui::parse(&b).unwrap();
    assert_eq!(g.local_animation(0).unwrap().name, "Named");
    assert_eq!(g.nodes[0].id, 99);
    let mut bad = b.clone();
    let offset = g.section(0xac).unwrap().offset;
    put(&mut bad, offset + 8, u32::MAX);
    assert!(Gui::parse(&bad).is_err());
}
#[test]
fn unknown_property_kind_is_preserved() {
    let b = file(&[
        (0xac, Some(0x38), words(&[0x876500fe, 0, 0xabcdef12]), 1),
        (0xf0, None, b"Mystery\0".to_vec(), 0),
    ]);
    let g = Gui::parse(&b).unwrap();
    let p = &g.properties[0];
    assert_eq!(p.kind, 254);
    assert_eq!(p.flags, 0x876500fe);
    assert_eq!(p.value_offset, 0xabcdef12);
    assert_eq!(p.word, None);
}
#[test]
fn typed_pools_and_unknown_filter() {
    let b = file(&[
        (0xac, Some(0x38), words(&[3, 0, 0, 6, 0, 0, 4, 0, 0]), 3),
        (0xf8, None, vec![1], 0),
        (0xfc, None, words(&[5]), 0),
        (0x100, None, vec![0; 16], 0),
        (0xf0, None, b"Name\0Value\0".to_vec(), 0),
    ]);
    let g = Gui::parse(&b).unwrap();
    assert_eq!(g.properties[0].word, Some(1));
    assert_eq!(g.properties[1].string, Some("Value"));
    let b = file(&[(0xe4, Some(0x74), words(&[0x12345678, 9]), 1)]);
    assert!(Gui::parse(&b).is_err());
}

#[test]
fn later_bad_pool_pointer_cannot_panic_earlier_slice() {
    let mut b = file(&[(0xf0, None, b"a\0".to_vec(), 0)]);
    put(&mut b, 0xfc, u32::MAX);
    assert!(Gui::parse(&b).is_err());
}
