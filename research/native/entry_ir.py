"""Action-entry translator: recognizes functions shaped
    push esi; mov esi, ecx; call BASE            (BASE has a memory-model form)
    <straight-line body>                         (grammar below)
    pop esi; ret
and emits each as a literal Rust function over ddnative::mem::Mem, in the
original's instruction order. The body may build and make one owner
motion request (the owner's vtable +0x90, six arguments), load/store/or/
and fields of the owner ([this+0x74]) and of the action (this), with
immediates or loaded values. Anything else rejects the function.

    python entry_ir.py scan       # recognize; write patterns/entry_ir.json
    python entry_ir.py vectors    # record whole-image vectors
    python entry_ir.py gen        # write crates/ddnative/src/generated/entries.rs + tests/entries.rs

Run with E:/ddda-re/tools/pyenv/Scripts/python.exe (scan, vectors); gen
needs only the system Python.
"""
import json
import os
import random
import re
import sqlite3
import struct
import sys

N = "E:/ddda-re/notes/native"
DB = "E:/ddda-re/notes/inventory/inventory.sqlite"
R = os.path.join(os.path.dirname(__file__), "..", "..")
BASE_GLOBALS = {  # verified bases with a memory-model form (ddnative::species::BASES)
    0x008D4840: 0x01996BC4, 0x00A4DB70: 0x0199F320, 0x008FBDD0: 0x01997F64, 0x00919DB0: 0x0199886C,
    0x0095A810: 0x0199A36C, 0x00A2AE80: 0x0199EE38, 0x00A202E0: 0x0199E9D8, 0x009C5890: 0x0199CEA4,
    0x009B4920: 0x0199C82C, 0x009BD970: 0x0199CD44,
}
REGS = ("eax", "ecx", "edx", "ebx", "edi", "ebp")
MEM = re.compile(r"^(byte|word|dword) ptr \[(\w+)(?: \+ (0x[0-9a-f]+|\d+))?\]$")


def translate(addr, size):
    """Rust body lines (after `base(m)`) or None."""
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    ins = [(i.mnemonic, i.op_str) for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(h.code(addr, size), addr)]
    if ins[:2] != [("push", "esi"), ("mov", "esi, ecx")] or ins[2][0] != "call" or not ins[2][1].startswith("0x"):
        return None
    base = int(ins[2][1], 16)
    if base not in BASE_GLOBALS:
        return None
    out, val, kind = [], {}, {}      # reg -> Rust expr; reg -> 'owner'|'vt'|'vfn'|'value'
    xmm0, slots, pushes, n = None, None, [], [0]

    def fresh(expr):
        n[0] += 1
        out.append(f"    let v{n[0]} = {expr};")
        return f"v{n[0]}"

    def operand_value(op):
        if op in REGS and kind.get(op) == "value":
            return val[op]
        if re.fullmatch(r"0x[0-9a-f]+|\d+", op):
            return f"0x{int(op, 0) & 0xFFFFFFFF:X}"
        return None

    def address(m):
        """Rust address expression for [reg + off], or None."""
        reg, off = m.group(2), int(m.group(3) or "0", 0)
        if reg == "esi":
            return f"this + 0x{off:X}"
        if kind.get(reg) == "owner":
            return f"{val[reg]} + 0x{off:X}"
        return None

    i = 3
    motion_done = False
    while i < len(ins):
        mn, op = ins[i]
        a = op.split(", ") if ", " in op else [op]
        if (mn, op) == ("pop", "esi") and i + 1 < len(ins) and ins[i + 1] == ("ret", "") and i + 2 == len(ins):
            if slots is not None and not motion_done:
                return None
            return base, out
        if mn == "movss" and len(a) == 2 and a[0] == "xmm0" and a[1].startswith("dword ptr [0x"):
            c = int(a[1].split("[")[1].rstrip("]"), 16)
            xmm0 = f"0x{struct.unpack('<I', h.code(c, 4))[0]:08X}"
        elif (mn, op) == ("xorps", "xmm0, xmm0"):
            xmm0 = "0x00000000"
        elif (mn, op) == ("sub", "esp, 0xc") and slots is None and not motion_done:
            slots = [None, None, None]
        elif mn == "movss" and a[0].startswith("dword ptr [esp") and a[1] == "xmm0" and slots is not None and xmm0:
            inner = a[0].split("[")[1].rstrip("]")
            k = 0 if inner == "esp" else int(inner.split("+ ")[1], 0)
            if k not in (0, 4, 8):
                return None
            slots[k // 4] = xmm0
        elif mn == "mov" and a[0] in REGS and MEM.match(a[1]):
            m = MEM.match(a[1])
            if m.group(1) != "dword":
                return None
            reg, off = m.group(2), int(m.group(3) or "0", 0)
            if reg == "esi" and off == 0x74:
                val[a[0]], kind[a[0]] = fresh("m.u32(this + 0x74)"), "owner"
            elif kind.get(reg) == "owner" and off == 0 and m.group(3) is None:
                kind[a[0]], val[a[0]] = "vt", val[reg]
            elif kind.get(reg) == "vt" and off == 0x90:
                kind[a[0]], val[a[0]] = "vfn", val[reg]
            else:
                ad = address(m)
                if ad is None:
                    return None
                val[a[0]], kind[a[0]] = fresh(f"m.u32({ad})"), "value"
        elif mn == "mov" and a[0] in REGS and operand_value(a[1]) is not None and a[1] not in REGS:
            val[a[0]], kind[a[0]] = operand_value(a[1]), "value"
        elif mn in ("or", "and", "mov") and MEM.match(a[0]):
            m = MEM.match(a[0])
            ad = address(m)
            v = operand_value(a[1]) if len(a) == 2 else None
            if ad is None or v is None:
                return None
            width = {"byte": ("u8", "set_u8"), "word": (None, "set_u16"), "dword": ("u32", "set_u32")}[m.group(1)]
            cast = {"byte": " as u8", "word": " as u16", "dword": ""}[m.group(1)]
            if mn == "mov":
                out.append(f"    m.{width[1]}({ad}, ({v}){cast});")
            else:
                if width[0] is None:
                    return None
                sym = "|" if mn == "or" else "&"
                out.append(f"    {{ let x = m.{width[0]}({ad}) {sym} (({v}){cast}); m.{width[1]}({ad}, x); }}")
        elif mn == "push" and not motion_done:
            v = operand_value(op)
            if v is None:
                return None
            pushes.append(v)
        elif mn == "call" and op in REGS and kind.get(op) == "vfn" and not motion_done:
            if slots is None or None in slots or len(pushes) != 3:
                return None
            args = list(reversed(pushes)) + slots
            out.append(f"    motion(m, {val[op]}, [{', '.join(args)}]);")
            motion_done = True
            for r in ("eax", "ecx", "edx"):
                val.pop(r, None)
                kind.pop(r, None)
        else:
            return None
        i += 1
    return None


def scan():
    db = sqlite3.connect(DB)
    found = []
    for a, ns, nm, sz in db.execute("SELECT address, namespace, name, size FROM functions WHERE NOT thunk "
                                    "AND status != 'platform' AND size BETWEEN 20 AND 400 AND address < 0x012E0000"):
        r = translate(a, sz)
        if r:
            found.append({"addr": a, "name": f"{ns}::{nm}", "size": sz, "base": r[0], "body": r[1]})
    json.dump(found, open(f"{N}/patterns/entry_ir.json", "w"))
    print(len(found), "entries translated")


def vectors():
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    S = h.SCRATCH
    found = json.load(open(f"{N}/patterns/entry_ir.json"))
    rnd = random.Random(59)
    OWN, VT, DTIF, CAPF, DTI, CAP, SIZE = 0x1000, 0x100, 0x180, 0x1C0, 0x200, 0x300, 0x9000
    cap_code = b"".join(bytes([0x8B, 0x44, 0x24, 4 + 4 * i, 0xA3]) + struct.pack("<I", S + CAP + 4 * i) for i in range(6))
    cap_code += bytes([0x89, 0x0D]) + struct.pack("<I", S + CAP + 24) + bytes([0xC2, 0x18, 0x00])
    cases = []
    for f in found:
        g = BASE_GLOBALS[f["base"]]
        for _ in range(3):
            img = bytearray(rnd.getrandbits(8) for _ in range(SIZE))
            struct.pack_into("<I", img, 0x44, S + OWN); struct.pack_into("<I", img, OWN, S + VT)
            struct.pack_into("<I", img, VT + 0x10, S + DTIF); struct.pack_into("<I", img, VT + 0x90, S + CAPF)
            img[DTIF:DTIF + 6] = bytes([0xB8]) + struct.pack("<I", S + DTI) + bytes([0xC3])
            img[CAPF:CAPF + len(cap_code)] = cap_code
            cid = rnd.getrandbits(32) | 1
            struct.pack_into("<I", img, DTI + 4, cid); struct.pack_into("<I", img, DTI + 0x10, 0)
            struct.pack_into("<I", img, OWN + 0x60E8, 0); struct.pack_into("<I", img, OWN + 0x7324, 0)
            gl = [rnd.getrandbits(32) & 0x7F7FFFFF for _ in range(3)]
            r = h.run(f["addr"], ecx=S, scratch=bytes(img), allow={f["base"], 0x007F1890},
                      poke={g: cid, 0x018CFE80: gl[0], 0x018CFE84: gl[1], 0x018CFE88: gl[2]})
            assert r["esp_delta"] == 0
            cases.append({"addr": f["addr"], "base": f["base"], "globals": gl,
                          "in": img.hex(), "out": r["scratch"][:SIZE].hex()})
    json.dump(cases, open(f"{N}/vectors/entry_ir.json", "w"))
    print(len(cases), "cases over", len(found), "functions")


def gen():
    found = sorted(json.load(open(f"{N}/patterns/entry_ir.json")), key=lambda f: f["addr"])
    L = ["//! Generated by research/native/entry_ir.py: do not edit.",
         "//! Action entries translated line by line over `Mem` (the original's",
         "//! byte layout): `base` is the species' verified base entry, `motion`",
         "//! the owner's motion request (vtable +0x90).",
         "#![allow(unused_variables, clippy::all)]", "",
         "use crate::mem::Mem;", "",
         "pub type Base<'a> = &'a mut dyn FnMut(&mut dyn Mem);",
         "pub type Motion<'a> = &'a mut dyn FnMut(&mut dyn Mem, u32, [u32; 6]);",
         "pub type EntryFn = fn(&mut dyn Mem, u32, Base, Motion);", ""]
    for f in found:
        L.append(f"// Translated from 0x{f['addr']:08X} (DDDA.exe 2017-12-13 Steam), {f['name']}.")
        L.append(f"pub fn f_{f['addr']:08x}(m: &mut dyn Mem, this: u32, base: Base, motion: Motion) {{")
        L.append("    base(m);")
        L += f["body"]
        L.append("}")
        L.append("")
    L.append("/// (address, base entry, function)")
    L.append("pub const ENTRIES: &[(u32, u32, EntryFn)] = &[")
    L += [f"    (0x{f['addr']:08X}, 0x{f['base']:08X}, f_{f['addr']:08x})," for f in found]
    L += ["];", ""]
    open(os.path.join(R, "crates/ddnative/src/generated/entries.rs"), "w").write("\n".join(L))
    addrs = " ".join(f"0x{f['addr']:08X}" for f in found)
    test = f'''//! Generated by research/native/entry_ir.py: do not edit.
use ddnative::generated::entries::ENTRIES;
use ddnative::mem::{{Image, Mem}};
use serde_json::Value;

fn vectors(name: &str) -> Option<Vec<Value>> {{
    let path = format!("E:/ddda-re/notes/native/vectors/{{name}}.json");
    let text = match std::fs::read_to_string(&path) {{
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{{path}}: {{e}}"),
        Err(_) => return None,
    }};
    serde_json::from_str::<Value>(&text).ok()?.as_array().cloned()
}}
fn hex(s: &str) -> Vec<u8> {{ (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect() }}

// verifies {addrs}
#[test]
fn every_generated_entry_matches_the_original() {{
    const S: u32 = 0x0E00_0000;
    const CAP: u32 = S + 0x300;
    let Some(cases) = vectors("entry_ir") else {{ return }};
    let mut seen = std::collections::HashSet::new();
    for c in &cases {{
        let addr = c["addr"].as_u64().unwrap() as u32;
        let (_, base_addr, f) = *ENTRIES.iter().find(|e| e.0 == addr).expect("generated entry");
        let g: Vec<u32> = c["globals"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
        let mut m = Image {{ base: S, bytes: hex(c["in"].as_str().unwrap()) }};
        let mut base = |m: &mut dyn Mem| {{
            // the DTI fixture always matches the class in these vectors
            assert!(ddnative::species::base_entry_mem(m, base_addr, S, true, [g[0], g[1], g[2]]));
        }};
        // the capture fixture the vectors used for the owner's +0x90
        let mut motion = |m: &mut dyn Mem, owner: u32, args: [u32; 6]| {{
            for (i, a) in args.into_iter().enumerate() {{ m.set_u32(CAP + 4 * i as u32, a); }}
            m.set_u32(CAP + 24, owner);
        }};
        f(&mut m, S, &mut base, &mut motion);
        let want = hex(c["out"].as_str().unwrap());
        if m.bytes != want {{
            let d: Vec<String> = (0..want.len()).filter(|&i| m.bytes[i] != want[i]).take(6)
                .map(|i| format!("+{{i:#x}} got {{:#04x}} want {{:#04x}}", m.bytes[i], want[i])).collect();
            panic!("{{addr:#010x}}: {{d:?}}");
        }}
        seen.insert(addr);
    }}
    assert_eq!(seen.len(), ENTRIES.len(), "every generated entry has vectors");
}}
'''
    open(os.path.join(R, "crates/ddnative/tests/entries.rs"), "w").write(test)
    print(len(found), "functions generated")


if __name__ == "__main__":
    {"scan": scan, "vectors": vectors, "gen": gen}[sys.argv[1]]()
