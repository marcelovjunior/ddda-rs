//! L184 s5: the uCnsEdit::vf24 (0x00860AC0) semantic port against the
//! original's recorded vectors (research/native/record_cns_edit.py), at the
//! ABI level: EAX is not part of the ABI (void), the callee-saved registers,
//! ESP and every memory byte at or above the returned stack pointer are.
//! Each guest call the original makes is replayed from the recording and
//! checked first: its target, return address and stack arguments, the
//! registers it takes its inputs in, and the stack buffers it would read.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use serde_json::Value;
use std::collections::BTreeMap;

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

/// Run `body` on every case of `path`, replaying and checking the
/// recorded guest calls. `compare_registers(target)` says which of
/// [eax, ecx, edx, ebx, esp, ebp, esi, edi] the callee takes as inputs.
fn cases(path: &str, body: x86::LiftFn, input_registers: fn(u32) -> &'static [usize], returns: bool) -> usize {
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
        let events = std::rc::Rc::new(case["events"].as_array().unwrap().clone());
        let cursor = std::rc::Rc::new(std::cell::RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            let event = trace.get(k).unwrap_or_else(|| panic!("case {i}: unexpected call to {target:x}"));
            let (call, back) = (&event["before"], &event["after"]);
            assert_eq!(target, n(&event["target"]), "case {i}: call {k} target");
            let want = regs(call);
            assert_eq!(c.esp, want[4], "case {i}: call {k} ({target:x}) ESP");
            for &r in input_registers(target) {
                let got = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi][r];
                assert_eq!(got, want[r], "case {i}: call {k} ({target:x}) register {r}");
            }
            // return address and stack arguments
            for (a, b) in pairs(&call["memory"]) {
                assert_eq!(m.u8(a), b, "case {i}: call {k} ({target:x}) stack {a:x}");
            }
            // the buffers the callee reads
            for read in event["reads"].as_array().unwrap() {
                let base = n(&read[0]);
                for (j, b) in read[1].as_array().unwrap().iter().enumerate() {
                    assert_eq!(m.u8(base + j as u32), n(b) as u8, "case {i}: call {k} ({target:x}) buffer {base:x}+{j:x}");
                }
            }
            for (a, b) in pairs(&back["memory"]) {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
            c.esp = regs(back)[4];
            *position.borrow_mut() += 1;
        })));
        if std::env::var_os("S5_TRACE").is_some() {
            eprintln!("case {i} mode {}", case["mode"]);
        }
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
        let got = [c.ebx, c.esp, c.ebp, c.esi, c.edi];
        let want = [after[3], after[4], after[5], after[6], after[7]];
        assert_eq!(got, want, "case {i} (mode {}): ebx/esp/ebp/esi/edi", case["mode"]);
        if returns {
            assert_eq!(c.eax, after[0], "case {i}: eax");
        }
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if a >= esp {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i} (mode {}): memory {a:x}", case["mode"]);
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

/// The registers each recorded callee takes inputs in.
fn input_registers(target: u32) -> &'static [usize] {
    match target {
        0x00cf_dc80 => &[0, 1],            // blend: EAX = &identity, ECX = &matrix
        0x0086_4660..=0x0086_d180 => &[0, 1, 6, 7], // matrix providers: EAX, ECX, ESI, EDI
        _ => &[0, 1],                       // the parent accessor (ECX = model)
    }
}

// verifies 0x00860ac0 (ABI level; void)
#[test]
fn semantic_cns_edit_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S5_CNS_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/cns_edit.json".into());
    let count = cases(&path, mtcore::semantic_s5::constraint_edit_apply, input_registers, false);
    eprintln!("{count} ABI-level uCnsEdit::vf24 cases passed");
}

// verifies 0x012f0906 / 0x012f0a55 (the CRT sinf/cosf, bit for bit)
#[test]
fn semantic_crt_sincos_matches_original() {
    use ddnative_base::mem::Pages;
    ddnative_host::install();
    let path = std::env::var("S5_SINCOS_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/cns_sincos.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut m = Memory::default();
    for (name, base) in [("sin", 0x0144_b410u32), ("cos", 0x0144_b870)] {
        let hex = data["tables"][name].as_str().unwrap();
        for i in 0..hex.len() / 2 {
            m.bytes.insert(base + i as u32, u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap());
        }
    }
    let (mut fast, mut slow) = (0, 0);
    for (i, case) in data["cases"].as_array().unwrap().iter().enumerate() {
        let cosine = case["fn"] == "cos";
        let bits = n(&case["x"]);
        let mut c = Cpu::default();
        c.esp = 0x0f00_8000;
        if case["slow"].as_bool().unwrap() {
            // the x87 range reduction: handed to the guest function itself
            let target = if cosine { 0x012f_0a55 } else { 0x012f_0906 };
            x86::set_ext_calls(Some(Box::new(move |_, c, t| {
                assert_eq!(t, target);
                assert_eq!(c.xmm[0][0], bits);
                c.xmm[0][0] = 0x1234_5678;
            })));
            let pg = Pages::of(&m);
            let got = mtcore::semantic_s5::crt_sincos(&mut m, &mut c, pg, 0x0f00_8000, f32::from_bits(bits), cosine, 0x0086_0b54);
            x86::set_ext_calls(None);
            assert_eq!(got.to_bits(), 0x1234_5678, "case {i}");
            slow += 1;
        } else {
            let pg = Pages::of(&m);
            let got = mtcore::semantic_s5::crt_sincos(&mut m, &mut c, pg, 0x0f00_8000, f32::from_bits(bits), cosine, 0);
            assert_eq!(got.to_bits(), n(&case["out"]), "case {i}: {} of {bits:08x}", case["fn"]);
            fast += 1;
        }
    }
    eprintln!("{fast} native + {slow} guest-delegated sin/cos cases passed");
}

/// Registers the callee takes inputs in, as above, for the parent-space edit
/// (0x0086b2e0): only its virtual accessor is called.
// verifies 0x0086b2e0 (ABI level; EAX = the output buffer)
#[test]
fn semantic_scale_translate_in_parent_space_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S5_PARENT_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/cns_parent.json".into());
    let count = cases(&path, mtcore::semantic_s5::scale_translate_in_parent_space, input_registers, true);
    eprintln!("{count} ABI-level 0x0086b2e0 cases passed");
}

// verifies 0x00864660 (ABI level; EAX = the output buffer)
#[test]
fn semantic_aim_twist_provider_matches_original() {
    ddnative_host::install();
    let path = std::env::var("S5_AIM_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L184/cns_aim.json".into());
    let count = cases(&path, mtcore::semantic_s5::aim_twist_provider, input_registers, true);
    eprintln!("{count} ABI-level 0x00864660 cases passed");
}
