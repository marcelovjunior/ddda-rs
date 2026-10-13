//! Allocation-free object decoders checked against the original L117 calls.
use ddnative_base::mem::Mem;
use mtdraw::apply::{self, ConstantKind};
use mtstate::Call;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
struct ReadOnly(BTreeMap<u32, u8>);
impl Mem for ReadOnly {
    fn u8(&self, a: u32) -> u8 {
        *self.0.get(&a).unwrap_or(&0)
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("pure decoder attempted a write");
    }
}
fn number(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn words(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(number).collect()
}

#[test]
fn pure_sources_match_original_device_arguments() {
    let path = std::env::var("MTSTATE_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L117/state-vectors.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            assert!(std::env::var_os("MTDRAW_REQUIRE_APPLY").is_none(), "{path}: {e}");
            return;
        }
    };
    let root: Value = serde_json::from_str(&text).unwrap();
    let (ctx, binding) = (0x0e000000, 0x0e002000);
    let mut checked = 0;
    for case in root["cases"].as_array().unwrap() {
        let mut m = ReadOnly::default();
        for p in case["words"].as_array().unwrap() {
            for (i, b) in number(&p[1]).to_le_bytes().into_iter().enumerate() {
                m.0.insert(number(&p[0]) + i as u32, b);
            }
        }
        let original: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| Call {
                slot: number(&c["slot"]),
                args: words(&c["args"]),
                data: words(&c["data"]),
            })
            .collect();
        match case["kind"].as_str().unwrap() {
            "streams" => {
                let decoded = apply::decode_streams(&m, binding);
                for call in original {
                    match call.slot {
                        100 => {
                            let s = decoded.vertex[call.args[0] as usize];
                            assert_eq!(&call.args[1..], &[s.handle_20, s.offset, s.stride]);
                        }
                        104 => assert_eq!(call.args, [decoded.index_handle]),
                        _ => panic!("unexpected stream call"),
                    }
                }
                checked += 1;
            }
            "targets" => {
                let decoded = apply::decode_targets(&m, binding);
                for call in original {
                    match call.slot {
                        37 => assert_eq!(call.args[1], decoded.color[call.args[0] as usize].surface_14),
                        39 => assert_eq!(call.args, [decoded.depth.surface_14]),
                        47 | 75 => {}
                        _ => panic!("unexpected target call"),
                    }
                }
                checked += 1;
            }
            "fixed" if m.u32(ctx + 12) == 0 => {
                let d = apply::decode_fixed(&m, binding);
                let mut calls = mtstate::packed(mtstate::BLEND, &d.blend.packed, None, None, 0);
                let rgba = d.blend_rgba_00;
                calls.push(Call {
                    slot: 57,
                    args: vec![193, (rgba & 0xff00ff00) | (rgba & 255) << 16 | (rgba >> 16 & 255)],
                    data: vec![],
                });
                calls.extend(mtstate::packed(mtstate::DEPTH, &d.depth.packed, None, None, 0));
                calls.push(Call {
                    slot: 57,
                    args: vec![57, d.stencil_reference as u32],
                    data: vec![],
                });
                calls.extend(mtstate::packed(mtstate::RASTER, &d.raster.packed, None, None, 0));
                assert_eq!(calls, original);
                checked += 1;
            }
            "shader" => {
                let d = apply::decode_shader(&m, binding);
                let mut constants = vec![];
                let mut textures = vec![];
                for (pixel, stage) in [(false, d.vertex), (true, d.pixel)] {
                    for t in stage.textures(&m, d.values) {
                        textures.push((t.slot + if pixel { 0 } else { 257 }, t.texture_handle));
                    }
                    for source in stage.constants(&m, d.values) {
                        let slot = if pixel { 109 } else { 94 }
                            + match source.kind {
                                ConstantKind::Integer => 2,
                                ConstantKind::Boolean => 4,
                                _ => 0,
                            };
                        if let Some(range) = source.range() {
                            if range.convert_signed {
                                for i in 0..range.count {
                                    constants.push(Call {
                                        slot,
                                        args: vec![range.register + i, 1],
                                        data: (0..4)
                                            .map(|j| {
                                                (m.u32(range.address + i * 16 + j * 4) as i32 as f32)
                                                    .to_bits()
                                            })
                                            .collect(),
                                    });
                                }
                            } else {
                                let width = if source.kind == ConstantKind::Boolean {
                                    1
                                } else {
                                    4
                                };
                                constants.push(Call {
                                    slot,
                                    args: vec![range.register, range.count],
                                    data: (0..range.count * width)
                                        .map(|i| m.u32(range.address + i * 4))
                                        .collect(),
                                });
                            }
                        } else {
                            let values = source.dimensions(&m).expect("L117 derived descriptor domain");
                            assert!(source.count <= 1);
                            constants.push(Call {
                                slot,
                                args: vec![source.register, source.count],
                                data: if source.count == 0 {
                                    vec![]
                                } else {
                                    values.to_vec()
                                },
                            });
                        }
                    }
                }
                let mut remaining = constants.as_slice();
                let expected_constants: Vec<_> = original
                    .iter()
                    .filter(|c| matches!(c.slot, 94 | 96 | 98 | 109 | 111 | 113))
                    .cloned()
                    .collect();
                for call in &original {
                    match call.slot {
                        87 => assert_eq!(call.args, [d.declaration]),
                        92 => assert_eq!(call.args, [d.vertex.handle]),
                        107 => assert_eq!(call.args, [d.pixel.handle]),
                        65 if call.args[1] != 0 => assert!(textures.contains(&(call.args[0], call.args[1]))),
                        94 | 96 | 98 | 109 | 111 | 113 => {
                            let index = remaining
                                .iter()
                                .position(|c| c == call)
                                .expect("original constant call missing from decoded source order");
                            remaining = &remaining[index + 1..];
                        }
                        _ => {}
                    }
                }
                if m.u32(ctx + 0x10) == 0 {
                    assert_eq!(constants, expected_constants);
                }
                checked += 1;
            }
            _ => {}
        }
    }
    assert!(checked > 0);
    eprintln!("{checked} original cases checked through pure object decoders");
}
