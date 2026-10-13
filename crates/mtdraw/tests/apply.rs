//! Private Unicorn vectors and (in the isolated runner) extracted lifted bodies.
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::BufRead;

// L125_REFERENCE_MODULE

#[derive(Default, Clone)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self.bytes.get(&a).unwrap_or(&0)
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.bytes.insert(a, v);
        self.writes.insert(a, v);
    }
}
fn number(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn words(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(number).collect()
}
fn bytes(v: &Value) -> BTreeMap<u32, u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| (number(&p[0]), number(&p[1]) as u8))
        .collect()
}
fn regs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}
fn xmm(v: &Value) -> [[u32; 4]; 8] {
    std::array::from_fn(|i| words(&v[i]).try_into().unwrap())
}
thread_local! {
    static CALLS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static EXPECTED: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static X87_CALLS: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}
fn mock(m: &mut dyn Mem, c: &mut Cpu, target: u32) {
    let n = match target {
        0xda7510 | 0xda77d0 | 0xda7b30 => 3,
        0xda7250 => 4,
        0xda68a0 => 0,
        t if (0x0d000000..0x0d001000).contains(&t) => match (t - 0x0d000000) / 16 {
            57 | 37 | 65 | 86 => 3,
            69 | 94 | 96 | 98 | 109 | 111 | 113 => 4,
            100 => 5,
            39 | 47 | 75 | 104 | 87 | 92 | 107 => 2,
            slot => panic!("unknown COM slot {slot}"),
        },
        _ => panic!("unknown call {target:x}"),
    };
    let args: Vec<_> = (0..n).map(|i| m.u32(c.esp + 4 + i * 4)).collect();
    let mut call = serde_json::json!({"target":target,"return_to":m.u32(c.esp),
                                 "regs":regs(c),"xmm":c.xmm,"args":args});
    let index = CALLS.with(|v| v.borrow().len());
    X87_CALLS.with(|v| {
        if let Some(status) = v.borrow().get(index) {
            assert_eq!(c.st.control(), 0x37f, "call {index} x87 control");
            assert_eq!(c.st.status(), *status, "call {index} x87 host status");
            assert_eq!(c.st.depth(), 0, "call {index} x87 depth");
        }
    });
    EXPECTED.with(|v| {
        let v = v.borrow();
        let expected = &v[index]["memory"];
        if let Some(values) = expected.as_array() {
            let actual: Vec<_> = values
                .iter()
                .map(|p| (number(&p[0]), m.u8(number(&p[0]))))
                .collect();
            call["memory"] = serde_json::to_value(actual).unwrap();
        }
    });
    let index = CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        calls.push(call);
        calls.len() as u32
    });
    c.eax = if target >= 0x0d000000 {
        0
    } else {
        0x51500000 | index
    };
    c.ecx = 0xc1c10000 ^ index;
    c.edx = 0xd2d20000 ^ index;
    c.esp += 4 + n * 4;
}
fn color(m: &mut dyn Mem, c: &mut Cpu) {
    mock(m, c, 0xda68a0);
}
fn lookup(t: u32) -> Option<LiftFn> {
    (t == 0xda68a0).then_some(color)
}

/// Independent host execution of the recorded original x87 instructions.
/// Translated from 0x00DAAE55..0x00DAAEBB and 0x00DAB325..0x00DAB38B
/// (DDDA.exe 2017-12-13 Steam). Keep the host's pre-test x87 image intact.
/// Unicorn is the data/control-flow oracle; hardware is the x87 status oracle.
fn hardware_x87(case: &Value) -> u16 {
    use std::arch::asm;
    let mut saved = [0u8; 108];
    let mut status: u16 = 0;
    unsafe {
        asm!("fnsave [{p}]", "fninit", p=in(reg) saved.as_mut_ptr(), options(nostack));
    }
    if let Some(ops) = case["x87_ops"].as_array() {
        for op in ops {
            let input = number(&op[1]);
            let mut output = 0u32;
            unsafe {
                match op[0].as_str().unwrap() {
                    "load" => asm!("fild dword ptr [{p}]",p=in(reg) &input,options(nostack)),
                    "one" => asm!("fld1", options(nostack)),
                    "divide" => asm!("fdivrp st(1), st(0)", options(nostack)),
                    "store" => asm!("fstp dword ptr [{p}]",p=in(reg) &mut output,options(nostack)),
                    _ => panic!("unknown recorded x87 instruction"),
                }
            }
        }
    }
    unsafe {
        asm!("fnstsw [{s}]", "frstor [{p}]",s=in(reg) &mut status,p=in(reg) saved.as_ptr(),options(nostack));
    }
    status
}

fn port(kind: &str) -> Option<LiftFn> {
    match kind {
        "streams" => Some(mtdraw::apply::streams),
        "targets" => Some(mtdraw::apply::targets),
        "fixed" => Some(mtdraw::apply::fixed),
        "shader" => Some(mtdraw::apply::shader),
        _ => None,
    }
}

fn run(body: LiftFn, case: &Value, label: &str) {
    let mut m = Memory {
        bytes: bytes(&case["initial"]),
        ..Default::default()
    };
    let r = words(&case["regs"]);
    let mut c = Cpu {
        eax: r[0],
        ecx: r[1],
        edx: r[2],
        ebx: r[3],
        esp: r[4],
        ebp: r[5],
        esi: r[6],
        edi: r[7],
        xmm: xmm(&case["xmm"]),
        ..Default::default()
    };
    CALLS.with(|v| v.borrow_mut().clear());
    let mut expected_calls = case["calls"].as_array().unwrap().clone();
    let mut statuses = vec![];
    for call in &mut expected_calls {
        if let Some(count) = call.as_object_mut().unwrap().remove("x87_op_count") {
            let ops = &case["x87_ops"].as_array().unwrap()[..number(&count) as usize];
            statuses.push(hardware_x87(&serde_json::json!({"x87_ops":ops})));
        }
    }
    X87_CALLS.with(|v| *v.borrow_mut() = statuses);
    EXPECTED.with(|v| *v.borrow_mut() = expected_calls.clone());
    body(&mut m, &mut c);
    let actual = CALLS.with(|v| v.borrow().clone());
    let expected = &expected_calls;
    assert_eq!(actual.len(), expected.len(), "{label} call count");
    for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, b, "{label} call {i}");
    }
    assert_eq!(regs(&c).as_slice(), words(&case["after"]), "{label} registers");
    assert_eq!(c.xmm, xmm(&case["after_xmm"]), "{label} xmm");
    let state = words(&case["x87"]);
    assert_eq!(c.st.control() as u32, state[0], "{label} x87 control");
    assert_eq!(
        c.st.status(),
        hardware_x87(case),
        "{label} x87 hardware status (Unicorn {})",
        state[1]
    );
    assert_eq!(c.st.depth(), 0, "{label} x87 depth");
    let expected = bytes(&case["writes"]);
    if m.writes != expected {
        let diff: Vec<_> = expected
            .iter()
            .filter(|(a, v)| m.writes.get(a) != Some(v))
            .take(12)
            .collect();
        let extra: Vec<_> = m
            .writes
            .iter()
            .filter(|(a, _)| !expected.contains_key(a))
            .take(12)
            .collect();
        panic!("{label} writes differ: expected {diff:x?}, extra {extra:x?}");
    }
}

#[test]
fn original_apply_abi() {
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    x86::set_ext_calls(Some(Box::new(mock)));
    let path = std::env::var("MTDRAW_APPLY_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L125/apply.jsonl".into());
    let file = match std::fs::File::open(&path) {
        Ok(t) => t,
        Err(e) => {
            assert!(std::env::var_os("MTDRAW_REQUIRE_APPLY").is_none(), "{path}: {e}");
            return;
        }
    };
    let mut count = 0;
    for (i, line) in std::io::BufReader::new(file).lines().enumerate() {
        let case: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let Some(kind) = case["kind"].as_str() else {
            continue;
        };
        if let Some(body) = port(kind) {
            run(body, &case, &format!("{kind} {i} readable"));
            // L125_REFERENCE_RUN
            count += 1;
        }
    }
    assert!(count > 0);
    eprintln!("{count} original ABI cases pass");
}
