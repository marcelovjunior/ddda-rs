//! Synthetic fixtures only: no shipped bytes.
use ddparam::*;

fn fixture(root: &str, child: &str, fields: &[(&str, u8, &[u8])], null: bool) -> Vec<u8> {
    let mut defs = Vec::new();
    let root_at = 8u32;
    let child_at = root_at + 12 + 48;
    let names_at = child_at + 12 + 24 * fields.len() as u32;
    defs.extend(root_at.to_le_bytes());
    defs.extend(child_at.to_le_bytes());
    let mut names = Vec::new();
    fn prop(d: &mut Vec<u8>, names: &mut Vec<u8>, base: u32, name: &str, ty: u8, attr: u8, size: u16) {
        d.extend((base + names.len() as u32).to_le_bytes());
        d.extend([ty, attr]);
        d.extend(size.to_le_bytes());
        d.extend([0; 16]);
        names.extend(name.as_bytes());
        names.push(0);
    }
    defs.extend(mtarc::type_hash(root).to_le_bytes());
    defs.extend(0u32.to_le_bytes());
    defs.extend(2u32.to_le_bytes());
    prop(&mut defs, &mut names, names_at, "mQuality", 6, 0x91, 4);
    prop(&mut defs, &mut names, names_at, "mArray", 2, 0xa0, 4);
    defs.extend(mtarc::type_hash(child).to_le_bytes());
    defs.extend(0u32.to_le_bytes());
    defs.extend((fields.len() as u32).to_le_bytes());
    for &(name, ty, value) in fields {
        prop(&mut defs, &mut names, names_at, name, ty, 0, value.len() as u16);
    }
    defs.extend(names);
    while defs.len() % 4 != 0 {
        defs.push(0);
    }
    let mut child_body = Vec::new();
    for &(_, _, value) in fields {
        child_body.extend(1u32.to_le_bytes());
        child_body.extend(value);
    }
    let mut body = Vec::new();
    body.extend(1u32.to_le_bytes());
    body.extend(2u32.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    if null {
        body.extend([0xfe, 0xff, 0, 0]);
    } else {
        body.extend(3u16.to_le_bytes());
        body.extend(1u16.to_le_bytes());
        body.extend((4 + child_body.len() as u32).to_le_bytes());
        body.extend(child_body);
    }
    let mut out = b"XFS\0".to_vec();
    out.extend(0x109u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend((if null { 1u32 } else { 2u32 }).to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend((defs.len() as u32).to_le_bytes());
    out.extend(defs);
    out.extend(1u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend((4 + body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

#[test]
fn synthetic_stamina_preserves_order_and_floats() {
    // Fields intentionally differ from definition order used by the game.
    let b = fixture(
        "rPlStamina",
        "cPlStamina",
        &[
            ("mMotSpd", 12, &0.75f32.to_le_bytes()),
            ("mStamina", 12, &12.5f32.to_le_bytes()),
            ("mFrame", 12, &2f32.to_le_bytes()),
        ],
        false,
    );
    assert_eq!(
        Stamina::from_bytes(&b).unwrap().entries,
        [StaminaEntry {
            stamina: 12.5,
            frame: 2.,
            motion_speed: 0.75
        }]
    );
    for n in 0..b.len() {
        assert!(Stamina::from_bytes(&b[..n]).is_err(), "prefix {n}");
    }
}
#[test]
fn rejects_wrong_root_child_type_and_null() {
    let f = [("mWeight", 12, &3f32.to_le_bytes()[..])];
    assert!(Weight::from_bytes(&fixture("rPlStamina", "cPlWeight", &f, false)).is_err());
    assert!(Weight::from_bytes(&fixture("rPlWeight", "cPlStamina", &f, false)).is_err());
    assert!(Weight::from_bytes(&fixture(
        "rPlWeight",
        "cPlWeight",
        &[("mWeight", 6, &3u32.to_le_bytes())],
        false
    ))
    .is_err());
    assert!(Weight::from_bytes(&fixture("rPlWeight", "cPlWeight", &f, true)).is_err());
    assert!(Weight::from_bytes(&fixture(
        "rPlWeight",
        "cPlWeight",
        &[("wrong", 12, &3f32.to_le_bytes())],
        false
    ))
    .is_err());
    assert!(Parameter::from_bytes("unknown", &[]).is_err());
}
#[test]
fn missing_extra_and_duplicate_fields_rejected() {
    assert!(Weight::from_bytes(&fixture("rPlWeight", "cPlWeight", &[], false)).is_err());
    let field = ("mWeight", 12, &3f32.to_le_bytes()[..]);
    assert!(Weight::from_bytes(&fixture("rPlWeight", "cPlWeight", &[field, field], false)).is_err());
}
