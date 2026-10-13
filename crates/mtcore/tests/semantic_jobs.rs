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
            let call = &calls[pos.get()];
            pos.set(pos.get() + 1);
            assert_eq!(target, n(&call["target"]));
            assert_eq!(c.ecx, n(&call["regs"][1]));
            assert_eq!(c.esp, n(&call["regs"][4]));
            if target == 0x00c26310 {
                assert_eq!(c.edx, n(&call["regs"][2]), "fastcall material");
            }
            if target == 0x00c242c0 {
                assert_eq!(
                    [c.eax, c.esi, c.edi],
                    [0, 6, 7].map(|i| n(&call["regs"][i])),
                    "fallback register arguments"
                );
            }
            for (i, v) in call["args"].as_array().unwrap().iter().enumerate() {
                assert_eq!(m.u32(c.esp + i as u32 * 4), n(v));
            }
            for (a, b) in pairs(&call["memory"]) {
                if !(0x0f000000..0x0f010000).contains(&a) {
                    assert_eq!(m.u8(a), b, "seed {seed}: call memory {a:x}");
                }
            }
            if call.get("reads").is_some() {
                for (a, b) in pairs(&call["reads"]) {
                    assert_eq!(m.u8(a), b, "callee read {a:x}");
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
        let mut got = [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let mut want = [0, 3, 4, 5, 6, 7].map(|i| n(&case["after"]["regs"][i]));
        if entry == 0xc24640 {
            got[0] = 0;
            want[0] = 0;
        } // void method
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
#[test]
fn original_bounds_tree() {
    verify(
        "E:/ddda-re/notes/native-L178/010a35d0.json",
        0x10a35d0,
        mtcore::semantic_jobs::query_bounds_tree,
    );
}

#[test]
fn original_instanced_mesh() {
    verify(
        "E:/ddda-re/notes/native-L178/00c24640.json",
        0xc24640,
        mtcore::semantic_instancing::draw_instanced_mesh,
    );
}

#[test]
fn original_sweep_support_shape() {
    verify(
        "E:/ddda-re/notes/native-L178/00d7cf70.json",
        0xd7cf70,
        mtcore::semantic_collision::sweep_support_shape,
    );
}

#[test]
fn rejects_heap_corruption() {
    let path = "E:/ddda-re/notes/native-L178/010a35d0.json";
    if !std::path::Path::new(path).exists() {
        assert!(std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none());
        return;
    }
    let error = std::panic::catch_unwind(|| {
        verify(path, 0x10a35d0, |m, c| {
            mtcore::semantic_jobs::query_bounds_tree(m, c);
            m.set_u32(0x0e000058, 0);
        })
    })
    .expect_err("heap corruption must fail");
    let message = error.downcast_ref::<String>().unwrap();
    assert!(message.contains("memory e000059"), "{message}");
}
