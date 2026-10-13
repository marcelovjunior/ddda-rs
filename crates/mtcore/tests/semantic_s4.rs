//! L184 (s4): semantic ports against the original's recorded vectors, at
//! the ABI level (see tests/semantic.rs, whose harness this copies).
use ddnative_base::{
    mem::Mem,
    x86::{Cpu, LiftFn},
};
use serde_json::Value;
use std::collections::BTreeMap;

thread_local! {
    static LENIENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Default)]
struct Memory {
    bytes: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        match self.bytes.get(&a) {
            Some(v) => *v,
            // a callee's input byte the port never wrote: the guest had zero there
            None if LENIENT.with(|l| l.get()) => 0,
            None => panic!("unrecorded read {a:x}"),
        }
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

/// Stack arguments of the callees the vectors record (thiscall: ECX is
/// checked separately).
fn stack_args(target: u32) -> u32 {
    match target {
        // 0x00E70710's callees: 0x00E6E230(ticks) and the vtable +0x64 hook
        // (0x30000164 in the vectors) take one argument, the rest none
        0x00e6_e230 | 0x3000_0164 => 1,
        // 0x00B66B30's callees: 0x00B66320(state), 0x00B71B90(flag), 0x01054860(1),
        // 0x00421ED0(index), 0x00E4B700(vector), 0x0081F8F0(object), the vtable
        // +0x210/+0x70 hooks and the input getter (0x3000103c) take one argument
        0x00b6_6320 | 0x00b7_1b90 | 0x0105_4860 | 0x0042_1ed0 | 0x00e4_b700 | 0x0081_f8f0 => 1,
        0x3000_0210 | 0x3000_0070 | 0x3000_103c => 1,
        0x007b_0580 => 3,
        0x0081_fd10 | 0x0083_7980 => 2,
        // 0x01054860's callees: 0x01054700(indices, count, flags), 0x00FFA620(0)
        0x0105_4700 => 3,
        // 0x00B72360's callees: 0x00DE45C0 (ten arguments), 0x00DE4710 and 0x00DEA870 (four)
        0x00de_45c0 => 10,
        0x00de_4710 | 0x00de_a870 => 4,
        // 0x0081FD10's callee 0x00421ED0(index)

        0x00ff_a620 => 1,
        _ => 0,
    }
}

/// As `abi_cases`, for a body that makes guest calls: each recorded call
/// must see the recorded memory (its arguments and the buffers they point
/// at) and ECX; the recorded callee's effects and EAX are then replayed.
fn abi_cases_with_calls(path: &str, body: LiftFn) -> usize {
    abi_cases_with_calls_ret(path, body, true)
}

/// `returns`: false for a void function, whose EAX is left over and not
/// part of its ABI.
fn abi_cases_with_calls_ret(path: &str, body: LiftFn, returns: bool) -> usize {
    abi_cases_full(path, body, returns, &[])
}

/// `native_callees`: recorded calls the semantic port does natively (a
/// CRT memcpy, say): their events are dropped and the final memory check
/// covers their effects.
fn abi_cases_full(path: &str, body: LiftFn, returns: bool, native_callees: &[u32]) -> usize {
    use ddnative_base::x86;
    use std::{cell::RefCell, rc::Rc};
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
        let events: Vec<Value> = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e.get("target").map_or(true, |t| !native_callees.contains(&n(t))))
            .cloned()
            .collect();
        let events = Rc::new(events);
        let cursor = Rc::new(RefCell::new(0usize));
        let (trace, position) = (events.clone(), cursor.clone());
        // Two recorded formats: a "call" event followed by its "return"
        // event (keys.json), or one event per call with "before" and
        // "after" states (the L165 corpora).
        let paired = events.first().is_some_and(|e| e.get("kind").is_some());
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let k = *position.borrow();
            let (call, back) = if paired {
                (&trace[k], &trace[k + 1])
            } else {
                (&trace[k]["before"], &trace[k]["after"])
            };
            let target_recorded = if paired { n(&trace[k]["target"]) } else { n(&trace[k]["target"]) };
            assert_eq!(target, target_recorded, "case {i}: call target");
            // callees that take their object in another register leave ECX undefined
            let ecx_unused = matches!(
                target,
                0x012f_0906 | 0x012f_0a55 | 0x012f_0d4c | 0x012f_1109 | 0x0078_a730 | 0x0085_0440 | 0x0085_08c0 | 0x0081_f2d0 | 0x00b7_1b90 | 0x00b7_2360 | 0x007b_0580 | 0x0081_fd10
                    | 0x0078_b4c0 | 0x0081_fc60 | 0x00b6_61f0 | 0x00b6_7930 | 0x00b7_2d30 | 0x00b6_6320 | 0x0081_f8f0 | 0x0083_7980
            );
            if !ecx_unused {
                assert_eq!(c.ecx, regs(call)[1], "case {i}: call {target:x} ECX");
            }
            // the other registers the callee takes as inputs (0x00B66B30's callees)
            let inputs: &[(usize, &str)] = match target {
                0x0078_a730 | 0x0085_0440 | 0x0085_08c0 | 0x0081_f2d0 | 0x00b7_1b90 | 0x00b7_2360 => &[(6, "ESI")],
                0x007b_0580 => &[(6, "ESI"), (7, "EDI")],
                0x0081_fd10 => &[(7, "EDI")],
                0x0078_b4c0 | 0x0081_fc60 | 0x00b6_61f0 | 0x00b6_7930 | 0x00b7_2d30 => &[(0, "EAX")],
                0x00b6_6320 => &[(2, "EDX")],
                _ => &[],
            };
            // XMM lanes the callee reads (the CRT math functions take floats in XMM0/XMM1)
            if let Some(list) = call.get("xmm") {
                for entry in list.as_array().unwrap() {
                    let (reg, lane, want) = (n(&entry[0]) as usize, n(&entry[1]) as usize, n(&entry[2]));
                    assert_eq!(c.xmm[reg][lane], want, "case {i}: call {target:x} XMM{reg} lane {lane}");
                }
            }
            // vectors the callee reads through a pointer (0x00E4B700, 0x007B0580)
            if let Some(data) = call.get("data") {
                for (a, b) in pairs(data) {
                    // a byte the port never touched stays whatever the guest had (zero in the recording)
                    LENIENT.with(|l| l.set(true));
                    let got = m.u8(a);
                    LENIENT.with(|l| l.set(false));
                    assert_eq!(got, b, "case {i}: call {target:x} vector byte {a:x}");
                }
            }
            for &(index, name) in inputs {
                let got = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi][index];
                assert_eq!(got, regs(call)[index], "case {i}: call {target:x} {name}");
            }
            assert_eq!(c.esp, regs(call)[4], "case {i}: call ESP");
            // What the callee reads: its return address, its three
            // arguments and the buffer the first two describe. The rest of
            // the recorded window is the caller's own frame (saved
            // registers, locals), which a semantic port need not rebuild.
            let recorded = pairs(&call["memory"]);
            let esp = regs(call)[4];
            let word = |a: u32| u32::from_le_bytes([0, 1, 2, 3].map(|k| recorded[&(a + k)]));
            // What the callee reads: its return address and arguments, and
            // for the interning call (0xE0E5E0: buffer, byte length, basis)
            // the buffer. The rest of the recorded window is the caller's
            // own frame (saved registers, locals).
            let (buffer, len) = if target == 0x00e0_e5e0 { (word(esp + 4), word(esp + 8)) } else { (0, 0) };
            // the return address and the callee's own stack arguments
            let words = 1 + stack_args(target);
            for (&a, &b) in &recorded {
                let argument = a >= esp && a < esp + 4 * words;
                let data = a >= buffer && a < buffer.wrapping_add(len);
                if argument || data {
                    assert_eq!(m.u8(a), b, "case {i}: call {target:x} memory {a:x}");
                }
            }
            let effects = if paired { pairs(&back["effects"]) } else { pairs(&back["memory"]) };
            for (a, b) in effects {
                m.set_u8(a, b);
            }
            c.eax = regs(back)[0];
            c.esp = regs(back)[4];
            // XMM lanes the callee returns
            if let Some(list) = back.get("xmm") {
                for entry in list.as_array().unwrap() {
                    c.xmm[n(&entry[0]) as usize][n(&entry[1]) as usize] = n(&entry[2]);
                }
            }
            // a float returned in ST0 (the input getter)
            if let Some(bits) = trace[k].get("st0") {
                c.st.push(n(bits));
            }
            *position.borrow_mut() += if paired { 2 } else { 1 };
        })));
        body(&mut m, &mut c);
        x86::set_ext_calls(None);
        assert_eq!(*cursor.borrow(), events.len(), "case {i}: calls made");
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


// verifies 0x00e70710 (ABI level; void)
#[test]
fn semantic_actor_update_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/actor_update.json", mtcore::semantic_s4::actor_update, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level actor_update cases passed");
}

// verifies 0x00b66b30 (ABI level; void)
#[test]
fn semantic_player_post_update_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/player_post_update.json", mtcore::semantic_s4_player::player_post_update, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level player_post_update cases passed");
}

// verifies 0x01054860 (ABI level; void)
#[test]
fn semantic_model_refresh_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/model_refresh.json", mtcore::semantic_s4_model::model_refresh, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level model_refresh cases passed");
}

// The ports copy floats the way the original's `FLD m32; FSTP m32` pairs do:
// a signalling NaN comes back quiet, everything else bit for bit (Unicorn,
// which recorded the vectors, does not quiet it, so the vectors hold only
// quiet NaNs). Checked here against the host's own x87.
#[cfg(target_arch = "x86_64")]
#[test]
fn x87_float_copy_quiets_signalling_nans_only() {
    let through_x87 = |bits: u32| -> u32 {
        let mut out = 0u32;
        // SAFETY: loads and stores one f32 through the x87 stack, leaving it balanced.
        unsafe {
            std::arch::asm!("fld dword ptr [{src}]", "fstp dword ptr [{dst}]", src = in(reg) &bits, dst = in(reg) &mut out);
        }
        out
    };
    for bits in [0x7f80_0001u32, 0xff80_0001, 0x7fa0_0000, 0x7fc0_0000, 0x7f80_0000, 0x0000_0001, 0x8000_0000, 0x3f80_0000, 0xffff_ffff] {
        let expected = if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 { bits | 0x0040_0000 } else { bits };
        assert_eq!(through_x87(bits), expected, "{bits:08x}");
    }
}

// verifies 0x0081fd10 (ABI level; void)
#[test]
fn semantic_joint_rebase_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/joint_rebase.json", mtcore::semantic_s4_joint::joint_rebase, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level joint_rebase cases passed");
}

// verifies 0x0078a730 (ABI level; void)
#[test]
fn semantic_aim_update_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls_ret("E:/ddda-re/notes/native-L184/actor_aim.json", mtcore::semantic_s4_aim::aim_update, false);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level aim_update cases passed");
}

// verifies 0x00b72360 (ABI level; returns AL)
#[test]
fn semantic_probe_ground_matches_original() {
    ddnative_host::install();
    let n = abi_cases_with_calls("E:/ddda-re/notes/native-L184/player_probe.json", mtcore::semantic_s4_probe::probe_ground);
    assert!(n >= 1000 || std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(), "only {n} cases");
    eprintln!("{n} ABI-level probe_ground cases passed");
}
