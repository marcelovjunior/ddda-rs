use mtcamera::CameraList;

fn put(b: &mut [u8], offset: usize, value: u32) {
    b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    // Two slots (one null), two frames, four separate arrays.
    let mut b = vec![0; 148];
    b[..8].copy_from_slice(b"LCM\0\x03\0\x02\0");
    put(&mut b, 8, 16);
    for (i, v) in [11, 22, 33, 44, 2, 1, 55, 60, 84, 108, 140]
        .into_iter()
        .enumerate()
    {
        put(&mut b, 16 + 4 * i, v);
    }
    for (i, v) in [
        0.0f32, 1.0, 2.0, 10.0, 11.0, 12.0, 0.0, 1.0, -8.0, 10.0, 11.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0,
        0.0, 0.0, 40.0, 80.0,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut b, 60 + i * 4, v.to_bits());
    }
    b
}

#[test]
fn preserves_slots_metadata_and_arrays() {
    let b = fixture();
    let list = CameraList::parse(&b).unwrap();
    assert_eq!(list.cameras.len(), 2);
    assert!(list.cameras[1].is_none());
    let camera = list.cameras[0].as_ref().unwrap();
    assert_eq!(camera.user_data, [11, 22, 33, 44]);
    assert_eq!(camera.opaque_18, 55);
    assert_eq!(camera.fov_type, 1);
    assert_eq!(camera.frames(), 2);
    let f = camera.frame(1).unwrap();
    assert_eq!(f.position, [10.0, 11.0, 12.0]);
    assert_eq!(f.target, [10.0, 11.0, 2.0]);
    assert_eq!(f.rotation, [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(f.fov_degrees, 80.0);
    assert!(camera.frame(2).is_none());
}

#[test]
fn rejects_truncation_version_and_bad_spans() {
    let b = fixture();
    for end in 0..b.len() {
        assert!(CameraList::parse(&b[..end]).is_err(), "length {end}");
    }
    for offset in [0, 4, 8, 32, 44, 48, 52, 56] {
        let mut bad = b.clone();
        put(&mut bad, offset, u32::MAX);
        assert!(CameraList::parse(&bad).is_err(), "offset {offset}");
    }
    // Exact EOF for empty arrays is legal; an offset past EOF is not.
    let mut empty = b;
    put(&mut empty, 32, 0);
    for o in [44, 48, 52, 56] {
        put(&mut empty, o, 148);
    }
    let list = CameraList::parse(&empty).unwrap();
    assert!(list.cameras[0].as_ref().unwrap().sample(0.0, false).is_none());
    put(&mut empty, 44, 149);
    assert!(CameraList::parse(&empty).is_err());
}

#[test]
fn sampling_boundaries_repeat_and_fractional_frame() {
    let b = fixture();
    let list = CameraList::parse(&b).unwrap();
    let c = list.cameras[0].as_ref().unwrap();
    assert_eq!(c.sample(-5.0, false), c.frame(0));
    assert_eq!(c.sample(999.0, false), c.frame(1));
    assert_eq!(c.sample(1.0, true), c.frame(1));
    for f in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(c.sample(f, false).is_none());
    }
    assert!(c.sample(2.0, true).is_none());
    let midpoint = c.sample(0.5, false).unwrap();
    assert_eq!(midpoint.position, [5.0, 6.0, 7.0]);
    assert_eq!(midpoint.target, [5.0, 6.0, -3.0]);
    assert_eq!(midpoint.fov_degrees, 60.0);
    assert!((midpoint.rotation[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((midpoint.rotation[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert_eq!(c.sample(1.5, true).unwrap().position, midpoint.position);
    // 0.75 must use the first row with t=.75, not round to row 1.
    assert_eq!(c.sample(0.75, false).unwrap().fov_degrees, 70.0);
}

#[test]
fn shortest_arc_negates_first_quaternion_and_near_equal_is_linear() {
    let mut b = fixture();
    put(&mut b, 128, 0);
    put(&mut b, 136, (-1.0f32).to_bits());
    let list = CameraList::parse(&b).unwrap();
    assert_eq!(
        list.cameras[0]
            .as_ref()
            .unwrap()
            .sample(0.5, false)
            .unwrap()
            .rotation,
        [0.0, 0.0, 0.0, -1.0]
    );
    put(&mut b, 136, 1.0f32.to_bits());
    let list = CameraList::parse(&b).unwrap();
    assert_eq!(
        list.cameras[0]
            .as_ref()
            .unwrap()
            .sample(0.25, false)
            .unwrap()
            .rotation,
        [0.0, 0.0, 0.0, 1.0]
    );
}

#[test]
fn pose_uses_rotation_not_stored_target_direction() {
    let b = fixture();
    let list = CameraList::parse(&b).unwrap();
    let c = list.cameras[0].as_ref().unwrap();
    let pose = c.frame(0).unwrap().pose();
    assert_eq!(pose.target, [0.0, 1.0, -8.0]);
    assert_eq!(pose.up, [0.0, 1.0, 0.0]);
    let pose = c.frame(1).unwrap().pose();
    assert_eq!(pose.position, [10.0, 11.0, 12.0]);
    assert_eq!(pose.target, [10.0, 11.0, 22.0]);
    let pose = c.sample(0.5, false).unwrap().pose();
    assert!((pose.target[0] + 5.0).abs() < 1e-5);
    assert!((pose.target[2] - 7.0).abs() < 1e-5);
}
