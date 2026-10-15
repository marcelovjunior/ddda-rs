//! Small-leaf ports against the original's recorded behaviour
//! (research/native/record_leaves.py; vectors in `LEAF_VECTORS_DIR`, default
//! E:/ddda-re/notes/native-leaves; skipped when absent unless DDNATIVE_REQUIRE_VECTORS is set).
//! ABI level: EAX/ECX/EDX (these functions are leaves, so their leftovers are checked too),
//! EBX/EBP/ESI/EDI, ESP and every memory byte at or above the returned ESP, and no write the
//! original does not make.
use ddnative_base::{
    mem::Mem,
    x86::Cpu,
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

/// Run `body` on every case of `name`; the number of cases.
fn run_cases(name: &str, body: fn(&mut dyn Mem, &mut Cpu), flags: bool) -> usize {
    let dir = std::env::var("LEAF_VECTORS_DIR").unwrap_or_else(|_| "E:/ddda-re/notes/native-leaves".into());
    let path = format!("{dir}/{name}");
    let text = match std::fs::read_to_string(&path) {
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
        body(&mut m, &mut c);
        let got = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        assert_eq!(got, after, "case {i}: eax ecx edx ebx esp ebp esi edi");
        if flags {
            // the arithmetic flags the original leaves, where the port claims them (undefined bits are not compared)
            let eflags = n(&case["after"]["eflags"]);
            let (value, defined) = c.fl.snapshot();
            let mask = defined & 0x8D5; // CF PF AF ZF SF OF
            assert_eq!(value & mask, eflags & mask, "case {i}: eflags (defined {defined:#x})");
        }
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "case {i}: extra write {a:x}");
            }
        }
    }
    cases.len()
}

/// Every leaf here is checked on all eight registers: their leftovers are part of what they do.
fn check(file: &str, body: fn(&mut dyn Mem, &mut Cpu), flags: bool) {
    let cases = run_cases(file, body, flags);
    if cases > 0 {
        assert!(cases >= 1000, "{file}: {cases} cases");
    }
    eprintln!("{cases} ABI-level {file} cases passed");
}

// verifies 0x005A65E0 (register arguments EAX, ECX)
#[test]
fn clear_grid_matches_original() {
    check("clear_grid.json", mtcore::semantic_leaves::clear_grid, false);
}

// verifies 0x00767B40
#[test]
fn lanes_in_range_matches_original() {
    check("lanes_in_range.json", mtcore::semantic_leaves::lanes_in_range, true);
}

// verifies 0x004EA200
#[test]
fn find_record_matches_original() {
    check("find_record.json", mtcore::semantic_leaves::find_record, true);
}

// verifies 0x01328FD0
#[test]
fn advance_bits_matches_original() {
    check("advance_bits.json", mtcore::semantic_leaves::advance_bits, true);
}


// verifies 0x00D00690
#[test]
fn xorshift_step_matches_original() {
    check("xorshift_step.json", mtcore::semantic_leaves::xorshift_step, true);
}

// verifies 0x00FF9A20 (flags not reproduced)
#[test]
fn table_entry_or_default_matches_original() {
    check("table_entry_or_default.json", mtcore::semantic_leaves::table_entry_or_default, false);
}

// verifies 0x00421ED0
#[test]
fn table_entry_or_null_matches_original() {
    check("table_entry_or_null.json", mtcore::semantic_leaves::table_entry_or_null, true);
}

// verifies 0x005D8EB0
#[test]
fn promote_state_matches_original() {
    check("fix_state_flags.json", mtcore::semantic_leaves::promote_state, true);
}

// verifies 0x00E0A4E0
#[test]
fn find_entry_by_key_matches_original() {
    check("find_entry_by_key.json", mtcore::semantic_leaves::find_entry_by_key, true);
}

// verifies 0x00DE5BD0
#[test]
fn init_record_a_matches_original() {
    check("init_record_a.json", mtcore::semantic_leaves::init_record_a, true);
}

// verifies 0x00DE0AA0
#[test]
fn init_record_b_matches_original() {
    check("init_record_b.json", mtcore::semantic_leaves::init_record_b, true);
}

// verifies 0x01105EF0 (EAX.AH holds the UCOMISS flags the original read with LAHF)
#[test]
fn set_float_if_changed_matches_original() {
    check("set_float_if_changed.json", mtcore::semantic_leaves::set_float_if_changed, true);
}

// verifies 0x01328F30 (the bit reader's look; the mask table is read from the image)
#[test]
fn look_bits_matches_original() {
    check("look_bits.json", mtcore::semantic_leaves::look_bits, true);
}

// verifies 0x011AE1F0 (SSE only: flags untouched)
#[test]
fn dominant_row_matches_original() {
    check("dominant_row.json", mtcore::semantic_leaves::dominant_row, false);
}

// verifies 0x004241F0 (register argument EAX; the image float triples are read from memory)
#[test]
fn init_node_matches_original() {
    check("init_node.json", mtcore::semantic_leaves::init_node, true);
}

// verifies 0x005F1F90 (register argument ECX; flags not reproduced)
#[test]
fn flag_to_index_matches_original() {
    check("flag_to_index.json", mtcore::semantic_leaves::flag_to_index, false);
}

// verifies 0x00DE5CC0
#[test]
fn init_record_c_matches_original() {
    check("init_record_c.json", mtcore::semantic_leaves::init_record_c, true);
}

// verifies 0x00B76580
#[test]
fn update_flag_bit4_matches_original() {
    check("update_flag_bit4.json", mtcore::semantic_leaves::update_flag_bit4, true);
}

// verifies 0x00850410 (flags not reproduced)
#[test]
fn clear_entry_flags_matches_original() {
    check("clear_entry_flag.json", mtcore::semantic_leaves::clear_entry_flags, false);
}

// verifies 0x00B85F70
#[test]
fn select_constant_matches_original() {
    check("select_constant.json", mtcore::semantic_leaves::select_constant, true);
}

// verifies 0x004CA510 (flags not reproduced)
#[test]
fn init_vtables_matches_original() {
    check("init_vtables.json", mtcore::semantic_leaves::init_vtables, false);
}
