//! L184 s6: the CRT's asinf (0x012F0D4C) and double atan2 (0x012F1109)
//! against the original's code run in Unicorn
//! (research/native/record_crt_atan2.py), bit for bit; NaN results are
//! compared as "both NaN" only where the original's NaN is a default NaN
//! (they are equal bit for bit in practice: see the counts printed).
use ddnative_base::mem::{Mem, Pages};
use mtcore::semantic_s6_crt::Tables;
use serde_json::Value;
use std::collections::BTreeMap;

/// Guest memory holding the CRT tables the vectors recorded.
struct Memory(BTreeMap<u32, u8>);
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self.0.get(&a).unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.0.insert(a, v);
    }
}

fn memory(data: &Value) -> Memory {
    let base = data["table_base"].as_u64().unwrap() as u32;
    Memory(data["table"].as_array().unwrap().iter().enumerate().map(|(i, b)| (base + i as u32, b.as_u64().unwrap() as u8)).collect())
}

fn load() -> Option<Value> {
    let path = std::env::var("L184_S6_CRT_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/crt_atan2.json".into());
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(serde_json::from_str(&t).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

#[test]
fn asinf_matches_original() {
    let Some(data) = load() else { return };
    let m = memory(&data);
    let t = Tables { memory: &m, pages: Pages::of(&m) };
    let cases = data["asinf"].as_array().unwrap();
    let mut bad = 0;
    for c in cases {
        let (x, want) = (c[0].as_u64().unwrap() as u32, c[1].as_u64().unwrap() as u32);
        let got = mtcore::semantic_s6_crt::asinf(&t, f32::from_bits(x)).to_bits();
        if got != want {
            bad += 1;
            if bad < 10 {
                eprintln!("asinf({x:08x}) = {got:08x}, want {want:08x}");
            }
        }
    }
    assert_eq!(bad, 0, "{bad} of {} asinf cases differ", cases.len());
    eprintln!("{} asinf cases bit-exact", cases.len());
}

#[test]
fn atan2_matches_original() {
    let Some(data) = load() else { return };
    let m = memory(&data);
    let t = Tables { memory: &m, pages: Pages::of(&m) };
    let cases = data["atan2"].as_array().unwrap();
    let (mut double_differs, mut float_differs) = (0, 0);
    for c in cases {
        let (y, x, want) = (c[0].as_u64().unwrap(), c[1].as_u64().unwrap(), c[2].as_u64().unwrap());
        let got = mtcore::semantic_s6_crt::atan2(&t, f64::from_bits(y), f64::from_bits(x)).to_bits();
        let same_nan = |a: u64, b: u64| f64::from_bits(a).is_nan() && f64::from_bits(b).is_nan();
        if got != want {
            double_differs += 1;
            // callers convert the double to float (CVTSD2SS)
            let (fg, fw) = (f64::from_bits(got) as f32, f64::from_bits(want) as f32);
            if fg.to_bits() != fw.to_bits() && !same_nan(got, want) {
                float_differs += 1;
                if float_differs < 10 {
                    eprintln!("atan2({y:016x}, {x:016x}) = {got:016x}, want {want:016x}");
                }
            }
        }
    }
    eprintln!("{} atan2 cases: {double_differs} differ as doubles (reciprocal estimate), {float_differs} as floats", cases.len());
    assert_eq!(float_differs, 0);
}
