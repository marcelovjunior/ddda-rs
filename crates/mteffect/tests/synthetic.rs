use mteffect::{EffectList, Provider};
fn w(b: &mut [u8], p: usize, x: u32) {
    b[p..p + 4].copy_from_slice(&x.to_le_bytes());
}
fn provider() -> Vec<u8> {
    let mut b = b"epv\0".to_vec();
    b.extend([0; 4]);
    b.extend(1u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.extend(b"effect\\test\0");
    b.extend([0; 8]);
    for i in 0..69u32 {
        b.extend(i.to_le_bytes());
    }
    b.extend(1u32.to_le_bytes());
    for x in [
        0x729,
        255,
        1f32.to_bits(),
        20f32.to_bits(),
        4f32.to_bits(),
        1,
        0,
        0,
        0,
        0,
        0,
        0,
    ] {
        b.extend(x.to_le_bytes());
    }
    b.extend(1u32.to_le_bytes());
    for x in [7u32, 0, 0] {
        b.extend(x.to_le_bytes());
    }
    b
}
#[test]
fn provider_fields_and_links() {
    let p = Provider::parse(&provider()).unwrap();
    assert_eq!(p.indices[0][0].lists[0], "effect\\test");
    assert_eq!(p.indices[0][0].parameters[68], 68);
    assert_eq!(p.motions[0].motion, 0x729);
    assert_eq!(p.motions[0].start_frame, 1.);
    assert_eq!(p.motions[0].end_frame, 20.);
    assert_eq!(p.events[0].event, 7);
    assert!(p.element(0, 0).is_some());
    assert!(p.element(0, 1).is_none());
}
#[test]
fn provider_rejects_every_truncation() {
    let b = provider();
    for n in 0..b.len() {
        assert!(Provider::parse(&b[..n]).is_err(), "length {n}");
    }
}
#[test]
fn provider_bad_header_count_and_trailing() {
    let mut b = provider();
    w(&mut b, 8, u32::MAX);
    assert!(Provider::parse(&b).is_err());
    let mut b = provider();
    w(&mut b, 4, 22);
    assert!(Provider::parse(&b).is_err());
    let mut b = provider();
    b.push(0);
    assert!(Provider::parse(&b).is_err());
    let mut b = provider();
    let event_element = b.len() - 4;
    w(&mut b, event_element, 1);
    assert!(Provider::parse(&b).is_err());
    let mut b = provider();
    let motion_element = b.len() - 20;
    w(&mut b, motion_element, 1);
    assert!(Provider::parse(&b).is_err());
}
fn effect() -> Vec<u8> {
    let mut b = vec![0; 48 + 16 + 0x170];
    b[..4].copy_from_slice(b"EFL\0");
    w(&mut b, 4, 0x20110318);
    let size = b.len() - 48;
    w(&mut b, 8, size as u32);
    b[16] = 1;
    w(&mut b, 48 + 4, (16 << 8) | 1);
    b[48 + 16 + 0x70..48 + 16 + 0x70 + 9].copy_from_slice(b"tex\\test\0");
    b[48 + 16 + 0x130..48 + 16 + 0x130 + 9].copy_from_slice(b"anim\\one\0");
    b
}
#[test]
fn effect_packed_offset_and_resource_types() {
    let e = EffectList::parse(&effect()).unwrap();
    assert_eq!(e.units[0].offsets[1], 16);
    assert_eq!(e.resources.len(), 2);
    assert_eq!(e.resources[0].class, "rTexture");
    assert_eq!(e.resources[0].path, "tex\\test");
    assert_eq!(e.resources[1].class, "rEffectAnim");
}
#[test]
fn effect_rejects_bad_sizes_offsets_strings() {
    let mut b = effect();
    w(&mut b, 8, u32::MAX);
    assert!(EffectList::parse(&b).is_err());
    let mut b = effect();
    w(&mut b, 52, 0xffffff01);
    assert!(EffectList::parse(&b).is_err());
    let mut b = effect();
    b[48 + 16 + 0x70..48 + 16 + 0xb0].fill(b'a');
    assert!(EffectList::parse(&b).is_err());
    let b = effect();
    for n in 0..b.len() {
        assert!(EffectList::parse(&b[..n]).is_err());
    }
}
#[test]
fn effect_generator_life_and_nested_list_offsets() {
    let mut b = vec![0; 48 + 0x200];
    b[..4].copy_from_slice(b"EFL\0");
    w(&mut b, 4, 0x20110318);
    w(&mut b, 8, 0x200);
    b[16] = 1;
    w(&mut b, 48, 16 << 8);
    w(&mut b, 60, 0x100 << 8);
    b[48 + 16 + 0xb8..48 + 16 + 0xba].copy_from_slice(&0xc0u16.to_le_bytes());
    b[48 + 16 + 0xc0..48 + 16 + 0xc5].copy_from_slice(b"list\0");
    b[48 + 0x106..48 + 0x108].copy_from_slice(&8u16.to_le_bytes());
    b[48 + 0x108 + 0x1c..48 + 0x108 + 0x1e].copy_from_slice(&32u16.to_le_bytes());
    b[48 + 0x128..48 + 0x12d].copy_from_slice(b"next\0");
    let e = EffectList::parse(&b).unwrap();
    assert_eq!(e.resources[0].class, "rEffectStrip");
    assert_eq!(e.resources[1].class, "rEffectList");
    assert_eq!(e.resources[1].path, "next");
    b[48 + 16 + 0xb8..48 + 16 + 0xba].copy_from_slice(&0xffffu16.to_le_bytes());
    assert!(EffectList::parse(&b).is_err());
}
