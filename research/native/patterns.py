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



# --- motion_entry: base entry, then one owner motion request ------------
REG32 = ("eax", "ecx", "edx", "ebx", "edi")


def motion_entry_spec(addr, size, verified):
    """Spec of a function shaped
        push esi; mov esi, ecx; call BASE            (BASE verified)
        <argument building>                          (see below)
        mov <r1>, [esi+0x74]; mov <r2>, [<r1>]; mov <r3>, [<r2>+0x90]
        call <r3>; pop esi; ret
    Arguments: `sub esp, 0xc` reserves three float slots filled by
    `movss [esp+k], xmm0` (xmm0 from `movss xmm0, [CONST]` or `xorps`);
    then `push imm` / `push reg` (reg from `mov reg, [esi+X]`). Returns
    {"base", "args": [("const", u32) | ("field", X)] in call order} or None."""
    ins = _ins(addr, size)
    if [i[1:] for i in ins[:2]] != [("push", "esi"), ("mov", "esi, ecx")]:
        return None
    if ins[2][1] != "call" or not ins[2][2].startswith("0x"):
        return None
    base = int(ins[2][2], 16)
    if base not in verified:
        return None
    xmm0, regs, slots, pushes = None, {}, None, []
    owner_chain, vcall = [], None
    i = 3
    while i < len(ins):
        a, mn, op = ins[i]
        if mn == "movss" and op.startswith("xmm0, dword ptr [0x"):
            c = int(op.split("[")[1].rstrip("]"), 16)
            xmm0 = ("const", struct.unpack("<I", h.code(c, 4))[0])
        elif mn == "xorps" and op == "xmm0, xmm0":
            xmm0 = ("const", 0)
        elif mn == "sub" and op == "esp, 0xc" and slots is None:
            slots = [None, None, None]
        elif mn == "movss" and op.startswith("dword ptr [esp") and op.endswith(", xmm0") and slots is not None and xmm0:
            inner = op.split("[")[1].split("]")[0]
            k = 0 if inner == "esp" else int(inner.split("+ ")[1], 0)
            if k not in (0, 4, 8):
                return None
            slots[k // 4] = xmm0
        elif mn == "mov" and op.split(", ")[0] in REG32 and op.endswith("dword ptr [esi + 0x74]") and not owner_chain:
            owner_chain = [op.split(", ")[0]]
        elif mn == "mov" and owner_chain and len(owner_chain) == 1 and op == f"{op.split(', ')[0]}, dword ptr [{owner_chain[0]}]":
            owner_chain.append(op.split(", ")[0])
        elif mn == "mov" and owner_chain and len(owner_chain) == 2 and op == f"{op.split(', ')[0]}, dword ptr [{owner_chain[1]} + 0x90]":
            owner_chain.append(op.split(", ")[0])
        elif mn == "mov" and op.split(", ")[0] in REG32 and re.fullmatch(r"\w+, dword ptr \[esi \+ (0x[0-9a-f]+)\]", op):
            x = int(op.split("+ ")[1].rstrip("]"), 0)
            if 0x50 <= x < 0x7C:
                return None  # written by the base entry: keep the spec simple
            regs[op.split(", ")[0]] = ("field", x)
        elif mn == "push":
            if op.startswith("0x") or op.isdigit():
                pushes.append(("const", int(op, 0) & 0xFFFFFFFF))
            elif op in regs:
                pushes.append(regs[op])
            else:
                return None
        elif mn == "call" and len(owner_chain) == 3 and op == owner_chain[2]:
            vcall = i
            break
        else:
            return None
        i += 1
    if vcall is None or slots is None or None in slots:
        return None
    if [x[1:] for x in ins[vcall + 1:]] != [("pop", "esi"), ("ret", "")]:
        return None
    if owner_chain[0] == "ecx":
        pass  # `this` for the call is [esi+0x74]
    else:
        return None
    args = list(reversed(pushes)) + slots
    return {"base": base, "args": args} if len(args) == 6 else None


def scan_motion_entries():
    db = sqlite3.connect(DB)
    verified = {a for (a,) in db.execute("SELECT address FROM functions WHERE status='verified'")}
    found = []
    for a, ns, nm, sz in db.execute("SELECT address, namespace, name, size FROM functions "
                                    "WHERE NOT thunk AND status NOT IN ('verified','platform') "
                                    "AND size BETWEEN 30 AND 200 AND address < 0x012E0000"):
        spec = motion_entry_spec(a, sz, verified)
        if spec:
            found.append({"addr": a, "name": f"{ns}::{nm}", **spec})
    json.dump(found, open(f"{OUT}/patterns/motion_entry.json", "w"))
    from collections import Counter
    print(len(found), "motion entries; bases:", Counter(hex(f["base"]) for f in found).most_common(8))


if __name__ == "__main__":
    {"drop_dead_refs": scan, "vectors": vectors, "motion_entry": scan_motion_entries}[sys.argv[1]]()
