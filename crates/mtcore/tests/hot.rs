//! Original Unicorn vectors and (via check_main_thread.py) isolated lifted
//! vectors. No shards or game window are needed for this test.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
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
fn regs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}
fn x87_image(v: &Value) -> [u8; 108] {
    let mut image = ddnative_base::x87::INIT;
    for (i, offset) in [0, 4, 8].into_iter().enumerate() {
        image[offset..offset + 2].copy_from_slice(&(n(&v[i]) as u16).to_le_bytes());
    }
    for i in 0..8 {
        let offset = 28 + 10 * i;
        image[offset..offset + 8].copy_from_slice(&v[3 + i][0].as_u64().unwrap().to_le_bytes());
        image[offset + 8..offset + 10].copy_from_slice(&(n(&v[3 + i][1]) as u16).to_le_bytes());
    }
    image
}
fn cpu(v: &Value) -> Cpu {
    let r: Vec<_> = v["regs"].as_array().unwrap().iter().map(n).collect();
    let mut c = Cpu::default();
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = r.try_into().unwrap();
    c.fl = x86::Flags::new(x86::RAW, 0, 0, n(&v["flags"]), 32);
    c.df = n(&v["flags"]) & 0x400 != 0;
    c.mxcsr = x86::Mxcsr(n(&v["mxcsr"]));
    c.st.state = x87_image(&v["x87"]);
    for i in 0..8 {
        for j in 0..4 {
            c.xmm[i][j] = n(&v["xmm"][i][j]);
        }
    }
    c
}
fn check(m: &dyn Mem, c: &Cpu, v: &Value, where_: &str) {
    assert_eq!(
        regs(c).to_vec(),
        v["regs"].as_array().unwrap().iter().map(n).collect::<Vec<_>>(),
        "{where_} regs"
    );
    let flags = c.fl.snapshot().0 | if c.df { 0x400 } else { 0 };
    assert_eq!(flags & 0xcd5, n(&v["flags"]), "{where_} flags");
    for i in 0..8 {
        for j in 0..4 {
            assert_eq!(c.xmm[i][j], n(&v["xmm"][i][j]), "{where_} xmm{i}.{j}");
        }
    }
    assert_eq!(c.mxcsr.0, n(&v["mxcsr"]), "{where_} mxcsr");
    assert_eq!(c.st.clone().state, x87_image(&v["x87"]), "{where_} x87 unchanged");
    for (a, b) in pairs(&v["memory"]) {
        assert_eq!(m.u8(a), b, "{where_} memory {a:x}");
    }
}
fn run(body: LiftFn, case: &Value) {
    assert_eq!(case["before"]["x87"], case["after"]["x87"]);
    for event in case["events"].as_array().unwrap() {
        assert_eq!(event["x87"], case["before"]["x87"]);
    }
    let mut m = Memory {
        bytes: pairs(&case["initial"]),
        ..Default::default()
    };
    let mut c = cpu(&case["before"]);
    let events = Rc::new(case["events"].as_array().unwrap().clone());
    let cursor = Rc::new(RefCell::new(0usize));
    let (trace, position) = (events.clone(), cursor.clone());
    let old = x86::set_ext_calls(Some(Box::new(move |m, c, target| {
        assert_eq!(target, 0xe0ec50);
        let i = *position.borrow();
        assert_eq!(trace[i]["kind"], "call");
        check(m, c, &trace[i], &format!("call {i}"));
        *position.borrow_mut() += 1;
        body(m, c);
        let i = *position.borrow();
        assert_eq!(trace[i]["kind"], "return");
        check(m, c, &trace[i], &format!("return {i}"));
        *position.borrow_mut() += 1;
    })));
    body(&mut m, &mut c);
    x86::set_ext_calls(None);
    assert!(old.is_none());
    assert_eq!(*cursor.borrow(), events.len());
    check(&m, &c, &case["after"], "return");
    assert_eq!(
        m.writes,
        pairs(&case["after"]["memory"]),
        "all writes, including spills"
    );
}

fn geometry_callback(m: &mut dyn Mem, c: &mut Cpu, event: &Value) {
    for (a, b) in pairs(&event["effects"]) {
        m.set_u8(a, b);
    }
    *c = cpu(event);
}

fn run_geometry(body: LiftFn, case: &Value) {
    let mut m = Memory {
        bytes: pairs(&case["initial"]),
        ..Default::default()
    };
    let mut c = cpu(&case["before"]);
    let events = Rc::new(case["events"].as_array().unwrap().clone());
    let cursor = Rc::new(RefCell::new(0usize));
    let (trace, position) = (events.clone(), cursor.clone());
    let old = x86::set_ext_calls(Some(Box::new(move |m, c, target| {
        let i = *position.borrow();
        assert_eq!(target, n(&trace[i]["target"]));
        check(m, c, &trace[i], "geometry callee entry");
        geometry_callback(m, c, &trace[i + 1]);
        check(m, c, &trace[i + 1], "geometry callee return");
        *position.borrow_mut() += 2;
    })));
    assert!(old.is_none());
    body(&mut m, &mut c);
    x86::set_ext_calls(None);
    assert_eq!(*cursor.borrow(), events.len());
    check(&m, &c, &case["after"], "geometry return");
    assert_eq!(m.writes, pairs(&case["after"]["memory"]));
}

fn geometry_vectors() -> Option<Value> {
    let path = "E:/ddda-re/notes/native-L130/geometry.json";
    match std::fs::read_to_string(path) {
        Ok(s) => Some(serde_json::from_str(&s).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

fn identifier_vectors() -> Option<Value> {
    let path = "E:/ddda-re/notes/native-L130/identifiers.json";
    match std::fs::read_to_string(path) {
        Ok(s) => Some(serde_json::from_str(&s).unwrap()),
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => None,
    }
}

// verifies 0x00cd8760
#[test]
fn identifiers_match_original_and_lifted() {
    ddnative_host::install();
    let Some(data) = identifier_vectors() else {
        return;
    };
    for case in data["cases"].as_array().unwrap() {
        run_geometry(mtcore::hot::resolve_identifier, case);
        // L130_IDENTIFIER_REFERENCE
    }
    eprintln!(
        "{} exact identifier cases passed",
        data["cases"].as_array().unwrap().len()
    );
}

#[test]
#[ignore = "paired identifier microbenchmark"]
fn identifiers_cost() {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let data = identifier_vectors().unwrap();
    let bodies: Vec<(&str, LiftFn)> = vec![("port", mtcore::hot::resolve_identifier)]; // L130_IDENTIFIER_COST
    let mut m = Paged::default();
    for index in [0, 104, 105, 111, 116, 117] {
        let case = &data["cases"][index];
        let initial = pairs(&case["initial"]);
        let initial_cpu = cpu(&case["before"]);
        let effects: Vec<_> = case["events"]
            .as_array()
            .unwrap()
            .chunks(2)
            .map(|e| (cpu(&e[1]), pairs(&e[1]["effects"])))
            .collect();
        for round in 0..5 {
            for offset in 0..bodies.len() {
                let (name, body) = bodies[(round + offset) % bodies.len()];
                let effects = effects.clone();
                let cursor = Rc::new(RefCell::new(0usize));
                let position = cursor.clone();
                x86::set_ext_calls(Some(Box::new(move |m, c, _| {
                    let mut i = position.borrow_mut();
                    let (state, writes) = &effects[*i];
                    for (&a, &b) in writes {
                        m.set_u8(a, b);
                    }
                    *c = state.clone();
                    *i += 1;
                })));
                let mut elapsed = std::time::Duration::ZERO;
                for iteration in 0..5020 {
                    for (&a, &b) in &initial {
                        m.set_u8(a, b);
                    }
                    let mut c = initial_cpu.clone();
                    *cursor.borrow_mut() = 0;
                    let start = std::time::Instant::now();
                    body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                    let time = start.elapsed();
                    if iteration >= 20 {
                        elapsed += time;
                    }
                    if iteration == 0 {
                        check(&m, &c, &case["after"], "identifier bench gate");
                    }
                }
                eprintln!(
                    "identifier count={} mode={} round={round} {name} ns/call={:.1}",
                    case["count"],
                    case["mode"],
                    elapsed.as_nanos() as f64 / 5000.0
                );
                x86::set_ext_calls(None);
            }
        }
    }
}

// verifies 0x011787c0
#[test]
fn geometry_matches_original_and_lifted() {
    ddnative_host::install();
    let Some(data) = geometry_vectors() else {
        return;
    };
    for case in data["cases"].as_array().unwrap() {
        run_geometry(mtcore::hot::dispatch_geometry, case);
        // L130_GEOMETRY_REFERENCE
    }
    eprintln!(
        "{} exact geometry cases passed",
        data["cases"].as_array().unwrap().len()
    );
}

#[test]
#[ignore = "paired geometry microbenchmark"]
fn geometry_cost() {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let data = geometry_vectors().unwrap();
    let bodies: Vec<(&str, LiftFn)> = vec![("port", mtcore::hot::dispatch_geometry)]; // L130_GEOMETRY_COST
    let mut m = Paged::default();
    for index in [0, 160, 560, 576, 640, 1680] {
        let case = &data["cases"][index];
        let initial = pairs(&case["initial"]);
        let initial_cpu = cpu(&case["before"]);
        let effects = case["events"]
            .as_array()
            .unwrap()
            .get(1)
            .map(|event| (cpu(event), pairs(&event["effects"])));
        for round in 0..5 {
            for offset in 0..bodies.len() {
                let (name, body) = bodies[(round + offset) % bodies.len()];
                let effects = effects.clone();
                x86::set_ext_calls(Some(Box::new(move |m, c, _| {
                    let (state, writes) = effects.as_ref().unwrap();
                    for (&a, &b) in writes {
                        m.set_u8(a, b);
                    }
                    *c = state.clone();
                })));
                let mut elapsed = std::time::Duration::ZERO;
                for iteration in 0..10020 {
                    for (&a, &b) in &initial {
                        m.set_u8(a, b);
                    }
                    let mut c = initial_cpu.clone();
                    let start = std::time::Instant::now();
                    body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                    let time = start.elapsed();
                    if iteration >= 20 {
                        elapsed += time;
                    }
                    if iteration == 0 {
                        check(&m, &c, &case["after"], "geometry bench gate");
                    }
                }
                eprintln!(
                    "geometry shape={} alternate={} round={round} {name} ns/call={:.1}",
                    case["shape"],
                    case["alternate"],
                    elapsed.as_nanos() as f64 / 10000.0
                );
                x86::set_ext_calls(None);
            }
        }
    }
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

#[test]
#[ignore = "paired microbenchmark; run through check_main_thread.py --bench"]
fn dependencies_cost() {
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let text = std::fs::read_to_string(
        std::env::var("L130_VECTORS")
            .unwrap_or_else(|_| "E:/ddda-re/notes/native-L130/dependencies.json".into()),
    )
    .unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let bodies: Vec<(&str, LiftFn)> = vec![("port", mtcore::hot::expand_dependencies)]; // L130_COST_BODIES
    let mut m = Paged::default();
    for index in [0, 2, 11, 23, 35, 77] {
        let case = &data["cases"][index];
        let initial = pairs(&case["initial"]);
        let initial_cpu = cpu(&case["before"]);
        for round in 0..3 {
            for offset in 0..bodies.len() {
                let (name, body) = bodies[(offset + round) % bodies.len()];
                x86::set_ext_calls(Some(Box::new(move |m, c, target| {
                    assert_eq!(target, 0xe0ec50);
                    body(m, c);
                })));
                let mut elapsed = std::time::Duration::ZERO;
                for iteration in 0..1020 {
                    for (&a, &b) in &initial {
                        m.set_u8(a, b);
                    }
                    let mut c = initial_cpu.clone();
                    let start = std::time::Instant::now();
                    body(std::hint::black_box(&mut m), std::hint::black_box(&mut c));
                    let time = start.elapsed();
                    if iteration >= 20 {
                        elapsed += time;
                    }
                    if iteration == 0 {
                        check(&m, &c, &case["after"], "bench gate");
                    }
                    std::hint::black_box(c.eax);
                }
                eprintln!(
                    "case={index} depth={} width={} terminals={} round={round} {name} ns/call={:.1}",
                    case["depth"],
                    case["width"],
                    case["terminals"],
                    elapsed.as_nanos() as f64 / 1000.0
                );
            }
        }
    }
    x86::set_ext_calls(None);
}

// verifies 0x00e0ec50
#[test]
fn dependencies_match_original_and_lifted() {
    ddnative_host::install();
    let path = std::env::var("L130_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L130/dependencies.json".into());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    for (i, case) in cases.iter().enumerate() {
        eprintln!(
            "case {i}: depth={} width={} terminals={}",
            case["depth"], case["width"], case["terminals"]
        );
        run(mtcore::hot::expand_dependencies, case);
        // L130_REFERENCE_RUN
    }
    eprintln!("{} exact whole-machine cases passed", cases.len());
}
