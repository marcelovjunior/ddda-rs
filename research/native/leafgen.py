"""Generate Rust ports of straight-line leaf functions from the exe's disassembly.

A candidate is a function without branches, calls or x87 that uses only a small instruction subset (moves,
lea, simple integer ALU, scalar single-precision SSE). The function is executed symbolically; the generator
emits the loads and stores in program order (so aliasing needs no reasoning), the final value of every
register the function changes, and `ESP` after the return. Nothing is decided by the generator alone: every
generated port is checked against the original on recorded vectors (`record_generated.py`,
`crates/mtcore/tests/semantic_generated.rs`), and a function the symbolic executor cannot model is skipped.

  python3 research/native/leafgen.py LIST.txt OUT.rs OUT_specs.json     (LIST.txt: one hex address per line)
  python3 research/native/leafgen.py --discover N  > list.txt           (candidates of >= N instructions)

Flags are not reproduced (documented in every generated function); the lifted corpus is scanned for callers
that read flags after the call (`--flag-readers`).
Reads the user's own exe at run time; nothing from it is committed (only the generated Rust).
"""
import collections
import glob
import json
import os
import re
import struct
import sys

import capstone
from capstone import x86 as cx

EXE = os.environ.get("DDDA_EXE", "E:/ddda-re/bin/DDDA.exe.unpacked.exe")
ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
REGS = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
SUBSET = {"mov", "movss", "xorps", "pxor", "xor", "or", "and", "add", "sub", "lea", "push", "pop", "ret", "movzx",
          "inc", "dec", "shl", "shr", "sar", "neg", "not", "imul", "nop", "xchg", "mulss", "addss", "subss",
          "movq", "movaps", "movups", "movdqa", "movdqu"}
M32 = 0xFFFFFFFF


class Unsupported(Exception):
    pass


def load_text():
    data = open(EXE, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    nsec = struct.unpack_from("<H", data, pe + 6)[0]
    opt = struct.unpack_from("<H", data, pe + 20)[0]
    base = struct.unpack_from("<I", data, pe + 24 + 28)[0]
    secs = []
    for i in range(nsec):
        name, vsize, va, rsize, rptr = struct.unpack_from("<8sIIII", data, pe + 24 + opt + 40 * i)
        secs.append((base + va, vsize, data[rptr:rptr + rsize]))
    return secs


SECS = None


def code_at(addr, n=16):
    global SECS
    if SECS is None:
        SECS = load_text()
    for va, vs, raw in SECS:
        if va <= addr < va + vs:
            return raw[addr - va:addr - va + n]
    raise Unsupported(f"no code at {addr:#x}")


MD = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
MD.detail = True


def straight_line(addr, limit=400):
    """The instructions up to the first ret, or None if there is a branch, a call or anything outside the subset."""
    out, pc = [], addr
    while len(out) < limit:
        ins = next(MD.disasm(code_at(pc), pc), None)
        if ins is None or ins.mnemonic not in SUBSET:
            return None
        out.append(ins)
        if ins.mnemonic == "ret":
            return out
        pc += ins.size
    return None


# ---- expressions --------------------------------------------------------------------------------------
# ('c', n) constant | ('in', reg) input register | ('arg', k) stack argument k (0-based) | ('t', n) a named temp
# ('bin', op, a, b) | ('un', op, a) | ('f', op, a, b) float op on bit patterns | ('b8', lo, hi_old) byte merge


def const(n):
    return ("c", n & M32)


def is_const(e):
    return e[0] == "c"


class Machine:
    def __init__(self):
        self.regs = {r: ("in", r) for r in REGS}
        self.xmm = {}      # name -> four lane expressions
        self.stmts = []
        self.ntemp = 0
        self.stack = {}           # symbolic stack slots: offset from entry esp -> expr
        self.esp = 0              # offset from entry esp
        self.bases = set()        # expressions used as addresses (for the recorder)
        self.abs_reads, self.abs_writes = set(), set()
        self.max_arg = -1
        self.ret_pop = 0
        self.writes = 0

    # -- temps
    def temp(self, rust, kind="u32"):
        self.ntemp += 1
        name = f"v{self.ntemp}"
        self.stmts.append(f"let {name} = {strip_outer(rust)};")
        return ("t", name)

    # -- rendering
    def show(self, e):
        k = e[0]
        if k == "c":
            return hexs(e[1])
        if k == "in":
            return e[1]
        if k == "arg":
            return f"arg{e[1]}"
        if k == "t":
            return e[1]
        if k == "bin":
            op, a, b = e[1], self.show(e[2]), self.show(e[3])
            return {
                "add": f"{a}.wrapping_add({b})", "sub": f"{a}.wrapping_sub({b})", "xor": f"({a} ^ {b})",
                "or": f"({a} | {b})", "and": f"({a} & {b})", "mul": f"{a}.wrapping_mul({b})",
                "shl": f"{a}.wrapping_shl({b})", "shr": f"({a} >> ({b} & 31))", "sar": f"((({a}) as i32 >> ({b} & 31)) as u32)",
            }[op]
        if k == "un":
            op, a = e[1], self.show(e[2])
            return {"neg": f"{a}.wrapping_neg()", "not": f"!{a}"}[op]
        if k == "f":
            op, a, b = e[1], self.show(e[2]), self.show(e[3])
            helper = {"mul": "mulss", "add": "addss", "sub": "subss"}[op]
            # the asm-exact helper: x86 returns the first NaN operand's payload, which a plain f32 operation need not
            return f"x86::{helper}({a}, {b})"
        if k == "b8":
            return f"(({self.show(e[2])} & 0xFFFF_FF00u32) | ({self.show(e[1])} & 0xFFu32))"
        raise Unsupported(f"show {e}")

    # -- algebra with constant folding
    def bin(self, op, a, b):
        if is_const(a) and is_const(b):
            x, y = a[1], b[1]
            r = {"add": x + y, "sub": x - y, "xor": x ^ y, "or": x | y, "and": x & y, "mul": x * y,
                 "shl": x << (y & 31), "shr": x >> (y & 31),
                 "sar": (((x ^ 0x80000000) - 0x80000000) >> (y & 31))}[op]
            return const(r)
        if op in ("xor", "sub") and a == b:
            return const(0)
        if op in ("add", "or", "xor") and is_const(b) and b[1] == 0:
            return a
        if op == "add" and is_const(a) and a[1] == 0:
            return b
        if op == "and" and ((is_const(b) and b[1] == 0) or (is_const(a) and a[1] == 0)):
            return const(0)
        if op == "or" and ((is_const(b) and b[1] == M32) or (is_const(a) and a[1] == M32)):
            return const(M32)
        if op == "add" and a[0] == "bin" and a[1] == "add" and is_const(a[3]) and is_const(b):
            return self.bin("add", a[2], const(a[3][1] + b[1]))
        if op == "add" and is_const(a) and not is_const(b):
            return self.bin("add", b, a)
        if op == "sub" and is_const(b):
            return self.bin("add", a, const(-b[1]))
        return ("bin", op, a, b)

    def mat(self, e):
        """Name a non-trivial expression once (so a value used twice is computed once)."""
        if e[0] in ("c", "in", "arg", "t"):
            return e
        return self.temp(self.show(e))

    # -- addresses
    def addr(self, mem):
        """Effective address of a capstone memory operand: ('stack', offset) or an expression."""
        base = self.regs[REGS[REGS.index(MD.reg_name(mem.base))]] if mem.base else const(0)
        if mem.base and MD.reg_name(mem.base) == "esp":
            if mem.index:
                raise Unsupported("indexed stack access")
            return ("stack", self.esp + mem.disp)
        e = base
        if mem.index:
            idx = self.regs[MD.reg_name(mem.index)]
            if mem.scale != 1:
                idx = self.bin("mul", idx, const(mem.scale))
            e = self.bin("add", e, idx)
        e = self.bin("add", e, const(mem.disp))
        return e

    def note_base(self, e):
        if e[0] == "c":
            self.abs_reads.add(e[1])
        else:
            self.bases.add(e)

    # -- memory
    def load(self, mem, size):
        a = self.addr(mem)
        if a[0] == "stack":
            off = a[1]
            if off in self.stack:
                v = self.stack[off]
                if size == 4:
                    return v
                raise Unsupported("partial stack read")
            if off >= 4 and (off - 4) % 4 == 0 and size == 4:
                k = (off - 4) // 4
                self.max_arg = max(self.max_arg, k)
                return ("arg", k)
            raise Unsupported(f"stack read at {off}")
        self.note_base(a)
        rd = {4: "u32", 2: "u16", 1: "u8"}[size]
        cast = "" if size == 4 else " as u32"
        return self.temp(f"pg.{rd}(m, {self.addr_rust(a)}){cast}")

    def addr_rust(self, a):
        return self.show(a)

    def store(self, mem, size, value):
        a = self.addr(mem)
        if a[0] == "stack":
            if size != 4:
                raise Unsupported("partial stack write")
            self.stack[a[1]] = value
            if a[1] >= 4:
                # an argument slot (above the return address): the caller can see it, so it is a real store
                self.stmts.append(f"pg.set_u32(m, entry.wrapping_add({a[1]}), {strip_outer(self.show(value))});")
                self.writes += 1
            return
        if a[0] == "c":
            self.abs_writes.add(a[1])
        else:
            self.bases.add(a)
        wr = {4: "set_u32", 2: "set_u16", 1: "set_u8"}[size]
        v = self.show(value)
        if size == 2:
            v = f"{v} as u16" if value[0] != "c" else hexs(value[1] & 0xFFFF)
        if size == 1:
            v = f"{v} as u8" if value[0] != "c" else hexs(value[1] & 0xFF)
        self.stmts.append(f"pg.{wr}(m, {self.addr_rust(a)}, {strip_outer(v)});")
        self.writes += 1

    # -- operands
    def read(self, op, size=4):
        if op.type == cx.X86_OP_IMM:
            return const(op.imm)
        if op.type == cx.X86_OP_REG:
            name = MD.reg_name(op.reg)
            if name in REGS:
                if size == 4:
                    return self.regs[name]
            if name in LOW8 and size == 1:
                return self.bin("and", self.regs[LOW8[name]], const(0xFF))
            raise Unsupported(f"register {name}/{size}")
        if op.type == cx.X86_OP_MEM:
            return self.load(op.mem, size)
        raise Unsupported("operand")

    def write(self, op, value, size=4):
        if op.type == cx.X86_OP_REG:
            name = MD.reg_name(op.reg)
            if name in REGS and size == 4:
                self.regs[name] = value
                return
            if name in LOW8 and size == 1:
                full = LOW8[name]
                old = self.regs[full]
                if is_const(value) and is_const(old):
                    self.regs[full] = const((old[1] & 0xFFFFFF00) | (value[1] & 0xFF))
                else:
                    self.regs[full] = ("b8", value, old)
                return
            raise Unsupported(f"register write {name}/{size}")
        if op.type == cx.X86_OP_MEM:
            self.store(op.mem, size, value)
            return
        raise Unsupported("write operand")

    # -- execution
    def step(self, ins):
        mn, ops = ins.mnemonic, ins.operands
        size = ops[0].size if ops else 4
        if mn in ("add", "sub") and ops[0].type == cx.X86_OP_REG and MD.reg_name(ops[0].reg) == "esp":
            if ops[1].type != cx.X86_OP_IMM:
                raise Unsupported("esp arithmetic with a register")
            self.esp += ops[1].imm if mn == "add" else -ops[1].imm
            return
        if mn not in ("push", "pop", "ret") and any(o.type == cx.X86_OP_REG and MD.reg_name(o.reg) in ("esp", "sp") for o in ops):
            raise Unsupported("esp used as a value")
        if mn == "nop":
            return
        if mn == "ret":
            self.ret_pop = ops[0].imm if ops else 0
            return
        if mn == "push":
            self.esp -= 4
            self.stack[self.esp] = self.read(ops[0])
            return
        if mn == "pop":
            self.write(ops[0], self.stack.get(self.esp) or self._pop_missing())
            self.esp += 4
            return
        if mn == "mov":
            if size == 4 and ops[0].type == cx.X86_OP_REG and ops[1].type == cx.X86_OP_REG:
                self.write(ops[0], self.read(ops[1]))
            else:
                v = self.read(ops[1], ops[1].size if ops[1].type != cx.X86_OP_IMM else size)
                self.write(ops[0], v, size)
            return
        if mn == "movzx":
            self.write(ops[0], self.read(ops[1], ops[1].size), 4)
            return
        if mn == "lea":
            self.write(ops[0], self.addr_value(ops[1].mem))
            return
        if mn == "xchg":
            a, b = self.read(ops[0]), self.read(ops[1])
            self.write(ops[0], b)
            self.write(ops[1], a)
            return
        if mn in ("xor", "or", "and", "add", "sub"):
            if size != 4 and not (ops[0].type == cx.X86_OP_REG and MD.reg_name(ops[0].reg) in LOW8):
                if ops[0].type != cx.X86_OP_MEM or size != 1:
                    raise Unsupported(f"{mn} width {size}")
            a, b = self.read(ops[0], size), self.read(ops[1], size)
            r = self.bin(mn, a, b)
            if not (size == 4) and size != 1:
                raise Unsupported("width")
            self.write(ops[0], self.mat(r) if ops[0].type == cx.X86_OP_REG else r, size)
            return
        if mn in ("inc", "dec"):
            if size != 4:
                raise Unsupported("inc/dec width")
            a = self.read(ops[0])
            self.write(ops[0], self.bin("add" if mn == "inc" else "sub", a, const(1)), 4)
            return
        if mn in ("neg", "not"):
            if size != 4:
                raise Unsupported("neg/not width")
            a = self.read(ops[0])
            self.write(ops[0], const((-a[1]) if mn == "neg" else ~a[1]) if is_const(a) else ("un", mn, a), 4)
            return
        if mn in ("shl", "shr", "sar"):
            if size != 4 or ops[1].type != cx.X86_OP_IMM:
                raise Unsupported("shift form")
            self.write(ops[0], self.bin(mn, self.read(ops[0]), const(ops[1].imm)), 4)
            return
        if mn == "imul":
            if len(ops) == 3:
                a, b = self.read(ops[1]), self.read(ops[2])
            elif len(ops) == 2:
                a, b = self.read(ops[0]), self.read(ops[1])
            else:
                raise Unsupported("one-operand imul")
            self.write(ops[0], self.bin("mul", a, b), 4)
            return
        # SSE: an xmm register is four 32-bit lanes, each an expression (None: not modelled / unknown)
        if mn == "movss":
            d = MD.reg_name(ops[0].reg) if ops[0].type == cx.X86_OP_REG else None
            if d is None:
                self.store(ops[0].mem, 4, self.lane(ops[1], 0))
            elif ops[1].type == cx.X86_OP_MEM:
                self.xmm[d] = [self.read(ops[1]), const(0), const(0), const(0)]
            else:
                lanes = list(self.xmm.get(d, [None] * 4))
                lanes[0] = self.lane(ops[1], 0)
                self.xmm[d] = lanes
            return
        if mn == "movq":
            if ops[0].type == cx.X86_OP_MEM:
                self.store_lanes(ops[0].mem, [self.lane(ops[1], 0), self.lane(ops[1], 1)])
            elif ops[1].type == cx.X86_OP_MEM:
                self.xmm[MD.reg_name(ops[0].reg)] = self.load_lanes(ops[1].mem, 2) + [const(0), const(0)]
            else:
                self.xmm[MD.reg_name(ops[0].reg)] = [self.lane(ops[1], 0), self.lane(ops[1], 1), const(0), const(0)]
            return
        if mn in ("movaps", "movups", "movdqa", "movdqu"):
            if ops[0].type == cx.X86_OP_MEM:
                self.store_lanes(ops[0].mem, [self.lane(ops[1], k) for k in range(4)])
            elif ops[1].type == cx.X86_OP_MEM:
                self.xmm[MD.reg_name(ops[0].reg)] = self.load_lanes(ops[1].mem, 4)
            else:
                self.xmm[MD.reg_name(ops[0].reg)] = [self.lane(ops[1], k) for k in range(4)]
            return
        if mn in ("xorps", "pxor"):
            a, b = MD.reg_name(ops[0].reg), MD.reg_name(ops[1].reg) if ops[1].type == cx.X86_OP_REG else None
            if a != b:
                raise Unsupported("xorps of different registers")
            self.xmm[a] = [const(0)] * 4
            return
        if mn in ("mulss", "addss", "subss"):
            a = self.lane(ops[0], 0)
            b = self.read(ops[1]) if ops[1].type == cx.X86_OP_MEM else self.lane(ops[1], 0)
            d = MD.reg_name(ops[0].reg)
            lanes = list(self.xmm[d])
            lanes[0] = self.mat(("f", mn[:3], a, b))
            self.xmm[d] = lanes
            return
        raise Unsupported(mn)

    def addr_value(self, mem):
        a = self.addr(mem)
        if a[0] == "stack":
            raise Unsupported("lea of a stack slot")
        return a

    def lane(self, op, k):
        name = MD.reg_name(op.reg)
        lanes = self.xmm.get(name)
        if lanes is None or lanes[k] is None:
            raise Unsupported(f"{name} lane {k} read before written")
        return lanes[k]

    def load_lanes(self, mem, n):
        a = self.addr(mem)
        if a[0] == "stack":
            raise Unsupported("vector load from the stack")
        out = []
        for k in range(n):
            out.append(self.load_at(self.bin("add", a, const(4 * k)), 4))
        return out

    def store_lanes(self, mem, values):
        a = self.addr(mem)
        if a[0] == "stack":
            raise Unsupported("vector store to the stack")
        for k, v in enumerate(values):
            self.store_at(self.bin("add", a, const(4 * k)), 4, v)

    def load_at(self, a, size):
        self.note_base(a)
        return self.temp(f"pg.u32(m, {self.show(a)})")

    def store_at(self, a, size, value):
        if a[0] == "c":
            self.abs_writes.add(a[1])
        else:
            self.bases.add(a)
        self.stmts.append(f"pg.set_u32(m, {self.show(a)}, {strip_outer(self.show(value))});")
        self.writes += 1

    def _pop_missing(self):
        raise Unsupported("pop of a value that was not pushed")


LOW8 = {"al": "eax", "cl": "ecx", "dl": "edx", "bl": "ebx"}


def strip_outer(text):
    """Remove one pair of parentheses that encloses the whole expression."""
    if text.startswith("(") and text.endswith(")"):
        depth = 0
        for i, ch in enumerate(text):
            depth += ch == "("
            depth -= ch == ")"
            if depth == 0 and i < len(text) - 1:
                return text
        return text[1:-1]
    return text


def hexs(n):
    n &= M32
    if n < 10:
        return str(n)
    return f"0x{n:X}" if n <= 0xFFFF else f"0x{n >> 16:04X}_{n & 0xFFFF:04X}"


def used_inputs(m):
    """The input registers, and stack arguments, that the final state or an address depends on (for the recorder)."""
    found = set()

    def walk(e):
        if e[0] == "in":
            found.add(e[1])
        elif e[0] == "arg":
            found.add(f"arg{e[1]}")
        elif e[0] in ("bin", "f"):
            walk(e[2]), walk(e[3])
        elif e[0] == "un":
            walk(e[2])
        elif e[0] == "b8":
            walk(e[1]), walk(e[2])
    for e in m.bases:
        walk(e)
    return found


def translate(addr, name, flag_readers=0):
    ins = straight_line(addr)
    if ins is None:
        raise Unsupported("not straight-line / outside the subset")
    m = Machine()
    for i in ins:
        m.step(i)
    if m.esp != 0:
        raise Unsupported(f"stack not balanced ({m.esp})")
    if m.writes == 0 and not any(m.regs[r] != ("in", r) for r in REGS):
        raise Unsupported("no effect")
    length = ins[-1].address + ins[-1].size - addr
    # which inputs are read at all (for naming and the signature comment)
    text = "\n    ".join(m.stmts)
    uses = lambda s: re.search(rf"\b{s}\b", text + " " + " ".join(m.show(m.regs[r]) for r in REGS if m.regs[r] != ("in", r)))
    inputs = [r for r in ("eax", "ecx", "edx", "ebx", "esi", "edi", "ebp") if uses(r)]
    args = [f"arg{k}" for k in range(m.max_arg + 1)]
    return m, inputs, args, length, ins


def prune_dead(stmts, finals):
    """Drop `let vN = ...;` statements nothing refers to (the temp names are unique)."""
    live = list(stmts)
    changed = True
    while changed:
        changed = False
        text = " ".join(live) + " " + " ".join(finals)
        for st in list(live):
            mm = re.match(r"let (v\d+) = ", st)
            if mm and len(re.findall(rf"\b{mm.group(1)}\b", text)) == 1:
                live.remove(st)
                changed = True
    return live


def rust_name(label, addr):
    n = re.sub(r"[^0-9a-zA-Z]+", "_", label or "").strip("_").lower()
    if not n or n.startswith("global_fun") or n.startswith("supplementary_entry"):
        return f"fn_{addr:08x}"
    return f"{n}_{addr:08x}"


def emit(addr, label, m, inputs, args, length):
    fname = rust_name(label, addr)
    method = "::" in (label or "") and "ecx" in inputs and "eax" not in inputs
    ren = {"ecx": "this"} if method else {}
    lines = [f"// Translated from 0x{addr:08x} (DDDA.exe 2017-12-13 Steam), {label or 'unnamed'}.  (generated: research/native/leafgen.py)"]
    abi = []
    if inputs:
        abi.append("register arguments " + ", ".join(i.upper() for i in inputs))
    if args:
        abi.append(f"{len(args)} stack argument{'s' if len(args) != 1 else ''}")
    ret = f"`ret {m.ret_pop:#x}`" if m.ret_pop else "plain `ret`"
    lines.append(f"/// 0x{addr:08X}, `{label or 'unnamed'}` ({'; '.join(abi) or 'no arguments'}; {ret}): straight-line, {m.writes} store{'s' if m.writes != 1 else ''}"
                 f" to memory. Registers as the original leaves them; flags are not reproduced.")
    lines.append(f"pub fn {fname}(m: &mut dyn Mem, c: &mut Cpu) {{")
    lines.append("    let pg = Pages::of(m);")
    lines.append("    let entry = c.esp;")
    for r in inputs:
        lines.append(f"    let {ren.get(r, r)} = c.{r};")
    for k in range(len(args)):
        lines.append(f"    let arg{k} = pg.u32(m, entry.wrapping_add({4 + 4 * k}));")
    body = prune_dead(m.stmts, [m.show(m.regs[r]) for r in REGS if m.regs[r] != ("in", r)])
    for s in body:
        for r, nm in ren.items():
            s = re.sub(rf"\b{r}\b", nm, s)
        lines.append("    " + s)
    for r in REGS:
        if r == "esp":
            continue
        e = m.regs[r]
        if e == ("in", r):
            continue
        v = m.show(e)
        for rr, nm in ren.items():
            v = re.sub(rf"\b{rr}\b", nm, v)
        v = strip_outer(v)
        if v == (ren.get(r, r)):
            continue
        lines.append(f"    c.{r} = {v};")
    lines.append(f"    c.esp = entry.wrapping_add({4 + m.ret_pop});")
    lines.append("}")
    return fname, "\n".join(lines)


def names():
    out = {}
    for f in glob.glob(os.path.join(ROOT, "crates/ddlift/*/src/generated/lifted_*.rs")):
        for mm in re.finditer(r"// Translated from 0x([0-9A-Fa-f]{8}) \(DDDA.exe 2017-12-13 Steam\)(?:, ([^\n]*?)\.)?\n", open(f).read()):
            out[int(mm.group(1), 16)] = mm.group(2)
    return out


def ported():
    t = open(os.path.join(ROOT, "crates/ddports/src/lib.rs")).read().lower().replace("_", "")
    return t


def discover(min_instr):
    funcs = set()
    for f in glob.glob(os.path.join(ROOT, "crates/ddlift/*/src/generated/lifted_*.rs")):
        for mm in re.finditer(r"pub fn f_([0-9a-f]{8})\(", open(f).read()):
            funcs.add(int(mm.group(1), 16))
    p = ported()
    for a in sorted(funcs):
        if f"0x{a:08x}" in p:
            continue
        ins = straight_line(a)
        if ins and len(ins) >= min_instr:
            try:
                translate(a, None)
            except Unsupported:
                continue
            print(f"{a:08x}")


TEST_HEADER = r'''//! Generated ports (crates/mtcore/src/semantic_generated.rs) against the original's recorded behaviour
//! (research/native/record_generated.py; vectors in `GEN_VECTORS_DIR`, default E:/ddda-re/notes/native-generated; skipped when
//! absent unless DDNATIVE_REQUIRE_VECTORS is set). ABI level: all eight registers, and every memory byte at or above the
//! stack scratch below the returned ESP, with no write the original does not make. Flags are not compared. Generated by research/native/leafgen.py.
use ddnative_base::{mem::Mem, x86::Cpu};
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

fn check(file: &str, body: fn(&mut dyn Mem, &mut Cpu)) {
    let dir = std::env::var("GEN_VECTORS_DIR").unwrap_or_else(|_| "E:/ddda-re/notes/native-generated".into());
    let path = format!("{dir}/{file}");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => return,
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() >= 100, "{file}: {} cases", cases.len());
    for (i, case) in cases.iter().enumerate() {
        let mut m = Memory { bytes: pairs(&case["initial"]), writes: BTreeMap::new() };
        let after = regs(&case["after"]);
        let mut c = Cpu::default();
        [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi] = regs(&case["before"]);
        body(&mut m, &mut c);
        let got = [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
        assert_eq!(got, after, "{file} case {i}: eax ecx edx ebx esp ebp esi edi");
        let esp = after[4];
        let expected = pairs(&case["after"]["memory"]);
        for (&a, &b) in &expected {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(m.bytes.get(&a).copied(), Some(b), "{file} case {i}: memory {a:x}");
            }
        }
        for (&a, &b) in &m.writes {
            if !(0x0F00_0000..esp).contains(&a) {
                assert_eq!(expected.get(&a).copied(), Some(b), "{file} case {i}: extra write {a:x}");
            }
        }
    }
}
'''


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "--discover":
        discover(int(sys.argv[2]))
        return
    lst, out_rs, out_spec = sys.argv[1:4]
    out_test = sys.argv[4] if len(sys.argv) > 4 else None
    addrs = [int(x, 16) for x in open(lst).read().split()]
    nm = names()
    chunks, specs, used, skipped = [], [], collections.Counter(), []
    for a in addrs:
        try:
            m, inputs, args, length, ins = translate(a, nm.get(a))
            fname, text = emit(a, nm.get(a), m, inputs, args, length)
        except Unsupported as e:
            skipped.append((a, str(e)))
            continue
        if fname in used:
            fname = f"{fname}_{a:x}"
        used[fname] += 1
        bases = used_inputs(m)
        specs.append(dict(addr=a, name=fname, length=length, label=nm.get(a), inputs=inputs, stack_args=m.max_arg + 1,
                          float_arith=("x86::" in text),
                          ret_pop=m.ret_pop, pointer_inputs=sorted(bases), abs_reads=sorted(m.abs_reads), abs_writes=sorted(m.abs_writes)))
        chunks.append(text)
    header = ("#![allow(unused_parens, unused_mut)]\n"
              "//! Ports of straight-line leaf functions, generated from the disassembly by research/native/leafgen.py and checked\n"
              "//! against the original on recorded vectors (crates/mtcore/tests/semantic_generated.rs). Do not edit: regenerate.\n"
              "use ddnative_base::{\n    mem::{Mem, Pages},\n    x86::{self, Cpu, LiftFn},\n};\n\n")
    table = ("\n\n/// Every port above by address, for the runtime to register (`DDRUNTIME_SEMANTIC_GENERATED=1`).\n"
             "pub const PORTS: &[(u32, LiftFn)] = &[\n"
             + "".join(f"    (0x{sp['addr']:08X}, {sp['name']}),\n" for sp in specs) + "];\n")
    open(out_rs, "w").write(header + "\n\n".join(chunks) + table)
    json.dump(specs, open(out_spec, "w"), indent=1)
    if out_test:
        open(out_test, "w").write(TEST_HEADER + "".join(
            f"\n// verifies 0x{sp['addr']:08X}\n#[test]\nfn {sp['name']}_matches_original() {{\n"
            f"    check(\"gen_{sp['name']}.json\", mtcore::semantic_generated::{sp['name']});\n}}\n" for sp in specs))
    print(f"{len(specs)} functions generated, {len(skipped)} skipped")
    for a, why in skipped[:15]:
        print(f"  skipped {a:08x}: {why}")


if __name__ == "__main__":
    main()
