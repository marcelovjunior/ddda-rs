//! The push-out's move collides with the ground (0x0076D6B0 before the
//! scenery sweep): a push up a slope cannot bury the body.

use std::collections::HashMap;

use ddsim::{sweep::Face, Ground, Sim};

/// The slope y = 0.5 x (27 degrees), as triangles.
struct Slope;

impl Ground for Slope {
    fn height(&self, x: f32, _z: f32, below_y: f32) -> Option<f32> {
        let h = 0.5 * x;
        (h <= below_y).then_some(h)
    }
    fn triangles_in(&self, _lo: [f32; 3], _hi: [f32; 3], out: &mut Vec<Face>) -> bool {
        out.push(([[-1000.0, -500.0, -1000.0], [-1000.0, -500.0, 1000.0], [1000.0, 500.0, -1000.0]], 0));
        out.push(([[1000.0, 500.0, -1000.0], [-1000.0, -500.0, 1000.0], [1000.0, 500.0, 1000.0]], 0));
        true
    }
}

#[test]
fn a_push_up_a_slope_keeps_the_body_on_the_ground() {
    let mut sim = Sim::new(HashMap::new());
    sim.add_player(0, [0.0; 3], 0.0);
    for _ in 0..10 {
        sim.push_player(0, [6.0, 0.0], &Slope);
    }
    let p = sim.player(0).unwrap().position;
    assert!(p[0] > 20.0, "the push still moves the body uphill: {p:?}");
    assert!(p[1] >= 0.5 * p[0] - 1.0, "the body stays on the slope, not under it: {p:?}");
}

#[test]
fn a_push_on_a_ground_without_triangles_moves_x_and_z_only() {
    struct Flat;
    impl Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
    }
    let mut sim = Sim::new(HashMap::new());
    sim.add_player(0, [1.0, 2.0, 3.0], 0.0);
    sim.push_player(0, [4.0, -5.0], &Flat);
    assert_eq!(sim.player(0).unwrap().position, [5.0, 2.0, -2.0]);
}
