"""Pattern families: functions built from one repeated block whose only
variation is a field offset (not an in-image constant, so families.py
does not group them). Each recognizer parses the disassembly strictly and
returns the per-function parameters; ports live in crates/ddnative and
each function is replayed by its own vectors.

    python patterns.py drop_dead_refs     # list functions + offsets
    python patterns.py vectors            # record vectors for every match

Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import os
import random
import re
import sqlite3
import struct
import sys

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402

DB = "E:/ddda-re/notes/inventory/inventory.sqlite"
OUT = "E:/ddda-re/notes/native"
FIELD = re.compile(r"^dword ptr \[ecx \+ (0x[0-9a-f]+|\d+)\]$")


def _ins(addr, size):
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    return [(i.address, i.mnemonic, i.op_str) for i in md.disasm(h.code(addr, size), addr)]


def drop_dead_refs_offsets(addr, size):
    """Offsets X of a function made only of blocks
        mov eax, [ecx+X]; (test eax, eax | cmp eax, edx); je L
        mov eax, [eax+4]; and eax, 7; cmp eax, 2; je L; cmp eax, 1; je L
        mov dword ptr [ecx+X], (0 | edx)
    L:  ... ret
    with an optional leading `xor edx, edx`. Returns None if any
    instruction falls outside the pattern."""
    ins = _ins(addr, size)
    i, offs, zero_edx = 0, [], False
    if ins and ins[0][1] == "xor" and ins[0][2] == "edx, edx":
        zero_edx, i = True, 1
    while i < len(ins):
        if ins[i][1] == "ret" and ins[i][2] == "":
            return offs if offs and i == len(ins) - 1 else None
        blk = ins[i:i + 10]
        if len(blk) < 10:
            return None
        m = FIELD.match(blk[0][2].split(", ", 1)[1]) if blk[0][1] == "mov" and blk[0][2].startswith("eax, ") else None
        if not m:
            return None
        x = int(m.group(1), 0)
        null_ok = blk[1][1:] in (("test", "eax, eax"),) or (zero_edx and blk[1][1:] == ("cmp", "eax, edx"))
        expect = [("je",), ("mov", "eax, dword ptr [eax + 4]"), ("and", "eax, 7"), ("cmp", "eax, 2"),
                  ("je",), ("cmp", "eax, 1"), ("je",)]
        ok = null_ok and all(blk[2 + k][1] == e[0] and (len(e) == 1 or blk[2 + k][2] == e[1])
                             for k, e in enumerate(expect))
        store = blk[9]
        tgt = f"dword ptr [ecx + {hex(x)}], "
        ok = ok and store[1] == "mov" and store[2] in (tgt + "0", tgt + "edx" if zero_edx else None)
        if not ok:
            return None
        offs.append(x)
        i += 10
    return None


def scan():
    db = sqlite3.connect(DB)
    found = []
    for a, ns, nm, sz in db.execute("SELECT address, namespace, name, size FROM functions "
                                    "WHERE NOT thunk AND size BETWEEN 30 AND 400 AND address < 0x012E0000"):
        offs = drop_dead_refs_offsets(a, sz)
        if offs:
            found.append({"addr": a, "name": f"{ns}::{nm}", "offsets": offs})
    json.dump(found, open(f"{OUT}/patterns/drop_dead_refs.json", "w"))
    print(len(found), "functions;", sum(len(f["offsets"]) for f in found), "slots")
    return found


def vectors():
    found = json.load(open(f"{OUT}/patterns/drop_dead_refs.json"))
    rnd = random.Random(29)
    S = h.SCRATCH
    cases = []
    for f in found:
        span = max(f["offsets"]) + 4
        for _ in range(12):
            m = bytearray(rnd.getrandbits(8) for _ in range(span + 0x400))
            refs, states = [], []
            for k, x in enumerate(f["offsets"]):
                kind = rnd.choice(["null", "unit", "unit", "unit"])
                if kind == "null":
                    ref, st = 0, 0
                else:
                    ref = S + span + 0x40 * k + 0x10
                    st = (rnd.getrandbits(29) << 3) | rnd.randrange(8)
                    struct.pack_into("<I", m, ref - S + 4, st)
                struct.pack_into("<I", m, x, ref)
                refs.append(ref)
                states.append(st)
            r = h.run(f["addr"], ecx=S, scratch=bytes(m))
            assert r["esp_delta"] == 0
            cases.append({"addr": f["addr"], "offsets": f["offsets"], "refs": refs, "states": states,
                          "out": [struct.unpack_from("<I", r["scratch"], x)[0] for x in f["offsets"]]})
    json.dump(cases, open(f"{OUT}/vectors/drop_dead_refs.json", "w"))
    print(len(cases), "cases over", len(found), "functions")


if __name__ == "__main__":
    {"drop_dead_refs": scan, "vectors": vectors}[sys.argv[1]]()
