"""Static audit: does any code in DDDA.exe observe dead stack? (L24)

A function's writes below its caller's ESP (pushes, spills, its callees'
frames) are dead once it returns, unless later code reads them. Two ways
code could:

1. a read below the current ESP (`[esp - N]`, or an EBP-relative address
   that lies below ESP), on the stack nobody owns;
2. a read of a byte of its own frame (below its entry ESP) before writing
   it on that path: the frame is allocated over earlier callees' dead
   stack, so such a read sees their leftovers (e.g. a 16-byte copy of a
   vector whose fourth lane was never written).

    PY research/native/stack_audit.py [--limit N] [--json OUT]

For every inventory function (lift.decode_body, ext mode) it follows each
path, tracking ESP and EBP relative to the entry ESP (`and esp, -N` starts
a separate base for the aligned locals), and the bytes of the frame
definitely written on the path. It reports kind 1 and kind 2 reads.

Conservative choices (they avoid false alarms, not misses of the kinds
above): once a frame address escapes (`lea r, [esp/ebp + d]`, `mov r,
esp/ebp`), every frame byte counts as written from then on, since a callee
may fill it; an indirect call or a call whose stack cleanup is unknown
makes ESP-relative addresses untrackable on that path (counted); indexed
stack operands are skipped (counted); each instruction is visited at most
a few times per function. Functions that cannot be decoded are listed.
"""
import argparse
import json
import os
import sqlite3
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402
import lift  # noqa: E402

DB = os.environ.get("INVENTORY_DB", "E:/ddda-re/notes/inventory/inventory.sqlite")
VISITS = 3


class State:
    __slots__ = ("esp", "base", "ebp", "written", "escaped")

    def __init__(self, esp, base, ebp, written, escaped):
        self.esp, self.base, self.ebp, self.written, self.escaped = esp, base, ebp, written, escaped

    def copy(self):
        return State(self.esp, self.base, self.ebp, set(self.written), self.escaped)


def audit(addr, size, stats):
    ins, switches, stops, _ = lift.decode_body(addr, size, ext=True)
    by = {i.address: i for i in ins}
    found = []
    visits = Counter()
    todo = [(addr, State(0, "E", None, set(), False))]
    while todo:
        pc, st = todo.pop()
        while True:
            i = by.get(pc)
            if i is None or visits[pc] >= VISITS:
                break
            visits[pc] += 1
            mn, ops = i.mnemonic, i.operands
            regs = lambda o: (i.reg_name(o.mem.base) if o.mem.base else None,
                              i.reg_name(o.mem.index) if o.mem.index else None)

            def place(o):
                """(space, offset) of a stack memory operand, or None."""
                b, x = regs(o)
                if b not in ("esp", "ebp"):
                    return None
                if x is not None:
                    stats["indexed stack operand"] += 1
                    return None
                if b == "esp":
                    if st.esp is None:
                        stats["untracked esp access"] += 1
                        return None
                    return (st.base, st.esp + o.mem.disp, "esp")
                if st.ebp is None:
                    return None
                return ("E", st.ebp + o.mem.disp, "ebp")

            # escapes: a stack address leaves the instruction's own use
            if mn == "lea" and ops[1].type == 3 and regs(ops[1])[0] in ("esp", "ebp") \
                    and i.reg_name(ops[0].reg) not in ("esp", "ebp"):
                st.escaped = True
            if mn == "mov" and len(ops) == 2 and ops[1].type == 1 and i.reg_name(ops[1].reg) in ("esp", "ebp") \
                    and ops[0].type == 1 and i.reg_name(ops[0].reg) not in ("esp", "ebp"):
                st.escaped = True
            if mn == "push" and ops[0].type == 1 and i.reg_name(ops[0].reg) in ("esp",):
                st.escaped = True
            # memory reads and writes through esp/ebp
            reads, writes = [], []
            for idx, o in enumerate(ops):
                if o.type != 3 or mn == "lea":
                    continue
                p = place(o)
                if p is None:
                    continue
                r, w = mem_access(mn, idx, len(ops))
                if r:
                    reads.append((p, o.size))
                if w:
                    writes.append((p, o.size))
            for (space, off, via), n in reads:
                cur = st.esp if space == st.base and st.esp is not None else None
                if via == "esp" and off < (cur if cur is not None else off):
                    pass
                if via == "esp" and st.esp is not None and o_below(off, st.esp):
                    found.append((pc, "below esp", f"{mn} {i.op_str}"))
                elif via == "ebp" and st.base == "E" and st.esp is not None and o_below(off, st.esp):
                    found.append((pc, "below esp", f"{mn} {i.op_str}"))
                elif (space != "E" or off < 0) and not st.escaped:
                    missing = [b for b in range(off, off + n) if (space, b) not in st.written]
                    if missing:
                        found.append((pc, "unwritten frame byte", f"{mn} {i.op_str} ({len(missing)} of {n} bytes)"))
            for (space, off, via), n in writes:
                st.written.update((space, b) for b in range(off, off + n))
            # stack pointer effects
            if mn == "push":
                if st.esp is not None:
                    st.esp -= 4
                    st.written.update((st.base, b) for b in range(st.esp, st.esp + 4))
            elif mn == "pop":
                if ops[0].type == 1 and i.reg_name(ops[0].reg) == "ebp":
                    st.ebp = None
                if st.esp is not None:
                    st.esp += 4
            elif mn in ("sub", "add") and ops[0].type == 1 and i.reg_name(ops[0].reg) == "esp":
                if ops[1].type == 2 and st.esp is not None:
                    st.esp += -ops[1].imm if mn == "sub" else ops[1].imm
                else:
                    st.esp = None
                    stats["dynamic esp adjustment"] += 1
            elif mn == "and" and ops[0].type == 1 and i.reg_name(ops[0].reg) == "esp":
                st.base, st.esp = f"A{pc:x}", 0
            elif mn == "mov" and ops[0].type == 1 and i.reg_name(ops[0].reg) == "ebp" \
                    and ops[1].type == 1 and i.reg_name(ops[1].reg) == "esp":
                st.ebp = st.esp if st.base == "E" else None
            elif mn == "mov" and ops[0].type == 1 and i.reg_name(ops[0].reg) == "esp":
                if ops[1].type == 1 and i.reg_name(ops[1].reg) == "ebp" and st.ebp is not None:
                    st.base, st.esp = "E", st.ebp
                else:
                    st.esp = None
            elif mn == "leave":
                if st.ebp is not None:
                    st.base, st.esp, st.ebp = "E", st.ebp + 4, None
                else:
                    st.esp = None
            elif mn == "lea" and ops[0].type == 1 and i.reg_name(ops[0].reg) == "esp":
                st.esp = None
            elif mn == "call":
                if st.escaped is False and st.esp is not None:
                    pass
                if ops[0].type == 2:
                    pop = h.static_pop(ops[0].imm & 0xFFFFFFFF)
                    if st.esp is not None:
                        st.esp += pop
                else:
                    st.esp = None
                    stats["indirect call (esp untracked after)"] += 1
            if mn in ("ret", "retn", "repz ret", "hlt", "int3", "ud2") or pc in stops:
                break
            nxt = pc + i.size
            if mn.startswith("j"):
                if ops[0].type == 2:
                    t = ops[0].imm & 0xFFFFFFFF
                    targets = [t] if t in by else []
                else:
                    targets = list(switches.get(pc) or [])
                if mn == "jmp":
                    for t in targets[1:]:
                        todo.append((t, st.copy()))
                    if not targets:
                        break
                    pc = targets[0]
                    continue
                for t in targets:
                    todo.append((t, st.copy()))
            pc = nxt
    return found


# Instructions whose first (memory) operand is only written.
STORES = {"mov", "movss", "movsd", "movq", "movd", "movaps", "movups", "movapd", "movupd", "movdqa",
          "movdqu", "movlpd", "movlps", "movhpd", "movhps", "movntps", "movnti", "fst", "fstp", "fist",
          "fistp", "fisttp", "fnstcw", "fstcw", "fnstsw", "fstsw", "fbstp", "fnsave", "fsave",
          "fnstenv", "fstenv", "stmxcsr", "pop", "movsx", "movzx"}
READ_ONLY = {"cmp", "test", "push", "bt", "comiss", "ucomiss", "comisd", "ucomisd"}


def mem_access(mn, idx, nops):
    """(reads, writes) of the memory operand at position idx."""
    if mn in READ_ONLY:
        return True, False
    if idx == 0 and (mn in STORES or mn.startswith("set")):
        return False, True
    if idx == 0 and nops == 1 and mn.startswith("f"):
        return True, False  # fld, fadd, fcom, fldcw...
    if idx == 0:
        return True, True  # read-modify-write
    return True, False


def o_below(off, esp):
    return off < esp


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--limit", type=int)
    ap.add_argument("--json")
    a = ap.parse_args()
    db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows = db.execute("SELECT address, size FROM functions WHERE thunk = 0 AND size >= 1 ORDER BY address").fetchall()
    rows = rows[:a.limit]
    stats, report, failed = Counter(), {}, []
    for k, (addr, size) in enumerate(rows):
        try:
            f = audit(addr, size, stats)
        except Exception as e:  # noqa: BLE001 - undecodable bodies are listed
            failed.append((addr, type(e).__name__))
            continue
        if f:
            report[addr] = f
        if k % 5000 == 0:
            print(f"{k}/{len(rows)} functions, {len(report)} flagged", file=sys.stderr, flush=True)
    kinds = Counter(kind for f in report.values() for _, kind, _ in f)
    print(f"{len(rows)} functions, {len(failed)} not decodable, {len(report)} flagged: {dict(kinds)}")
    print("stats:", dict(stats))
    if a.json:
        json.dump({"flagged": {f"0x{k:08X}": v for k, v in report.items()},
                   "failed": [f"0x{x:08X} {e}" for x, e in failed], "stats": stats}, open(a.json, "w"), indent=1)


if __name__ == "__main__":
    main()
