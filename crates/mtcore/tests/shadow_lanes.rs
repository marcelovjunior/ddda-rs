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
    // targets and fixed state compare by content (their applies are
    // pointer-independent for equal bytes): a copy elsewhere is equal
    m.zero(0x6000, 0x5c);
    m.set_u32(0x3044, 0x6000);
    m.zero(0x6100, 0x14);
    m.set_u32(0x3048, 0x6100);
    assert_eq!(before.first_difference(&snap(&m, c)), None);
    // ... but not when a byte differs
    let mut changed = m.clone();
    changed.set_u8(0x6110, 1);
    assert_eq!(
        before.first_difference(&snap(&changed, c)),
        Some(Difference { path: vec![1], field: "bindings" })
    );
    // stream bindings keep pointer identity
    m.zero(0x6200, 0x30);
    m.set_u32(0x3050, 0x6200); // same bytes, different reuse pattern
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

#[test]
fn shader_snapshots_cover_original_recorded_constant_uploads() {
    // Reuse L117's original DAA8B0 call oracle. Those sparse fixtures omit
    // emitter-only metadata+20; give the diagnostic the fixture's three
    // indexed values (texture, sampler, constants). This extra extent is not
    // read by DAA8B0 and is not evidence about a live game's metadata object.
    let path = "E:/ddda-re/notes/native-L117/state-vectors.json";
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {error}"),
        Err(_) => return,
    };
    let root: serde_json::Value = serde_json::from_str(&text).unwrap();
    struct Sparse(Memory);
    impl Mem for Sparse {
        fn u8(&self, a: u32) -> u8 {
            *self.0 .0.get(&a).unwrap_or(&0)
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.0.set_u8(a, v);
        }
    }
    struct Wire<'a> {
        bytes: &'a [u8],
        at: usize,
    }
    impl Wire<'_> {
        fn word(&mut self) -> u32 {
            let value = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
            self.at += 4;
            value
        }
        fn block(&mut self) -> Vec<u32> {
            let bytes = self.word();
            assert_eq!(bytes % 4, 0);
            (0..bytes / 4).map(|_| self.word()).collect()
        }
    }
    let (mut accepted, mut refused, mut uploads) = (0, 0, 0);
    for case in root["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["kind"] == "shader")
    {
        let mut m = Sparse(Memory::default());
        for pair in case["words"].as_array().unwrap() {
            m.set_u32(pair[0].as_u64().unwrap() as u32, pair[1].as_u64().unwrap() as u32);
        }
        let binding = 0x0e002000;
        m.set_u32(m.u32(binding + 8) + 0x20, 3);
        let bytes = match capture_shader(&m, binding, 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(CaptureError::Shader(_)) => {
                refused += 1;
                continue;
            }
            Err(error) => panic!("unexpected L117 capture rejection {error:?}"),
        };
        let mut wire = Wire { bytes: &bytes, at: 0 };
        let _header = wire.block();
        let metadata = wire.block();
        let mut calls = Vec::<(u32, Vec<u32>, Vec<u32>)>::new();
        for stage in 0..2 {
            for _ in 0..metadata[2 + stage] & 255 {
                let section = wire.word();
                let descriptor = wire.word();
                if section == 0 {
                    wire.word();
                    wire.word();
                    continue;
                }
                let slot = (if stage == 0 { 94 } else { 109 })
                    + match section {
                        2 => 2,
                        3 => 4,
                        _ => 0,
                    };
                let register = (descriptor >> 16) & 255;
                let mut count = descriptor >> 24;
                if section == 4 {
                    wire.word(); // resource identity
                    let a = wire.word();
                    let b = wire.word();
                    let dims = [a & 0xffff, (a >> 16) & 0x3fff, (b >> 16) & 0x7ff];
                    let mut data = dims
                        .map(|n| {
                            if (descriptor >> 8) & 255 == 3 {
                                (1.0 / (n as f32)).to_bits()
                            } else {
                                (n as f32).to_bits()
                            }
                        })
                        .to_vec();
                    data.push(0);
                    if count == 0 {
                        data.clear();
                    }
                    calls.push((slot, vec![register, count], data));
                } else {
                    let tag = wire.word();
                    if section == 1 && tag != 0 {
                        count = count.min(tag * 16);
                    }
                    let data = wire.block();
                    if section == 1 && descriptor & 1 != 0 {
                        for (i, vec4) in data.chunks_exact(4).enumerate() {
                            calls.push((
                                slot,
                                vec![register + i as u32, 1],
                                vec4.iter().map(|n| (*n as i32 as f32).to_bits()).collect(),
                            ));
                        }
                    } else {
                        calls.push((slot, vec![register, count], data));
                    }
                }
            }
        }
        let expected: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|call| {
                let slot = call["slot"].as_u64().unwrap() as u32;
                let words = |field: &str| {
                    call[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u32)
                        .collect::<Vec<_>>()
                };
                matches!(slot, 94 | 96 | 98 | 109 | 111 | 113).then(|| (slot, words("args"), words("data")))
            })
            .collect();
        if m.u32(0x0e000010) == 0 {
            assert_eq!(calls, expected);
        } else {
            let mut remaining = calls.as_slice();
            for call in &expected {
                let index = remaining
                    .iter()
                    .position(|decoded| decoded == call)
                    .expect("original upload missing from frozen snapshot");
                remaining = &remaining[index + 1..];
            }
        }
        uploads += expected.len();
        accepted += 1;
    }
    assert!(accepted >= 100);
    assert!(uploads > 0);
    eprintln!("original DAA8B0: {accepted} shader snapshots, {uploads} uploads matched; {refused} conservative refusals");
}
