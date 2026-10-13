use mtstate::{Call, Memory};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
struct Guest(BTreeMap<u32, u32>);
impl Memory for Guest {
    fn read32(&self, a: u32) -> u32 {
        *self.0.get(&a).unwrap_or(&0)
    }
    fn write32(&mut self, a: u32, v: u32) {
        self.0.insert(a, v);
    }
}
fn number(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn words(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(number).collect()
}

#[test]
fn original_calls_and_cache_writes() {
    let path = std::env::var("MTSTATE_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L117/state-vectors.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            assert!(
                std::env::var_os("MTSTATE_REQUIRE_VECTORS").is_none(),
                "{path}: {e}"
            );
            eprintln!("private vectors absent: {path}");
            return;
        }
    };
    let root: Value = serde_json::from_str(&text).unwrap();
    let cases = root["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let mut calls = 0;
    for (index, c) in cases.iter().enumerate() {
        let mut m = Guest::default();
        for pair in c["words"].as_array().unwrap() {
            m.write32(number(&pair[0]), number(&pair[1]));
        }
        let kind = c["kind"].as_str().unwrap();
        let ctx = 0x0e000000;
        let new = ctx + 0x2000;
        let actual = match kind {
            k if k.starts_with("construct_") => {
                let d = words(&c["descriptor"]);
                let (offset, payload) = match k {
                    "construct_blend" => (
                        0x20,
                        mtstate::descriptor::blend(d.try_into().unwrap())
                            .unwrap()
                            .to_vec(),
                    ),
                    "construct_depth" => (
                        0x18,
                        mtstate::descriptor::depth(d.try_into().unwrap())
                            .unwrap()
                            .to_vec(),
                    ),
                    "construct_raster" => (
                        0x20,
                        mtstate::descriptor::raster(d.try_into().unwrap())
                            .unwrap()
                            .into_iter()
                            .flatten()
                            .collect(),
                    ),
                    "construct_sampler" => (
                        0x30,
                        mtstate::descriptor::sampler(d.try_into().unwrap())
                            .unwrap()
                            .to_vec(),
                    ),
                    _ => unreachable!(),
                };
                for (i, v) in payload.into_iter().enumerate() {
                    m.write32(new + offset + i as u32 * 4, v);
                }
                vec![]
            }
            "fixed" => mtstate::fixed(&mut m, ctx, new),
            "streams" => mtstate::streams(&mut m, ctx, new),
            "targets" => mtstate::targets(
                &mut m,
                ctx,
                new,
                words(&c["invalid_viewport"]).try_into().unwrap(),
            ),
            "shader" => {
                let srgb = m.read8(ctx + 0x5031) != 0;
                mtstate::shader(&mut m, ctx, new, srgb, number(&c["filtering"])).unwrap()
            }
            _ => {
                let fields = match kind {
                    "blend" => mtstate::BLEND,
                    "depth" => mtstate::DEPTH,
                    "raster" => mtstate::RASTER,
                    "sampler" => mtstate::SAMPLER,
                    _ => panic!("{kind}"),
                };
                let p: Vec<_> = (0..3).map(|i| m.read32(new + i * 4)).collect();
                let old = number(&c["old"]);
                let o: Vec<_> = (0..3).map(|i| m.read32(old + i * 4)).collect();
                mtstate::packed(
                    fields,
                    &p,
                    (old != 0).then_some(o.as_slice()),
                    (kind == "sampler").then(|| number(&c["stage"])),
                    number(&c["filtering"]),
                )
            }
        };
        let expected: Vec<_> = c["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| Call {
                slot: number(&c["slot"]),
                args: words(&c["args"]),
                data: words(&c["data"]),
            })
            .collect();
        assert_eq!(actual, expected, "case {index} {kind}");
        for pair in c["after"].as_array().unwrap() {
            let a = number(&pair[0]);
            assert_eq!(m.read32(a), number(&pair[1]), "case {index} {kind}, memory {a:x}");
        }
        calls += actual.len();
    }
    eprintln!(
        "{} original cases, {calls} calls and all cache words equal",
        cases.len()
    );
}
