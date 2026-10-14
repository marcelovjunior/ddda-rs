//! L184 s3: semantic ports against the original's recorded vectors, at the
//! ABI level: EAX (when the function returns one), EBX/EBP/ESI/EDI, ESP and
//! every memory byte at or above the returned stack pointer, plus every
//! guest call (target, ESP, the registers the callee reads, its stack
//! arguments). ECX/EDX, the flags and the popped scratch are not compared.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        *self.bytes.get(&a).unwrap_or_else(|| panic!("unrecorded read {a:x}"))
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
    v.as_array().unwrap().iter().map(|p| (n(&p[0]), n(&p[1]) as u8)).collect()
}

fn regs(v: &Value) -> [u32; 8] {
    let r: Vec<u32> = v["regs"].as_array().unwrap().iter().map(n).collect();
    r.try_into().unwrap()
}

/// What a recorded callee reads: its stack argument words and the registers
/// (indices into [eax, ecx, edx, ebx, esp, ebp, esi, edi]) that carry
/// inputs. ESP is always compared.
struct Callee {
    stack_words: u32,
    registers: &'static [usize],
    xmm_inputs: bool,
}

const EAX: usize = 0;
const ECX: usize = 1;
const ESI: usize = 6;
const EDI: usize = 7;
/// Pseudo register: only the low byte of EDX is compared.
const DL: usize = 100;

fn cpu_regs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}

/// Run `body` on every case of `path`, replaying the recorded callee events.
fn abi_cases(path: &str, body: LiftFn, returns: bool, callee: fn(u32) -> Callee) -> usize {
    abi_cases_checked(path, body, returns, callee, |_, _, _| {})
}

/// As `abi_cases`, with `extra` run on the CPU after every case (to check
/// state the ABI comparison does not cover, such as ST0).
fn abi_cases_checked(path: &str, body: LiftFn, returns: bool, callee: fn(u32) -> Callee, extra: fn(&mut Cpu, &Value, usize)) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return 0,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let before = regs(&case["before"]);
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
        let events = Rc::new(case["events"].as_array().unwrap().clone());
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            assert!(k < trace.len(), "case {i}: extra call {target:x}");
            let (call, back) = (&trace[k]["before"], &trace[k]["after"]);
            assert_eq!(target, n(&trace[k]["target"]), "case {i}: call {k} target");
            let want = regs(call);
            let have = cpu_regs(c);
            let spec = callee(target);
            assert_eq!(have[4], want[4], "case {i}: call {k} {target:x} ESP");
            for &r in spec.registers {
                if r == DL {
                    assert_eq!(have[2] & 0xff, want[2] & 0xff, "case {i}: call {k} {target:x} DL");
                    continue;
                }
                assert_eq!(have[r], want[r], "case {i}: call {k} {target:x} register {r}");
            }
            if spec.xmm_inputs {
                for j in 0..2 {
                    let want: Vec<u32> = call["xmm"][j].as_array().unwrap().iter().map(n).collect();
                    assert_eq!(c.xmm[j].to_vec(), want, "case {i}: call {k} {target:x} xmm{j}");
                }
            }
            let recorded = pairs(&call["memory"]);
            let esp = want[4];
            for (&a, &b) in &recorded {
                if a >= esp && a < esp + 4 * (1 + spec.stack_words) {
                    assert_eq!(m.u8(a), b, "case {i}: call {k} {target:x} memory {a:x}");
                }
            }
            for (a, b) in pairs(&back["memory"]) {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
            c.esp = regs(back)[4];
            if let Some(v) = trace[k].get("st0f") {
                c.st.push(n(v));
            }
            if let Some(v) = trace[k].get("st0d") {
                let mut d = [0u8; 10];
                d[..8].copy_from_slice(&v.as_u64().unwrap().to_le_bytes());
                c.st.exec::<0x00dd>(&mut d);
            }
            if let Some(v) = trace[k].get("xmm0") {
                c.xmm[0] = [n(v), 0, 0, 0];
            }
            *position.borrow_mut() += 1;
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        extra(&mut c, case, i);
        let eax = if returns { [c.eax, after[0]] } else { [0, 0] };
        let got = [eax[0], c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [eax[1], after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i}: eax/ebx/esp/ebp/esi/edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if a >= esp {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if a >= esp {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}


fn cascade_callee(target: u32) -> Callee {
    match target {
        0x00fe_d950 | 0x00fe_d940 | 0x00fe_d920 => Callee { stack_words: 0, registers: &[ECX], xmm_inputs: false },
        0x012f_2f20 => Callee { stack_words: 0, registers: &[], xmm_inputs: true },
        0x012f_3690 => Callee { stack_words: 2, registers: &[], xmm_inputs: false },
        0x00cf_d5e0 => Callee { stack_words: 3, registers: &[ECX], xmm_inputs: false },
        0x00cf_c8b0 => Callee { stack_words: 4, registers: &[ECX], xmm_inputs: false },
        _ => panic!("unexpected callee {target:x}"),
    }
}

/// The recorded ST0 (10 bytes of the extended value) against the CPU's,
/// then the x87 state is released.
fn check_st0(c: &mut Cpu, case: &Value, i: usize) {
    let want: Vec<u8> = case["st0"].as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect();
    let (mantissa, exponent) = c.st.top_extended().unwrap_or_else(|| panic!("case {i}: x87 stack empty"));
    let mut have = mantissa.to_le_bytes().to_vec();
    have.extend(exponent.to_le_bytes());
    assert_eq!(have, want, "case {i}: ST0");
    assert_eq!(c.st.depth() as u64, case["depth"].as_u64().unwrap(), "case {i}: x87 depth");
    (ddnative_base::hooks::get().x87_release)(&mut c.st);
}

// verifies 0x00ee5ce0 (ABI level; ST0 result)
#[test]
fn semantic_setup_cascade_view_matches_original() {
    ddnative_host::install();
    let n = abi_cases_checked("E:/ddda-re/notes/native-L184/cascade_view.json", mtcore::semantic_s3b::setup_cascade_view, false, cascade_callee, check_st0);
    eprintln!("{n} ABI-level setup_cascade_view cases passed");
}

/// Cost of one hardware x87 step through `c.st.exec` (the port does about 22 per call).
#[test]
#[ignore = "microbenchmark"]
fn x87_step_cost() {
    ddnative_host::install();
    let mut c = Cpu::default();
    let start = std::time::Instant::now();
    let iterations = 200_000u32;
    for _ in 0..iterations {
        let mut d = [0u8; 10];
        d[..4].copy_from_slice(&1.5f32.to_le_bytes());
        c.st.exec::<0x00d9>(&mut d); // fld m32
        c.st.exec::<0x08d8>(&mut d); // fmul m32
        let mut out = [0u8; 10];
        c.st.exec::<0x18d9>(&mut out); // fstp m32
    }
    eprintln!("{:.1} ns per x87 step", start.elapsed().as_nanos() as f64 / (3 * iterations) as f64);
    (ddnative_base::hooks::get().x87_release)(&mut c.st);
}
