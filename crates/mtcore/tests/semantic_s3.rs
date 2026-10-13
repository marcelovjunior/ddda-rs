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

fn finish_draw_callee(target: u32) -> Callee {
    match target {
        0x0076_f740 => Callee { stack_words: 5, registers: &[EDI] },
        0x0088_4270 | 0x0076_b470 | 0x0079_4550 => Callee { stack_words: 1, registers: &[] },
        0x007a_5fa0 | 0x00c5_2c50 => Callee { stack_words: 0, registers: &[ESI] },
        0x005d_1af0 => Callee { stack_words: 2, registers: &[] },
        0x00c5_40d0 => Callee { stack_words: 0, registers: &[EAX] },
        0x0049_e300 => Callee { stack_words: 1, registers: &[EAX] },
        // the unit's virtual methods (+0x50, +0x54, +0x14) and the held
        // objects' +0x20: thiscall without arguments
        0x3000_0050 | 0x3000_0054 | 0x3000_0020 | 0x3000_0014 => Callee { stack_words: 0, registers: &[ECX] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x00c52a30 (ABI level; void)
#[test]
fn semantic_finish_draw_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/unit_draw_finish.json",
        mtcore::semantic_s3::finish_draw,
        false,
        finish_draw_callee,
    );
    eprintln!("{n} ABI-level finish_draw cases passed");
}

fn update_model_bounds_callee(target: u32) -> Callee {
    match target {
        0x00ea_3d00 => Callee { stack_words: 0, registers: &[ECX] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x00ffa620 (ABI level; void, `ret 4`)
#[test]
fn semantic_update_model_bounds_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/model_bounds.json",
        mtcore::semantic_s3::update_model_bounds,
        false,
        update_model_bounds_callee,
    );
    eprintln!("{n} ABI-level update_model_bounds cases passed");
}

// Backed pages exercise the same fast accessor path as the parallel runtime.
#[derive(Default)]
struct Paged {
    table: ddnative_base::mem::PageTable,
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        Some(std::ptr::NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a >> 12) as usize);
        assert!(!p.is_null());
        // SAFETY: this table owns the backed page for the lifetime of self.
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: this table owns the backed page and a is inside it.
        unsafe {
            ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32);
        }
    }
}

/// ns/call of `body` on the recorded case with the most instructions of the
/// original (`insns`), over backed pages, callees replaced by instant stubs.
fn bench(path: &str, name: &str, body: LiftFn, callee: fn(u32) -> Callee) {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = data["cases"].as_array().unwrap();
    // the cost of the two clock reads around every call
    let timer = {
        let mut elapsed = std::time::Duration::ZERO;
        for _ in 0..200_000 {
            let start = std::time::Instant::now();
            std::hint::black_box(&start);
            elapsed += start.elapsed();
        }
        elapsed.as_nanos() as f64 / 200_000.0
    };
    let mean = cases.iter().map(|c| c["insns"].as_f64().unwrap()).sum::<f64>() / cases.len() as f64;
    // the busiest case, and a middle one, by the original's instruction count
    let mut order: Vec<usize> = (0..cases.len()).collect();
    order.sort_by_key(|&i| cases[i]["insns"].as_u64().unwrap());
    for (label, index) in [("median", order[order.len() / 2]), ("busiest", *order.last().unwrap())] {
        let case = &cases[index];
        let initial = pairs(&case["initial"]);
        let before = regs(&case["before"]);
        let mut m = Paged::default();
        x86::set_ext_calls(Some(Box::new(move |_, c, target| {
            c.esp = c.esp.wrapping_add(4 * (1 + callee(target).stack_words));
        })));
        let mut best = f64::MAX;
        for _round in 0..5 {
            let mut elapsed = std::time::Duration::ZERO;
            for iteration in 0..20020 {
                for (&a, &b) in &initial {
                    m.set_u8(a, b);
                }
                let mut c = Cpu::default();
                [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = before;
                let start = std::time::Instant::now();
                body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                if iteration >= 20 {
                    elapsed += start.elapsed();
                }
            }
            best = best.min(elapsed.as_nanos() as f64 / 20000.0 - timer);
        }
        x86::set_ext_calls(None);
        eprintln!(
            "{name} {label}: original executes {} instructions (mean {mean:.0}); port {best:.1} ns/call (clock overhead {timer:.0} ns removed; {} guest calls stubbed)",
            case["insns"],
            case["events"].as_array().unwrap().len()
        );
    }
}

#[test]
#[ignore = "microbenchmark"]
fn finish_draw_cost() {
    bench("E:/ddda-re/notes/native-L184/unit_draw_finish.json", "finish_draw", mtcore::semantic_s3::finish_draw, finish_draw_callee);
}

#[test]
#[ignore = "microbenchmark"]
fn update_model_bounds_cost() {
    bench("E:/ddda-re/notes/native-L184/model_bounds.json", "update_model_bounds", mtcore::semantic_s3::update_model_bounds, update_model_bounds_callee);
}

fn update_scene_objects_callee(target: u32) -> Callee {
    match target {
        0x0076_f2d0 | 0x00f7_7bf0 | 0x0110_7d70 | 0x0077_9bf0 => Callee { stack_words: 1, registers: &[] },
        0x0110_6e20 => Callee { stack_words: 2, registers: &[] },
        0x0077_3830 => Callee { stack_words: 0, registers: &[ESI, EDI] },
        0x3000_003c => Callee { stack_words: 3, registers: &[ECX] },
        0x00ed_5370 => Callee { stack_words: 6, registers: &[ECX] },
        0x00ed_5360 => Callee { stack_words: 2, registers: &[ECX] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x0076b470 (ABI level; void, `ret 4`)
#[test]
fn semantic_update_scene_objects_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/scene_update.json",
        mtcore::semantic_s3::update_scene_objects,
        false,
        update_scene_objects_callee,
    );
    eprintln!("{n} ABI-level update_scene_objects cases passed");
}

fn sync_runtime_object_callee(target: u32) -> Callee {
    match target {
        // the import (stdcall, one argument) and the allocator (cdecl, two)
        0x3000_0100 | 0x007a_65c0 => Callee { stack_words: 1, registers: &[] },
        0x00f0_e170 => Callee { stack_words: 2, registers: &[] },
        0x00f1_2400 | 0x00f2_3830 | 0x00f2_3710 | 0x00de_0b90 | 0x00f0_ebc0 | 0x00f0_eba0 => Callee { stack_words: 0, registers: &[ECX] },
        0x00db_58b0 => Callee { stack_words: 4, registers: &[ECX] },
        0x00f2_3f80 | 0x00f0_ea30 | 0x00f0_eb60 => Callee { stack_words: 1, registers: &[ECX] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x007a5fa0 (ABI level; void, ESI = the record)
#[test]
fn semantic_sync_runtime_object_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/proxy_update.json",
        mtcore::semantic_s3::sync_runtime_object,
        false,
        sync_runtime_object_callee,
    );
    eprintln!("{n} ABI-level sync_runtime_object cases passed");
}

fn apply_effect_toggle_callee(target: u32) -> Callee {
    match target {
        0x0108_3990 | 0x0108_4210 | 0x0108_19e0 => Callee { stack_words: 0, registers: &[ECX] },
        // ESI = the object and DL = the state (compared as the low byte, see DL)
        0x0088_d660 => Callee { stack_words: 0, registers: &[ESI, DL] },
        0x0108_06c0 => Callee { stack_words: 1, registers: &[ECX] },
        0x0079_4a90 => Callee { stack_words: 1, registers: &[EDI] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x00794550 (ABI level; void, `ret 4`; 0x010805D0, 0x011181F0 and
// 0x00F65CB0 are done natively and ran as original code in the recording)
#[test]
fn semantic_apply_effect_toggle_matches_original() {
    ddnative_host::install();
    let n = abi_cases(
        "E:/ddda-re/notes/native-L184/effect_toggle.json",
        mtcore::semantic_s3::apply_effect_toggle,
        false,
        apply_effect_toggle_callee,
    );
    eprintln!("{n} ABI-level apply_effect_toggle cases passed");
}

/// The recorded ST0 (10 bytes of the extended value) against the CPU's,
/// then the x87 state is released.
fn check_st0(c: &mut Cpu, case: &Value, i: usize) {
    let want: Vec<u8> = case["st0"].as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect();
    let (mantissa, exponent) = c.st.top_extended().unwrap_or_else(|| panic!("case {i}: x87 stack empty"));
    let mut have = mantissa.to_le_bytes().to_vec();
    have.extend(exponent.to_le_bytes());
    assert_eq!(have, want, "case {i}: ST0");
    assert_eq!(c.st.depth(), 1, "case {i}: x87 depth");
    (ddnative_base::hooks::get().x87_release)(&mut c.st);
}

fn no_callees(target: u32) -> Callee {
    panic!("unexpected callee {target:x}")
}

// verifies 0x00d53730 (ABI level; ST0 = distance^2)
#[test]
fn semantic_closest_point_on_box_matches_original() {
    ddnative_host::install();
    let n = abi_cases_checked("E:/ddda-re/notes/native-L184/box_closest.json", mtcore::semantic_s3::closest_point_on_box, false, no_callees, check_st0);
    eprintln!("{n} ABI-level closest_point_on_box cases passed");
}

// verifies 0x00d53f30 (ABI level; ST0 = distance^2)
#[test]
fn semantic_distance_sq_to_box_matches_original() {
    ddnative_host::install();
    let n = abi_cases_checked("E:/ddda-re/notes/native-L184/box_wrapper.json", mtcore::semantic_s3::distance_sq_to_box, false, no_callees, check_st0);
    eprintln!("{n} ABI-level distance_sq_to_box cases passed");
}

// verifies 0x00d13ab0 (ABI level; includes the radius left in the argument slot)
#[test]
fn semantic_sphere_touches_box_matches_original() {
    ddnative_host::install();
    let n = abi_cases("E:/ddda-re/notes/native-L184/box_touch.json", mtcore::semantic_s3::sphere_touches_box, true, no_callees);
    eprintln!("{n} ABI-level sphere_touches_box cases passed");
}

/// A float with the bit patterns that stress extended rounding: wide and
/// equal exponents, denormals, few-bit significands.
fn random_float(r: u64) -> u32 {
    let sign = ((r >> 40) as u32 & 1) << 31;
    let fraction = (r as u32) & 0x7f_ffff;
    let field = match (r >> 33) & 7 {
        0 => 0,                                // zero / denormal
        1 | 2 => 120 + ((r >> 8) as u32 & 15), // a narrow band: ties and cancellation
        3 => 1 + ((r >> 12) as u32 % 254),     // everything
        _ => 100 + ((r >> 12) as u32 % 54),
    };
    let fraction = if (r >> 36) & 3 == 0 { fraction & 0x7f_f000 } else { fraction }; // few-bit significands
    sign | (field << 23) | fraction
}

/// The software extended arithmetic against the x87 hardware on random
/// inputs: wide and equal exponents, denormals, near-cancellation, ties.
#[test]
fn software_extended_squared_distance_matches_x87() {
    ddnative_host::install();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut c = Cpu::default();
    let mut compared = 0u32;
    for case in 0..300_000u32 {
        // axes in exponent bands 0, 30..34 or 62..66 apart: the squares are
        // 60 to 130 bits apart, where the sum's sticky bit decides ties
        let shift = [0u32, 30, 31, 32, 33, 34, 62, 63, 64, 65, 66];
        let offsets = [0, 1, 2].map(|_| shift[(next() % shift.len() as u64) as usize]);
        let near = |bits: u32, offset: u32| if (bits >> 23) & 0xff == 0 || (bits >> 23) & 0xff > 0x7f { bits } else { bits.wrapping_add(offset << 23) };
        let point = [0, 1, 2].map(|k| near(random_float(next()), offsets[k] / 2));
        // the clamped copy is either the point itself or another value
        let clamped = [0, 1, 2].map(|k| if next() & 3 == 0 { point[k] } else { near(random_float(next()), offsets[k] / 2) });
        let want = mtcore::semantic_s3::squared_distance_software_register(point, clamped);
        mtcore::semantic_s3::squared_distance_hardware(&mut c, point, clamped);
        let have = c.st.top_extended().unwrap();
        (ddnative_base::hooks::get().x87_release)(&mut c.st);
        c.st = Default::default();
        if let Some(want) = want {
            assert_eq!(have, want, "case {case}: {point:x?} {clamped:x?}");
            compared += 1;
        }
    }
    assert!(compared > 100_000);
}

fn leaf_callee(target: u32) -> Callee {
    match target {
        // the virtual +0x30 of the root: thiscall with the out vector
        0x3000_0030 => Callee { stack_words: 1, registers: &[ECX] },
        0x00ff_0780 | 0x0111_d610 => Callee { stack_words: 1, registers: &[ECX] },
        0x00ff_0ad0 => Callee { stack_words: 2, registers: &[ECX] },
        _ => panic!("unexpected callee {target:x}"),
    }
}

// verifies 0x00ea1eb0 (ABI level; only AL is the answer, the rest of EAX is compared too)
#[test]
fn semantic_copy_position_matches_original() {
    ddnative_host::install();
    let n = abi_cases("E:/ddda-re/notes/native-L184/leaf_copy16.json", mtcore::semantic_s3::copy_position, true, leaf_callee);
    eprintln!("{n} ABI-level copy_position cases passed");
}

// verifies 0x00ea1f40 (ABI level)
#[test]
fn semantic_world_position_matches_original() {
    ddnative_host::install();
    let n = abi_cases("E:/ddda-re/notes/native-L184/leaf_origin.json", mtcore::semantic_s3::world_position, true, leaf_callee);
    eprintln!("{n} ABI-level world_position cases passed");
}

// verifies 0x00ea1ee0 (ABI level)
#[test]
fn semantic_classify_overlap_matches_original() {
    ddnative_host::install();
    let n = abi_cases("E:/ddda-re/notes/native-L184/leaf_flags.json", mtcore::semantic_s3::classify_overlap, true, leaf_callee);
    eprintln!("{n} ABI-level classify_overlap cases passed");
}

// verifies 0x00e61010 (ABI level)
#[test]
fn semantic_any_member_test_matches_original() {
    ddnative_host::install();
    let n = abi_cases("E:/ddda-re/notes/native-L184/leaf_any.json", mtcore::semantic_s3::any_member_test, true, leaf_callee);
    eprintln!("{n} ABI-level any_member_test cases passed");
}
