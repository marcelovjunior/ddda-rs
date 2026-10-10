use ddenemyrt::{body::{model_matrix, Posed}, goblin::Target, npc_sensor::{Alert, DistanceHate, Sensor, SensorRow}};

struct Scenery(bool);
impl ddsim::Ground for Scenery {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
    fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<ddsim::RayHit> {
        self.0.then_some(ddsim::RayHit { point: std::array::from_fn(|i| (from[i] + to[i]) * 0.5), normal: [0.0, 0.0, 1.0] })
    }
}
fn target(pos: [f32; 3]) -> Target { Target { id: 17, pos, alive: true } }

#[test]
fn joint_vision_distinguishes_fov_los_group_and_radius_scale() {
    let sensor = Sensor(vec![SensorRow { class: 1, types: 0x16, enabled_by: 2,
        shape: 5, radius: 1500.0, fov: 60.0, flags: 9, joint: 3,
        offset: [0.0; 3], direction: [0.0, 0.0, 1.0] }]);
    // Authored joint faces+x; body root faces+z. A joint scale must not
    // widen range/FOV. This synthetic pose isolates the native matrix branch.
    let mut joint = model_matrix([0.0; 3], std::f32::consts::FRAC_PI_2);
    for row in joint.iter_mut().take(3) { for value in row { *value *= 7.0; } }
    let pose = Posed { model: model_matrix([0.0; 3], 0.0), joints: vec![(3, joint)] };
    let query = |mask, scale, pos, wall, subtype| sensor.query(mask, 1, 1, &pose, scale, target(pos), subtype, &Scenery(wall)).unwrap();
    for subtype in [1, 2, 4] {
        assert!(query(2, 1.0, [1000.0, 0.0, 0.0], false, subtype));
        assert!(!query(1, 1.0, [1000.0, 0.0, 0.0], false, subtype));
        assert!(!query(2, 1.0, [1000.0, 0.0, 0.0], true, subtype));
        assert!(!query(2, 1.0, [0.0, 0.0, 1000.0], false, subtype));
        assert!(!query(2, 1.0, [1600.0, 0.0, 0.0], false, subtype));
        assert!(query(2, 2.0, [1600.0, 0.0, 0.0], false, subtype));
    }
    assert!(!query(2, 1.0, [100.0, 0.0, 0.0], false, 3));
    assert!(!query(2, f32::NAN, [100.0, 0.0, 0.0], false, 1));
}

#[test]
fn alert_and_influence_forget_are_separate_from_target_fallback() {
    let mut alert = Alert::default();
    alert.update(0, None, [0.0; 3], 1.0);
    assert_eq!(alert.state, 0);
    alert.update(0, Some([1000.0, 0.0, 0.0]), [0.0; 3], 1.0);
    assert_eq!((alert.state, alert.search_left), (1, 900.0));
    alert.update(1, None, [0.0; 3], 899.0);
    assert_eq!(alert.state, 1);
    alert.update(1, None, [0.0; 3], 1.0);
    assert_eq!(alert.state, 0);
    alert.update(1, Some([100.0, 0.0, 0.0]), [0.0; 3], 1.0);
    assert_eq!(alert.state, 2);
    alert.update(2, None, [0.0; 3], 1.0);
    assert_eq!(alert.state, 0);
    assert_eq!([Alert::group(0), Alert::group(1), Alert::group(2)], [1, 2, 4]);
    let all = [target([2970.0, 0.0, 0.0])];
    let mut hate = DistanceHate::default();
    assert!(hate.select(&all, &all, [0.0; 3]).is_some());
    for _ in 0..3 { hate.tick(); }
    assert!(hate.select(&[], &all, [0.0; 3]).is_none());
}

#[test]
fn expired_native_hate_slot_is_reused_before_later_equal_scores() {
    let mut hate = DistanceHate::default();
    let first = Target { id: 1, pos: [2970.0, 0.0, 0.0], alive: true };
    let later = Target { id: 2, pos: [100.0, 0.0, 0.0], alive: true };
    hate.select(&[first, later], &[first, later], [0.0; 3]);
    for _ in 0..3 { hate.tick(); }
    let replacement = Target { id: 3, ..later };
    // Both contributions are identical. Native slot0 is vacant after the
    // short first influence expires, so its replacement wins the strict tie.
    assert_eq!(hate.select(&[later, replacement], &[later, replacement], [0.0; 3]).unwrap().id, 3);
    hate.retire(3);
    assert_eq!(hate.select(&[], &[later], [0.0; 3]).unwrap().id, 2);
}

#[test]
fn nearly_vertical_joint_uses_native_horizontal_forward_fallback() {
    let sensor = Sensor(vec![SensorRow { class: 1, types: 0x16, enabled_by: 2,
        shape: 5, radius: 1500.0, fov: 60.0, flags: 8, joint: 3,
        offset: [0.0; 3], direction: [0.0, 0.0, 1.0] }]);
    let mut joint = model_matrix([0.0; 3], 0.0);
    joint[2] = [1.0e-5, 1.0, 0.0, 0.0];
    let pose = Posed { model: model_matrix([0.0; 3], 0.0), joints: vec![(3, joint)] };
    assert!(sensor.query(2, 1, 1, &pose, 1.0, target([0.0, 0.0, 100.0]), 1, &Scenery(false)).unwrap());
    assert!(!sensor.query(2, 1, 1, &pose, 1.0, target([100.0, 0.0, 0.0]), 1, &Scenery(false)).unwrap());
}
