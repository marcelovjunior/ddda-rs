//! L55: original recorded cases through emu::run_observed, not lifted roots.
//! Run explicitly with --features emu,game --test emu_differential -- --ignored.
//! L55_SAMPLE selects verified addresses; L55_REPORT receives private JSONL.
#![cfg(all(feature = "emu", feature = "game"))]

use ddnative_base::mem::{Mem, PageTable, HEAP_BASE, READ, WRITE};
use ddnative_base::x86::{self, Cpu, Entry};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::GuestMemory,
};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs::File,
    io::{BufReader, BufWriter, Write},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

const REGS: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
fn u(v: &Value) -> u32 {
    v.as_u64().expect("u32 field") as u32
}
fn regs(c: &Cpu) -> [u32; 8] {
    [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi]
}
fn bytes(v: &Value) -> Vec<(u32, Vec<u8>)> {
    v.as_array()
        .expect("memory runs")
        .iter()
        .map(|r| {
            let s = r[1].as_str().unwrap();
            (
                u(&r[0]),
                (0..s.len() / 2)
                    .map(|k| u8::from_str_radix(&s[k * 2..k * 2 + 2], 16).unwrap())
                    .collect(),
            )
        })
        .collect()
}

struct Memory {
    guest: GuestMemory,
    written: BTreeSet<u32>,
    heap: u32,
    expose_table: Cell<bool>,
    baseline: HashMap<u32, (Vec<u8>, u8)>,
    touched: BTreeSet<u32>,
}
impl Memory {
    fn new(exe: &ddexe::ExeFile) -> Self {
        let mut guest = GuestMemory::new();
        let image = ddruntime::image::load(&mut guest, exe).unwrap();
        let mut baseline = HashMap::new();
        for section in image {
            for p in ((section.va & !4095)..section.va + section.size).step_by(4096) {
                baseline
                    .entry(p)
                    .or_insert_with(|| (guest.read(p, 4096).unwrap(), guest.perm(p)));
            }
        }
        Self {
            guest,
            written: BTreeSet::new(),
            heap: HEAP_BASE,
            expose_table: Cell::new(true),
            baseline,
            touched: BTreeSet::new(),
        }
    }
    fn reset(&mut self) {
        for p in std::mem::take(&mut self.touched) {
            let (data, perm) = &self.baseline[&p];
            self.guest.poke(p, data).unwrap();
            self.guest.protect(p, 4096, *perm);
        }
        self.written.clear();
        self.heap = HEAP_BASE;
        self.expose_table.set(true);
    }
    fn map(&mut self, a: u32, size: usize) {
        if size == 0 {
            return;
        }
        assert!(
            a as u64 + size as u64 <= 1u64 << 32,
            "fixture run wraps the address space"
        );
        for p in (a as u64 & !4095..a as u64 + size as u64).step_by(4096) {
            let p = p as u32;
            if !self.guest.is_mapped(p) {
                self.guest.map(p, 4096, READ | WRITE).unwrap();
            }
            self.baseline
                .entry(p)
                .or_insert_with(|| (self.guest.read(p, 4096).unwrap(), self.guest.perm(p)));
            self.touched.insert(p);
        }
    }
}
impl Mem for Memory {
    fn u8(&self, a: u32) -> u8 {
        self.guest.u8(a)
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.map(a, 1);
        self.guest.set_u8(a, v);
        self.written.insert(a);
    }
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        // Only the emulator needs direct page access. Subsequent lifted
        // call-outs must use this Mem's setters so their writes are observed.
        if self.expose_table.replace(false) {
            self.guest.page_table()
        } else {
            None
        }
    }
    fn alloc(&mut self, size: u32) -> u32 {
        let a = self.heap;
        self.heap = self.heap.wrapping_add(size.max(1)).wrapping_add(15) & !15;
        // Match Sparse/harness: allocation advances the cursor only. Input
        // and output runs map the bytes actually touched, even when a random
        // unused allocation argument is very large.
        a
    }
}

thread_local! {
    static MOCKS: RefCell<VecDeque<Value>> = RefCell::new(Default::default());
    static CALL_TARGET: Cell<u32> = const { Cell::new(0) };
}
struct Terminal(Cpu);
fn mock(_: &mut dyn Mem, cpu: &mut Cpu) {
    let event = MOCKS.with(|q| q.borrow_mut().pop_front().expect("unrecorded call"));
    let target = CALL_TARGET.with(Cell::get);
    assert_eq!(u(&event["target"]), target, "call target/order");
    assert_eq!(
        regs(cpu),
        REGS.map(|k| u(&event["regs"][k])),
        "registers at call {target:#x}"
    );
    if event["terminal"].as_bool() == Some(true) {
        std::panic::resume_unwind(Box::new(Terminal(cpu.clone())));
    }
    let out = &event["out"];
    cpu.eax = u(&out["eax"]);
    cpu.ecx = u(&out["ecx"]);
    cpu.edx = u(&out["edx"]);
    for k in 0..4 {
        cpu.xmm[0][k] = u(&out["xmm0"][k]);
    }
    cpu.esp = cpu.esp.wrapping_add(4 + u(&out["pop"]));
    for _ in 0..out["st_pop"].as_u64().unwrap_or(0) {
        cpu.st.register::<0xd8dd>();
    }
    if let Some(v) = out["st_push"].as_u64() {
        cpu.st.push(v as u32);
    }
}
fn compare(c: &Value, m: &mut Memory, closed: &HashMap<u32, x86::LiftFn>) -> Result<(), String> {
    m.reset();
    if let Some(recorded) = c["cpu_dependent"].as_str() {
        static BRAND: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let brand = BRAND.get_or_init(|| {
            let brand: String = (0x80000002..=0x80000004)
                .flat_map(|leaf| {
                    // SAFETY: these brand leaves are available on the x86-64 recorder host.
                    let r = unsafe { std::arch::x86_64::__cpuid(leaf) };
                    [r.eax, r.ebx, r.ecx, r.edx]
                })
                .flat_map(u32::to_le_bytes)
                .map(char::from)
                .collect();
            brand.trim_end_matches('\0').to_owned()
        });
        if recorded != brand {
            return Err("unsupported: CPU-dependent case recorded on a different processor".into());
        }
    }
    let addr = u(&c["addr"]);
    let calls = c["calls"].as_array().cloned().unwrap_or_default();
    let targets: Vec<_> = calls
        .iter()
        .map(|e| u(&e["target"]))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if c["machine"].as_array().is_some_and(|v| !v.is_empty()) {
        return Err("unsupported: recorded CPUID/RDTSC protocol has no emulator instruction binding".into());
    }
    for (a, data) in bytes(&c["reads"]) {
        m.map(a, data.len());
        m.guest.poke(a, &data).unwrap();
    }
    for (a, data) in bytes(&c["writes"]) {
        m.map(a, data.len());
        // Sparse and the recording emulator permit these recorded stores,
        // including random destinations inside a normally read-only image
        // section. Only fixture memory is changed; the exact write-set check
        // still rejects additional stores.
        m.guest.protect(a, data.len() as u32, READ | WRITE);
    }
    let r = &c["regs_in"];
    let mut cpu = Cpu::from(Entry {
        eax: u(&r["eax"]),
        ecx: u(&r["ecx"]),
        edx: u(&r["edx"]),
        ebx: u(&r["ebx"]),
        esp: u(&r["esp"]),
        ebp: u(&r["ebp"]),
        esi: u(&r["esi"]),
        edi: u(&r["edi"]),
        xmm0: 0,
    });
    cpu.fs_base = 0x7ffd_e000;
    for v in c["st_in"].as_array().into_iter().flatten().rev() {
        let mut data = [0u8; 10];
        data[..8].copy_from_slice(&v[0].as_u64().unwrap().to_le_bytes());
        data[8..].copy_from_slice(&(u(&v[1]) as u16).to_le_bytes());
        cpu.st.exec::<0x28db>(&mut data);
    }
    if let Some(s) = c.get("segments") {
        cpu.seg = x86::Segments {
            es: u(&s["es"]) as u16,
            cs: u(&s["cs"]) as u16,
            ss: u(&s["ss"]) as u16,
            ds: u(&s["ds"]) as u16,
            fs: u(&s["fs"]) as u16,
            gs: u(&s["gs"]) as u16,
        };
    }
    let mut registry = Registry::new();
    for (&a, &f) in closed {
        registry.insert(a, Binding::Lifted(f)).unwrap();
    }
    for &a in &targets {
        if registry.get(a).is_some() {
            return Err("unsupported: mock target is now closed (recording boundary changed)".into());
        }
        // Reserve call-token pages too: a recorded arbitrary token must not
        // overlap the emulator's private state pages, even if never read.
        m.map(a, 1);
        registry.insert(a, Binding::Lifted(mock)).unwrap();
    }
    MOCKS.with(|q| *q.borrow_mut() = calls.into());
    let registry = Arc::new(registry);
    let r = registry.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, a| {
        CALL_TARGET.with(|target| target.set(a));
        r.call(m, c, a).unwrap();
    })));
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    let written = Rc::new(RefCell::new(BTreeSet::new()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ddruntime::emu::run_observed_with_calls(
            &registry,
            m,
            &mut cpu,
            addr,
            written.clone(),
            Duration::from_secs(60),
            |a| CALL_TARGET.with(|target| target.set(a)),
        );
    }));
    m.touched.extend(written.borrow().iter().map(|a| a & !4095));
    x86::set_ext_calls(None);
    if let Err(error) = result {
        if let Some(t) = error.downcast_ref::<Terminal>() {
            cpu = t.0.clone();
        } else {
            x86::ldmxcsr(x86::MXCSR_DEFAULT);
            return Err(error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "emulator fault".into()));
        }
    }
    let mut differences = Vec::new();
    if !MOCKS.with(|q| q.borrow().is_empty()) {
        differences.push("missing calls".into());
    }
    for (k, got) in REGS.into_iter().zip(regs(&cpu)) {
        if got != u(&c["regs_out"][k]) {
            differences.push(format!("{k}: {got:#x} != {:#x}", u(&c["regs_out"][k])));
        }
    }
    if cpu.xmm[0][0] != u(&c["xmm0_out"]) {
        differences.push("xmm0".into());
    }
    if c.get("mxcsr").is_some() && x86::stmxcsr() != u(&c["mxcsr"]) {
        differences.push(format!("mxcsr: {:#x} != {:#x}", x86::stmxcsr(), u(&c["mxcsr"])));
    }
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    if c.get("x87_control").is_some() {
        if cpu.st.control() as u32 != u(&c["x87_control"]) {
            differences.push("x87 control".into());
        }
        if cpu.st.depth() != u(&c["x87_depth"]) {
            differences.push(format!("x87 depth: {} != {}", cpu.st.depth(), u(&c["x87_depth"])));
        }
        let ext = c["st0_ext"]
            .as_array()
            .map(|v| (v[0].as_u64().unwrap(), u(&v[1]) as u16));
        if cpu.st.top_extended() != ext {
            differences.push(format!("extended st0: {:?} != {ext:?}", cpu.st.top_extended()));
        }
    }
    if let Some(want) = c["st0_out"].as_u64() {
        if cpu.exit().st0 != Some(want as u32) {
            differences.push("st0 single".into());
        }
    }
    let want: BTreeMap<_, _> = bytes(&c["writes"])
        .into_iter()
        .flat_map(|(a, b)| {
            b.into_iter()
                .enumerate()
                .map(move |(k, b)| (a.wrapping_add(k as u32), b))
        })
        .collect();
    m.written.extend(written.borrow().iter());
    let got: BTreeMap<_, _> = m.written.iter().map(|&a| (a, m.u8(a))).collect();
    if got != want {
        let diff: Vec<_> = want
            .iter()
            .filter(|(a, v)| got.get(a) != Some(v))
            .take(4)
            .collect();
        let extra: Vec<_> = got.keys().filter(|a| !want.contains_key(a)).take(4).collect();
        differences.push(format!(
            "writes: expected {} bytes, got {}; differing {diff:x?}; extra {extra:x?}",
            want.len(),
            got.len()
        ));
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("; "))
    }
}

struct CaseSeed<'a>(&'a HashMap<u32, String>);
impl<'de> DeserializeSeed<'de> for CaseSeed<'_> {
    type Value = Option<Value>;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for CaseSeed<'_> {
    type Value = Option<Value>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "recorded case")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
        let mut selected = None;
        let mut fields = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if key == "addr" {
                if selected.is_some() {
                    return Err(serde::de::Error::custom("duplicate case address"));
                }
                let addr = a.next_value::<u32>()?;
                selected = Some(self.0.contains_key(&addr));
                fields.insert(key, json!(addr));
                if selected == Some(false) {
                    fields.clear();
                }
            } else if selected == Some(false) {
                let _: IgnoredAny = a.next_value()?;
            } else {
                fields.insert(key, a.next_value::<Value>()?);
            }
        }
        match selected {
            Some(true) => Ok(Some(Value::Object(fields))),
            Some(false) => Ok(None),
            None => Err(serde::de::Error::custom("case has no address")),
        }
    }
}

struct Stream<'a, F>(&'a mut F, &'a HashMap<u32, String>);
impl<'de, F: FnMut(Value)> DeserializeSeed<'de> for Stream<'_, F> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(Value)> Visitor<'de> for Stream<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "case array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while let Some(v) = a.next_element_seed(CaseSeed(self.1))? {
            if let Some(v) = v {
                (self.0)(v);
            }
        }
        Ok(())
    }
}

#[test]
#[ignore = "requires private recorded cases, executable, and L55_SAMPLE/L55_REPORT"]
fn recorded_cases_match_emulator() {
    let sample: Value = serde_json::from_reader(BufReader::new(
        File::open(std::env::var("L55_SAMPLE").expect("L55_SAMPLE")).unwrap(),
    ))
    .unwrap();
    let selected: HashMap<_, _> = sample["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (u(&v["addr"]), v["cls"].as_str().unwrap().to_owned()))
        .collect();
    let mut report = BufWriter::new(File::create(std::env::var("L55_REPORT").expect("L55_REPORT")).unwrap());
    let exe = ddexe::ExeFile::open(
        std::env::var("DDDA_EXE").unwrap_or_else(|_| "E:/ddda-re/bin/DDDA.exe.unpacked.exe".into()),
    )
    .unwrap();
    let closed = ddnative::generated::lifted::dispatch_table();
    let mut memory = Memory::new(&exe);
    let root = std::env::var("LIFT_DATA").unwrap_or_else(|_| "E:/ddda-re/notes/native".into());
    let file = File::open(format!("{root}/vectors/lift.json")).unwrap();
    let mut counts: BTreeMap<String, [usize; 3]> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut ordinals: HashMap<u32, usize> = HashMap::new();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut visit = |c: Value| {
        let a = u(&c["addr"]);
        let Some(cls) = selected.get(&a) else { return };
        let ordinal = ordinals.entry(a).or_default();
        let case_index = *ordinal;
        *ordinal += 1;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare(&c, &mut memory, &closed)))
                .unwrap_or_else(|error| {
                    Err(error
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "fixture setup/comparison panic".into()))
                });
        x86::set_ext_calls(None);
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        let (status, index, detail) = match result {
            Ok(()) => ("pass", 0, String::new()),
            Err(s) if s.starts_with("unsupported:") => ("unsupported", 2, s),
            Err(s) => ("mismatch", 1, s),
        };
        counts.entry(cls.clone()).or_default()[index] += 1;
        seen.insert(a);
        writeln!(
            report,
            "{}",
            json!({"addr":a,"case_index":case_index,"class":cls,"status":status,"detail":detail})
        )
        .unwrap();
        report.flush().unwrap();
    };
    Stream(&mut visit, &selected)
        .deserialize(&mut serde_json::Deserializer::from_reader(BufReader::new(file)))
        .unwrap();
    std::panic::set_hook(hook);
    eprintln!(
        "L55 counts [pass,mismatch,unsupported]: {counts:?}; {} functions",
        seen.len()
    );
    writeln!(report, "{}", json!({"summary":counts,"functions":seen.len()})).unwrap();
    assert_eq!(seen.len(), selected.len(), "selected functions must have cases");
    assert!(
        counts.values().all(|c| c[1] == 0),
        "emulator mismatches; see L55_REPORT"
    );
}
