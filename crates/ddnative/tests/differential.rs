//! Differential tests: replay vectors recorded from the ORIGINAL functions
//! (research/native/vectors_player_action.py, private output under
//! E:/ddda-re) through the Rust ports. Skips when the vectors are absent.

use ddnative::{model, player_action};
use serde_json::Value;

/// The vectors recorded from the original. Missing vectors skip the test,
/// except under DDNATIVE_REQUIRE_VECTORS=1 (the commit gate), where they
/// fail it: a port is never "verified" by a test that did not run.
fn vectors(name: &str) -> Option<Vec<Value>> {
    let path = format!("E:/ddda-re/notes/native/vectors/{name}.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return None,
    };
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

// verifies 0x00AC5100
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

// verifies 0x004C6B50
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

// verifies 0x007F1890
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

// verifies 0x00572860
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

// verifies 0x010FA570 0x010FA5B0
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

// verifies 0x00850100
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

// verifies 0x00BB4730
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

// verifies 0x00B88AB0 0x00B53190 0x00B6AE40 0x00B531B0 0x0089EFE0
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

// verifies 0x008C7780
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

// verifies 0x00B84300
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

// verifies 0x00AAEF50
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

// verifies 0x004CBBE0
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

// verifies 0x004DFA00
#[test]
fn every_action_constructor_with_8c_matches_the_original() {
    use ddnative::action;
    let Some(cases) = vectors("action_ctor_8c_family") else { return };
    for c in &cases {
        let mut obj = hex(c["in"].as_str().unwrap());
        assert!(action::construct_with_8c(Some(&mut obj), u(c, "vtable")));
        assert_eq!(obj, hex(c["out"].as_str().unwrap()), "constructor {:#010x}", u(c, "addr"));
    }
}

// verifies 0x004F57E0
#[test]
fn every_drop_dead_refs_function_matches_the_original() {
    use ddnative::unit::drop_dead_refs;
    let Some(cases) = vectors("drop_dead_refs") else { return };
    for c in &cases {
        let mut refs = u32s(&c["refs"]);
        let states = u32s(&c["states"]);
        let lookup: Vec<(u32, u32)> = refs.iter().copied().zip(states.iter().copied()).collect();
        drop_dead_refs(&mut refs, |r| lookup.iter().find(|(a, _)| *a == r).unwrap().1);
        assert_eq!(refs, u32s(&c["out"]), "function {:#010x}: {c}", u(c, "addr"));
    }
}

// verifies 0x00ACC4A0
#[test]
fn tired_message_matches_the_original() {
    let Some(cases) = vectors("tired_vf09") else { return };
    for c in &cases {
        let (mut b84, mut b85) = (u(c, "b84") as u8, u(c, "b85") as u8);
        let ret = ddnative::action::tired_message(u(c, "msg"), &mut b84, &mut b85);
        assert_eq!((ret, u32::from(b84), u32::from(b85)), (u(c, "ret"), u(c, "o84"), u(c, "o85")), "{c}");
    }
}

// verifies 0x00BB2150
#[test]
fn npc_death_once_matches_the_original() {
    let Some(cases) = vectors("npc_vf118") else { return };
    for c in &cases {
        let mut mark = u(c, "mark") as u8;
        let got = ddnative::character::npc_death_once(&mut mark, u(c, "fat") as u8, u(c, "dead") as u8);
        assert_eq!((u32::from(got), u32::from(mark)), (u(c, "al"), u(c, "omark")), "{c}");
    }
}

// verifies 0x00B44FB0
#[test]
fn wand_base_refs_match_the_original() {
    let Some(cases) = vectors("wand_base_vf10") else { return };
    for c in &cases {
        let refs = u32s(&c["refs"]);
        let states = u32s(&c["states"]);
        let mut r2 = [refs[0], refs[1]];
        let mut t = u(c, "t");
        ddnative::action::wand_base_refs(&mut r2, &mut t,
            |r| states[refs.iter().position(|&a| a == r).unwrap()],
            |_| (u(c, "cnt") as i32, u(c, "flag") as u8));
        assert_eq!(vec![r2[0], r2[1], t], u32s(&c["out"]), "{c}");
    }
}

// verifies 0x004E0EB0
#[test]
fn every_enemy_action_entry_matches_the_original() {
    use ddnative::action::{enemy_action_enter, EnemyAction, Owner};
    let Some(cases) = vectors("enemy_action_enter") else { return };
    for c in &cases {
        let i = &c["in"];
        let mut owner = Owner { category_206c: u(c, "cat"), word_2370: u(i, "w2370"), word_2374: u(i, "w2374"),
            flags_273c: u(i, "flags"), status_2e34: u(i, "status"), pending_2e38: u(c, "pending") };
        let mut o2ba0 = u(i, "o2ba0");
        let mut act = EnemyAction { byte_50: 0xEE, owner_54: 1, word_58: 1, word_60: 1, float_64: 1.0,
            byte_6c: 0, word_70: 1, owner_74: 1 };
        enemy_action_enter(&mut act, u(c, "owner"), &mut owner, &mut o2ba0, || c["is_class"].as_bool().unwrap());
        let o = &c["out"];
        assert_eq!((u32::from(act.byte_50), act.owner_54, act.word_58, act.word_60, act.float_64.to_bits(),
            u32::from(act.byte_6c), act.word_70, act.owner_74, o2ba0, owner.status_2e34, owner.flags_273c),
            (u(o, "b50"), u(o, "o54"), u(o, "w58"), u(o, "w60"), u(o, "f64"), u(o, "b6c"), u(o, "w70"),
             u(o, "o74"), u(o, "o2ba0"), u(o, "status"), u(o, "flags")), "entry {:#010x}: {c}", u(c, "addr"));
    }
}

// verifies 0x00AC4F70
#[test]
fn player_action_enter_matches_the_original() {
    use ddnative::player_action::{enter, EnterPlayer};
    let Some(cases) = vectors("player_action_vf05") else { return };
    let f = |b: u32| f32::from_bits(b);
    for c in &cases {
        let mut p = EnterPlayer {
            category_206c: u(c, "cat"), word_2370: u(c, "w2370"), word_2374: u(c, "w2374"),
            flags_273c: u(c, "flags"), saved_2740: u(c, "saved"), status_2e34: u(c, "status"),
            pending_2e38: u(c, "pending"), window_4be1: u(c, "window") as u8, word_3558: u(c, "w3558"),
            word_35e8: 1, byte_49c0: 1, word_2fb0: 1, word_3068: 1, float_306c: 1.0, word_3064: 1,
            value_2654: f(u(c, "v2654")), push_rank_1df0: 0, vec_1ac0: [1.0; 4],
            capsule_1e68: 1.0, radius_31b4: 1.0, radius_1e6c: 1.0, byte_31b3: 1,
        };
        let g = u32s(&c["globals"]);
        let (d3, d4) = (u(c, "d3"), u(c, "d4"));
        let (b50, a5c, a60, a64) = enter(&mut p, [f(g[0]), f(g[1]), f(g[2])],
            |id| match id { 0xD3 => ddnative::x87_copy(f(d3)), 0xD4 => ddnative::x87_copy(f(d4)), _ => panic!() });
        let o = &c["out"];
        let got = [u32::from(b50), a5c, a60.to_bits(), a64, p.word_2370, p.word_2374, p.flags_273c, p.status_2e34,
            p.word_35e8, p.word_2fb0, p.word_3068, p.float_306c.to_bits(), p.word_3064, p.push_rank_1df0,
            p.vec_1ac0[0].to_bits(), p.vec_1ac0[1].to_bits(), p.vec_1ac0[2].to_bits(), p.vec_1ac0[3].to_bits(),
            p.capsule_1e68.to_bits(), p.radius_31b4.to_bits(), p.radius_1e6c.to_bits(),
            u32::from(p.byte_49c0), u32::from(p.byte_31b3)];
        let want = ["b50", "a5c", "a60", "a64", "w2370", "w2374", "flags", "status", "w35e8", "w2fb0", "w3068",
            "f306c", "w3064", "rank", "v0", "v1", "v2", "v3", "h1e68", "r31b4", "r1e6c", "b49c0", "b31b3"]
            .map(|k| u(o, k));
        assert_eq!(got, want, "{c}");
    }
}

// verifies 0x008D4840
#[test]
fn goblin_action_enter_matches_the_original() {
    use ddnative::action::{EnemyAction, Owner};
    use ddnative::enemy::{goblin_action_enter, GoblinMotion};
    let Some(cases) = vectors("goblin_action_vf05") else { return };
    let f = |b: u32| f32::from_bits(b);
    for c in &cases {
        let i = &c["in"];
        let mut owner = Owner { category_206c: u(c, "cat"), word_2370: u(i, "w2370"), word_2374: u(i, "w2374"),
            flags_273c: u(i, "flags"), status_2e34: u(i, "status"), pending_2e38: u(c, "pending") };
        let mut o2ba0 = u(i, "o2ba0");
        let mut act = EnemyAction { byte_50: 0xEE, owner_54: 1, word_58: 1, word_60: 1, float_64: 1.0, byte_6c: 0,
            word_70: 1, owner_74: 1 };
        let mut g = GoblinMotion { velocity_2530: [9.0; 4], accel_2540: [9.0; 4], word_2574: 7,
            value_2578: 9.0, value_257c: f(u(i, "v257c")), byte_5c7c: 7 };
        let gl = u32s(&c["globals"]);
        goblin_action_enter(&mut act, u(c, "owner"), &mut owner, &mut o2ba0, || true, &mut g, [f(gl[0]), f(gl[1]), f(gl[2])]);
        let o = &c["out"];
        assert_eq!((u32::from(act.byte_50), act.owner_54, act.owner_74, o2ba0, owner.status_2e34, owner.flags_273c),
            (u(o, "b50"), u(o, "o54"), u(o, "o74"), u(o, "o2ba0"), u(o, "status"), u(o, "flags")), "{c}");
        assert_eq!(g.velocity_2530.map(f32::to_bits).to_vec(), u32s(&o["vel"]), "{c}");
        assert_eq!(g.accel_2540.map(f32::to_bits).to_vec(), u32s(&o["acc"]), "{c}");
        assert_eq!((u32::from(g.word_2574), g.value_2578.to_bits(), g.value_257c.to_bits(), u32::from(g.byte_5c7c)),
            (u(o, "w2574"), u(o, "v2578"), u(o, "v257c"), u(o, "b5c7c")), "{c}");
    }
}

// verifies 0x00A4DB70 0x008FBDD0 0x00919DB0 0x0095A810 0x00A2AE80 0x00A202E0 0x009C5890 0x009B4920 0x009BD970
#[test]
fn every_species_base_entry_matches_the_original() {
    use ddnative::action::{enemy_action_enter, EnemyAction, Owner};
    use ddnative::mem::{Image, Mem};
    use ddnative::species as sp;
    let Some(cases) = vectors("species") else { return };
    const S: u32 = 0x0E00_0000;
    const OWN: u32 = S + 0x1000;
    for c in &cases {
        let mut m = Image { base: S, bytes: hex(c["in"].as_str().unwrap()) };
        // the verified prefix over typed views of the image
        let mut owner = Owner { category_206c: m.u32(OWN + 0x206C), word_2370: m.u32(OWN + 0x2370),
            word_2374: m.u32(OWN + 0x2374), flags_273c: m.u32(OWN + 0x273C),
            status_2e34: m.u32(OWN + 0x2E34), pending_2e38: m.u32(OWN + 0x2E38) };
        let mut o2ba0 = m.u32(OWN + 0x2BA0);
        let mut act = EnemyAction { byte_50: m.u8(S + 0x50), owner_54: m.u32(S + 0x54), word_58: m.u32(S + 0x58),
            word_60: m.u32(S + 0x60), float_64: f32::from_bits(m.u32(S + 0x64)), byte_6c: m.u8(S + 0x6C),
            word_70: m.u32(S + 0x70), owner_74: m.u32(S + 0x74) };
        enemy_action_enter(&mut act, OWN, &mut owner, &mut o2ba0, || c["is_class"].as_bool().unwrap());
        m.set_u8(S + 0x50, act.byte_50); m.set_u32(S + 0x54, act.owner_54); m.set_u32(S + 0x58, act.word_58);
        m.set_u32(S + 0x60, act.word_60); m.set_u32(S + 0x64, act.float_64.to_bits()); m.set_u8(S + 0x6C, act.byte_6c);
        m.set_u32(S + 0x70, act.word_70); m.set_u32(S + 0x74, act.owner_74);
        m.set_u32(OWN + 0x2370, owner.word_2370); m.set_u32(OWN + 0x2374, owner.word_2374);
        m.set_u32(OWN + 0x273C, owner.flags_273c); m.set_u32(OWN + 0x2E34, owner.status_2e34);
        m.set_u32(OWN + 0x2BA0, o2ba0);
        // the species tail
        let g: [u32; 3] = u32s(&c["globals"]).try_into().unwrap();
        let o = act.owner_74;
        match c["kind"].as_str().unwrap() {
            "em5800" => sp::em5800_tail(&mut m, S, o),
            "em0200" => sp::em0200_tail(&mut m, o, g),
            "em0400" => sp::em0400_tail(&mut m, o, g),
            "em0900" => sp::em0900_tail(&mut m, o, g),
            "em5500t" => sp::em5500_tentacle_tail(&mut m, S, o),
            "em5500_00" => sp::em5500_00_tail(&mut m, S, o),
            "em5300" => sp::em5300_tail(&mut m, o),
            "em5200" => sp::em5200_tail(),
            k => panic!("{k}"),
        }
        let want = hex(c["out"].as_str().unwrap());
        if m.bytes != want {
            let diff: Vec<String> = (0..want.len()).filter(|&i| m.bytes[i] != want[i]).take(8)
                .map(|i| format!("+{i:#x}: got {:#04x} want {:#04x}", m.bytes[i], want[i])).collect();
            panic!("{:#010x} ({}): {diff:?}", u(c, "addr"), c["kind"]);
        }
    }
}
