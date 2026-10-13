//! L184 s10: differential tests for ports that call other native ports
//! directly. The unfolded version (the original's guest calls, dispatched
//! here to the callees' ABI ports) and the folded one run on the same
//! memory (a recorded vector's bytes, every other byte a seeded
//! pseudo-random function of its address, zero-biased) and must leave the
//! same registers, the same writes (outside the discarded stack below the
//! entry frame) and make the same remaining guest calls, with the same
//! ESP and ECX at each.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

struct Fuzz {
    seed: u64,
    base: BTreeMap<u32, u8>,
    writes: BTreeMap<u32, u8>,
    budget: std::cell::Cell<i64>,
}
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
impl Mem for Fuzz {
    fn u8(&self, a: u32) -> u8 {
        self.budget.set(self.budget.get() - 1);
        if self.budget.get() < 0 {
            panic!("budget");
        }
        if let Some(&b) = self.writes.get(&a) {
            return b;
        }
        if let Some(&b) = self.base.get(&a) {
            return b;
        }
        let h = mix(self.seed ^ (u64::from(a) << 20));
        // zero-biased: counts and flags stay small
        if h & 3 != 0 {
            0
        } else {
            (h >> 8) as u8
        }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.budget.set(self.budget.get() - 1);
        if self.budget.get() < 0 {
            panic!("budget");
        }
        self.writes.insert(a, v);
    }
}

fn n(v: &Value) -> u32 {
    v.as_u64().unwrap() as u32
}
fn pairs(v: &Value) -> BTreeMap<u32, u8> {
    v.as_array().unwrap().iter().map(|p| (n(&p[0]), n(&p[1]) as u8)).collect()
}

/// Guest calls the tested functions make: target -> stack argument words.
const STACK_ARGS: &[(u32, u32)] = &[
    (0x0d00_0000, 1),
    (0x0d00_0100, 2),
    (0x00ff_1090, 2),
    (0x0106_55a0, 2),
    (0x0106_2f20, 1),
    (0x00fe_d890, 1),
    (0x0106_6380, 2),
    (0x0106_5e40, 2),
    (0x0106_58b0, 3),
    (0x0106_5b00, 3),
    (0x00fe_fb90, 4),
    (0x00ff_00a0, 5),
    (0x0106_6c60, 2),
    (0x0114_4fe0, 2),
];

type Log = Vec<(u32, u32, u32)>;
struct Outcome {
    regs: [u32; 6],
    writes: BTreeMap<u32, u8>,
    log: Log,
    dispatched: Vec<u32>,
}

/// Run `body` once. `dispatch`: ports standing in for guest calls (the
/// unfolded run); `folded`: targets that are not logged.
fn run(body: LiftFn, case: &Value, seed: u64, dispatch: &[(u32, LiftFn)], folded: &[u32], prepare: Option<fn(&mut BTreeMap<u32, u8>, &Cpu)>) -> Option<Outcome> {
    let regs: Vec<u32> = case["before"]["regs"].as_array().unwrap().iter().map(n).collect();
    let mut c = Cpu::default();
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = regs.clone().try_into().unwrap();
    let entry = c.esp;
    let mut base = pairs(&case["initial"]);
    if let Some(prepare) = prepare {
        prepare(&mut base, &c);
    }
    let mut m = Fuzz { seed, base, writes: BTreeMap::new(), budget: std::cell::Cell::new(40_000) };
    let log = Rc::new(RefCell::new(Log::new()));
    let dispatched = Rc::new(RefCell::new(Vec::<u32>::new()));
    let dispatched2 = dispatched.clone();
    let (log2, folded2, dispatch2) = (log.clone(), folded.to_vec(), dispatch.to_vec());
    let calls = Rc::new(std::cell::Cell::new(0u64));
    x86::set_ext_calls(Some(Box::new(move |m, c, target| {
        if !folded2.contains(&target) {
            log2.borrow_mut().push((target, c.ecx, c.esp));
        }
        if let Some(&(_, port)) = dispatch2.iter().find(|d| d.0 == target) {
            dispatched2.borrow_mut().push(target);
            return port(m, c);
        }
        let args = STACK_ARGS.iter().find(|a| a.0 == target).unwrap_or_else(|| panic!("unknown call {target:x}")).1;
        let k = calls.get();
        calls.set(k + 1);
        c.eax = mix(seed ^ u64::from(target) ^ (k << 40)) as u32;
        if target == 0x0d00_0000 {
            // material->get_color(out): an RGBA record with an opaque alpha
            for (i, b) in 0x3f80_0000u32.to_le_bytes().into_iter().enumerate() {
                m.set_u8(0x0e10_000c + i as u32, b);
            }
            c.eax = 0x0e10_0000;
        }
        c.ecx = 0xc1a0;
        c.edx = 0xd1a0;
        c.esp = c.esp.wrapping_add(4 + 4 * args);
    })));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&mut m, &mut c)));
    x86::set_ext_calls(None);
    match result {
        Ok(()) => {}
        Err(e) => {
            if e.downcast_ref::<&str>() == Some(&"budget") {
                return None;
            }
            std::panic::resume_unwind(e);
        }
    }
    let writes = m.writes.into_iter().filter(|&(a, _)| !(entry.wrapping_sub(0x4000)..entry).contains(&a)).collect();
    let log = log.borrow().clone();
    let dispatched = dispatched.borrow().clone();
    Some(Outcome { regs: [c.eax, c.ebx, c.esp, c.ebp, c.esi, c.edi], writes, log, dispatched })
}

fn differential(path: &str, entry: Option<u32>, prepare: Option<fn(&mut BTreeMap<u32, u8>, &Cpu)>, unfolded: LiftFn, folded_body: LiftFn, dispatch: &[(u32, LiftFn)], folded: &[u32], seeds: u64, compare_eax: bool) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    ddnative_host::install();
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    let (mut compared, mut skipped) = (0, 0);
    let mut coverage: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, case) in cases.iter().enumerate() {
        if entry.is_some_and(|e| case.get("entry").map(n) != Some(e)) {
            continue;
        }
        for k in 0..seeds {
            let seed = (i as u64) << 8 | k;
            let a = run(unfolded, case, seed, dispatch, folded, prepare);
            let b = run(folded_body, case, seed, &[], folded, prepare);
            let (Some(a), Some(b)) = (a, b) else {
                skipped += 1;
                continue;
            };
            let mask = |r: [u32; 6]| if compare_eax { r } else { [0, r[1], r[2], r[3], r[4], r[5]] };
            assert_eq!(mask(a.regs), mask(b.regs), "case {i} seed {k}: registers");
            assert_eq!(a.log, b.log, "case {i} seed {k}: remaining guest calls");
            assert_eq!(a.writes, b.writes, "case {i} seed {k}: writes");
            for t in &a.dispatched {
                *coverage.entry(*t).or_default() += 1;
            }
            compared += 1;
        }
    }
    eprintln!("{compared} differential runs agree ({skipped} skipped over budget): {path}; folded calls exercised {coverage:x?}");
    for (t, _) in dispatch {
        assert!(coverage.get(t).copied().unwrap_or(0) > 0, "folded callee {t:x} never reached");
    }
    assert!(compared > 0);
}

// 0x01066C60 with set_pass 0x00FF1090 and apply_material 0x010655A0 folded in
#[test]
fn select_material_pass_folded_matches_guest_calls() {
    differential(
        "E:/ddda-re/notes/native-L174/01066c60.json",
        None,
        None,
        mtcore::semantic_draw_b::select_material_pass_guest_callees,
        mtcore::semantic_draw_b::select_material_pass,
        &[
            (0x00ff_1090, mtcore::semantic::set_pass as LiftFn),
            (0x0106_55a0, mtcore::semantic_draw_b::apply_material_guest_finalizer),
            (0x0106_2f20, mtcore::semantic_draw_b::finalize_material_abi),
        ],
        &[0x00ff_1090, 0x0106_55a0, 0x0106_2f20],
        4,
        true,
    );
}

// 0x01145BF0 with 0x01066C60 (and everything below it) folded in
#[test]
fn apply_standard_material_folded_matches_guest_calls() {
    differential(
        "E:/ddda-re/notes/native-L177/01145bf0.json",
        Some(0x0114_5bf0),
        Some(material_vtable),
        mtcore::semantic_draw_c::apply_standard_material_guest_callee,
        mtcore::semantic_draw_c::apply_standard_material,
        &[
            (0x0106_6c60, mtcore::semantic_draw_b::select_material_pass_guest_callees as LiftFn),
            (0x00ff_1090, mtcore::semantic::set_pass),
            (0x0106_55a0, mtcore::semantic_draw_b::apply_material_guest_finalizer),
            (0x0106_2f20, mtcore::semantic_draw_b::finalize_material_abi),
        ],
        &[0x0106_6c60, 0x00ff_1090, 0x0106_55a0, 0x0106_2f20],
        6,
        true,
    );
}

/// The material's vtable slot +0x24 (get_color) is a known guest function.
fn material_vtable(base: &mut BTreeMap<u32, u8>, c: &Cpu) {
    let vtable = 0x0e20_0000u32;
    let (manager, manager_vtable) = (0x0e30_0000u32, 0x0e31_0000u32);
    for (a, v) in [(c.ecx, vtable), (vtable + 0x24, 0x0d00_0000), (0x018d_2c80, manager), (manager, manager_vtable), (manager_vtable + 0x38, 0x0d00_0100)] {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            base.insert(a + i as u32, b);
        }
    }
}
