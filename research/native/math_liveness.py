"""Caller liveness of the xmm scratch registers of the wired math ports (L24).

A port in crates/mtmath/src/abi.rs reproduces every general register, xmm0,
the x87 state and every byte the original writes, plus the xmm registers in
its PORTS mask. It leaves the other xmm registers as they were, while the
original may leave scratch values there. That is only
equivalent if no caller reads such a register after the call before
writing it. This script checks it for every direct call site:

    PY research/native/math_liveness.py

For each ported address it finds the xmm registers outside the mask the
original writes, then walks every path of each caller from the return
address (lift.decode_body of the caller: direct branches, bounded switch
tables) until each of them is fully written. A read on the way is reported,
as is a call site it cannot follow:

- `ret` ends a path: the registers are already clobbered for the caller's
  caller, which cannot expect a value in them either;
- a direct call to G continues inside G (memoized per callee and pending
  set) and, if G may return without writing them, after the call; the
  platform models (harness.PLATFORM: memcpy, memset, the allocators, all
  cdecl/thiscall with stack arguments) read no xmm register;
- an indirect call reads no xmm register: MSVC's x86 conventions for calls
  through pointers (thiscall, cdecl, stdcall) pass no xmm arguments, and
  whole-program register conventions apply only to direct calls;
- a tail jump to the port, or an indirect jump that is not a bounded
  switch, is reported (not followed).

Registers are tracked per 32-bit lane: a scalar op or `movlpd` writes some
lanes and leaves the others pending without reading them. Exits 1 if
anything is reported.
"""
import os
import re
import sqlite3
import sys
from functools import lru_cache

sys.path.insert(0, os.path.dirname(__file__))
import lift  # noqa: E402

REPO = os.path.join(os.path.dirname(__file__), "..", "..")
DB = os.environ.get("INVENTORY_DB", "E:/ddda-re/notes/inventory/inventory.sqlite")
XMM = {f"xmm{k}" for k in range(8)}
SC, SD, ALL = (0,), (0, 1), (0, 1, 2, 3)  # 32-bit lanes: single, double, all
FULL_MOVES = {"movaps", "movups", "movapd", "movupd", "movdqa", "movdqu"}
ZERO_IDIOMS = {"xorps", "xorpd", "pxor", "pcmpeqd"}
SS_BINARY = {"addss", "subss", "mulss", "divss", "minss", "maxss"}
SD_BINARY = {"addsd", "subsd", "mulsd", "divsd", "minsd", "maxsd"}
SS_UNARY = {"sqrtss", "rcpss", "rsqrtss", "cvtsi2ss"}
COMPARE = {"comiss": SC, "ucomiss": SC, "comisd": SD, "ucomisd": SD}
TO_INT = {"cvttss2si": SC, "cvtss2si": SC, "cvttsd2si": SD, "cvtsd2si": SD}
SHIFTS = {"psrlq", "psllq", "psrld", "pslld", "psrldq", "pslldq", "psrlw", "psllw", "psraw", "psrad"}


def ported():
    """{address: mask of the xmm registers the adapter reproduces}, from
    the PORTS table of crates/mtmath/src/abi.rs."""
    text = open(os.path.join(REPO, "crates/mtmath/src/abi.rs"), encoding="utf-8").read()
    rows = re.findall(r"\(0x[0-9A-Fa-f_]+, f_([0-9a-f]{8}), (0b[01_]+)\)", text)
    return {int(a, 16): int(mask.replace("_", ""), 2) for a, mask in rows}


def db():
    return sqlite3.connect(f"file:{DB}?mode=ro", uri=True)


@lru_cache(maxsize=None)
def body(addr):
    with db() as c:
        row = c.execute("SELECT size FROM functions WHERE address=?", (addr,)).fetchone()
    ins, switches, stops, _ = lift.decode_body(addr, row[0] if row else 1, ext=True)
    return {i.address: i for i in ins}, switches


def access(i):
    """(lanes read, lanes written): sets of (xmm register, 32-bit lane).

    A write of some lanes leaves the others pending; it does not read them.
    Unlisted instructions read every lane of every xmm operand and write
    every lane of an xmm destination (conservative for this check)."""
    mn, ops = i.mnemonic, i.operands
    x = [i.reg_name(o.reg) if o.type == 1 and i.reg_name(o.reg) in XMM else None for o in ops]
    if not any(x):
        r, w = i.regs_access()
        if ({i.reg_name(k) for k in r} | {i.reg_name(k) for k in w}) & XMM:
            raise ValueError(f"implicit xmm access at {i.address:#x}: {mn} {i.op_str}")
        return set(), set()
    d = x[0]
    s = x[1] if len(x) > 1 else None
    store = ops[0].type == 3
    L = lambda reg, lanes: {(reg, k) for k in lanes} if reg else set()
    if mn in ZERO_IDIOMS and d and d == s:
        return set(), L(d, ALL)
    if mn in ("movss", "movsd"):
        lanes = SC if mn == "movss" else SD
        if store:
            return L(s, lanes), set()
        if s is None:  # load from memory zeroes the upper lanes
            return set(), L(d, ALL)
        return L(s, lanes), L(d, lanes)
    if mn in ("movlps", "movlpd", "movhps", "movhpd"):
        lanes = SD if mn in ("movlps", "movlpd") else (2, 3)
        return (L(s, lanes), set()) if store else (set(), L(d, lanes))
    if mn in FULL_MOVES:
        return (L(s, ALL), set()) if store else (L(s, ALL), L(d, ALL))
    if mn in ("movd", "movq"):
        lanes = SC if mn == "movd" else SD
        if d:  # into xmm: the upper lanes are zeroed
            return L(s, lanes), L(d, ALL)
        return L(s, lanes), set()
    if mn in SS_BINARY:
        return L(d, SC) | L(s, SC), L(d, SC)
    if mn in SD_BINARY:
        return L(d, SD) | L(s, SD), L(d, SD)
    if mn in SS_UNARY or mn == "sqrtsd":
        lanes = SC if mn != "sqrtsd" else SD
        return L(s, lanes), L(d, lanes)
    if mn == "cvtsd2ss":
        return L(s, SD), L(d, SC)
    if mn == "cvtss2sd":
        return L(s, SC), L(d, SD)
    if mn in COMPARE:
        return L(d, COMPARE[mn]) | L(s, COMPARE[mn]), set()
    if mn in TO_INT:
        return L(s, TO_INT[mn]), set()
    if mn in ("pshufd", "pshuflw", "pshufhw"):
        return L(s, ALL), L(d, ALL)
    if mn in SHIFTS and len(ops) == 2 and ops[1].type == 2:
        return L(d, ALL), L(d, ALL)
    reads = set().union(*(L(r, ALL) for r in x if r))
    return reads, (L(d, ALL) if d and not store else set())


def clobbered(addr):
    """Lanes of xmm1..xmm7 the original may write."""
    ins, _ = body(addr)
    out = set()
    for i in ins.values():
        out |= access(i)[1]
    return {(r, k) for r, k in out if r != "xmm0"}


def successors(i, switches, ins):
    mn = i.mnemonic
    if mn in ("ret", "retn", "repz ret", "hlt", "int3", "ud2"):
        return []
    nxt = [i.address + i.size]
    if mn.startswith("j"):
        if i.operands[0].type == 2:
            t = i.operands[0].imm & 0xFFFFFFFF
            tgt = [t] if t in ins else []
        else:
            tgt = list(switches.get(i.address) or [])
        return tgt if mn == "jmp" else tgt + nxt
    return nxt


ACTIVE = set()


@lru_cache(maxsize=None)
def walk(func, start, pending):
    """Problems on paths from `start` in `func` with `pending` registers
    unwritten, and whether some path returns with some of them pending."""
    import harness as h
    if func in h.PLATFORM:
        return [], True
    if (func, start, pending) in ACTIVE:
        # a cycle: the active walk of the same entry and pending set finds
        # any read on these paths itself
        return [], True
    ACTIVE.add((func, start, pending))
    try:
        return _walk(func, start, pending)
    finally:
        ACTIVE.discard((func, start, pending))


def _walk(func, start, pending):
    ins, switches = body(func)
    problems, returns = [], False
    seen = set()
    todo = [(start, pending)]
    while todo:
        pc, pend = todo.pop()
        if not pend or (pc, pend) in seen:
            continue
        seen.add((pc, pend))
        i = ins.get(pc)
        if i is None:
            problems.append(f"{func:#010x}: path leaves the decoded body at {pc:#010x}")
            continue
        mn = i.mnemonic
        if mn == "call":
            op = i.operands[0]
            if op.type == 2:
                g = op.imm & 0xFFFFFFFF
                try:
                    sub, ret_pending = walk(g, g, pend)
                except lift.Unsupported as e:
                    problems.append(f"{func:#010x}: callee {g:#010x} not decodable ({e})")
                    continue
                problems += sub
                if not ret_pending:
                    continue
            todo.append((i.address + i.size, pend))
            continue
        reads, written = access(i)
        hit = reads & pend
        if hit:
            problems.append(f"{func:#010x}: {sorted(hit)} read at {pc:#010x} ({mn} {i.op_str})")
        pend = pend - written
        if mn.startswith("j") and i.operands[0].type != 2 and switches.get(pc) is None:
            problems.append(f"{func:#010x}: indirect jump at {pc:#010x}")
            continue
        if mn in ("ret", "retn") and pend:
            returns = True
        for s in successors(i, switches, ins):
            todo.append((s, pend))
    return problems, returns


def main():
    sys.setrecursionlimit(20000)
    bad = 0
    with db() as c:
        for addr, exact in sorted(ported().items()):
            regs = frozenset((r, k) for r, k in clobbered(addr) if not exact >> int(r[3:]) & 1)
            callers = [a for a, in c.execute("SELECT DISTINCT caller FROM calls WHERE callee=?", (addr,))]
            sites = problems = 0
            for f in callers:
                ins, _ = body(f)
                for i in ins.values():
                    if i.mnemonic.startswith("j") and i.operands[0].type == 2 and i.operands[0].imm == addr:
                        print(f"  {addr:#010x}: tail jump from {i.address:#010x}")
                        problems += 1
                    if i.mnemonic == "call" and i.operands[0].type == 2 and i.operands[0].imm == addr:
                        sites += 1
                        if regs:
                            found, _ = walk(f, i.address + i.size, regs)
                            for p in sorted(set(found)):
                                print(f"  {addr:#010x} site {i.address:#010x}: {p}")
                            problems += len(set(found))
            names = sorted({r for r, _ in regs})
            print(f"0x{addr:08X}: scratch {names or '-'}; {len(callers)} callers, {sites} sites, "
                  f"{problems} problems")
            bad += problems
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
