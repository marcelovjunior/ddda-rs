//! The platform models (ddnative::platform) against the CRT routines they
//! stand for; vectors from research/native/vectors_platform.py.
use ddnative::mem::{Mem, Sparse};
use ddnative::platform;
use ddnative::x86::Cpu;
use serde_json::Value;

fn vectors(name: &str) -> Option<Vec<Value>> {
    let path = format!("E:/ddda-re/notes/native/vectors/{name}.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return None,
    };
    serde_json::from_str::<Value>(&text).ok()?.as_array().cloned()
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|k| u8::from_str_radix(&s[2 * k..2 * k + 2], 16).unwrap()).collect()
}

#[test]
fn memset_and_memcpy_models_match_the_crt() {
    let Some(cases) = vectors("platform") else { return };
    const STACK: u32 = 0x0F00_0800;
    for (n, c) in cases.iter().enumerate() {
        let base = c["scratch_base"].as_u64().unwrap() as u32;
        let (input, want) = (hex(c["scratch_in"].as_str().unwrap()), hex(c["scratch_out"].as_str().unwrap()));
        let mut m = Sparse::default();
        for (k, b) in input.iter().enumerate() {
            m.bytes.insert(base + k as u32, *b);
        }
        m.set_u32(STACK, 0x0DEA_D000);
        for (k, a) in c["args"].as_array().unwrap().iter().enumerate() {
            m.set_u32(STACK + 4 + 4 * k as u32, a.as_u64().unwrap() as u32);
        }
        let mut cpu = Cpu { esp: STACK, ..Cpu::default() };
        let model = match c["model"].as_str().unwrap() {
            "memset" => platform::memset,
            _ => platform::memcpy,
        };
        model(&mut m, &mut cpu);
        let got: Vec<u8> = (0..input.len() as u32).map(|k| m.u8(base + k)).collect();
        assert!(got == want, "case {n} ({}): buffer differs", c["model"]);
        assert_eq!(cpu.eax as u64, c["eax"].as_u64().unwrap(), "case {n}: eax");
        assert_eq!(cpu.esp, STACK + 4, "case {n}: ret");
    }
}
