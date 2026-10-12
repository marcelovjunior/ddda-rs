"""The lifter: translates a function of DDDA.exe into
literal Rust over ddnative::mem::Mem, instruction by instruction, with
exact x86 semantics for the supported subset, and rejects anything else.
A `call` (or outward tail jump) is supported when its target is itself
lifted: lifted functions share the whole machine state (x86::Cpu), and
`scan` repeats rounds until no further function lifts.
Verified by run_sparse vectors (random registers, random memory on
demand, every byte read and written recorded).

    python lift.py scan [N]     # lift every function it can; original model bodies use ext entry contracts
    python lift.py scan-extra   # append supplementary entries, preserving existing boundaries
    python lift.py vectors      # record sparse vectors for each lifted function
    python lift.py gen          # crates/ddlift/lift_*/src/generated + ddnative facade/test
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
from functools import lru_cache
from contextlib import closing

# Private pipeline data (patterns/lift.json, vectors/lift.json, ...).
# LIFT_DATA points a worktree at its own copy so parallel work never
# overwrites the main run's files (the generated test reads the same
# variable at run time).
N = os.environ.get("LIFT_DATA", "E:/ddda-re/notes/native")
DB = os.environ.get("INVENTORY_DB", "E:/ddda-re/notes/inventory/inventory.sqlite")
R = os.path.join(os.path.dirname(__file__), "..", "..")

GPR32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
SUB8 = {"al": ("eax", 0), "cl": ("ecx", 0), "dl": ("edx", 0), "bl": ("ebx", 0),
        "ah": ("eax", 8), "ch": ("ecx", 8), "dh": ("edx", 8), "bh": ("ebx", 8)}
SUB16 = {"ax": "eax", "cx": "ecx", "dx": "edx", "bx": "ebx", "sp": "esp", "bp": "ebp", "si": "esi", "di": "edi"}
XMM = [f"xmm{i}" for i in range(8)]


class Unsupported(Exception):
    pass


SAVE = ("*cpu = Cpu { eax, ecx, edx, ebx, esp, ebp, esi, edi, "
        "xmm: [xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7], fl, st, df: cpu.df };")
LOAD = ("Cpu { eax, ecx, edx, ebx, esp, ebp, esi, edi, "
        "xmm: [xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7], fl, st, df: _ } = std::mem::take(cpu);")
CALLEE = re.compile(r"super::lifted::f_([0-9a-f]{8})\(")


def callees(arms):
    """Lifted functions the arms call or tail-jump to."""
    return sorted({int(x, 16) for a in arms for x in CALLEE.findall(a)})


X87_PUSH = {"fld", "fld1", "fldz", "fild", "fldl2e", "fldln2"}
X87_POP = {"fstp", "fistp", "faddp", "fsubp", "fsubrp", "fmulp", "fdivp", "fdivrp", "fcomp", "fucomp", "fcompi", "fucompi"}
X87_READ = {"fst", "fist", "fchs", "fabs", "fadd", "fsub", "fsubr", "fmul", "fdiv", "fdivr",
            "fiadd", "fisub", "fisubr", "fimul", "fidiv", "fidivr", "fxch", "fcom", "fucom", "ficom",
            "fsqrt", "fsin", "fcos", "fcomi", "fucomi"}
X87_SUPPORTED = X87_PUSH | X87_POP | X87_READ | {"fcompp", "fucompp", "ficomp", "fnstcw", "fldcw", "fnstsw", "fstsw"}
X87_CONTROL = {"fnstcw", "fldcw", "fnstsw", "fstsw", "fwait", "wait", "fninit", "fnclex", "fnstenv", "fldenv"}
# Net x87 stack effect of each closed lifted function at its `ret` (filled by
# lift(); a caller adds it at a direct call). None: inconsistent between rets.
X87_NET = {}
# Per function, the return addresses of mocked call sites whose callee is
# taken to return a float in st0 (filled by lift(); stored in lift.json as
# "x87_push" for the recorder and the replay).
X87_PUSH_SITES = {}
BODY_INFO = {}
_BODY_CACHE = {}


def cached_body(addr, size, ext=False):
    """Only the callable set changes between scan rounds, not decoded bytes."""
    import harness as h
    key = (addr, size, h.code, h.image, h.executable, function_entries(), noreturn_entries(), ext)
    if key not in _BODY_CACHE:
        try:
            _BODY_CACHE[key] = decode_body(addr, size, ext=ext)
        except Unsupported as e:
            _BODY_CACHE[key] = str(e)
    result = _BODY_CACHE[key]
    if isinstance(result, str):
        raise Unsupported(result)
    return result


def release_body(addr, size):
    import harness as h
    for ext in (False, True):
        _BODY_CACHE.pop((addr, size, h.code, h.image, h.executable, function_entries(), noreturn_entries(), ext), None)


@lru_cache(maxsize=1)
def function_entries():
    with closing(sqlite3.connect(f"file:{DB}?mode=ro", uri=True)) as db:
        return frozenset(a for a, in db.execute("SELECT address FROM functions"))


@lru_cache(maxsize=8)
def ordered_entries(entries):
    return sorted(entries)


@lru_cache(maxsize=1)
def noreturn_entries():
    # These inventory symbols name CRT routines with a non-returning contract.
    # Addresses come from the player's inventory, not a copied address table.
    names = {"_abort", "_exit", "__exit", "__amsg_exit"}
    with closing(sqlite3.connect(f"file:{DB}?mode=ro", uri=True)) as db:
        return frozenset(a for a, name in db.execute("SELECT address, name FROM functions") if name in names)


def decode_body(addr, size, entries=None, noreturn=None, read=None, is_code=None, ext=False):
    """Deterministic reachable instructions. Inventory sizes are not spans.

    Direct branches to another inventory entry are tail calls. Shared tails
    reached at interior addresses are decoded in both bodies. Unbounded
    switch discovery stays within the entry's inventory band (or an already
    discovered shared island); explicit cmp bounds can extend beyond it.
    In ext mode, non-switch indirect jumps retain all discovered local
    starts as possible destinations; other runtime targets are external.
    """
    import bisect
    import heapq
    import harness as h
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    entries = function_entries() if entries is None else entries
    noreturn = noreturn_entries() if noreturn is None else noreturn
    read = h.code if read is None else read
    is_code = h.executable if is_code is None else is_code
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    ordered = ordered_entries(frozenset(entries))
    band = bisect.bisect_right(ordered, addr)
    upper = ordered[band] if band < len(ordered) else 0x100000000
    found, owners, switches, stops = {}, {}, {}, set()
    todo = [addr]
    while todo:
        pc = heapq.heappop(todo)
        history = []
        while pc not in found:
            if pc in owners:
                raise Unsupported("disassembly branch into instruction interior")
            if len(found) >= 20000:
                raise Unsupported("body instruction limit")
            if not is_code(pc):
                raise Unsupported("disassembly outside executable sections")
            try:
                i = next(md.disasm(read(pc, 15), pc, count=1))
            except (KeyError, StopIteration):
                raise Unsupported("disassembly undecodable reachable instruction") from None
            if not is_code(pc, i.size) or any(a in owners for a in range(pc, pc + i.size)):
                raise Unsupported("disassembly overlapping instruction")
            found[pc] = i
            owners.update((a, pc) for a in range(pc, pc + i.size))
            history.append(i)
            mn = i.mnemonic
            if mn in ("int3", "ud2"):
                stops.add(pc)
                break
            if mn in ("ret", "retn", "repz ret", "hlt"):
                break
            if mn == "call" and i.operands[0].type == 2 and i.operands[0].imm in noreturn:
                stops.add(pc)
                break
            if mn.startswith("j"):
                if i.operands[0].type == 2:
                    target = i.operands[0].imm & 0xffffffff
                    if target == addr or target not in entries:
                        heapq.heappush(todo, target)
                else:
                    accept = lambda t: ((addr <= t < upper or t in found) and
                                        (t == addr or t not in entries) and is_code(t))
                    try:
                        targets = switch_targets(history, len(history) - 1, addr, upper, read,
                                                 accept=accept, discover=True,
                                                 bounded_accept=lambda t: (t == addr or t not in entries) and is_code(t))
                    except Unsupported as error:
                        if (not ext or mn != "jmp" or i.operands[0].size != 4
                                or str(error) != "indirect jump (not an absolute switch table)"):
                            raise
                        # No speculative decode through a runtime pointer.
                        # Known instruction starts remain local destinations;
                        # every other target crosses the external boundary.
                        switches[pc] = None
                        break
                    switches[pc] = targets
                    for target in targets:
                        heapq.heappush(todo, target)
                if mn == "jmp":
                    break
            pc += i.size
    ins = sorted(found.values(), key=lambda i: i.address)
    for pc, targets in switches.items():
        if targets is None:
            switches[pc] = sorted(found)
    ranges = []
    for i in ins:
        if ranges and ranges[-1][1] == i.address:
            ranges[-1][1] += i.size
        else:
            ranges.append([i.address, i.address + i.size])
    return ins, switches, stops, ranges


def x87_effect(i):
    """Required stack depth and net change for supported x87 instructions."""
    mn = i.mnemonic
    regs = [int(i.reg_name(o.reg)[3:-1]) for o in i.operands
            if o.type == 1 and i.reg_name(o.reg).startswith("st(")]
    needed = max([r + 1 for r in regs] + [0 if mn in X87_PUSH else 1])
    if mn in ("fcompp", "fucompp"):
        needed = 2
    if mn in ("fcom", "fcomp", "fucom", "fucomp", "fxch") and not i.operands:
        needed = 2
    delta = (mn in X87_PUSH) - (mn in X87_POP or mn == "ficomp")
    if mn in ("fcompp", "fucompp"):
        delta -= 2
    return needed, delta


def returns_float(ins, k, depth=0):
    """Prove a one-value deficit downstream of this mocked call.

    Track supported x87 work and direct branches, stopping at another call,
    an unknown jump, trap or return. Never supply entry-stack arguments or
    guess across intervening callees. One mock can return only one value.
    """
    at = {i.address: i for i in ins}
    todo = [(ins[k].address + ins[k].size, depth, 0)]
    seen, required = set(), 0
    while todo:
        pc, current, added = todo.pop()
        if pc not in at or (pc, current, added) in seen:
            continue
        seen.add((pc, current, added))
        i = at[pc]
        mn = i.mnemonic
        if mn in ("call", "ret", "retn", "repz ret", "int3", "ud2", "hlt"):
            continue
        if mn in X87_SUPPORTED and mn not in X87_CONTROL:
            needed, delta = x87_effect(i)
            missing = max(0, needed - current)
            added += missing
            if added > 1:
                return False
            required = max(required, added)
            current += missing + delta
            if current > 8:
                return False
        elif mn.startswith('f') and mn not in X87_CONTROL:
            return False
        if mn.startswith('j'):
            if i.operands[0].type != 2:
                continue
            todo.append((i.operands[0].imm & 0xffffffff, current, added))
            if mn == 'jmp':
                continue
        todo.append((pc + i.size, current, added))
    return required == 1


def x87_depth_check(ins, addr, end, callable=frozenset(), ext=False, switches=None, stops=frozenset()):
    """Every x87 read or pop must find a value on the stack: walk the
    control flow from depth 0. A direct call to a closed lifted function adds
    its net effect (X87_NET); a mocked call adds 1 where its result is
    consumed (returns_float: the recorder's mock and the replay push a
    value there) and 0 elsewhere. Anything else read at depth 0 rejects.
    Returns (set of depths at the function's rets, push sites)."""
    from capstone.x86 import X86_OP_IMM
    at = {i.address: k for k, i in enumerate(ins)}
    seen, todo = {}, [(at[addr], 0)]
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
            if mn in X87_SUPPORTED and mn not in X87_CONTROL:
                needed, delta = x87_effect(i)
                if depth < needed:
                    raise Unsupported(f"x87-return (needs {needed} values at {i.address:#x}, has {depth})")
                depth += delta
                if depth > 8:
                    raise Unsupported("x87-overflow")
            elif mn.startswith('f') and mn not in X87_CONTROL:
                raise Unsupported(f"x87-unsupported {mn}")
            elif mn == "call" and k + 1 < len(ins):
                direct = i.operands[0].type == X86_OP_IMM
                t = i.operands[0].imm if direct else None
                if direct and t in callable:
                    net = X87_NET.get(t, 0)
                    if net is None:
                        raise Unsupported(f"x87-callee (inconsistent x87 effect of {t:#x})")
                    depth += net
                elif ext and returns_float(ins, k, depth):
                    depth += 1
                    push_sites.add(ins[k + 1].address)
            if i.address in stops:
                break
            if mn in ("ret", "retn", "repz ret"):
                exits.add(depth)
                break
            if mn.startswith("j"):
                t = i.operands[0].imm if i.operands and i.operands[0].type == 2 else None
                if t in at and (t == addr or t not in function_entries()):
                    todo.append((at[t], depth))
                for target in (switches or {}).get(i.address, []):
                    todo.append((at[target], depth))
                if mn == "jmp":
                    break
            k = at.get(i.address + i.size, len(ins))
    return exits, sorted(push_sites)


def switch_targets(ins, k, addr, end, read, accept=None, discover=False, bounded_accept=None):
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
    # A proven adjacent bound determines the slots to discover even when
    # the case code is outside the entry's inventory band.
    bound, remap_addr = None, None
    bound_index = op.mem.index
    prev_bound = k - 1
    if prev_bound >= 0 and ins[prev_bound].mnemonic == "movzx":
        dst, src = ins[prev_bound].operands
        if (dst.type == X86_OP_REG and dst.reg == op.mem.index and dst.size == 4
                and src.type == X86_OP_MEM and src.size == 1 and not src.mem.segment
                and not src.mem.index and src.mem.base
                and ins[prev_bound].reg_name(src.mem.base) in GPR32):
            remap_addr = src.mem.disp & 0xffffffff
            bound_index = src.mem.base
            prev_bound -= 1
    if prev_bound >= 1 and ins[prev_bound].mnemonic in ("ja", "jae"):
        cmp = ins[prev_bound - 1]
        if (cmp.mnemonic == "cmp" and len(cmp.operands) == 2
                and cmp.operands[0].type == X86_OP_REG and cmp.operands[0].reg == bound_index
                and cmp.operands[0].size == 4 and cmp.operands[1].type == X86_OP_IMM):
            bound = (cmp.operands[1].imm & 0xffffffff) + (ins[prev_bound].mnemonic == "ja")
            if not 0 < bound <= 4096:
                raise Unsupported("indirect jump (switch bound limit)")
            if remap_addr is not None:
                try:
                    remapped = read(remap_addr, bound)
                except KeyError:
                    remapped = b""
                if len(remapped) != bound:
                    raise Unsupported("indirect jump (unmapped switch remapping)")
                bound = max(remapped) + 1
    if remap_addr is not None and bound_index != op.mem.index and bound is None:
        raise Unsupported("indirect jump (remapping source lacks proven bound)")
    starts = {i.address for i in ins}
    targets = []
    cursor = table
    while cursor <= 0xFFFFFFFC:
        if discover and bound is not None and len(targets) == bound:
            break
        if len(targets) >= 4096:
            raise Unsupported("indirect jump (switch table limit)")
        try:
            data = read(cursor, 4)
        except KeyError:
            break
        if len(data) != 4:
            break
        target, = struct.unpack("<I", data)
        valid = (bounded_accept(target) if discover and bound is not None and bounded_accept
                 else accept(target) if accept else addr <= target < end)
        if not valid:
            break
        if not discover and target not in starts:
            raise Unsupported("indirect jump (switch target is not an instruction start)")
        targets.append(target)
        cursor += 4
    if not targets:
        raise Unsupported("indirect jump (empty or unmapped switch table)")
    if discover and bound is not None and len(targets) != bound:
        raise Unsupported("indirect jump (switch bound disagrees with table)")

    # MSVC commonly compresses a sparse switch with a byte lookup immediately
    # before the jump. The cmp bounds that lookup, not the dword table.
    index = op.mem.index
    remap = None
    prev = k - 1
    if prev >= 0 and ins[prev].mnemonic == "movzx":
        dst, src = ins[prev].operands
        if dst.type == X86_OP_REG and dst.reg == index:
            if (dst.size != 4 or src.type != X86_OP_MEM or src.size != 1
                    or src.mem.segment or src.mem.index or src.mem.base != bound_index):
                raise Unsupported("indirect jump (unrecognised switch remapping)")
            remap = src.mem.disp & 0xFFFFFFFF
            prev -= 1
    if prev >= 1 and ins[prev].mnemonic in ("ja", "jae"):
        cmp = ins[prev - 1]
        if (cmp.mnemonic == "cmp" and len(cmp.operands) == 2
                and cmp.operands[0].type == X86_OP_REG
                and cmp.operands[0].reg == bound_index and cmp.operands[0].size == 4
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
    ins, switches, stops, ranges = cached_body(addr, size, ext)
    starts = {i.address for i in ins}
    entries = function_entries()
    inside = lambda t: t in starts and (t == addr or t not in entries)
    leaders = {addr}
    for k, i in enumerate(ins):
        if k and ins[k-1].address + ins[k-1].size != i.address:
            leaders.add(i.address)
        if i.mnemonic.startswith("j"):
            if i.operands[0].type != X86_OP_IMM:
                leaders.update(switches[i.address])
            else:
                t = i.operands[0].imm
                if not inside(t):
                    if t not in callable and t not in h.PLATFORM and not ext:
                        raise Unsupported("jump out of the function")
                else:
                    leaders.add(t)
            if i.address + i.size in starts:
                leaders.add(i.address + i.size)
        if (i.mnemonic in ("ret", "retn", "repz ret") or i.address in stops) and k + 1 < len(ins):
            leaders.add(ins[k + 1].address)
    exits, push_sites = x87_depth_check(ins, addr, None, callable, ext, switches, stops)
    X87_PUSH_SITES[addr] = push_sites
    BODY_INFO[addr] = {"body": sorted(starts), "body_ranges": ranges,
                       "noreturn_sites": sorted(stops)}
    if not ext:
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
        segment = i.reg_name(m.segment) if m.segment else None
        if segment not in (None, 'fs', 'ds', 'es', 'ss'):
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
        if segment == 'fs' and i.mnemonic != 'lea':
            e = f"cpu.fs_base.wrapping_add({e})"
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

    def vector(i, op, width=None):
        if op.type == X86_OP_REG:
            return xmm(i, op)
        if op.type == X86_OP_IMM:
            return f"[{op.imm & 0xffffffff}u32, 0, 0, 0]"
        width = op.size if width is None else width
        if width not in (4, 8, 16):
            raise Unsupported("SSE memory width")
        a = mem_addr(i, op)
        values = [f"m.u32(a.wrapping_add({k * 4}))" if k * 4 < width else "0" for k in range(4)]
        return f"{{ let a = {a}; [{', '.join(values)}] }}"

    def target(t):
        """Rust path of a call target: a platform model, a lifted function,
        or (ext) the untranslated-code handler."""
        if t in h.PLATFORM:
            path = f"crate::platform::{h.PLATFORM[t]}"
        elif t in callable:
            path = f"super::lifted::f_{t:08x}"
        else:
            path = f"(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_ext(m, cpu, 0x{t:X}))"
        if t in noreturn_entries():
            return f'(|m: &mut dyn Mem, cpu: &mut Cpu| {{ {path}(m, cpu); panic!("noreturn callee returned"); }})'
        return path

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
            nxt = i.address + i.size if i.address + i.size in starts else None
            if i.address != b and i.address in leaders:
                body.append(f"pc = 0x{i.address:X}; continue;")
                terminated = True
                break
            bits = ops[0].size * 8 if ops else 32
            mask = "u32::MAX" if bits == 32 else f"0x{(1 << bits) - 1:X}"
            if mn in ("sbb", "adc"):
                body.append(f"{{ let (a, b) = ({load(i, ops[0])}, {load(i, ops[1])}); "
                            f"let r = fl.alu_carry(a, b, {bits}, {str(mn == 'sbb').lower()}); {store(i, ops[0], 'r')} }}")
            elif mn in ("mul", "imul") and len(ops) == 1:
                if bits not in (8, 16, 32):
                    raise Unsupported("multiply width")
                body.append(f"{{ let b = {load(i, ops[0])}; let (lo, hi) = fl.multiply(eax, b, {bits}, {str(mn == 'imul').lower()});")
                body.append(set_reg("ax", "lo | (hi << 8)") if bits == 8 else
                            set_reg("ax" if bits == 16 else "eax", "lo") + set_reg("dx" if bits == 16 else "edx", "hi"))
                body.append("}")
            elif mn in ("div", "idiv"):
                if bits not in (8, 16, 32):
                    raise Unsupported("divide width")
                body.append(f"{{ let d = {load(i, ops[0])}; let (q, r) = x86::divide(eax, {'eax >> 8' if bits == 8 else 'edx'}, d, {bits}, {str(mn == 'idiv').lower()}).expect(\"x86 divide error\");")
                body.append(set_reg("ax", "q | (r << 8)") if bits == 8 else
                            set_reg("ax" if bits == 16 else "eax", "q") + set_reg("dx" if bits == 16 else "edx", "r"))
                body.append("fl = Flags::partial(0, 0); }")
            elif mn in ("rol", "ror", "shld", "shrd"):
                right = str(mn in ("ror", "shrd")).lower()
                expression = (f"fl.rotate(a, n, {bits}, {right})" if len(ops) == 2 else
                              f"fl.double_shift(a, b, n, {bits}, {right})")
                body.append(f"{{ let a = {load(i, ops[0])}; let n = {load(i, ops[-1])}; "
                            + (f"let b = {load(i, ops[1])}; " if len(ops) == 3 else "")
                            + f"let r = {expression}; {store(i, ops[0], 'r')} }}")
            elif mn in ("bsf", "bsr"):
                body.append(f"{{ let v = {load(i, ops[1])}; let r = fl.bit_scan(v, {bits}, {str(mn == 'bsr').lower()}); {store(i, ops[0], 'r')} }}")
            elif mn in ("bt", "bts", "btr", "btc"):
                if bits not in (16, 32):
                    raise Unsupported("bit-test width")
                change = {"bt":0,"bts":1,"btr":2,"btc":3}[mn]
                body.append(f"{{ let index = {load(i, ops[1])};")
                if ops[0].type == X86_OP_MEM:
                    signed_index = "index as i16 as i32" if bits == 16 else "index as i32"
                    adjustment = (f".wrapping_add((({signed_index} >> {4 if bits == 16 else 5}).wrapping_mul({bits//8})) as u32)"
                                  if ops[1].type == X86_OP_REG else "")
                    # Register indices address a signed bit string; immediates
                    # use only the low log2(width) bits within the operand.
                    body.append(f"let a = {mem_addr(i, ops[0])}{adjustment}; let v = m.u{bits}(a) as u32; let r = fl.bit_test(v, index, {bits}, {change});")
                    if change:
                        body.append(f"m.set_u{bits}(a, r as u{bits});")
                else:
                    body.append(f"let v = {load(i, ops[0])}; let r = fl.bit_test(v, index, {bits}, {change});")
                    if change:
                        body.append(store(i, ops[0], "r"))
                body.append("}")
            elif mn == "xchg":
                body.append(f"{{ let (a, b) = ({load(i, ops[0])}, {load(i, ops[1])});")
                if ops[1].type == X86_OP_MEM:
                    body.append(store(i, ops[1], "a") + store(i, ops[0], "b"))
                else:
                    body.append(store(i, ops[0], "b") + store(i, ops[1], "a"))
                body.append("}")
            elif mn.startswith("cmov"):
                # CMOV reads its memory source even if the condition is false.
                body.append(f"{{ let v = {load(i, ops[1])}; if fl.cond(\"{mn[4:]}\") {{ {store(i, ops[0], 'v')} }} }}")
            elif mn in ("cwde", "cbw", "cwd"):
                body.append({"cwde": set_reg("eax", "eax as i16 as i32 as u32"),
                             "cbw": set_reg("ax", "eax as i8 as i32 as u32"),
                             "cwd": set_reg("dx", "if eax & 0x8000 != 0 { 0xffff } else { 0 }")}[mn])
            elif mn in ("cld", "std"):
                body.append(f"cpu.df = {str(mn == 'std').lower()};")
            elif (mn.split()[-1] in ("movsb", "movsw", "movsd", "stosb", "stosw", "stosd")
                  and not any(o.type == X86_OP_REG and i.reg_name(o.reg) in XMM for o in ops)):
                if i.addr_size != 4 or any(o.type == X86_OP_MEM and o.mem.segment and i.reg_name(o.mem.segment) not in ("es", "ds", "ss", "fs") for o in ops):
                    raise Unsupported("string address/segment override")
                op = mn.split()[-1]
                if mn not in (op, "rep " + op):
                    raise Unsupported("string repeat prefix")
                fs = any(o.type == X86_OP_MEM and i.reg_name(o.mem.segment) == 'fs' for o in ops)
                if fs:
                    body.append('{ let source_base = cpu.fs_base;')
                helper, segment = ('string_from', ', source_base') if fs else ('string', '')
                body.append(f"x86::{helper}(m, cpu, {dict(b=1,w=2,d=4)[op[-1]]}, {str(op.startswith('movs')).lower()}, {str(mn.startswith('rep ')).lower()}{segment});")
                if fs:
                    body.append('}')
            elif mn.startswith("prefetch"):
                pass  # an architectural hint, not a demand memory read
            elif mn == "sfence":
                body.append("x86::sfence();")
            elif mn == "mov":
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
                if bits == 32:
                    body.append(f"{{ let (a, c) = ({a}, {c}); let r = (a as i32).wrapping_mul(c as i32) as u32; "
                                f"fl = Flags::new(x86::IMUL, a, c, r, 32); {store(i, ops[0], 'r')} }}")
                elif bits == 16:
                    body.append(f"{{ let (a, c) = ({a}, {c}); let (r, _) = fl.multiply(a, c, 16, true); {store(i, ops[0], 'r')} }}")
                else:
                    raise Unsupported("imul width")
            elif mn == "cdq":
                body.append("edx = if (eax as i32) < 0 { u32::MAX } else { 0 };")
            elif mn.startswith("set"):
                body.append(store(i, ops[0], f"fl.cond(\"{mn[3:]}\") as u32"))
            elif mn == "push":
                body.append(f"{{ let v = {load(i, ops[0])}; esp = esp.wrapping_sub(4); m.set_u32(esp, v); }}")
            elif mn == "pop":
                body.append(f"{{ let v = m.u32(esp); esp = esp.wrapping_add(4); {store(i, ops[0], 'v')} }}")
            elif mn == "leave":
                if 0x66 in i.prefix or 0x67 in i.prefix:
                    raise Unsupported("leave operand/address size")
                body.append("esp = ebp; ebp = m.u32(esp); esp = esp.wrapping_add(4);")
            elif mn in ("int3", "ud2"):
                body.append(f'panic!("guest {mn} trap at 0x{i.address:08X}");')
                terminated = True
                break
            elif mn in ("cpuid", "rdtsc", "pushfd"):
                raise Unsupported(f"platform-dependent {mn} (no event/state contract)")
            elif mn in ("ret", "retn", "repz ret"):
                extra = ops[0].imm if ops else 0
                body.append(f"esp = esp.wrapping_add(4 + {extra}); {SAVE} return;")
                terminated = True
                break
            elif mn == "jmp" and ops[0].type != X86_OP_IMM:
                if ext:
                    # Targets outside known local destinations leave the body
                    # (switch overflow or a non-switch function pointer).
                    # The recorder and replay share this tail-call boundary.
                    cases = " | ".join(f"0x{t:X}" for t in sorted(set(switches[i.address])))
                    body.append(f"{{ let t = {load(i, ops[0])}; if matches!(t, {cases}) {{ pc = t; continue; }} "
                                f"{SAVE} x86::call_ext(m, cpu, t); return; }}")
                else:
                    body.append(f"pc = {load(i, ops[0])}; continue;")
                terminated = True
                break
            elif mn == "jmp" and not inside(ops[0].imm):
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
            elif mn.startswith("j") and not inside(ops[0].imm):
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
            elif mn == "call" and i.address in stops:
                if not ext and ops[0].imm not in callable:
                    raise Unsupported("noreturn call")
                body.append(f"esp = esp.wrapping_sub(4); m.set_u32(esp, 0x{i.address + i.size:X}); "
                            f"{SAVE} {target(ops[0].imm)}(m, cpu); panic!(\"noreturn callee returned\");")
                terminated = True
                break
            elif mn == "call" and nxt is None:
                raise Unsupported("noreturn call (last instruction)")
            elif mn == "call" and (ops[0].type != X86_OP_IMM or
                                   (ops[0].imm not in callable and ops[0].imm not in h.PLATFORM)):
                if not ext:
                    raise Unsupported("icall" if ops[0].type != X86_OP_IMM else "call")
                # x87_depth_check and x87_push already describe mocked float
                # returns, including an immediate consuming instruction.
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
            elif mn == "pextrw" and len(ops) == 3 and ops[2].type == X86_OP_IMM:
                word = ops[2].imm & 7
                value = f"(({xmm(i, ops[1])}[{word // 2}] >> {(word % 2) * 16}) & 0xFFFF)"
                body.append(store(i, ops[0], value))
            elif mn == "pinsrw" and len(ops) == 3 and ops[2].type == X86_OP_IMM:
                word = ops[2].imm & 7
                lane, shift = word // 2, (word % 2) * 16
                dst = xmm(i, ops[0])
                body.append(f"{{ let v = {load(i, ops[1])} & 0xFFFF; "
                            f"{dst}[{lane}] = ({dst}[{lane}] & !(0xFFFFu32 << {shift})) | (v << {shift}); }}")
            elif mn == "movd":
                if ops[0].type == X86_OP_REG and i.reg_name(ops[0].reg) in XMM:
                    body.append(f"{xmm(i, ops[0])} = [{load(i, ops[1])}, 0, 0, 0];")
                else:
                    body.append(store(i, ops[0], f"{xmm(i, ops[1])}[0]"))
            elif mn == "movsd":
                if ops[0].type == X86_OP_REG:
                    d = xmm(i, ops[0])
                    body.append(f"{{ let s = {vector(i, ops[1])}; {d} = [s[0], s[1], "
                                + (f"{d}[2], {d}[3]" if ops[1].type == X86_OP_REG else "0, 0") + "]; }")
                else:
                    d, s = mem_addr(i, ops[0]), xmm(i, ops[1])
                    body.append(f"{{ let a = {d}; m.set_u32(a, {s}[0]); m.set_u32(a.wrapping_add(4), {s}[1]); }}")
            elif mn in ("movhlps", "movlhps"):
                d, s = xmm(i, ops[0]), xmm(i, ops[1])
                body.append(f"{d} = [{s}[2], {s}[3], {d}[2], {d}[3]];" if mn == "movhlps" else
                            f"{d} = [{d}[0], {d}[1], {s}[0], {s}[1]];")
            elif mn in ("movlps", "movhps", "movlpd", "movhpd"):
                lane = 2 if mn.startswith("movh") else 0
                if ops[0].type == X86_OP_REG:
                    d = xmm(i, ops[0])
                    body.append(f"{{ let s = {vector(i, ops[1])}; {d}[{lane}] = s[0]; {d}[{lane+1}] = s[1]; }}")
                else:
                    s = xmm(i, ops[1])
                    body.append(f"{{ let a = {mem_addr(i, ops[0])}; m.set_u32(a, {s}[{lane}]); m.set_u32(a.wrapping_add(4), {s}[{lane+1}]); }}")
            elif mn in ("cvtss2si", "cvtsd2si", "cvttsd2si"):
                body.append(store(i, ops[0], f"x86::{mn}({vector(i, ops[1])})"))
            elif mn == "cvtsi2sd":
                d = xmm(i, ops[0])
                body.append(f"{d} = x86::cvtsi2sd({d}, {load(i, ops[1])});")
            elif mn in ("movmskps", "movmskpd", "pmovmskb"):
                extract, count = {"movmskps":("(s[k] >> 31)",4), "movmskpd":("(s[2*k+1] >> 31)",2),
                                  "pmovmskb":("((s[k/4] >> (8*(k%4)+7)) & 1)",16)}[mn]
                body.append(f"{{ let s = {vector(i, ops[1])}; let mut r = 0u32; for k in 0..{count} {{ r |= {extract} << k; }} {store(i, ops[0], 'r')} }}")
            elif mn in ("comisd", "ucomisd"):
                # Capstone 5 labels this memory operand m128, but COMISD
                # and UCOMISD consume only a scalar m64 (Intel SDM).
                body.append(f"fl = x86::comisd({vector(i, ops[0])}, {vector(i, ops[1], 8)});")
            elif mn in ("divps", "minps", "maxps", "sqrtps", "addpd", "subpd", "mulpd", "divpd", "minpd", "maxpd", "sqrtpd",
                        "addsd", "subsd", "mulsd", "divsd", "minsd", "maxsd", "sqrtsd", "cvtps2pd", "cvtpd2ps", "cvtss2sd", "cvtsd2ss",
                        "cvtdq2ps", "cvttps2dq", "cvtps2dq", "cvtdq2pd", "cvttpd2dq", "cvtpd2dq", "unpcklps", "unpckhps", "unpcklpd", "unpckhpd",
                        "paddb", "paddw", "paddd", "paddq", "psubb", "psubw", "psubd", "psubq", "pcmpeqb", "pcmpeqw", "pcmpeqd", "pcmpgtb", "pcmpgtw", "pcmpgtd",
                        "pslld", "psllw", "psllq", "psrld", "psrlw", "psrlq", "psrad", "psraw", "pmullw", "pmulhw", "pmulhuw", "pmuludq", "pmaddwd",
                        "punpcklbw", "punpcklwd", "punpckldq", "punpcklqdq", "punpckhbw", "punpckhwd", "punpckhdq", "punpckhqdq",
                        "packsswb", "packssdw", "packuswb", "paddusb", "paddusw", "paddsb", "paddsw", "psubusb", "psubusw", "psubsb", "psubsw"):
                d = xmm(i, ops[0])
                body.append(f"{d} = x86::{mn}({d}, {vector(i, ops[1])});")
            elif mn in ("pshufd", "pshuflw", "pshufhw", "shufpd"):
                helper = dict(pshufd="shuffle_d", pshuflw="shuffle_lw", pshufhw="shuffle_hw", shufpd="shuffle_pd")[mn]
                d = xmm(i, ops[0])
                body.append(f"{d} = x86::{helper}::<{ops[2].imm & 255}>({d}, {vector(i, ops[1])});")
            elif mn.startswith("cmp") and mn[-2:] in ("ps", "pd", "ss", "sd"):
                predicate = mn[3:-2]
                predicates = {"eq":0, "lt":1, "le":2, "unord":3, "neq":4, "nlt":5, "nle":6, "ord":7}
                imm = ops[2].imm if len(ops) == 3 else predicates.get(predicate)
                if imm is None or not 0 <= imm < 8:
                    raise Unsupported("SSE comparison predicate")
                d = xmm(i, ops[0])
                body.append(f"{d} = x86::compare_{mn[-2:]}::<{imm}>({d}, {vector(i, ops[1])});")
            # ---- SSE
            elif mn == "movss":
                if ops[0].type == X86_OP_REG and ops[1].type == X86_OP_MEM:
                    body.append(f"{xmm(i, ops[0])} = [m.u32({mem_addr(i, ops[1])}), 0, 0, 0];")
                elif ops[0].type == X86_OP_MEM:
                    body.append(f"m.set_u32({mem_addr(i, ops[0])}, {xmm(i, ops[1])}[0]);")
                else:
                    body.append(f"{xmm(i, ops[0])}[0] = {xmm(i, ops[1])}[0];")
            elif mn in ("movaps", "movups", "movdqa", "movdqu", "movapd", "movupd", "movntps", "movntpd", "movntdq"):
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
            elif mn in ("xorps", "pxor", "andps", "orps", "andnps", "xorpd", "andpd", "orpd", "andnpd", "pand", "por", "pandn"):
                d = xmm(i, ops[0])
                src = xmm(i, ops[1]) if ops[1].type == X86_OP_REG else None
                if src is None:
                    a = mem_addr(i, ops[1])
                    body.append(f"{{ let a = {a}; let s = [m.u32(a), m.u32(a.wrapping_add(4)), m.u32(a.wrapping_add(8)), m.u32(a.wrapping_add(12))];")
                    src = "s"
                else:
                    body.append("{")
                f = ("d ^ s" if mn in ("xorps", "xorpd", "pxor") else "d & s" if mn in ("andps", "andpd", "pand") else
                     "d | s" if mn in ("orps", "orpd", "por") else "!d & s")
                body.append(f"for k in 0..4 {{ let (d, s) = ({d}[k], {src}[k]); {d}[k] = {f}; }} }}")
            elif mn in ("addss", "subss", "mulss", "divss", "minss", "maxss", "sqrtss"):
                d = xmm(i, ops[0])
                s = f"{xmm(i, ops[1])}[0]" if ops[1].type == X86_OP_REG else f"m.u32({mem_addr(i, ops[1])})"
                body.append(f"{d}[0] = x86::sqrtss({s});" if mn == "sqrtss" else f"{d}[0] = x86::{mn}({d}[0], {s});")
            elif mn in ("addps", "subps", "mulps"):
                d = xmm(i, ops[0])
                if ops[1].type != X86_OP_REG:
                    body.append(f"{d} = x86::{mn}({d}, {vector(i, ops[1])});")
                else:
                    s = xmm(i, ops[1])
                    body.append(f"for k in 0..4 {{ {d}[k] = x86::{mn[:3]}ss({d}[k], {s}[k]); }}")
            elif mn == "shufps":
                d, s = xmm(i, ops[0]), (xmm(i, ops[1]) if ops[1].type == X86_OP_REG else None)
                if s is None:
                    body.append(f"{d} = x86::shuffle_ps::<{ops[2].imm & 255}>({d}, {vector(i, ops[1])});")
                else:
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
            # Hardware x87 over saved 80-bit state. Keep original opcode and
            # register operands; memory ModRM becomes [rax] in the helper.
            elif mn in X87_SUPPORTED:
                if mn in ("fnstsw", "fstsw") and ops[0].type == X86_OP_REG:
                    body.append(set_reg("ax", "st.status() as u32"))
                else:
                    raw = bytes(i.bytes)
                    while raw and raw[0] in (0x26, 0x36, 0x3e, 0x64):
                        raw = raw[1:]  # segment applied by mem_addr, not host x87
                    if len(raw) < 2 or not 0xD8 <= raw[0] <= 0xDF:
                        raise Unsupported(f"x87-prefix {mn}")
                    mem = next((o for o in ops if o.type == X86_OP_MEM), None)
                    if mem is not None:
                        if mem.size not in (2, 4, 8, 10):
                            raise Unsupported(f"x87-width {mn}")
                        opcode = raw[0] | ((raw[1] & 0x38) << 8)
                        write = mn in ("fst", "fstp", "fist", "fistp", "fnstcw", "fnstsw", "fstsw")
                        body.append(f"st.memory::<0x{opcode:04x}, {mem.size}, {str(write).lower()}>(m, {mem_addr(i, mem)});")
                    else:
                        opcode = raw[0] | (raw[1] << 8)
                        if mn in ("fcomi", "fcompi", "fucomi", "fucompi"):
                            body.append(f"fl = st.compare_flags::<0x{opcode:04x}>();")
                        else:
                            body.append(f"st.register::<0x{opcode:04x}>();")
            elif mn == "sahf":
                body.append("fl.sahf((eax >> 8) as u8);")
            elif mn == "lahf":
                body.append("eax = (eax & !0xff00) | ((fl.lahf() as u32) << 8);")
            elif mn in ("wait", "fwait"):
                pass  # masked x87 exceptions: wait has no effect
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
    # Prepared pre-DF caches use the old state-transfer markers.
    arm = arm.replace(SAVE.replace(", df: cpu.df", ""), "").replace(LOAD.replace(", df: _", ""), "")
    fields = {r: f"cpu.{r}" for r in GPR32 + ["fl", "st"]}
    fields.update({r: f"cpu.xmm[{i}]" for i, r in enumerate(XMM)})
    arm = re.sub(r'"(?:\\.|[^"\\])*"|\b(?:' + "|".join(fields) + r')\b',
                 lambda m: fields.get(m[0], m[0]), arm)
    return fast_memory(arm)


# The six Mem accessors the templates emit, lowered to the inline fast path
# on `dyn Mem` (ddnative-base mem.rs): `pt` is the page table read once at
# function entry (rust_fn); with None (fixtures, Sparse) each one makes the
# same trait call as before.
MEM_ACCESS = re.compile(r'"(?:\\.|[^"\\])*"|\bm\.(u8|u16|u32|set_u8|set_u16|set_u32)\(')


def fast_memory(arm):
    """Lower `m.u32(a)` to `m.fast_u32(pt, a)` (all six accessors), leaving
    Rust strings untouched. Gen-time only: cached arms keep `m.u32`."""
    return MEM_ACCESS.sub(lambda m: f"m.fast_{m[1]}(pt, " if m[1] else m[0], arm)


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
        # the page table behind m (a Copy pointer, valid for the whole call)
        "    let pt = m.page_table();",
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
    rows = db.execute("SELECT address, namespace, name, size FROM functions WHERE "
                      "status != 'verified' AND size >= 1 "
                      "ORDER BY address").fetchall()
    # verified functions are lifted too (hand ports keep their own tests): a
    # caller can only lift if its callees are lifted
    rows += db.execute("SELECT address, namespace, name, size FROM functions WHERE status='verified' "
                       "AND size >= 1").fetchall()
    db.close()
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    # Model bodies get a separate original-entry contract in the ext pass.
    # They never join the closed set: callers must keep using the models.
    rows = sorted(rows)
    rows = rows[:limit]
    # rounds: a function whose calls all go to functions lifted in an earlier
    # round (or earlier in this one) lifts too; repeat until nothing changes
    done, rnd = set(), 0
    while True:
        rnd += 1
        why, before = Counter(), len(ok)
        for a, ns, nm, sz in rows:
            if a in done or a in h.PLATFORM:
                continue
            try:
                arms = lift(a, sz, done)
                ok.append({"addr": a, "name": f"{ns}::{nm}", "size": sz, "arms": arms, "calls": callees(arms), "x87_push": X87_PUSH_SITES.get(a, []), **BODY_INFO.get(a, {})})
                done.add(a)
                release_body(a, sz)
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
                       "ext": True, **BODY_INFO.get(a, {})})
            if a in h.PLATFORM:
                ok[-1]['original_entry'] = True
            release_body(a, sz)
        except Unsupported as e:
            why[str(e).split(" ")[0]] += 1
        except Exception as e:  # noqa: BLE001
            why["error:" + type(e).__name__] += 1
    print(f"ext pass: +{len(ok) - before}", flush=True)
    with open(f"{N}/patterns/lift.json", "w") as stream:
        json.dump(ok, stream)
    with open(f"{N}/patterns/lift_rejections.json", "w") as stream:
        json.dump(dict(why.most_common()), stream)
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
            if f["addr"] in h.PLATFORM and not f.get('original_entry'):
                raise Unsupported("platform model address")
            f["arms"] = lift(f["addr"], f["size"], closed, ext=bool(f.get("ext")))
            f["x87_push"] = X87_PUSH_SITES.get(f["addr"], [])
            f["calls"] = callees(f["arms"])
            f.update(BODY_INFO[f["addr"]])
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


def seed_closed_contracts(selected):
    """Restore the selected closed x87 contracts without retaining bodies."""
    closed = {f['addr'] for f in selected if not f.get('ext')}
    # Restore closed x87 contracts in dependency order. Only float users
    # need retranslation; the others have zero net stack effect.
    pending = [f for f in selected if f['addr'] in closed]
    while pending:
        ready = [f for f in pending if set(f.get('calls', [])) <= X87_NET.keys()]
        if not ready:
            raise ValueError('selected closed call graph has a cycle or missing callee')
        for f in ready:
            a, size = f['addr'], f['size']
            try:
                if any('st.' in arm for arm in f['arms']) or any(X87_NET[x] for x in f.get('calls', [])):
                    lift(a, size, closed)
                else:
                    X87_NET[a] = 0
            finally:
                release_body(a, size)
        ready_addrs = {f['addr'] for f in ready}
        pending = [f for f in pending if f['addr'] not in ready_addrs]
    return closed


def scan_extra():
    """Add supplementary entries without changing existing mock boundaries.

    Existing selected bodies are retained verbatim. All additions use ext
    mode and only the already selected closed callees, so successful old
    recordings remain valid. A later full scan may choose new boundaries
    and must use the normal vector invalidation path.
    """
    import harness as h
    from collections import Counter
    with open(f"{N}/patterns/lift.json") as stream:
        selected = json.load(stream)
    have = {f['addr'] for f in selected}
    closed = seed_closed_contracts(selected)
    with closing(sqlite3.connect(f"file:{DB}?mode=ro", uri=True)) as db:
        rows = db.execute('SELECT f.address,f.name,f.size FROM supplementary s JOIN functions f USING(address) ORDER BY address').fetchall()
    rejected, added = {}, 0
    for a, name, size in rows:
        if a in have or a in h.PLATFORM:
            continue
        try:
            arms = lift(a, size, closed, ext=True)
            selected.append(dict(addr=a, name=f'Supplementary::{name}', size=size, arms=arms,
                                 calls=callees(arms), x87_push=X87_PUSH_SITES.get(a, []),
                                 ext=True, **BODY_INFO[a]))
            added += 1
        except Unsupported as e:
            rejected[a] = str(e)
        finally:
            release_body(a, size)
    with open(f'{N}/patterns/lift.json', 'w') as stream:
        json.dump(selected, stream)
    with open(f'{N}/patterns/lift_extra_rejections.json', 'w') as stream:
        json.dump(rejected, stream, indent=2)
    print(f'{len(have)} retained; {added} supplementary entries lifted; {len(rejected)} rejected', flush=True)
    print(Counter(s.split()[0] for s in rejected.values()), flush=True)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "relift":
        relift()
    if cmd == "scan-extra":
        scan_extra()
    if cmd == "scan":
        scan(int(sys.argv[2]) if len(sys.argv) > 2 else None)
    elif cmd == "one":
        import sqlite3 as _s
        a = int(sys.argv[2], 16)
        sz, ns, nm = _s.connect(DB).execute("select size, namespace, name from functions where address=?", (a,)).fetchone()
        print(rust_fn(a, f"{ns}::{nm}", lift(a, sz)))


_CLOSED = frozenset()


def vector_key(f):
    """Versioned recorder boundary; old caches need an explicit migration."""
    if "body" not in f:
        return None
    import hashlib
    fields = [f.get(k) for k in ("body", "noreturn_sites", "calls", "ext", "x87_push")]
    if f.get('original_entry'):
        fields.append('original-entry-v1')
    return "body-v1:" + hashlib.sha256(json.dumps(fields, separators=(",", ":")).encode()).hexdigest()


def _init_worker(closed):
    global _CLOSED
    _CLOSED = frozenset(closed)


def _vectors_for(args):
    addr, per, end = args[:3]
    size = args[3] if len(args) > 3 else None
    push = frozenset(args[4]) if len(args) > 4 else frozenset()
    body = args[5] if len(args) > 5 else None
    stops = args[6] if len(args) > 6 else []
    key = args[7] if len(args) > 7 else None
    sys.path.insert(0, os.path.dirname(__file__))
    import harness as h
    inputs = {"zero_inputs": True} if os.environ.get("LIFT_ZERO_INPUTS") == "1" else {}
    if len(args) > 8 and args[8]:
        inputs['original_entry'] = True
    if body is not None:
        inputs.update(body=body, noreturn_sites=stops, noreturn_targets=noreturn_entries())
    out, got, seed = [], 0, 0
    # a time budget per function: loops over random counts of random
    # pointers can take minutes per seed and never give a vector
    import time
    deadline = time.time() + float(os.environ.get("LIFT_FN_SECONDS", "20"))
    while got < per and seed < per * 8 and time.time() < deadline:
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
        out.append({"addr": addr, **r, **({"body_key": key} if key else {})})
        got += 1
    # each Unicorn instance sits in reference cycles (hooks close over it):
    # collect them now, or a long run creeps up by about 1 MB per function
    import gc
    gc.collect()
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
    keys = {f["addr"]: vector_key(f) for f in lifted}
    def fresh(c):
        if keys.get(c["addr"]) is not None and c.get("body_key") != keys[c["addr"]]:
            return False
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
            cases += [c for c in r["cases"] if c["addr"] in want and fresh(c)]
            if not r["cases"] and r.get("body_key") == keys.get(r["addr"]):
                failed.append(r["addr"])
    have = {c["addr"] for c in cases}
    # functions that already failed to get a vector are not retried (each costs 8x the seeds)
    nov_path = f"{N}/patterns/lift_novector.json"
    nov_keys_path = f"{N}/patterns/lift_novector_keys.json"
    nov_keys = json.load(open(nov_keys_path)) if os.path.exists(nov_keys_path) else {}
    novector = (set(json.load(open(nov_path))) if os.path.exists(nov_path) else set()) | set(failed)
    novector = {a for a in novector if a in failed or keys.get(a) is None or nov_keys.get(str(a)) == keys[a]}
    todo = [(f["addr"], per, f["addr"] + f["size"] if f.get("ext") else None, f["size"], f.get("x87_push", []), f.get("body"), f.get("noreturn_sites", []), keys[f["addr"]], f.get('original_entry',False))
            for f in lifted if f["addr"] not in have and f["addr"] not in novector]
    closed = [f["addr"] for f in lifted if not f.get("ext")]
    print(len(have), "functions reuse their vectors;", len(todo), "to record", flush=True)
    log = open(prog, "a")
    # a worker is replaced after 8 functions: some functions leave hundreds of
    # MB of Unicorn state behind that gc does not free (workers reached 3 GB)
    with Pool(int(os.environ.get("LIFT_WORKERS", os.cpu_count())), _init_worker, (closed,),
              maxtasksperchild=8) as pool:
        for n, (addr, out) in enumerate(pool.imap_unordered(_vectors_for, todo, chunksize=1)):
            cases += out
            if not out:
                failed.append(addr)
            log.write(json.dumps({"addr": addr, "cases": out, "body_key": keys[addr]}) + "\n")
            log.flush()
            if n % 1000 == 999:
                print(n + 1, "recorded", flush=True)
    json.dump(sorted(novector | set(failed)), open(nov_path, "w"))
    json.dump({a: keys.get(a) for a in novector | set(failed)}, open(nov_keys_path, "w"))
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


SHARDS = 16


def shard(addr):
    """Fixed 1 MiB address bands, modulo 16; independent of the selected set."""
    return (addr >> 20) % SHARDS


def write_changed(path, text):
    """An unchanged shard must not make Cargo recompile it."""
    from pathlib import Path
    path = Path(path)
    if path.exists() and path.read_text(encoding="utf-8") == text:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def shard_fn(f):
    def target(match):
        addr = int(match[1], 16)
        if shard(addr) == shard(f["addr"]):
            return match[0]
        return f"(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_lifted(m, cpu, 0x{addr:08X}))("
    return CALLEE.sub(target, rust_fn(f["addr"], f["name"], f["arms"]))


def wired_ports():
    """Addresses in the PORTS table of crates/mtmath/src/abi.rs: verified
    readable ports that replace their lifted bodies (L24, docs/MATH.md)."""
    path = os.path.join(R, "crates/mtmath/src/abi.rs")
    if not os.path.exists(path):
        return set()
    text = open(path, encoding="utf-8").read()
    return {int(a, 16) for a in re.findall(r"\(0x[0-9A-Fa-f_]+, f_([0-9a-f]{8}), 0b[01_]+\)", text)}


def wired_lines(selected, ported, chunk):
    """The mod.rs lines that put the mtmath ports over a shard's lifted
    bodies (explicit imports shadow the lifted_NNN globs) and keep each
    lifted body as replaced::f_X."""
    mine = [(f["addr"], f"lifted_{k // chunk:03}") for k, f in enumerate(selected) if f["addr"] in ported]
    text = "".join(f"pub use mtmath::abi::f_{a:08x};\n" for a, _ in mine)
    if mine:
        text += "/// The lifted bodies of the functions mtmath replaces.\npub mod replaced {\n"
        text += "".join(f"    pub use super::{m}::f_{a:08x};\n" for a, m in mine)
        text += "}\n"
    return text


def gen(chunk=400):
    from pathlib import Path
    lifted = sorted(json.loads(Path(N, "patterns/lift.json").read_text()), key=lambda f: f["addr"])
    addresses = {f["addr"] for f in lifted}
    if len(addresses) != len(lifted):
        raise ValueError("duplicate lifted address")
    for f in lifted:
        missing = set(callees(f["arms"])) - addresses
        if missing:
            raise ValueError(f"0x{f['addr']:08X}: missing lifted callees {sorted(missing)}")
    out_dir = os.path.join(R, "crates/ddnative/src/generated")
    ported = wired_ports()
    if ported - addresses:
        raise ValueError(f"mtmath ports without a lifted body: {sorted(hex(a) for a in ported - addresses)}")
    mods = []
    for index in range(SHARDS):
        selected = [f for f in lifted if shard(f["addr"]) == index]
        folder = os.path.join(R, f"crates/ddlift/lift_{index:02}/src")
        names = []
        for k in range(0, len(selected), chunk):
            name = f"lifted_{k // chunk:03}"
            names.append(name)
            lines = ["//! Generated by research/native/lift.py: do not edit.",
                     "#![allow(warnings, clippy::all)]",
                     "use crate::mem::Mem;", "use crate::x86::{self, Cpu, Flags};", ""]
            lines += [shard_fn(f) for f in selected[k:k + chunk]]
            write_changed(os.path.join(folder, "generated", name + ".rs"), "\n".join(lines))
        mods += names
        index_text = "//! Generated by research/native/lift.py: do not edit.\n"
        index_text += "use crate::x86::LiftFn;\n"
        index_text += "".join(f"mod {name}; pub use {name}::*;\n" for name in names)
        # Verified readable ports (crates/mtmath, L24) replace their lifted
        # bodies: an explicit import shadows the glob above, so intra-shard
        # calls, FUNCTIONS and lookup() all reach the port. The lifted body
        # stays reachable as replaced::f_X (benchmarks, tests).
        index_text += wired_lines(selected, ported, chunk)
        # Cached intra-shard calls retain their original super::lifted path.
        index_text += "mod lifted { pub use super::*; }\n"
        index_text += "pub const FUNCTIONS: &[(u32, LiftFn)] = &[\n"
        index_text += "".join(f"(0x{f['addr']:08X}, f_{f['addr']:08x}),\n" for f in selected)
        index_text += "];\npub fn lookup(addr: u32) -> Option<LiftFn> {\n"
        index_text += "    FUNCTIONS.binary_search_by_key(&addr, |e| e.0).ok().map(|i| FUNCTIONS[i].1)\n}\n"
        write_changed(os.path.join(folder, "generated", "mod.rs"), index_text)
        # Remove obsolete generated modules, never unrelated source files.
        for old in Path(folder, "generated").glob("lifted_*.rs"):
            if old.stem not in names:
                old.unlink()
    mod_path = os.path.join(out_dir, "mod.rs")
    mod = Path(mod_path).read_text().splitlines()
    mod = [l for l in mod if not l.startswith("pub mod lifted_")]
    if "pub mod lifted;" not in mod:
        mod.append("pub mod lifted;")
    write_changed(mod_path, "\n".join(mod) + "\n")
    # Old monolithic files are not compiled or scanned by the inventory.
    for old in Path(out_dir).glob("lifted_*.rs"):
        old.unlink()
    lookup = "//! Generated facade: initialization, legacy entry points and lookup.\n"
    lookup += "use crate::mem::Mem;\nuse crate::x86::{self, Cpu, LiftFn};\n"
    lookup += "fn lookup(addr: u32) -> Option<LiftFn> { match (addr >> 20) % 16 {\n"
    lookup += "".join(f"{i} => ddlift_{i:02}::lookup(addr),\n" for i in range(SHARDS))
    lookup += "_ => unreachable!(), } }\n"
    lookup += "pub fn init() { x86::set_lifted_lookup(lookup); }\n"
    # Preserve public f_ADDRESS entry points, including callers that do not
    # use all()/dispatch_table(). Raw shard entry points require init().
    for f in lifted:
        name = f"f_{f['addr']:08x}"
        lookup += f"pub fn {name}(m: &mut dyn Mem, cpu: &mut Cpu) {{ init(); ddlift_{shard(f['addr']):02}::{name}(m, cpu); }}\n"
    lookup += "pub fn all() -> Vec<(u32, LiftFn)> { init(); let mut v = Vec::new();\n"
    lookup += "".join(f"v.extend_from_slice(ddlift_{i:02}::FUNCTIONS);\n" for i in range(SHARDS))
    lookup += "v.sort_unstable_by_key(|e| e.0); v }\n"
    lookup += "/// External-call functions stay mocked at indirect boundaries.\n"
    lookup += "pub const EXT: &[u32] = &[" + ", ".join(f"0x{f['addr']:08X}" for f in lifted if f.get("ext")) + "];\n"
    lookup += "pub fn dispatch_table() -> std::collections::HashMap<u32, LiftFn> {\n"
    lookup += "let ext: std::collections::HashSet<u32> = EXT.iter().copied().collect();\n"
    lookup += "let mut t: std::collections::HashMap<u32, LiftFn> = all().into_iter().filter(|(a, _)| !ext.contains(a)).collect();\n"
    lookup += "for &(a, _, f) in crate::platform::MODELS { t.insert(a, f); } t }\n"
    write_changed(os.path.join(out_dir, "lifted.rs"), lookup)
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
            cpu.fs_base = 0x7FFD_E000; // harness.TEB_BASE (fixed recorder thread)
            f(&mut m, &mut cpu);
            if c.get("x87_control").is_some() {{
                assert_eq!(cpu.st.control() as u32, u(&c["x87_control"]), "x87 control");
                assert_eq!(cpu.st.depth(), u(&c["x87_depth"]), "x87 depth");
                let extended = c["st0_ext"].as_array().map(|v| (v[0].as_u64().unwrap(), u(&v[1]) as u16));
                assert_eq!(cpu.st.top_extended(), extended, "extended st0");
            }}
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
    write_changed(os.path.join(R, "crates/ddnative/tests/lifted.rs"), test)
    print(len(lifted), "functions generated in", len(mods), "modules across", SHARDS, "crates")


if __name__ == "__main__" and sys.argv[1] in ("vectors", "gen"):
    {"vectors": vectors, "gen": gen}[sys.argv[1]]()
