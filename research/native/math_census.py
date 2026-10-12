"""Census of the MT Framework math library in the user's DDDA.exe (L24).

The math types (MtVector3/4, MtMatrix, MtQuaternion) have no DTI class, so
Ghidra leaves their out-of-line helpers unnamed. This finds them by shape:
small functions whose body is mostly float instructions (SSE or x87), few
or no calls, ranked by direct callers from the inventory database.

    PY research/native/math_census.py [--min-callers N] [--top N] [--json OUT]

Prints address, size, callers, callees, float share, SSE/x87 counts and the
first instructions. Reads the exe and the inventory at run time; nothing
from them is committed. Write any JSON output under E:/ddda-re/notes.
"""
import argparse
import json
import os
import sqlite3
import sys

from capstone import Cs, CS_ARCH_X86, CS_MODE_32

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402

DB = os.environ.get("INVENTORY_DB", "E:/ddda-re/notes/inventory/inventory.sqlite")

SSE_FLOAT = ("ss", "ps")  # scalar/packed single suffixes
SSE_OTHER = {"shufps", "unpcklps", "unpckhps", "movlhps", "movhlps", "movaps", "movups", "movss",
             "comiss", "ucomiss", "cvtsi2ss", "cvttss2si", "cvtss2si", "cvtss2sd", "cvtsd2ss",
             "andps", "andnps", "orps", "xorps", "rcpss", "rsqrtss", "rcpps", "rsqrtps", "sqrtss",
             "sqrtps", "maxss", "minss", "maxps", "minps", "movlps", "movhps", "cmpps", "cmpss"}


def classify(addr, size):
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    body = h.code(addr, size)
    sse = x87 = calls = total = 0
    head = []
    for ins in md.disasm(body, addr):
        total += 1
        m = ins.mnemonic
        if len(head) < 8:
            head.append(f"{m} {ins.op_str}".strip())
        if m.startswith("f"):
            x87 += 1
        elif m in SSE_OTHER or (len(m) > 3 and m.endswith(SSE_FLOAT) and m not in ("movs",)):
            sse += 1
        elif m == "call":
            calls += 1
    return total, sse, x87, calls, head


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--min-callers", type=int, default=5)
    ap.add_argument("--max-size", type=int, default=2048)
    ap.add_argument("--min-share", type=float, default=0.35)
    ap.add_argument("--top", type=int, default=200)
    ap.add_argument("--json")
    a = ap.parse_args()
    db = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows = db.execute("SELECT address, namespace, name, size, callers, callees, status FROM functions "
                      "WHERE thunk = 0 AND size BETWEEN 1 AND ? AND callers >= ? ORDER BY callers DESC",
                      (a.max_size, a.min_callers)).fetchall()
    out = []
    for addr, ns, name, size, callers, callees, status in rows:
        try:
            total, sse, x87, calls, head = classify(addr, size)
        except KeyError:
            continue
        if total == 0:
            continue
        share = (sse + x87) / total
        if share < a.min_share:
            continue
        out.append({"addr": addr, "name": f"{ns}::{name}", "size": size, "callers": callers,
                    "callees": callees, "status": status, "insns": total, "sse": sse, "x87": x87,
                    "calls": calls, "share": round(share, 2), "head": head})
    for f in out[:a.top]:
        print(f"0x{f['addr']:08X} {f['size']:5} callers={f['callers']:5} callees={f['callees']} "
              f"share={f['share']:.2f} sse={f['sse']} x87={f['x87']} calls={f['calls']} {f['name']} | "
              + "; ".join(f["head"][:5]))
    print(f"{len(out)} functions", file=sys.stderr)
    if a.json:
        with open(a.json, "w") as fh:
            json.dump(out, fh, indent=1)


if __name__ == "__main__":
    main()
