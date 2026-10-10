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

#[test]
fn action_base_enter_matches_the_original() {
    use ddnative::action;
    let Some(cases) = vectors("action_enter") else { return };
    for c in &cases {
        let mut o = action::Owner {
            category_206c: u(c, "cat"), word_2370: u(c, "w70"), word_2374: u(c, "w74"),
            flags_273c: u(c, "flags"), status_2e34: u(c, "status"), pending_2e38: u(c, "pending"),
        };
        let b50 = action::base_enter(&mut o);
        let out = &c["out"];
        assert_eq!(u32::from(b50), u(out, "b50"), "{c}");
        assert_eq!((o.word_2370, o.word_2374, o.flags_273c, o.status_2e34, o.pending_2e38),
            (u(out, "w70"), u(out, "w74"), u(out, "flags"), u(out, "status"), u(out, "pending")), "{c}");
    }
}

#[test]
fn demo_interface_state_matches_the_original() {
    use ddnative::action;
    let Some(cases) = vectors("demo_state") else { return };
    let words = |v: &Value| -> Vec<u32> { v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect() };
    for c in &cases {
        let input: [u32; 9] = words(&c["in"]).try_into().unwrap();
        let mut owner = action::DemoOwner {
            byte_0e: u(c, "b0e") as u8,
            work_40: (u(c, "work") != 0).then_some(input),
        };
        let ret = action::demo_state((u(c, "owner") != 0).then_some(&mut owner));
        assert_eq!(ret, u(c, "ret"), "{c}");
        // the block in memory: cleared only through the owner's pointer
        let block = if u(c, "owner") != 0 { owner.work_40.unwrap_or(input) } else { input };
        assert_eq!(block.to_vec(), words(&c["outwork"]), "{c}");
        assert_eq!(words(&c["guards"]), vec![0xA5A5_A5A5, 0x5A5A_5A5A], "{c}");
    }
}

#[test]
fn particle_counters_and_flag_match_the_original() {
    use ddnative::particle;
    let Some(cases) = vectors("particle_vf08_vf10") else { return };
    for c in &cases {
        let g0 = particle::Generator { flags_3c: u(c, "flags"), counter_44: 0x1111_1111, counter_48: 0x2222_2222, seed_4c: u(c, "seed") };
        let mut g = g0;
        particle::reset_counters(&mut g);
        assert_eq!((g.counter_44, g.counter_48, g.flags_3c), (u(c, "c44"), u(c, "c48"), g0.flags_3c), "{c}");
        let mut g = g0;
        particle::clear_flag(&mut g, u(c, "arg") as u8);
        assert_eq!(g.flags_3c, u(c, "flags10"), "{c}");
        assert_eq!(u(c, "esp10"), 4, "ret 4: one stack argument");
    }
}

#[test]
fn character_damage_edges_match_the_original() {
    use ddnative::character::{damage_edges, DamageEdges};
    let Some(cases) = vectors("character_vf111") else { return };
    for c in &cases {
        let mut e = DamageEdges {
            category_206d: u(c, "cat") as u8, action_2360: u(c, "a2360"), flags_273c: u(c, "f273c"),
            prev_2ea0: u(c, "prev") as u8, start_2ea1: 0xAA, end_2ea2: 0xBB, byte_2ea3: 0xCC,
        };
        damage_edges(&mut e, || u(c, "flag") as u8);
        let out: Vec<u8> = c["out"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
        assert_eq!(vec![e.prev_2ea0, e.start_2ea1, e.end_2ea2, e.byte_2ea3], out, "{c}");
    }
}

#[test]
fn npc_flag_location_matches_the_original() {
    use ddnative::character::{npc_flags, NpcFlags};
    let Some(cases) = vectors("npc_vf180") else { return };
    for c in &cases {
        let addr = match npc_flags(u(c, "intel")) {
            NpcFlags::Intel(a) => a,
            NpcFlags::Own => u(c, "this").wrapping_add(0x2738),
        };
        assert_eq!(addr, u(c, "ret"), "{c}");
    }
}

#[test]
fn player_predicates_and_table_match_the_original() {
    use ddnative::{action, character as ch};
    if let Some(cases) = vectors("player_vf18") {
        for c in &cases { assert_eq!(u32::from(ch::player_vf18(u(c, "state"))), u(c, "al"), "{c}"); }
    }
    if let Some(cases) = vectors("player_vf207") {
        for c in &cases { assert_eq!(u32::from(ch::player_vf207(u(c, "w"), u(c, "a"))), u(c, "al"), "{c}"); }
    }
    if let Some(cases) = vectors("player_vf187") {
        for c in &cases {
            let mut v = f32::from_bits(u(c, "val"));
            ch::player_vf187(u(c, "state"), &mut v);
            assert_eq!(v.to_bits(), u(c, "out"), "{c}");
            assert_eq!(u(c, "esp"), 4, "ret 4");
        }
    }
    if let Some(cases) = vectors("player_vf100") {
        for c in &cases {
            let (row, arg, want) = (u(c, "row"), u(c, "arg"), u(c, "table") as u8);
            let got = ch::player_vf100(row, arg, |i| { assert_eq!(i, (row << 8) + arg); want });
            assert_eq!(u32::from(got), u(c, "al"), "{c}");
        }
    }
    if let Some(cases) = vectors("action_vf08") {
        for c in &cases {
            let mut o = action::Owner { category_206c: u(c, "cat"), word_2370: 0, word_2374: 0,
                flags_273c: 0, status_2e34: 0xDEAD_BEEF, pending_2e38: 0 };
            action::base_exit_status(&mut o);
            assert_eq!(o.status_2e34, u(c, "status"), "{c}");
        }
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}
fn u32s(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()
}

#[test]
fn every_action_constructor_matches_the_original() {
    use ddnative::action;
    let Some(cases) = vectors("action_ctor_family") else { return };
    assert_eq!(cases.len(), 957);
    for c in &cases {
        let mut obj = hex(c["in"].as_str().unwrap());
        assert!(action::construct(Some(&mut obj), u(c, "vtable")));
        assert_eq!(obj, hex(c["out"].as_str().unwrap()), "constructor {:#010x}", u(c, "addr"));
        assert!(!action::construct(None, u(c, "vtable")));
    }
}

#[test]
fn player_joint3_position_matches_the_original() {
    use ddnative::character::player_joint3_position;
    let Some(cases) = vectors("player_vf85") else { return };
    for c in &cases {
        let row: [u32; 4] = u32s(&c["row"]).try_into().unwrap();
        let origin: [u32; 4] = u32s(&c["origin"]).try_into().unwrap();
        let got = player_joint3_position(u(c, "map3") as u8, u(c, "joints"),
            |_| row.map(f32::from_bits), origin.map(f32::from_bits));
        assert_eq!(got.map(f32::to_bits).to_vec(), u32s(&c["out"]), "{c}");
    }
}

#[test]
fn enemy_blend_matches_the_original() {
    use ddnative::enemy::{blend, BlendSource, BlendTarget};
    let Some(cases) = vectors("enemy_vf195") else { return };
    for c in &cases {
        let srcs: Vec<BlendSource> = c["srcs"].as_array().unwrap().iter().map(|s| BlendSource {
            values: u32s(&s["values"]).iter().map(|&b| f32::from_bits(b)).collect::<Vec<_>>().try_into().unwrap(),
            t: f32::from_bits(u(s, "t")),
        }).collect();
        let n = u(c, "n") as usize;
        let mut tgts = vec![BlendTarget { state_08: 0x7777_7777, value_114: 0.0, value_118: 0.0 }; n];
        let null_src = u(c, "null_src") != 0;
        let null_tgt = u(c, "null_tgt") != 0;
        blend((!null_src).then_some(&srcs[..]), (!null_tgt).then_some(&mut tgts[..]));
        for (i, want) in c["tgts"].as_array().unwrap().iter().enumerate() {
            let want = u32s(want);
            assert_eq!(tgts[i].state_08, want[0], "{c}");
            if want[0] == 4 {
                assert_eq!((tgts[i].value_114.to_bits(), tgts[i].value_118.to_bits()), (want[1], want[2]), "{c}");
            }
        }
    }
}

#[test]
fn every_extended_action_constructor_matches_the_original() {
    use ddnative::action;
    let Some(cases) = vectors("action_ctor_ext_family") else { return };
    assert!(cases.len() > 200);
    for c in &cases {
        let mut obj = hex(c["in"].as_str().unwrap());
        action::construct_extended(&mut obj, u(c, "vtable"));
        assert_eq!(obj, hex(c["out"].as_str().unwrap()), "constructor {:#010x}", u(c, "addr"));
    }
}
