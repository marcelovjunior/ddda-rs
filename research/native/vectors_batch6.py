"""Vectors for the enemy-family action entry (16 classes). Private output.
The owner's virtual getDTI (vtable +0x10) is a 6-byte fixture in scratch
(`mov eax, dti; ret`): the callee, not the code under test."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
OUT = "E:/ddda-re/notes/native"; S = h.SCRATCH; rnd = random.Random(37)
fams = json.load(open('E:/ddda-re/notes/inventory/exact_families.json'))
fam = next(x for x in fams if x['rep'] == 0x004E0EB0)
cases = []
OWN, VT, CODE, DTI = 0x1000, 0x100, 0x180, 0x200
for m in fam['members']:
    a, class_global = m['addr'], m['params'][1]
    for _ in range(10):
        img = bytearray(rnd.getrandbits(8) for _ in range(0x4000))
        cat = rnd.choice([0, 2, 0x80, 0x82, 0xFFFFFF7F, 0xFFFFFFFF])
        pending = rnd.choice([0, 0x40, 0x400, 0x440, rnd.getrandbits(32)])
        struct.pack_into("<I", img, 0x44, S + OWN)
        struct.pack_into("<I", img, OWN, S + VT)
        struct.pack_into("<I", img, VT + 0x10, S + CODE)
        depth = rnd.choice([0, 1, 2, -1])  # -1: not of the class
        class_id = rnd.getrandbits(32) | 1
        chain = [rnd.getrandbits(32) | 1 for _ in range(3)]
        if depth >= 0: chain[depth] = class_id
        for k in range(3):
            struct.pack_into("<I", img, DTI + 0x20 * k + 4, chain[k] if chain[k] != class_id or depth == k else chain[k] ^ 2)
            struct.pack_into("<I", img, DTI + 0x20 * k + 0x10, S + DTI + 0x20 * (k + 1) if k < 2 else 0)
        img[CODE:CODE + 6] = bytes([0xB8]) + struct.pack("<I", S + DTI) + bytes([0xC3])
        for off, v in [(0x206C, cat), (0x2E38, pending)]:
            struct.pack_into("<I", img, OWN + off, v)
        r = h.run(a, ecx=S, scratch=bytes(img), poke={class_global: class_id}, allow={0x007F1890})
        o = r["scratch"]
        rd = lambda off: struct.unpack_from("<I", o, off)[0]
        cases.append({"addr": a, "owner": S + OWN, "cat": cat, "pending": pending, "is_class": depth >= 0,
                      "w70": rd(OWN + 0x2370), "in": {k: struct.unpack_from("<I", img, OWN + off)[0] for k, off in
                      [("w2370", 0x2370), ("w2374", 0x2374), ("flags", 0x273C), ("status", 0x2E34), ("o2ba0", 0x2BA0)]},
                      "out": {"b50": o[0x50], "o54": rd(0x54), "w58": rd(0x58), "w60": rd(0x60), "f64": rd(0x64),
                              "b6c": o[0x6C], "w70": rd(0x70), "o74": rd(0x74), "o2ba0": rd(OWN + 0x2BA0),
                              "status": rd(OWN + 0x2E34), "flags": rd(OWN + 0x273C)}})
json.dump(cases, open(f"{OUT}/vectors/enemy_action_enter.json", "w"))
json.dump({"representative": 0x004E0EB0, "members": [m['addr'] for m in fam['members']],
           "rule": "exact family; rel slot = cActionBase::vf05, const slot = class-id global"},
          open(f"{OUT}/families/enemy_action_enter.json", "w"))
print(len(cases))
