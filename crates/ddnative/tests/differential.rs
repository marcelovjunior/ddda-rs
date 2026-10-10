//! Differential tests: replay vectors recorded from the ORIGINAL functions
//! (research/native/vectors_player_action.py, private output under
//! E:/ddda-re) through the Rust ports. Skips when the vectors are absent.

use ddnative::{model, player_action};
use serde_json::Value;

fn vectors(name: &str) -> Option<Vec<Value>> {
    let path = format!("E:/ddda-re/notes/native/vectors/{name}.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()?.as_array().cloned()
}

fn u(v: &Value, k: &str) -> u32 {
    v[k].as_u64().unwrap() as u32
}

struct Release(Vec<(u32, u32)>);
impl player_action::Effects for Release {
    fn release(&mut self, handle: u32, mode: u32) {
        self.0.push((handle, mode));
    }
}

#[test]
fn player_action_base_update_matches_the_original() {
    let Some(cases) = vectors("player_action_vf06") else { return };
    assert!(!cases.is_empty());
    for c in &cases {
        let mut p = player_action::Player {
            flags_273c: u(c, "flags"),
            value_2534: f32::from_bits(u(c, "v2534")),
            value_2654: f32::from_bits(u(c, "v2654")),
            dead_31ea: u(c, "dead") as u8,
            value_4b20: u(c, "v4b20"),
            window_4be1: u(c, "window") as u8,
            effect_4c18: u(c, "effect"),
        };
        let mut fx = Release(vec![]);
        let action60 = player_action::base_update(&mut p, &mut fx);
        let out = &c["out"];
        if let Some(handle) = out.get("release") {
            // the original reached 0x00427560 with this handle
            assert_eq!(fx.0, vec![(handle.as_u64().unwrap() as u32, 2)], "{c}");
            assert_eq!(p.effect_4c18, 0, "{c}");
        } else {
            assert!(fx.0.is_empty(), "{c}");
            assert_eq!(p.value_4b20, u(out, "v4b20"), "{c}");
            assert_eq!(u32::from(p.window_4be1), u(out, "window"), "{c}");
            assert_eq!(p.effect_4c18, u(out, "effect"), "{c}");
            assert_eq!(action60.to_bits(), u(out, "action60"), "{c}");
        }
    }
}

#[test]
fn model_world_origin_matches_the_original() {
    let Some(cases) = vectors("model_vf85") else { return };
    for c in &cases {
        let row: Vec<u32> = c["row"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
        let got = model::world_origin(&std::array::from_fn(|i| f32::from_bits(row[i])));
        let want: Vec<u32> = c["out"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
        assert_eq!(got.map(f32::to_bits).to_vec(), want, "{c}");
    }
}
