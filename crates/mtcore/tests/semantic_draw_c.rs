use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self
            .bytes
            .get(&a)
            .unwrap_or_else(|| panic!("unrecorded read {a:x}"))
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.bytes.insert(a, v);
        self.writes.insert(a, v);
    }
}
fn n(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn pairs(v: &Value) -> BTreeMap<u32, u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| (n(&p[0]), n(&p[1]) as u8))
        .collect()
}
fn observable(a: u32, esp: u32) -> bool {
    // Exclude only the discarded guest stack, never heap addresses below ESP.
    !(0x0f000000..esp).contains(&a)
}
struct FloatingControl(u32);
impl Drop for FloatingControl {
    fn drop(&mut self) {
        x86::ldmxcsr(self.0);
    }
}
fn floating_control(c: &mut Cpu, before: &Value) -> FloatingControl {
    let old = x86::stmxcsr();
    x86::ldmxcsr(n(&before["mxcsr"]));
    let mut control = [0; 10];
    control[..2].copy_from_slice(&(n(&before["x87"][0]) as u16).to_le_bytes());
    c.st.exec::<0x28d9>(&mut control);
    FloatingControl(old)
}
fn verify(path: &str, entry: u32, body: LiftFn) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    ddnative_host::install();
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut count = 0;
    for case in data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| n(&v["entry"]) == entry)
    {
        let seed = n(&case["seed"]);
        let mut m = Memory {
            bytes: pairs(&case["initial"]),
            ..Default::default()
        };
        let mut expected = m.bytes.clone();
        expected.extend(pairs(&case["after"]["memory"]));
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] =
            std::array::from_fn(|i| n(&case["before"]["regs"][i]));
        let calls = case["calls"].as_array().cloned().unwrap_or_default();
        let total = calls.len();
        let cursor = Rc::new(Cell::new(0));
        let pos = cursor.clone();
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            if target == 0x01008200 {
                nested_lookup(m, c);
                return;
            }
            let call = &calls[pos.get()];
            pos.set(pos.get() + 1);
            assert_eq!(target, n(&call["target"]));
            if target != 0x012efea0 {
                assert_eq!(c.ecx, n(&call["regs"][1]));
            }
            assert_eq!(c.esp, n(&call["regs"][4]));
            for (i, v) in call["args"].as_array().unwrap().iter().enumerate() {
                assert_eq!(m.u32(c.esp + i as u32 * 4), n(v));
            }
            for (a, b) in pairs(&call["memory"]) {
                if !(0x0f000000..0x0f010000).contains(&a) {
                    assert_eq!(m.u8(a), b, "call memory {a:x}");
                }
            }
            for (a, b) in pairs(&call["effects"]) {
                m.set_u8(a, b);
            }
            c.eax = n(&call["result"]);
            c.esp = n(&call["return_esp"]);
            c.ecx = 0xc1a0;
            c.edx = 0xd1a0;
        })));
        let _floating = floating_control(&mut c, &case["before"]);
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(cursor.get(), total, "seed {seed}: calls");
        let got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [0, 3, 4, 5, 6, 7].map(|i| n(&case["after"]["regs"][i]));
        assert_eq!(got, want, "entry {entry:x} seed {seed}: ABI");
        for (&a, &b) in &expected {
            if observable(a, c.esp) {
                assert_eq!(
                    m.bytes.get(&a),
                    Some(&b),
                    "entry {entry:x} seed {seed}: memory {a:x}"
                );
            }
        }
        for (&a, &b) in &m.writes {
            if observable(a, c.esp) {
                assert_eq!(
                    expected.get(&a),
                    Some(&b),
                    "entry {entry:x} seed {seed}: extra write {a:x}"
                );
            }
        }
        count += 1;
    }
    assert!(count > 0);
    eprintln!("{entry:08x}: {count} ABI original vectors pass");
    count
}
fn nested_lookup(m: &mut dyn Mem, c: &mut Cpu) {
    mtcore::semantic_draw_c::single_key(m, c);
}

#[test]
fn original_single_key() {
    verify(
        "E:/ddda-re/notes/native-L158/job_lookup.json",
        0x1008200,
        mtcore::semantic_draw_c::single_key,
    );
}
#[test]
fn original_package_key() {
    verify(
        "E:/ddda-re/notes/native-L175/00e0f3c0.json",
        0xe0f3c0,
        mtcore::semantic_draw_c::package_key,
    );
}
#[test]
fn original_begin_material() {
    verify(
        "E:/ddda-re/notes/native-L175/00fed190.json",
        0xfed190,
        mtcore::semantic_draw_c::begin_material,
    );
}
#[test]
fn original_primitive_material() {
    verify(
        "E:/ddda-re/notes/native-L175/00ea23f0.json",
        0xea23f0,
        mtcore::semantic_draw_c::primitive_material,
    );
}

#[test]
fn original_commit_primitive_state() {
    verify(
        "E:/ddda-re/notes/native-L177/00fee320.json",
        0xfee320,
        mtcore::semantic_draw_c::commit_primitive_state,
    );
}

#[test]
fn original_draw_model_mesh() {
    verify(
        "E:/ddda-re/notes/native-L177/00ffc350.json",
        0xffc350,
        mtcore::semantic_draw_c::draw_model_mesh,
    );
}

#[test]
fn original_apply_standard_material() {
    verify(
        "E:/ddda-re/notes/native-L177/01145bf0.json",
        0x1145bf0,
        mtcore::semantic_draw_c::apply_standard_material,
    );
}

#[test]
fn commit_oracle_rejects_corrupt_dirty_flags() {
    let path = "E:/ddda-re/notes/native-L177/00fee320.json";
    if !std::path::Path::new(path).exists() {
        assert!(std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none());
        return;
    }
    let result = std::panic::catch_unwind(|| {
        verify(path, 0xfee320, |m, c| {
            let context = c.ecx;
            mtcore::semantic_draw_c::commit_primitive_state(m, c);
            m.set_u32(
                context.wrapping_add(0x154),
                m.u32(context.wrapping_add(0x154)) ^ 1,
            );
        })
    });
    let error = result.expect_err("correct registers cannot hide corrupted dirty flags");
    let text = error.downcast_ref::<String>().unwrap();
    assert!(text.contains("memory e000154"), "unexpected failure: {text}");
}

#[test]
fn abi_oracle_rejects_corrupt_heap_below_stack() {
    let path = "E:/ddda-re/notes/native-L175/00fed190.json";
    if !std::path::Path::new(path).exists() {
        assert!(
            std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(),
            "missing {path}"
        );
        return;
    }
    let result = std::panic::catch_unwind(|| {
        verify(path, 0xfed190, |m, c| {
            let context = c.ecx;
            mtcore::semantic_draw_c::begin_material(m, c);
            // Leave the correct return/registers intact; corrupt only guest heap.
            m.set_u32(context.wrapping_add(0x68), c.eax ^ 1);
        });
    });
    let error = result.expect_err("corrupt heap output must fail ABI comparison");
    let text = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(text.contains("memory e000068"), "unexpected failure: {text}");
}
