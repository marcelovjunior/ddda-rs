"""The lifter: translates a function of DDDA.exe into
literal Rust over ddnative::mem::Mem, instruction by instruction, with
exact x86 semantics for the supported subset, and rejects anything else.
A `call` (or outward tail jump) is supported when its target is itself
lifted: lifted functions share the whole machine state (x86::Cpu), and
`scan` repeats rounds until no further function lifts.
Verified by run_sparse vectors (random registers, random memory on
demand, every byte read and written recorded).

    python lift.py scan [N]     # lift every non-platform function it can; patterns/lift.json
    python lift.py vectors      # record sparse vectors for each lifted function
    python lift.py gen          # crates/ddnative/src/generated/lifted*.rs + tests/lifted.rs
    python lift.py relift       # after a codegen change: re-translate the current list
    python lift.py one ADDR     # print the Rust for one function

Run scan/vectors/one with E:/ddda-re/tools/pyenv/Scripts/python.exe;
gen needs only the system Python. docs/NATIVE_PIPELINE.md, "The lifter".
"""
import json
import os
import re
import sqlite3
import struct
import sys

# Private pipeline data (patterns/lift.json, vectors/lift.json, ...).
# LIFT_DATA points a worktree at its own copy so parallel work never
# overwrites the main run's files (the generated test reads the same
# variable at run time).
N = os.environ.get("LIFT_DATA", "E:/ddda-re/notes/native")
DB = "E:/ddda-re/notes/inventory/inventory.sqlite"
R = os.path.join(os.path.dirname(__file__), "..", "..")

GPR32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
SUB8 = {"al": ("eax", 0), "cl": ("ecx", 0), "dl": ("edx", 0), "bl": ("ebx", 0),
        "ah": ("eax", 8), "ch": ("ecx", 8), "dh": ("edx", 8), "bh": ("ebx", 8)}
SUB16 = {"ax": "eax", "cx": "ecx", "dx": "edx", "bx": "ebx", "sp": "esp", "bp": "ebp", "si": "esi", "di": "edi"}
XMM = [f"xmm{i}" for i in range(8)]


class Unsupported(Exception):
    pass


SAVE = ("*cpu = Cpu { eax, ecx, edx, ebx, esp, ebp, esi, edi, "
        "xmm: [xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7], fl, st };")
LOAD = ("Cpu { eax, ecx, edx, ebx, esp, ebp, esi, edi, "
        "xmm: [xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7], fl, st } = std::mem::take(cpu);")
CALLEE = re.compile(r"super::lifted::f_([0-9a-f]{8})\(")


def callees(arms):
    """Lifted functions the arms call or tail-jump to."""
    return sorted({int(x, 16) for a in arms for x in CALLEE.findall(a)})


X87_PUSH = {"fld", "fld1", "fldz", "fild"}
X87_POP = {"fstp"}
X87_READ = {"fst", "fchs", "fabs"}
X87_CONTROL = {"fnstcw", "fldcw", "fnstsw", "fstsw", "fwait", "wait", "fninit", "fnclex", "fnstenv", "fldenv"}
# Net x87 stack effect of each closed lifted function at its `ret` (filled by
# lift(); a caller adds it at a direct call). None: inconsistent between rets.
X87_NET = {}
# Per function, the return addresses of mocked call sites whose callee is
# taken to return a float in st0 (filled by lift(); stored in lift.json as
# "x87_push" for the recorder and the replay).
X87_PUSH_SITES = {}


def returns_float(ins, k):
    """Does the code after the call at ins[k] consume st0 before pushing
    anything? Then the callee returned a float there (MSVC returns float and
    double in st0). Looks at the rest of the call's basic block only."""
    for i in ins[k + 1:]:
        mn = i.mnemonic
        if mn == "call" or mn.startswith("j") or mn in ("ret", "retn"):
            return False
        if not mn.startswith("f") or mn in X87_CONTROL:
            continue
        return mn not in X87_PUSH
    return False


def x87_depth_check(ins, addr, end, callable=frozenset(), ext=False):
    """Every x87 read or pop must find a value on the stack: walk the
    control flow from depth 0. A direct call to a closed lifted function adds
    its net effect (X87_NET); a mocked call adds 1 where its result is
    consumed (returns_float: the recorder's mock and the replay push a
    value there) and 0 elsewhere. Anything else read at depth 0 rejects.
    Returns (set of depths at the function's rets, push sites)."""
    from capstone.x86 import X86_OP_IMM
    at = {i.address: k for k, i in enumerate(ins)}
    seen, todo = {}, [(0, 0)]
    exits, push_sites = set(), set()
    # blocks only reachable through a switch table are walked afterwards
    # from depth 0 (conservative: a switch inside x87 code is rare)
    pending = list(range(len(ins)))
    while todo or pending:
        if not todo:
            k = pending.pop(0)
            if ins[k].address not in seen:
                todo.append((k, 0))
            continue
        k, depth = todo.pop()
        while k < len(ins):
            i = ins[k]
            if seen.get(i.address, 99) <= depth:
                break  # already walked with this depth or less
            seen[i.address] = depth
            mn = i.mnemonic
            if mn in X87_POP or mn in X87_READ:
                if depth == 0:
                    raise Unsupported(f"x87-return (st0 read at {i.address:#x} with nothing pushed)")
                depth -= mn in X87_POP
            elif mn in X87_PUSH:
                depth += 1
            elif mn == "call" and k + 1 < len(ins):
                direct = i.operands[0].type == X86_OP_IMM
                t = i.operands[0].imm if direct else None
                if direct and t in callable:
                    net = X87_NET.get(t, 0)
                    if net is None:
                        raise Unsupported(f"x87-callee (inconsistent x87 effect of {t:#x})")
                    depth += net
                elif ext and returns_float(ins, k):
                    depth += 1
                    push_sites.add(ins[k + 1].address)
            if mn in ("ret", "retn"):
                exits.add(depth)
                break
            if mn.startswith("j"):
                t = i.operands[0].imm if i.operands and i.operands[0].type == 2 else None
                if t is not None and addr <= t < end and t in at:
                    todo.append((at[t], depth))
                if mn == "jmp":
                    break
            k += 1
    return exits, sorted(push_sites)


def switch_targets(ins, k, addr, end, read):
    """Recognise an absolute x86 dword switch table, never a computed tail call.

    The image supplies discovery only; generated code still reads through Mem.
    A nearby cmp/ja (or cmp/jae) bounds either the dword table or an optional
    byte remapping table. Other shapes retain the conservative table-only check.
    """
    from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM
    jump = ins[k]
    op = jump.operands[0]
    if (jump.mnemonic != "jmp" or op.type != X86_OP_MEM or op.size != 4
            or op.mem.segment or op.mem.base or not op.mem.index
            or op.mem.scale != 4):
        raise Unsupported("indirect jump (not an absolute switch table)")
    table = op.mem.disp & 0xFFFFFFFF
    starts = {i.address for i in ins}
    targets = []
    cursor = table
    while cursor <= 0xFFFFFFFC:
        try:
            data = read(cursor, 4)
        except KeyError:
            break
        if len(data) != 4:
            break
        target, = struct.unpack("<I", data)
        if not addr <= target < end:
            break
        if target not in starts:
            raise Unsupported("indirect jump (switch target is not an instruction start)")
        targets.append(target)
        cursor += 4
    if not targets:
        raise Unsupported("indirect jump (empty or unmapped switch table)")

    # MSVC commonly compresses a sparse switch with a byte lookup immediately
    # before the jump. The cmp bounds that lookup, not the dword table.
    index = op.mem.index
    remap = None
    prev = k - 1
    if prev >= 0 and ins[prev].mnemonic == "movzx":
        dst, src = ins[prev].operands
        if dst.type == X86_OP_REG and dst.reg == index:
            if (dst.size != 4 or src.type != X86_OP_MEM or src.size != 1
                    or src.mem.segment or src.mem.index or src.mem.base != index):
                raise Unsupported("indirect jump (unrecognised switch remapping)")
            remap = src.mem.disp & 0xFFFFFFFF
            prev -= 1
    if prev >= 1 and ins[prev].mnemonic in ("ja", "jae"):
        cmp = ins[prev - 1]
        if (cmp.mnemonic == "cmp" and len(cmp.operands) == 2
                and cmp.operands[0].type == X86_OP_REG
                and cmp.operands[0].reg == index and cmp.operands[0].size == 4
                and cmp.operands[1].type == X86_OP_IMM):
            count = (cmp.operands[1].imm & 0xFFFFFFFF) + (ins[prev].mnemonic == "ja")
            if remap is None:
                if count != len(targets):
                    raise Unsupported("indirect jump (switch bound disagrees with table)")
            else:
                try:
                    indices = read(remap, count)
                except KeyError:
                    indices = b""
                if not count or len(indices) != count or max(indices) >= len(targets):
                    raise Unsupported("indirect jump (switch remapping exceeds table)")
    return targets


def lift(addr, size, callable=frozenset(), ext=False):
    """Rust match arms for the function, or raise Unsupported. `callable`
    holds the lifted functions a `call` or an outward (tail) jump may go to.
    With `ext`, any other call or outward jump (direct, indirect, imports)
    goes through x86::call_ext, which the vectors replay."""
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
                leaders.update(switch_targets(ins, k, addr, end, h.code))
            else:
                t = i.operands[0].imm
                if not (addr <= t < end):
                    if t not in callable and t not in h.PLATFORM and not ext:
                        raise Unsupported("jump out of the function")
                else:
                    leaders.add(t)
            if k + 1 < len(ins):
                leaders.add(ins[k + 1].address)
        if i.mnemonic in ("ret", "retn") and k + 1 < len(ins):
            leaders.add(ins[k + 1].address)
    if not leaders <= {i.address for i in ins}:
        raise Unsupported("jump into the middle of an instruction")
    exits, push_sites = x87_depth_check(ins, addr, end, callable, ext)
    X87_PUSH_SITES[addr] = push_sites
    if not ext:  # callers add this at a direct call
        X87_NET[addr] = exits.pop() if len(exits) == 1 else (0 if not exits else None)

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

    def target(t):
        """Rust path of a call target: a platform model, a lifted function,
        or (ext) the untranslated-code handler."""
        if t in h.PLATFORM:
            return f"crate::platform::{h.PLATFORM[t]}"
        if t in callable:
            return f"super::lifted::f_{t:08x}"
        return f"(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_ext(m, cpu, 0x{t:X}))"

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
                body.append(f"{{ let (a, c) = ({a}, {c}); let r = (a as i32).wrapping_mul(c as i32) as u32; "
                            f"fl = Flags::new(x86::IMUL, a, c, r, 32); {store(i, ops[0], 'r')} }}")
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
                body.append(f"esp = esp.wrapping_add(4 + {extra}); {SAVE} return;")
                terminated = True
                break
            elif mn == "jmp" and ops[0].type == X86_OP_MEM:
                body.append(f"pc = {load(i, ops[0])}; continue;")
                terminated = True
                break
            elif mn == "jmp" and not (addr <= ops[0].imm < end):
                # tail call: the callee returns straight to our caller
                body.append(f"{SAVE} {target(ops[0].imm)}(m, cpu); return;")
                terminated = True
                break
            elif mn == "jmp":
                body.append(f"pc = 0x{ops[0].imm:X}; continue;")
                terminated = True
                break
            elif mn.startswith("j") and mn != "jmp" and nxt is None:
                raise Unsupported("falls off the end (conditional jump last)")
            elif mn.startswith("j") and not (addr <= ops[0].imm < end):
                body.append(f"if fl.cond(\"{mn[1:]}\") {{ {SAVE} {target(ops[0].imm)}(m, cpu); return; }} "
                            f"pc = 0x{nxt:X}; continue;")
                terminated = True
                break
            elif mn.startswith("j") and nxt is None:
                raise Unsupported("falls off the end (conditional jump last)")
            elif mn.startswith("j"):
                body.append(f"pc = if fl.cond(\"{mn[1:]}\") {{ 0x{ops[0].imm:X} }} else {{ 0x{nxt:X} }}; continue;")
                terminated = True
                break
            elif mn == "call" and nxt is None:
                raise Unsupported("noreturn call (last instruction)")
            elif mn == "call" and (ops[0].type != X86_OP_IMM or
                                   (ops[0].imm not in callable and ops[0].imm not in h.PLATFORM)):
                if not ext:
                    raise Unsupported("icall" if ops[0].type != X86_OP_IMM else "call")
                if k + 1 < len(ins) and ins[k + 1].mnemonic.startswith("f") and                         ins[k + 1].mnemonic not in ("fld", "fldz", "fld1"):
                    raise Unsupported("x87-return")  # the mock returns nothing on the x87 stack
                # the operand is evaluated before the return address is pushed
                body.append(f"{{ let t = {load(i, ops[0])}; esp = esp.wrapping_sub(4); m.set_u32(esp, 0x{nxt:X}); "
                            f"{SAVE} x86::call_ext(m, cpu, t); {LOAD} }}")
            elif mn == "call":
                t = ops[0].imm
                # push the return address, run the callee on the whole state, carry on
                body.append(f"esp = esp.wrapping_sub(4); m.set_u32(esp, 0x{nxt:X}); "
                            f"{SAVE} {target(t)}(m, cpu); {LOAD}")
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


ALU_BLOCK = re.compile(
    r"\{ let a = ([^;]+) & (u32::MAX|0xFF|0xFFFF);\s+"
    r"let c = ([^;]+) & \2;\s+let r = ([^;]+);\s+"
    r"fl = Flags::new\(x86::(ADD|SUB|LOGIC), a, c, r, (8|16|32)\);")


def compact_alu(arm):
    """Fold only exact binary-ALU templates; keep evaluation/store order.

    Unknown templates stay literal. In particular this is not flag
    liveness analysis: every flag write, including unused ones, remains.
    """
    def replace(m):
        a, mask, b, result, kind, bits = m.groups()
        if mask != {"8": "0xFF", "16": "0xFFFF", "32": "u32::MAX"}[bits]:
            return m[0]
        operations = {
            (f"(a.wrapping_add(c) & {mask})", "ADD"): "add",
            (f"(a.wrapping_sub(c) & {mask})", "SUB"): "sub",
            (f"((a & c) & {mask})", "LOGIC"): "and",
            (f"((a | c) & {mask})", "LOGIC"): "or",
            (f"((a ^ c) & {mask})", "LOGIC"): "xor",
        }
        op = operations.get((result, kind))
        if op is None:
            return m[0]
        return f"{{ let r = fl.alu_{op}({a}, {b}, {bits});"
    return ALU_BLOCK.sub(replace, arm)


def cpu_arm(arm):
    """Lower cached local-register IR onto the shared Cpu in place.

    SAVE/LOAD delimit calls in patterns/lift.json; removing them is valid
    because every preceding assignment already updated the same Cpu. Keep
    the cache representation (and thus scan/callee discovery) unchanged.
    Substitute whole identifiers, leaving Rust strings untouched.
    """
    arm = compact_alu(arm).replace(SAVE, "").replace(LOAD, "")
    fields = {r: f"cpu.{r}" for r in GPR32 + ["fl", "st"]}
    fields.update({r: f"cpu.xmm[{i}]" for i, r in enumerate(XMM)})
    return re.sub(r'"(?:\\.|[^"\\])*"|\b(?:' + "|".join(fields) + r')\b',
                  lambda m: fields.get(m[0], m[0]), arm)


def straight_line(addr, arms):
    """A sole entry arm that returns without using dispatcher control flow."""
    if len(arms) != 1:
        return None
    match = re.fullmatch(r"\s*0x([0-9A-Fa-f]+) => \{ (.*) \}", arms[0], re.S)
    if match is None or int(match[1], 16) != addr:
        return None
    body = match[2].strip()
    if not body.endswith("return;") or re.search(r"\b(?:pc|continue|break)\b", body):
        return None
    return body


def rust_fn(addr, name, arms):
    header = [
        f"// Translated from 0x{addr:08X} (DDDA.exe 2017-12-13 Steam), {name}.",
        f"pub fn f_{addr:08x}(m: &mut dyn Mem, cpu: &mut Cpu) {{",
    ]
    traced = os.environ.get("LIFT_TRACE", "").lower() in (f"0x{addr:x}", f"{addr:x}")
    body = None if traced else straight_line(addr, arms)  # a trace needs the block loop
    if body is not None:
        return "\n".join([*header, "    " + cpu_arm(body), "}", ""])
    return "\n".join([*header,
        f"    let mut pc: u32 = 0x{addr:X};",
        "    loop {",
        *([] if not traced else [
            '        eprintln!("trace {pc:#x} eax={:#x} ecx={:#x} edx={:#x} ebx={:#x} esp={:#x} ebp={:#x} esi={:#x} edi={:#x}", '
            'cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi);']),
        "        match pc {",
        *(cpu_arm(a) for a in arms),
        "            _ => unreachable!(),",
        "        }",
        "    }",
        "}", ""])


def scan(limit=None):
    db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    from collections import Counter
    ok, why = [], Counter()
    rows = db.execute("SELECT address, namespace, name, size FROM functions WHERE NOT thunk "
                      "AND status NOT IN ('verified', 'platform') AND size >= 1 "
                      "AND address < 0x012E0000 ORDER BY address").fetchall()
    # verified functions are lifted too (hand ports keep their own tests): a
    # caller can only lift if its callees are lifted
    rows += db.execute("SELECT address, namespace, name, size FROM functions WHERE status='verified' "
                       "AND NOT thunk AND size >= 1 AND address < 0x012E0000").fetchall()
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    # a platform model's own address is never lifted: run_sparse runs the
    # model there, so its vectors would describe the model, not the code
    rows = sorted(r for r in rows if r[0] not in h.PLATFORM)
    rows = rows[:limit]
    # rounds: a function whose calls all go to functions lifted in an earlier
    # round (or earlier in this one) lifts too; repeat until nothing changes
    done, rnd = set(), 0
    while True:
        rnd += 1
        why, before = Counter(), len(ok)
        for a, ns, nm, sz in rows:
            if a in done:
                continue
            try:
                arms = lift(a, sz, done)
                ok.append({"addr": a, "name": f"{ns}::{nm}", "size": sz, "arms": arms, "calls": callees(arms), "x87_push": X87_PUSH_SITES.get(a, [])})
                done.add(a)
            except Unsupported as e:
                why[str(e).split(" ")[0]] += 1
            except Exception as e:  # noqa: BLE001
                why["error:" + type(e).__name__] += 1
        print(f"round {rnd}: +{len(ok) - before}", flush=True)
        if len(ok) == before:
            break
    # last pass: the rest may call untranslated code through x86::call_ext
    # (vectors mock those calls); such functions are not callable themselves
    why, before = Counter(), len(ok)
    for a, ns, nm, sz in rows:
        if a in done:
            continue
        try:
            arms = lift(a, sz, done, ext=True)
            ok.append({"addr": a, "name": f"{ns}::{nm}", "size": sz, "arms": arms, "calls": callees(arms), "x87_push": X87_PUSH_SITES.get(a, []),
                       "ext": True})
        except Unsupported as e:
            why[str(e).split(" ")[0]] += 1
        except Exception as e:  # noqa: BLE001
            why["error:" + type(e).__name__] += 1
    print(f"ext pass: +{len(ok) - before}", flush=True)
    json.dump(ok, open(f"{N}/patterns/lift.json", "w"))
    print(len(ok), "lifted of", len(rows), "; top rejections:", why.most_common(14))


def relift():
    """Re-translate the current list in place after a codegen change (same
    functions, same callees; their vectors stay valid)."""
    lifted = json.load(open(f"{N}/patterns/lift.json"))
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    closed = {f["addr"] for f in lifted if not f.get("ext")}
    # closed functions' x87 effects first: a caller adds its callee's
    for _ in range(2):
        for f in lifted:
            if f["addr"] in closed and f["addr"] not in h.PLATFORM:
                try:
                    lift(f["addr"], f["size"], closed)
                except Unsupported:
                    pass
    keep, dropped = [], []
    for f in lifted:
        try:
            if f["addr"] in h.PLATFORM:
                raise Unsupported("platform model address")
            f["arms"] = lift(f["addr"], f["size"], closed, ext=bool(f.get("ext")))
            f["x87_push"] = X87_PUSH_SITES.get(f["addr"], [])
            assert callees(f["arms"]) == sorted(f.get("calls", [])), hex(f["addr"])
            keep.append(f)
        except Unsupported as e:  # a stricter lifter: the function no longer qualifies
            dropped.append((f["addr"], str(e)))
    # callers of a dropped function cannot compile: drop them too
    have = {f["addr"] for f in keep}
    while True:
        bad = [f for f in keep if not set(f.get("calls", [])) <= have]
        if not bad:
            break
        dropped += [(f["addr"], "calls a dropped function") for f in bad]
        keep = [f for f in keep if f not in bad]
        have = {f["addr"] for f in keep}
    json.dump(keep, open(f"{N}/patterns/lift.json", "w"))
    print(len(keep), "functions re-lifted;", len(dropped), "dropped:",
          [(hex(a), w[:40]) for a, w in dropped[:12]])


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "relift":
        relift()
    if cmd == "scan":
        scan(int(sys.argv[2]) if len(sys.argv) > 2 else None)
    elif cmd == "one":
        import sqlite3 as _s
        a = int(sys.argv[2], 16)
        sz, ns, nm = _s.connect(DB).execute("select size, namespace, name from functions where address=?", (a,)).fetchone()
        print(rust_fn(a, f"{ns}::{nm}", lift(a, sz)))


_CLOSED = frozenset()


def _init_worker(closed):
    global _CLOSED
    _CLOSED = frozenset(closed)


def _vectors_for(args):
    addr, per, end = args[:3]
    size = args[3] if len(args) > 3 else None
    push = frozenset(args[4]) if len(args) > 4 else frozenset()
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    inputs = {"zero_inputs": True} if os.environ.get("LIFT_ZERO_INPUTS") == "1" else {}
    out, got, seed = [], 0, 0
    while got < per and seed < per * 8:
        seed += 1
        try:
            if end is None:
                r = h.run_sparse(addr, (addr << 8) ^ seed, size=size, **inputs)
            else:  # ext: calls to untranslated code are mocked
                r = h.run_sparse(addr, (addr << 8) ^ seed, end=end, lifted=_CLOSED, x87_push=push, **inputs)
        except h.Rejected:
            continue
        except Exception:  # noqa: BLE001  (emulator errors: no vector)
            continue
        out.append({"addr": addr, **r})
        got += 1
    return addr, out


def vectors(per=3):
    """Record vectors for every lifted function that has none yet, then drop
    functions without one and, transitively, their callers. Existing cases
    remain valid only if their call-mocking boundary has not changed.
    LIFT_WORKERS limits the worker processes (default: every core).
    LIFT_ZERO_INPUTS=1 requests zero boundary fixtures for missing functions;
    it does not replace existing vectors or retry cached no-vector results.
    """
    from multiprocessing import Pool
    lifted = json.load(open(f"{N}/patterns/lift.json"))
    path = f"{N}/vectors/lift.json"
    old = json.load(open(path)) if os.path.exists(path) else []
    want = {f["addr"] for f in lifted}
    # a cached case is stale when its call boundary changed: a function that
    # is now closed must make no mocked calls, and an ext function's mocked
    # target must not have become lifted since
    closed_now = {f["addr"] for f in lifted if not f.get("ext")}
    def fresh(c):
        calls = c.get("calls") or []
        if c["addr"] in closed_now:
            return not calls
        return not any(e["target"] in closed_now for e in calls)
    stale = {c["addr"] for c in old if c["addr"] in want and not fresh(c)}
    cases = [c for c in old if c["addr"] in want and c["addr"] not in stale]
    if stale:
        print(len(stale), "functions have stale vectors (call boundary changed): re-recording", flush=True)
    # an interrupted run left its results in the progress file: keep them
    prog = f"{N}/vectors/lift_progress.jsonl"
    failed = []
    if os.path.exists(prog):
        for line in open(prog):
            try:
                r = json.loads(line)
            except ValueError:
                continue  # a line cut off by the interruption
            cases += [c for c in r["cases"] if c["addr"] in want]
            if not r["cases"]:
                failed.append(r["addr"])
    have = {c["addr"] for c in cases}
    # functions that already failed to get a vector are not retried (each costs 8x the seeds)
    nov_path = f"{N}/patterns/lift_novector.json"
    novector = (set(json.load(open(nov_path))) if os.path.exists(nov_path) else set()) | set(failed)
    todo = [(f["addr"], per, f["addr"] + f["size"] if f.get("ext") else None, f["size"], f.get("x87_push", []))
            for f in lifted if f["addr"] not in have and f["addr"] not in novector]
    closed = [f["addr"] for f in lifted if not f.get("ext")]
    print(len(have), "functions reuse their vectors;", len(todo), "to record", flush=True)
    log = open(prog, "a")
    with Pool(int(os.environ.get("LIFT_WORKERS", os.cpu_count())), _init_worker, (closed,)) as pool:
        for n, (addr, out) in enumerate(pool.imap_unordered(_vectors_for, todo, chunksize=8)):
            cases += out
            if not out:
                failed.append(addr)
            log.write(json.dumps({"addr": addr, "cases": out}) + "\n")
            log.flush()
            if n % 1000 == 999:
                print(n + 1, "recorded", flush=True)
    json.dump(sorted(novector | set(failed)), open(nov_path, "w"))
    keep = {c["addr"] for c in cases}
    while True:  # a caller of a dropped function cannot be compiled
        drop = {f["addr"] for f in lifted if f["addr"] in keep and not set(f.get("calls", [])) <= keep}
        if not drop:
            break
        keep -= drop
    cases = [c for c in cases if c["addr"] in keep]
    json.dump([f for f in lifted if f["addr"] in keep], open(f"{N}/patterns/lift.json", "w"))
    json.dump(cases, open(f"{N}/vectors/lift.json", "w"))
    log.close()
    os.remove(prog)  # everything is in lift.json now
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
             "//! Functions lifted instruction by instruction (docs/NATIVE_PIPELINE.md).",
             "#![allow(warnings, clippy::all)] // literal x86 translation: redundant parens, self-moves",
             "use crate::mem::Mem;", "use crate::x86::{self, Cpu, Flags};", ""]
        for f in part:
            L.append(rust_fn(f["addr"], f["name"], f["arms"]))
        L.append(f"pub const FUNCTIONS_{k // chunk:03}: &[(u32, fn(&mut dyn Mem, &mut Cpu))] = &[")
        L += [f"    (0x{f['addr']:08X}, f_{f['addr']:08x})," for f in part]
        L += ["];", ""]
        open(os.path.join(out_dir, f"{name}.rs"), "w").write("\n".join(L))
    mod = open(os.path.join(out_dir, "mod.rs")).read().splitlines()
    mod = [l for l in mod if not l.startswith("pub mod lifted_")] + [f"pub mod {m};" for m in mods]
    mod += [] if any("pub fn lifted(" in l for l in mod) else []
    open(os.path.join(out_dir, "mod.rs"), "w").write("\n".join(mod) + "\n")
    # every function is reachable as super::lifted::f_xxxxxxxx (lifted calls use that path)
    lookup = ("//! Generated by research/native/lift.py: do not edit.\nuse crate::mem::Mem;\nuse crate::x86::Cpu;\n\n"
              + "".join(f"pub use super::{m}::*;\n" for m in mods)
              + "\n/// Every lifted function: (address, function).\npub fn all() -> Vec<(u32, fn(&mut dyn Mem, &mut Cpu))> {\n"
              "    let mut v = Vec::new();\n" + "".join(f"    v.extend_from_slice(super::{m}::FUNCTIONS_{m[-3:]});\n" for m in mods)
              + "    v\n}\n"
              + "\n/// Lifted functions that call untranslated code (ext): a call through\n"
              "/// x86::call_ext never runs them directly (the vectors mocked them).\n"
              "pub const EXT: &[u32] = &[" + ", ".join(f"0x{f['addr']:08X}" for f in lifted if f.get("ext")) + "];\n"
              + "\n/// What x86::call_ext may run itself: every lifted function that is not\n"
              "/// ext, and the platform models (same rule as harness.run_sparse).\n"
              "pub fn dispatch_table() -> std::collections::HashMap<u32, fn(&mut dyn Mem, &mut Cpu)> {\n"
              "    let ext: std::collections::HashSet<u32> = EXT.iter().copied().collect();\n"
              "    let mut t: std::collections::HashMap<u32, fn(&mut dyn Mem, &mut Cpu)> =\n"
              "        all().into_iter().filter(|(a, _)| !ext.contains(a)).collect();\n"
              "    for &(a, _, f) in crate::platform::MODELS { t.insert(a, f); }\n"
              "    t\n}\n")
    open(os.path.join(out_dir, "lifted.rs"), "w").write(lookup)
    if "pub mod lifted;" not in open(os.path.join(out_dir, "mod.rs")).read():
        open(os.path.join(out_dir, "mod.rs"), "a").write("pub mod lifted;\n")
    # the test declares every address it verifies, 16 per line
    addrs = [f"0x{f['addr']:08X}" for f in lifted]
    decl = "\n".join("// verifies " + " ".join(addrs[i:i + 16]) for i in range(0, len(addrs), 16))
    test = f'''//! Generated by research/native/lift.py: do not edit.
use ddnative::generated::lifted;
use ddnative::mem::Sparse;
use ddnative::mem::Mem;
use ddnative::x86::{{self, Cpu, Entry}};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

fn vectors(name: &str) -> Option<Vec<Value>> {{
    let dir = std::env::var("LIFT_DATA").unwrap_or_else(|_| "E:/ddda-re/notes/native".into());
    let path = format!("{{dir}}/vectors/{{name}}.json");
    let text = match std::fs::read_to_string(&path) {{
        Ok(t) => t,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{{path}}: {{e}}"),
        Err(_) => return None,
    }};
    serde_json::from_str::<Value>(&text).ok()?.as_array().cloned()
}}
fn u(v: &Value) -> u32 {{ v.as_u64().unwrap() as u32 }}

type Dispatch = std::collections::HashMap<u32, fn(&mut dyn Mem, &mut Cpu)>;

/// Replays the calls to untranslated code a vector recorded: checks the
/// target and every register at the call, then returns what the mock did.
fn replay(calls: Rc<RefCell<VecDeque<Value>>>, table: Rc<Dispatch>) -> Box<x86::ExtCall> {{
    Box::new(move |m: &mut dyn Mem, cpu: &mut Cpu, target: u32| {{
        // lifted (not ext) functions and platform models run, exactly as the
        // recorder ran them; anything else was mocked and is replayed
        if let Some(f) = table.get(&target) {{
            return f(m, cpu);
        }}
        let ev = calls.borrow_mut().pop_front().unwrap_or_else(|| panic!("unrecorded call to {{target:#x}}"));
        assert_eq!(target, u(&ev["target"]), "call target");
        let r = &ev["regs"];
        assert_eq!([cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi],
                   ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"].map(|k| u(&r[k])), "registers at the call");
        let ret = m.u32(cpu.esp);
        let _ = ret;
        let o = &ev["out"];
        cpu.eax = u(&o["eax"]);
        cpu.ecx = u(&o["ecx"]);
        cpu.edx = u(&o["edx"]);
        for k in 0..4 {{ cpu.xmm[0][k] = u(&o["xmm0"][k]); }}
        cpu.esp = cpu.esp.wrapping_add(4 + u(&o["pop"]));
        if let Some(v) = o.get("st_push").and_then(|v| v.as_u64()) {{
            cpu.st.push(v as u32); // the mocked callee returned this float in st0
        }}
    }})
}}

{decl}
#[test]
fn every_lifted_function_matches_the_original() {{
    let Some(cases) = vectors("lift") else {{ return }};
    let all = lifted::all();
    let table = Rc::new(lifted::dispatch_table());
    let mut seen = std::collections::HashSet::new();
    let mut failures = Vec::new();
    // LIFT_DEBUG=0xADDR prints what differs for that function's vectors
    let debug = std::env::var("LIFT_DEBUG").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());
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
        let calls = Rc::new(RefCell::new(c.get("calls").and_then(|v| v.as_array()).cloned()
            .unwrap_or_default().into_iter().collect::<VecDeque<Value>>()));
        x86::set_ext_calls(Some(replay(calls.clone(), table.clone())));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{
            let mut cpu = Cpu::from(entry);
            f(&mut m, &mut cpu);
            cpu.exit()
        }}));
        let all_calls_made = calls.borrow().is_empty();
        let ok = match result {{
            Err(e) => {{
                if debug == Some(addr) {{
                    let msg = e.downcast_ref::<String>().cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    eprintln!("{{addr:#x}}: panicked: {{msg}}");
                }}
                false
            }}
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
                if debug == Some(addr) {{
                    let ro = &c["regs_out"];
                    eprintln!("{{addr:#x}}: calls made {{all_calls_made}}; got {{x:x?}}");
                    eprintln!("  want regs {{:x?}} xmm0 {{:#x}} st0 {{st0:x?}}",
                        ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"].map(|k| u(&ro[k])), u(&c["xmm0_out"]));
                    for (a, v) in &want {{ if m.written.get(a) != Some(v) {{ eprintln!("  [{{a:#x}}] want {{v:#04x}} got {{:x?}}", m.written.get(a)); }} }}
                    for (a, v) in &m.written {{ if !want.contains_key(a) {{ eprintln!("  [{{a:#x}}] extra write {{v:#04x}}"); }} }}
                }}
                all_calls_made && regs && m.written == want && x.xmm0 == u(&c["xmm0_out"]) && (st0.is_none() || x.st0 == st0)
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
