//! CPU-only tests of the strict stream oracle; no game or GPU execution.
#[path = "../src/shadow_lanes.rs"]
mod shadow_lanes;
use ddnative_base::mem::Mem;
use shadow_lanes::*;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Memory(BTreeMap<u32, u8>);
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self.0.get(&a).unwrap_or_else(|| panic!("unmapped {a:x}"))
    }
    fn set_u8(&mut self, a: u32, b: u8) {
        self.0.insert(a, b);
    }
}
impl Memory {
    fn zero(&mut self, a: u32, n: u32) {
        for i in 0..n {
            self.set_u8(a + i, 0);
        }
    }
    fn words(&mut self, a: u32, words: &[u32]) {
        for (i, w) in words.iter().enumerate() {
            self.set_u32(a + i as u32 * 4, *w);
        }
    }
}

fn scene(shift: u32) -> (Memory, u32) {
    let mut m = Memory::default();
    let context = 0x1000;
    m.words(context + 0x44, &[0x2000 + shift, 2]);
    m.words(0x2000 + shift, &[19, 0x3000 + shift, 20, 0x3040 + shift]);
    m.words(0x3000 + shift, &[0x60000, 0x9191, 0x4000 + shift, 1]);
    m.words(0x4000 + shift, &[7, 0x3080 + shift]);
    m.words(0x3040 + shift, &[0x70000, 1, 0x9090]);
    m.words(
        0x3080 + shift,
        &[
            0x10000,
            0x5000 + shift,
            0x5100 + shift,
            0x5200 + shift,
            0x5300 + shift,
            36,
            4,
            2,
        ],
    );
    m.zero(0x5000 + shift, 0x5c);
    m.zero(0x5100 + shift, 0x14);
    m.zero(0x5300 + shift, 0x30);
    m.words(0x5200 + shift, &[0x600000, 0, 0x9000, 0x5401 + shift]);
    m.zero(0x9000, 0x24);
    m.words(0x9004, &[0x9100, 0x1001, 0]); // one float descriptor
    m.set_u32(0x9020, 1);
    m.set_u32(0x9100, 0x01000000); // one float4, source index/offset zero
    m.words(0x5400 + shift, &[0x3f800000, 2, 3, 4]);
    (m, context)
}
fn snap(m: &Memory, context: u32) -> StreamSnapshot {
    capture_context(m, context, Limits::default()).unwrap()
}

#[test]
fn relocation_preserves_complete_ordered_snapshot() {
    let (a, context) = scene(0);
    let (b, _) = scene(0x10000);
    assert_eq!(snap(&a, context), snap(&b, context));
}

#[test]
fn corruption_of_keys_records_bindings_and_pointees_is_detected() {
    let (m, c) = scene(0);
    let reference = snap(&m, c);
    for address in [
        0x2000, 0x3000, 0x3004, 0x4000, 0x3044, 0x3048, 0x3094, 0x5000, 0x5100, 0x5300, 0x5200, 0x5400,
        0x9000,
    ] {
        let mut changed = m.clone();
        changed.set_u8(address, changed.u8(address) ^ 1);
        assert_ne!(reference, snap(&changed, c), "missed {address:x}");
    }
}

#[test]
fn order_and_count_are_not_sorted_or_ignored() {
    let (mut m, c) = scene(0);
    let reference = snap(&m, c);
    m.words(0x2000, &[20, 0x3040, 19, 0x3000]);
    assert_ne!(reference, snap(&m, c));
    m.set_u32(c + 0x48, 1);
    assert_ne!(reference, snap(&m, c));
}

#[test]
fn incomplete_or_cyclic_capture_never_returns_a_snapshot() {
    let (m, c) = scene(0);
    for limits in [
        Limits {
            records: 1,
            ..Limits::default()
        },
        Limits {
            bytes: 1,
            ..Limits::default()
        },
        Limits {
            depth: 0,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            capture_context(&m, c, limits),
            Err(CaptureError::Limit(_))
        ));
    }
    let mut cyclic = m.clone();
    cyclic.set_u32(0x4004, 0x3000);
    assert_eq!(
        capture_context(&cyclic, c, Limits::default()),
        Err(CaptureError::Cycle(0x3000))
    );
    let mut unknown = m.clone();
    unknown.set_u32(0x3040, 8 << 16);
    assert_eq!(
        capture_context(&unknown, c, Limits::default()),
        Err(CaptureError::UnsupportedKind(8))
    );
}

#[test]
fn shaders_refuse_unknown_derived_sources_and_bad_indices() {
    let (mut m, _) = scene(0);
    m.set_u32(0x9008, 0x10000001);
    m.set_u32(0x9100, 0x01000500); // unsupported derived kind 5
    assert!(matches!(
        capture_shader(&m, 0x5200, 4096),
        Err(CaptureError::Shader(_))
    ));
    m.set_u32(0x9008, 0x1001);
    m.set_u32(0x9100, 0x01000002); // index 1 but only one value
    assert!(matches!(
        capture_shader(&m, 0x5200, 4096),
        Err(CaptureError::Shader(_))
    ));
    m.set_u32(0x9100, 0x01000000);
    assert!(matches!(
        capture_shader(&m, 0x5200, 8),
        Err(CaptureError::Limit(_))
    ));
}

#[test]
fn tagged_float_bounds_boolean_width_and_zero_uploads() {
    let (mut m, _) = scene(0);
    m.set_u32(0x9100, 0xff000000); // 255 vec4s, low tag 1 clamps to 16
    m.zero(0x5400, 16 * 16);
    assert!(capture_shader(&m, 0x5200, 4096).is_ok());
    m.set_u32(0x9008, 0x00800001); // one bool descriptor
    m.set_u32(0x9100, 0x01000000);
    let bool_before = capture_shader(&m, 0x5200, 4096).unwrap();
    m.set_u32(0x5404, 0xdeadbeef); // outside one scalar BOOL
    assert_eq!(bool_before, capture_shader(&m, 0x5200, 4096).unwrap());
    m.set_u32(0x9100, 0); // zero upload; tagged null does not dereference
    m.set_u32(0x520c, 1);
    assert!(capture_shader(&m, 0x5200, 4096).is_ok());
}

#[test]
fn clear_and_copy_keep_associated_draws() {
    for (kind, offset) in [(4, 0x20), (5, 0x28)] {
        let (mut m, c) = scene(0);
        m.zero(0x6000, offset + 4);
        m.set_u32(0x6000, kind << 16);
        m.set_u32(0x6000 + offset, 0x3080);
        m.set_u32(0x4004, 0x6000);
        let before = snap(&m, c);
        m.set_u32(0x6004, 1);
        assert_ne!(before, snap(&m, c));
    }
}

#[test]
fn slot_restore_changes_only_eight_pointer_words() {
    let mut m = Memory::default();
    let scheduler = 0x100000;
    m.zero(scheduler + SLOT_BASE, LANE_COUNT as u32 * SLOT_STRIDE);
    let original = m.clone();
    let saved = LaneSlots::read(&m, scheduler);
    let installed = LaneSlots::contiguous(0x200000);
    installed.write(&mut m, scheduler);
    assert_eq!(LaneSlots::read(&m, scheduler), installed);
    for i in 0..LANE_COUNT as u32 {
        for byte in 4..SLOT_STRIDE {
            assert_eq!(m.u8(scheduler + SLOT_BASE + i * SLOT_STRIDE + byte), 0);
        }
    }
    saved.write(&mut m, scheduler);
    assert_eq!(original.0, m.0);
    assert_eq!(installed.0[7], 0x200000 + 7 * 0x33c84);
}

#[test]
fn original_merger_relocations_preserve_lane_order_and_nested_payloads() {
    let dir = std::path::Path::new("E:/ddda-re/notes/native-L172/lanes");
    let manifest = match std::fs::read_to_string(dir.join("manifest.json")) {
        Ok(s) => serde_json::from_str::<serde_json::Value>(&s).unwrap(),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => {
            panic!("original merger corpus: {e}")
        }
        Err(_) => return,
    };
    struct Dense {
        base: u32,
        bytes: Vec<u8>,
    }
    impl Mem for Dense {
        fn u8(&self, a: u32) -> u8 {
            self.bytes[(a - self.base) as usize]
        }
        fn set_u8(&mut self, _: u32, _: u8) {
            panic!("read-only corpus")
        }
    }
    let cases = manifest["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let load = |field: &str| Dense {
            base: manifest["base"].as_u64().unwrap() as u32,
            bytes: std::fs::read(dir.join(case[field].as_str().unwrap())).unwrap(),
        };
        let before = load("before");
        let after = load("after");
        let mut expected = StreamSnapshot { entries: Vec::new() };
        for root in case["roots"].as_array().unwrap() {
            let part = capture_stream(
                &before,
                root[0].as_u64().unwrap() as u32,
                root[1].as_u64().unwrap() as u32,
                Limits::default(),
                &mut |_, _| panic!("query-only original merger fixture"),
            )
            .unwrap();
            expected.entries.extend(part.entries);
        }
        let actual = capture_context(
            &after,
            case["context"].as_u64().unwrap() as u32,
            Limits::default(),
        )
        .unwrap();
        assert_eq!(expected, actual, "{}", case["after"]);
    }
    eprintln!("{} original FED3B0 merged streams match", cases.len());
}

#[test]
fn pointer_reuse_and_first_mismatch_path_are_preserved() {
    let (mut m, c) = scene(0);
    let mut record = [0u8; 32];
    m.read_bytes(0x3080, &mut record);
    for (i, b) in record.into_iter().enumerate() {
        m.set_u8(0x3040 + i as u32, b);
    }
    let before = snap(&m, c);
    assert_eq!(before.first_difference(&before), None);
    m.zero(0x6000, 0x5c);
    m.set_u32(0x3044, 0x6000); // same bytes, different reuse pattern
    let after = snap(&m, c);
    assert_eq!(
        before.first_difference(&after),
        Some(Difference {
            path: vec![1],
            field: "bindings"
        })
    );
    let mut changed = before.clone();
    changed.entries[0].record.children[0].key ^= 1;
    assert_eq!(
        before.first_difference(&changed),
        Some(Difference {
            path: vec![0, 0],
            field: "sort key"
        })
    );
}

#[test]
fn overflow_and_null_ranges_are_rejected_before_reading() {
    let m = Memory::default();
    let mut shader = |_: &dyn Mem, _| -> Result<Vec<u8>, CaptureError> { panic!("unexpected shader") };
    assert_eq!(
        capture_stream(&m, u32::MAX - 3, 1, Limits::default(), &mut shader),
        Err(CaptureError::AddressOverflow)
    );
    assert_eq!(
        capture_stream(&m, 0, 1, Limits::default(), &mut shader),
        Err(CaptureError::NullPointer)
    );
    assert_eq!(
        capture_stream(&m, 0, 0, Limits::default(), &mut shader)
            .unwrap()
            .entries
            .len(),
        0
    );
}
