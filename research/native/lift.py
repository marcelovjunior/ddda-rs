"""The lifter: translates a leaf function (no calls) of DDDA.exe into
literal Rust over ddnative::mem::Mem, instruction by instruction, with
exact x86 semantics for the supported subset, and rejects anything else.
Verified by run_sparse vectors (random registers, random memory on
demand, every byte read and written recorded).

    python lift.py scan [N]     # lift every non-platform leaf; patterns/lift.json
    python lift.py vectors      # record sparse vectors for each lifted function
    python lift.py gen          # crates/ddnative/src/generated/lifted*.rs + tests/lifted.rs
    python lift.py one ADDR     # print the Rust for one function

Run scan/vectors/one with E:/ddda-re/tools/pyenv/Scripts/python.exe;
gen needs only the system Python. docs/NATIVE_PIPELINE.md, "The lifter".
"""
import json
import os
import sqlite3
import struct
import sys

N = "E:/ddda-re/notes/native"
DB = "E:/ddda-re/notes/inventory/inventory.sqlite"
R = os.path.join(os.path.dirname(__file__), "..", "..")

GPR32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
SUB8 = {"al": ("eax", 0), "cl": ("ecx", 0), "dl": ("edx", 0), "bl": ("ebx", 0),
        "ah": ("eax", 8), "ch": ("ecx", 8), "dh": ("edx", 8), "bh": ("ebx", 8)}
SUB16 = {"ax": "eax", "cx": "ecx", "dx": "edx", "bx": "ebx", "sp": "esp", "bp": "ebp", "si": "esi", "di": "edi"}
XMM = [f"xmm{i}" for i in range(8)]


class Unsupported(Exception):
    pass


def lift(addr, size):
    """Rust function body for the function, or raise Unsupported."""
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    ins = list(md.disasm(h.code(addr, size), addr))
    if not ins or sum(i.size for i in ins) != size:
        raise Unsupported("disassembly does not cover the function")
    end = addr + size
    # basic blocks
    leaders = {addr}
    for k, i in enumerate(ins):
        if i.mnemonic.startswith("j"):
            if i.operands[0].type != X86_OP_IMM:
                raise Unsupported("indirect jump")
            t = i.operands[0].imm
            if not (addr <= t < end):
                raise Unsupported("jump out of the function")
            leaders.add(t)
            if k + 1 < len(ins):
                leaders.add(ins[k + 1].address)
        if i.mnemonic in ("ret", "retn") and k + 1 < len(ins):
            leaders.add(ins[k + 1].address)
    if not leaders <= {i.address for i in ins}:
        raise Unsupported("jump into the middle of an instruction")

    def reg(name):
        """Read a register as a u32 expression."""
        if name in GPR32:
            return name
        if name in SUB8:
            r, sh = SUB8[name]
            return f"(({r} >> {sh}) & 0xFF)"
        if name in SUB16:
            return f"({SUB16[name]} & 0xFFFF)"
        raise Unsupported(f"register {name}")

    def set_reg(name, v):
        if name in GPR32:
            return f"{name} = {v};"
        if name in SUB8:
            r, sh = SUB8[name]
            return f"{r} = ({r} & !(0xFF << {sh})) | ((({v}) & 0xFF) << {sh});"
        if name in SUB16:
            r = SUB16[name]
            return f"{r} = ({r} & !0xFFFF) | (({v}) & 0xFFFF);"
        raise Unsupported(f"register {name}")

    def bits_of(i, op):
        return op.size * 8

    def mem_addr(i, op):
        m = op.mem
        if m.segment != 0:
            raise Unsupported("segment override")
        parts = []
        if m.base != 0:
            parts.append(reg(i.reg_name(m.base)))
        if m.index != 0:
            parts.append(f"{reg(i.reg_name(m.index))}.wrapping_mul({m.scale})")
        e = parts[0] if parts else "0u32"
        for p in parts[1:]:
            e = f"{e}.wrapping_add({p})"
        if m.disp:
            e = f"{e}.wrapping_add(0x{m.disp & 0xFFFFFFFF:X})"
        return e

    def load(i, op):
        """u32 expression of an operand (zero-extended for 8/16-bit)."""
        if op.type == X86_OP_REG:
            return reg(i.reg_name(op.reg))
        if op.type == X86_OP_IMM:
            return f"0x{op.imm & ((1 << (op.size * 8)) - 1):X}u32"
        if op.type == X86_OP_MEM:
            a = mem_addr(i, op)
            return {1: f"(m.u8({a}) as u32)", 2: f"(m.u16({a}) as u32)", 4: f"m.u32({a})"}[op.size]
        raise Unsupported("operand")

    def store(i, op, v):
        if op.type == X86_OP_REG:
            return set_reg(i.reg_name(op.reg), v)
        if op.type == X86_OP_MEM:
            a = mem_addr(i, op)
            return {1: f"m.set_u8({a}, ({v}) as u8);", 2: f"m.set_u16({a}, ({v}) as u16);",
                    4: f"m.set_u32({a}, {v});"}[op.size]
        raise Unsupported("store operand")

    def xmm(i, op):
        n = i.reg_name(op.reg)
        if n not in XMM:
            raise Unsupported(f"xmm {n}")
        return n

    def sx(v, bits):
        return f"(({v}) as i{bits} as i32 as u32)" if bits in (8, 16) else v

    lines = []
    blocks = sorted(leaders)
    for b in blocks:
        body = []
        k = next(j for j, i in enumerate(ins) if i.address == b)
        terminated = False
        while k < len(ins):
            i = ins[k]
            mn, ops = i.mnemonic, i.operands
            nxt = ins[k + 1].address if k + 1 < len(ins) else None
            if i.address != b and i.address in leaders:
                body.append(f"pc = 0x{i.address:X}; continue;")
                terminated = True
                break
            bits = ops[0].size * 8 if ops else 32
            mask = "u32::MAX" if bits == 32 else f"0x{(1 << bits) - 1:X}"
            if mn == "mov":
                body.append(store(i, ops[0], load(i, ops[1])))
            elif mn in ("movzx", "movsx"):
                v = load(i, ops[1])
                body.append(store(i, ops[0], v if mn == "movzx" else sx(v, ops[1].size * 8)))
            elif mn == "lea":
                body.append(store(i, ops[0], mem_addr(i, ops[1])))
            elif mn in ("add", "sub", "cmp", "and", "or", "xor", "test"):
                a, c = load(i, ops[0]), load(i, ops[1])
                if len(ops) == 2 and ops[1].type == X86_OP_IMM and ops[1].size < ops[0].size:
                    c = f"0x{ops[1].imm & 0xFFFFFFFF:X}u32"
                op = {"add": "wrapping_add", "sub": "wrapping_sub", "cmp": "wrapping_sub"}.get(mn)
                if op:
                    r = f"(a.{op}(c) & {mask})"
                    kind = "ADD" if mn == "add" else "SUB"
                else:
                    sym = {"and": "&", "test": "&", "or": "|", "xor": "^"}[mn]
                    r = f"((a {sym} c) & {mask})"
                    kind = "LOGIC"
                body.append(f"{{ let a = {a} & {mask}; let c = {c} & {mask}; let r = {r}; "
                            f"fl = Flags::new(x86::{kind}, a, c, r, {bits});"
                            + ("" if mn in ("cmp", "test") else " " + store(i, ops[0], "r")) + " }")
            elif mn in ("inc", "dec", "neg", "not"):
                a = load(i, ops[0])
                if mn == "not":
                    body.append(store(i, ops[0], f"(!{a}) & {mask}"))
                elif mn == "neg":
                    body.append(f"{{ let a = {a} & {mask}; let r = 0u32.wrapping_sub(a) & {mask}; "
                                f"fl = Flags::new(x86::SUB, 0, a, r, {bits}); {store(i, ops[0], 'r')} }}")
                else:
                    op, kind = ("wrapping_add", "INC") if mn == "inc" else ("wrapping_sub", "DEC")
                    body.append(f"{{ let a = {a} & {mask}; let r = a.{op}(1) & {mask}; let cf = fl.cf(); "
                                f"fl = Flags::new(x86::{kind}, a, 1, r, {bits}); fl.cf_in = cf; {store(i, ops[0], 'r')} }}")
            elif mn in ("shl", "shr", "sar", "sal"):
                a = load(i, ops[0])
                cnt = "1u32" if len(ops) == 1 else load(i, ops[1])
                expr = {"shl": "a << n", "sal": "a << n", "shr": "a >> n",
                        "sar": f"((({sx('a', bits) if bits < 32 else 'a'}) as i32) >> n) as u32"}[mn]
                kind = {"shl": "SHL", "sal": "SHL", "shr": "SHR", "sar": "SAR"}[mn]
                body.append(f"{{ let a = {a} & {mask}; let n = {cnt} & 31; if n != 0 {{ let r = ({expr}) & {mask}; "
                            f"fl = Flags::new(x86::{kind}, a, n, r, {bits}); {store(i, ops[0], 'r')} }} }}")
            elif mn == "imul" and len(ops) in (2, 3):
                a = load(i, ops[1]) if len(ops) == 3 else load(i, ops[0])
                c = load(i, ops[2]) if len(ops) == 3 else load(i, ops[1])
                if bits != 32:
                    raise Unsupported("imul width")
                body.append(f"{{ let r = ({a} as i32).wrapping_mul({c} as i32) as u32; "
                            f"fl = Flags::new(x86::IMUL, 0, 0, r, 32); {store(i, ops[0], 'r')} }}")
            elif mn == "cdq":
                body.append("edx = if (eax as i32) < 0 { u32::MAX } else { 0 };")
            elif mn.startswith("set"):
                body.append(store(i, ops[0], f"fl.cond(\"{mn[3:]}\") as u32"))
            elif mn == "push":
                body.append(f"{{ let v = {load(i, ops[0])}; esp = esp.wrapping_sub(4); m.set_u32(esp, v); }}")
            elif mn == "pop":
                body.append(f"{{ let v = m.u32(esp); esp = esp.wrapping_add(4); {store(i, ops[0], 'v')} }}")
            elif mn in ("ret", "retn"):
                extra = ops[0].imm if ops else 0
                body.append(f"esp = esp.wrapping_add(4 + {extra}); return Exit {{ eax, ecx, edx, ebx, esp, ebp, esi, edi, xmm0: xmm0[0], st0: st.last().copied() }};")
                terminated = True
                break
            elif mn == "jmp":
                body.append(f"pc = 0x{ops[0].imm:X}; continue;")
                terminated = True
                break
            elif mn.startswith("j"):
                body.append(f"pc = if fl.cond(\"{mn[1:]}\") {{ 0x{ops[0].imm:X} }} else {{ 0x{nxt:X} }}; continue;")
                terminated = True
                break
            elif mn == "nop":
                pass
            # ---- SSE
            elif mn == "movss":
                if ops[0].type == X86_OP_REG and ops[1].type == X86_OP_MEM:
                    body.append(f"{xmm(i, ops[0])} = [m.u32({mem_addr(i, ops[1])}), 0, 0, 0];")
                elif ops[0].type == X86_OP_MEM:
                    body.append(f"m.set_u32({mem_addr(i, ops[0])}, {xmm(i, ops[1])}[0]);")
                else:
                    body.append(f"{xmm(i, ops[0])}[0] = {xmm(i, ops[1])}[0];")
            elif mn in ("movaps", "movups", "movdqa", "movdqu"):
                if ops[0].type == X86_OP_REG and ops[1].type == X86_OP_REG:
                    body.append(f"{xmm(i, ops[0])} = {xmm(i, ops[1])};")
                elif ops[0].type == X86_OP_REG:
                    a = mem_addr(i, ops[1])
                    body.append(f"{{ let a = {a}; {xmm(i, ops[0])} = [m.u32(a), m.u32(a.wrapping_add(4)), m.u32(a.wrapping_add(8)), m.u32(a.wrapping_add(12))]; }}")
                else:
                    a = mem_addr(i, ops[0])
                    x = xmm(i, ops[1])
                    body.append(f"{{ let a = {a}; for k in 0..4u32 {{ m.set_u32(a.wrapping_add(4 * k), {x}[k as usize]); }} }}")
            elif mn == "movq" and ops[0].type == X86_OP_REG and i.reg_name(ops[0].reg) in XMM:
                if ops[1].type == X86_OP_MEM:
                    a = mem_addr(i, ops[1])
                    body.append(f"{{ let a = {a}; {xmm(i, ops[0])} = [m.u32(a), m.u32(a.wrapping_add(4)), 0, 0]; }}")
                elif i.reg_name(ops[1].reg) in XMM:
                    body.append(f"{xmm(i, ops[0])} = [{xmm(i, ops[1])}[0], {xmm(i, ops[1])}[1], 0, 0];")
                else:
                    raise Unsupported("movq form")
            elif mn == "movq" and ops[0].type == X86_OP_MEM:
                a = mem_addr(i, ops[0])
                x = xmm(i, ops[1])
                body.append(f"{{ let a = {a}; m.set_u32(a, {x}[0]); m.set_u32(a.wrapping_add(4), {x}[1]); }}")
            elif mn in ("xorps", "pxor", "andps", "orps", "andnps"):
                d = xmm(i, ops[0])
                src = xmm(i, ops[1]) if ops[1].type == X86_OP_REG else None
                if src is None:
                    a = mem_addr(i, ops[1])
                    body.append(f"{{ let a = {a}; let s = [m.u32(a), m.u32(a.wrapping_add(4)), m.u32(a.wrapping_add(8)), m.u32(a.wrapping_add(12))];")
                    src = "s"
                else:
                    body.append("{")
                f = {"xorps": "d ^ s", "pxor": "d ^ s", "andps": "d & s", "orps": "d | s", "andnps": "!d & s"}[mn]
                body.append(f"for k in 0..4 {{ let (d, s) = ({d}[k], {src}[k]); {d}[k] = {f}; }} }}")
            elif mn in ("addss", "subss", "mulss", "divss", "minss", "maxss", "sqrtss"):
                d = xmm(i, ops[0])
                s = f"{xmm(i, ops[1])}[0]" if ops[1].type == X86_OP_REG else f"m.u32({mem_addr(i, ops[1])})"
                body.append(f"{d}[0] = x86::sqrtss({s});" if mn == "sqrtss" else f"{d}[0] = x86::{mn}({d}[0], {s});")
            elif mn in ("addps", "subps", "mulps"):
                d = xmm(i, ops[0])
                if ops[1].type != X86_OP_REG:
                    raise Unsupported("packed memory operand")
                s = xmm(i, ops[1])
                body.append(f"for k in 0..4 {{ {d}[k] = x86::{mn[:3]}ss({d}[k], {s}[k]); }}")
            elif mn == "shufps":
                d, s = xmm(i, ops[0]), (xmm(i, ops[1]) if ops[1].type == X86_OP_REG else None)
                if s is None:
                    raise Unsupported("shufps memory")
                imm = ops[2].imm
                body.append(f"{d} = [{d}[{imm & 3}], {d}[{(imm >> 2) & 3}], {s}[{(imm >> 4) & 3}], {s}[{(imm >> 6) & 3}]];")
            elif mn in ("comiss", "ucomiss"):
                d = xmm(i, ops[0])
                s = f"{xmm(i, ops[1])}[0]" if ops[1].type == X86_OP_REG else f"m.u32({mem_addr(i, ops[1])})"
                body.append(f"fl = x86::comiss({d}[0], {s});")
            elif mn == "cvttss2si":
                s = f"{xmm(i, ops[1])}[0]" if ops[1].type == X86_OP_REG else f"m.u32({mem_addr(i, ops[1])})"
                body.append(store(i, ops[0], f"x86::cvttss2si({s})"))
            elif mn == "cvtsi2ss":
                body.append(f"{xmm(i, ops[0])}[0] = x86::cvtsi2ss({load(i, ops[1])});")
            # ---- x87 copies only
            elif mn == "fld" and ops[0].type == X86_OP_MEM and ops[0].size == 4:
                body.append(f"st.push(crate::x87_copy(f32::from_bits(m.u32({mem_addr(i, ops[0])}))).to_bits());")
            elif mn in ("fstp", "fst") and ops[0].type == X86_OP_MEM and ops[0].size == 4:
                take = "st.pop().expect(\"x87 stack\")" if mn == "fstp" else "*st.last().expect(\"x87 stack\")"
                body.append(f"{{ let v = {take}; m.set_u32({mem_addr(i, ops[0])}, v); }}")
            elif mn == "fstp" and ops[0].type == X86_OP_REG and i.reg_name(ops[0].reg) == "st(0)":
                body.append("st.pop();")
            elif mn in ("fld1", "fldz"):
                body.append(f"st.push({'0x3F800000' if mn == 'fld1' else '0'});")
            elif mn == "fchs":
                body.append("{ let v = st.pop().expect(\"x87 stack\"); st.push(v ^ 0x8000_0000); }")
            elif mn == "fabs":
                body.append("{ let v = st.pop().expect(\"x87 stack\"); st.push(v & 0x7FFF_FFFF); }")
            else:
                raise Unsupported(f"{mn} {i.op_str}")
            k += 1
        if not terminated:
            if k >= len(ins):
                raise Unsupported("falls off the end")
        lines.append(f"            0x{b:X} => {{ {' '.join(body)} }}")
    return lines


def rust_fn(addr, name, arms):
    return "\n".join([
        f"// Translated from 0x{addr:08X} (DDDA.exe 2017-12-13 Steam), {name}.",
        "#[allow(unused_mut, unused_variables, unused_assignments, unreachable_code, clippy::all)]",
        f"pub fn f_{addr:08x}(m: &mut dyn Mem, r: Entry) -> Exit {{",
        "    let Entry { mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi, xmm0: x0in } = r;",
        "    let (mut xmm0, mut xmm1, mut xmm2, mut xmm3) = ([x0in, 0, 0, 0], [0u32; 4], [0u32; 4], [0u32; 4]);",
        "    let (mut xmm4, mut xmm5, mut xmm6, mut xmm7) = ([0u32; 4], [0u32; 4], [0u32; 4], [0u32; 4]);",
        "    let mut fl = Flags::default();",
        "    let mut st: Vec<u32> = Vec::new();",
        f"    let mut pc: u32 = 0x{addr:X};",
        "    loop {",
        "        match pc {",
        *arms,
        "            _ => unreachable!(),",
        "        }",
        "    }",
        "}", ""])


def scan(limit=None):
    db = sqlite3.connect(DB)
    from collections import Counter
    ok, why = [], Counter()
    rows = db.execute("SELECT address, namespace, name, size FROM functions WHERE NOT thunk "
                      "AND status NOT IN ('verified', 'platform') AND size BETWEEN 6 AND 600 "
                      "AND address < 0x012E0000 ORDER BY address").fetchall()
    for a, ns, nm, sz in rows[:limit]:
        try:
            arms = lift(a, sz)
            ok.append({"addr": a, "name": f"{ns}::{nm}", "size": sz, "arms": arms})
        except Unsupported as e:
            why[str(e).split(" ")[0]] += 1
        except Exception as e:  # noqa: BLE001
            why["error:" + type(e).__name__] += 1
    json.dump(ok, open(f"{N}/patterns/lift.json", "w"))
    print(len(ok), "lifted of", len(rows[:limit]), "; top rejections:", why.most_common(14))


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "scan":
        scan(int(sys.argv[2]) if len(sys.argv) > 2 else None)
    elif cmd == "one":
        import sqlite3 as _s
        a = int(sys.argv[2], 16)
        sz, ns, nm = _s.connect(DB).execute("select size, namespace, name from functions where address=?", (a,)).fetchone()
        print(rust_fn(a, f"{ns}::{nm}", lift(a, sz)))


def vectors(per=3):
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    lifted = json.load(open(f"{N}/patterns/lift.json"))
    cases, failed = [], []
    for f in lifted:
        got, seed = 0, 0
        while got < per and seed < per * 8:
            seed += 1
            try:
                r = h.run_sparse(f["addr"], (f["addr"] << 8) ^ seed)
            except h.Rejected:
                continue
            cases.append({"addr": f["addr"], **r})
            got += 1
        if got == 0:
            failed.append(f["addr"])
    keep = {c["addr"] for c in cases}
    json.dump([f for f in lifted if f["addr"] in keep], open(f"{N}/patterns/lift.json", "w"))
    json.dump(cases, open(f"{N}/vectors/lift.json", "w"))
    print(len(cases), "cases over", len(keep), "functions;", len(failed), "without a usable vector (dropped)")


def gen(chunk=400):
    lifted = sorted(json.load(open(f"{N}/patterns/lift.json")), key=lambda f: f["addr"])
    out_dir = os.path.join(R, "crates/ddnative/src/generated")
    mods = []
    for k in range(0, len(lifted), chunk):
        part = lifted[k:k + chunk]
        name = f"lifted_{k // chunk:03}"
        mods.append(name)
        L = ["//! Generated by research/native/lift.py: do not edit.",
             "//! Leaf functions lifted instruction by instruction (docs/NATIVE_PIPELINE.md).",
             "use crate::mem::Mem;", "use crate::x86::{self, Entry, Exit, Flags};", ""]
        for f in part:
            L.append(rust_fn(f["addr"], f["name"], f["arms"]))
        L.append("pub const FUNCTIONS: &[(u32, fn(&mut dyn Mem, Entry) -> Exit)] = &[")
        L += [f"    (0x{f['addr']:08X}, f_{f['addr']:08x})," for f in part]
        L += ["];", ""]
        open(os.path.join(out_dir, f"{name}.rs"), "w").write("\n".join(L))
    mod = open(os.path.join(out_dir, "mod.rs")).read().splitlines()
    mod = [l for l in mod if not l.startswith("pub mod lifted_")] + [f"pub mod {m};" for m in mods]
    mod += [] if any("pub fn lifted(" in l for l in mod) else []
    open(os.path.join(out_dir, "mod.rs"), "w").write("\n".join(mod) + "\n")
    lookup = ("//! Generated by research/native/lift.py: do not edit.\nuse crate::mem::Mem;\nuse crate::x86::{Entry, Exit};\n\n"
              "/// Every lifted function: (address, function).\npub fn all() -> Vec<(u32, fn(&mut dyn Mem, Entry) -> Exit)> {\n"
              "    let mut v = Vec::new();\n" + "".join(f"    v.extend_from_slice(super::{m}::FUNCTIONS);\n" for m in mods)
              + "    v\n}\n")
    open(os.path.join(out_dir, "lifted.rs"), "w").write(lookup)
    if "pub mod lifted;" not in open(os.path.join(out_dir, "mod.rs")).read():
        open(os.path.join(out_dir, "mod.rs"), "a").write("pub mod lifted;\n")
    # the test declares every address it verifies, 16 per line
    addrs = [f"0x{f['addr']:08X}" for f in lifted]
    decl = "\n".join("// verifies " + " ".join(addrs[i:i + 16]) for i in range(0, len(addrs), 16))
    test = f'''//! Generated by research/native/lift.py: do not edit.
use ddnative::generated::lifted;
use ddnative::mem::Sparse;
use ddnative::x86::Entry;
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
fn u(v: &Value) -> u32 {{ v.as_u64().unwrap() as u32 }}

{decl}
#[test]
fn every_lifted_function_matches_the_original() {{
    let Some(cases) = vectors("lift") else {{ return }};
    let all = lifted::all();
    let mut seen = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for c in &cases {{
        let addr = u(&c["addr"]);
        let f = all.iter().find(|e| e.0 == addr).expect("lifted function").1;
        let mut m = Sparse::default();
        for run in c["reads"].as_array().unwrap() {{
            let (a, hexs) = (u(&run[0]), run[1].as_str().unwrap());
            for k in 0..hexs.len() / 2 {{
                m.bytes.insert(a + k as u32, u8::from_str_radix(&hexs[2 * k..2 * k + 2], 16).unwrap());
            }}
        }}
        let ri = &c["regs_in"];
        let entry = Entry {{ eax: u(&ri["eax"]), ecx: u(&ri["ecx"]), edx: u(&ri["edx"]), ebx: u(&ri["ebx"]),
            esp: u(&ri["esp"]), ebp: u(&ri["ebp"]), esi: u(&ri["esi"]), edi: u(&ri["edi"]), xmm0: 0 }};
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut m, entry)));
        let ok = match result {{
            Err(_) => false,
            Ok(x) => {{
                let ro = &c["regs_out"];
                let regs = [x.eax, x.ecx, x.edx, x.ebx, x.esp, x.ebp, x.esi, x.edi]
                    == ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"].map(|k| u(&ro[k]));
                let mut want = std::collections::BTreeMap::new();
                for run in c["writes"].as_array().unwrap() {{
                    let (a, hexs) = (u(&run[0]), run[1].as_str().unwrap());
                    for k in 0..hexs.len() / 2 {{
                        want.insert(a + k as u32, u8::from_str_radix(&hexs[2 * k..2 * k + 2], 16).unwrap());
                    }}
                }}
                let st0 = c["st0_out"].as_u64().map(|v| v as u32);
                regs && m.written == want && x.xmm0 == u(&c["xmm0_out"]) && (st0.is_none() || x.st0 == st0)
            }}
        }};
        if !ok {{ failures.push(format!("{{addr:#010x}}")); }}
        seen.insert(addr);
    }}
    failures.sort(); failures.dedup();
    assert!(failures.is_empty(), "{{}} lifted functions differ: {{:?}}", failures.len(), &failures[..failures.len().min(40)]);
    assert_eq!(seen.len(), all.len(), "every lifted function has vectors");
}}
'''
    open(os.path.join(R, "crates/ddnative/tests/lifted.rs"), "w").write(test)
    print(len(lifted), "functions generated in", len(mods), "modules")


if __name__ == "__main__" and sys.argv[1] in ("vectors", "gen"):
    {"vectors": vectors, "gen": gen}[sys.argv[1]]()
