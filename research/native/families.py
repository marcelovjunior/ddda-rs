"""Code-shape families of DDDA.exe functions (docs/NATIVE_PIPELINE.md).

    python families.py shapes     # loose shape groups (constants + call targets masked)
    python families.py exact      # split them into EXACT families with parameter slots
    python families.py platform   # classify boilerplate families as the platform layer
    python families.py show N     # print the N largest non-platform exact families

A family is a set of functions with the same bytes except at "parameter
slots": 4-byte immediates or displacements whose value points into the
image (vtables, globals, tables) and rel32 call/jump targets. Exact
families are what one parameterized port in crates/ddnative may cover;
each member is still replayed individually by its differential test.

Outputs (private, derived from the exe) go to E:/ddda-re/notes/inventory
and E:/ddda-re/notes/native. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import collections
import hashlib
import json
import os
import sqlite3
import struct
import sys

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402

from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402

INV = "E:/ddda-re/notes/inventory"
NATIVE = "E:/ddda-re/notes/native"
LO, HI = 0x00400000, 0x02000000
DB = f"{INV}/inventory.sqlite"


def slots(addr, size):
    """(masked bytes, [(offset, kind)]) where kind is 'const' or 'rel'."""
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    raw = h.code(addr, size)
    b = bytearray(raw)
    out = []
    for i in md.disasm(raw, addr):
        o = i.address - addr
        if i.bytes[0] in (0xE8, 0xE9) and i.size == 5:
            b[o + 1:o + 5] = b"\0\0\0\0"
            out.append((o + 1, "rel"))
            continue
        enc = i.x86 if hasattr(i, "x86") else None
        for off_attr, size_attr in (("disp_offset", "disp_size"), ("imm_offset", "imm_size")):
            off = getattr(i, off_attr, 0)
            sz = getattr(i, size_attr, 0)
            if off and sz == 4:
                v = struct.unpack_from("<I", raw, o + off)[0]
                if LO <= v < HI:
                    b[o + off:o + off + 4] = b"\0\0\0\0"
                    out.append((o + off, "const"))
    return bytes(b), out


def shapes():
    db = sqlite3.connect(DB)
    groups = collections.defaultdict(list)
    for a, sz in db.execute("SELECT address, size FROM functions WHERE NOT thunk "
                            "AND size BETWEEN 8 AND 4000 AND address < 0x012E0000"):
        masked, _ = slots(a, sz)
        groups[(sz, hashlib.sha1(masked).hexdigest())].append(a)
    fams = sorted((g for g in groups.values() if len(g) > 1), key=len, reverse=True)
    json.dump(fams, open(f"{INV}/shape_families.json", "w"))
    print(len(fams), "shape families,", sum(map(len, fams)), "functions")


def exact():
    """With precise slots (capstone encoding offsets) a shape group is
    already exact: members differ only at the slots. Record each family's
    slots and every member's slot values."""
    fams = json.load(open(f"{INV}/shape_families.json"))
    db = sqlite3.connect(DB)
    out = []
    for g in fams:
        rep = g[0]
        size = db.execute("SELECT size FROM functions WHERE address=?", (rep,)).fetchone()[0]
        masked, sl = slots(rep, size)
        members = []
        for a in g:
            m2, sl2 = slots(a, size)
            if m2 != masked or [s for s in sl2] != sl:
                continue  # not exact after all: different slot layout
            raw = h.code(a, size)
            members.append({"addr": a, "params": [struct.unpack_from("<I", raw, o)[0] if k == "const"
                                                  else (a + o + 4 + struct.unpack_from("<i", raw, o)[0]) & 0xFFFFFFFF
                                                  for o, k in sl]})
        if len(members) > 1:
            out.append({"rep": rep, "size": size, "slots": sl, "members": members})
    out.sort(key=lambda f: -len(f["members"]))
    json.dump(out, open(f"{INV}/exact_families.json", "w"))
    print(len(out), "exact families,", sum(len(f["members"]) for f in out), "functions")


def first_call(f):
    """Target of the family representative's first rel32 call, or None."""
    for (off, kind), value in zip(f["slots"], f["members"][0]["params"]):
        if kind == "rel" and h.code(f["rep"] + off - 1, 1)[0] == 0xE8:
            return value
    return None


def platform():
    """Boilerplate Rust replaces (no 1:1 port): deleting destructors, DTI
    new-instance allocators (they call the allocator selector 0x00CF5E20
    first), construct-if-memory wrappers, the MT allocator entry points."""
    fams = json.load(open(f"{INV}/exact_families.json"))
    db = sqlite3.connect(DB)
    pl, why = set(), collections.Counter()
    for f in fams:
        addrs = [m["addr"] for m in f["members"]]
        names = collections.Counter(r[0] for r in db.execute(
            f"SELECT name FROM functions WHERE address IN ({','.join(map(str, addrs))})"))
        code = h.code(f["rep"], f["size"])
        reason = None
        if names.most_common(1)[0][0] == "dtor_deleting":
            reason = "deleting destructor"
        elif first_call(f) == 0x00CF5E20:
            reason = "DTI new instance (first call is the allocator selector)"
        elif f["size"] <= 12 and code[:2] == bytes([0xC7, 0x01]) and code[6] == 0xE9:
            reason = "base destructor chain (store vtable, jump to base)"
        elif f["size"] == 21 and code[:6] == bytes.fromhex("8b44240485c0"):
            reason = "construct-if-memory wrapper"
        if reason:
            pl.update(addrs)
            why[reason] += len(addrs)
    pl.update([0x00CF5E20, 0x00DE0B80, 0x00DE0B90, 0x00CFE960, 0x00CFE9A0])
    why["MT allocator entry points"] += 5
    json.dump(sorted(pl), open(f"{NATIVE}/platform.json", "w"))
    print(len(pl), "platform functions", dict(why))


def show(n=20):
    fams = json.load(open(f"{INV}/exact_families.json"))
    db = sqlite3.connect(DB)
    shown = 0
    for f in fams:
        addrs = [m["addr"] for m in f["members"]]
        st = collections.Counter(r[0] for r in db.execute(
            f"SELECT status FROM functions WHERE address IN ({','.join(map(str, addrs))})"))
        if st.get("platform", 0) + st.get("verified", 0) >= len(addrs) // 2:
            continue
        ns, nm = db.execute("SELECT namespace, name FROM functions WHERE address=?", (f["rep"],)).fetchone()
        print(f"{len(addrs):5} x {f['size']:4}B rep 0x{f['rep']:08X} {ns}::{nm}  slots {f['slots']}")
        shown += 1
        if shown >= n:
            break


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "show"
    {"shapes": shapes, "exact": exact, "platform": platform,
     "show": lambda: show(int(sys.argv[2]) if len(sys.argv) > 2 else 20)}[cmd]()
