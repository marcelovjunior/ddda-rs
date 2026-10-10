"""Vectors for cEm0100Action::vf05 (0x008D4840). Private output. The owner is
always a goblin (the original writes through the is-a result without a null
check); getDTI is a scratch fixture as in vectors_batch6.py."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
OUT = "E:/ddda-re/notes/native/vectors"; S = h.SCRATCH; rnd = random.Random(43)
OWN, VT, CODE, DTI = 0x1000, 0x100, 0x180, 0x200
f2b = lambda f: struct.unpack("<I", struct.pack("<f", f))[0]
FL = [0.0, -0.0, 1.0, -2.5, 3e9, 1e-38]
cases = []
for k in range(80):
    img = bytearray(rnd.getrandbits(8) for _ in range(0x7000))
    cat = rnd.choice([0, 2, 0x80, 0x82, 0xFFFFFFFF]); pending = rnd.choice([0, 0x40, 0x400, rnd.getrandbits(32)])
    struct.pack_into("<I", img, 0x44, S + OWN); struct.pack_into("<I", img, OWN, S + VT)
    struct.pack_into("<I", img, VT + 0x10, S + CODE)
    depth = rnd.choice([0, 1, 2]); cid = rnd.getrandbits(32) | 1
    for j in range(3):
        struct.pack_into("<I", img, DTI + 0x20 * j + 4, cid if j == depth else (rnd.getrandbits(32) | 1) ^ (cid & 0))
        struct.pack_into("<I", img, DTI + 0x20 * j + 0x10, S + DTI + 0x20 * (j + 1) if j < 2 else 0)
    img[CODE:CODE + 6] = bytes([0xB8]) + struct.pack("<I", S + DTI) + bytes([0xC3])
    struct.pack_into("<I", img, OWN + 0x206C, cat); struct.pack_into("<I", img, OWN + 0x2E38, pending)
    gl = [f2b(rnd.choice(FL)) for _ in range(3)]
    rd_in = lambda off: struct.unpack_from("<I", img, OWN + off)[0]
    ins = {key: rd_in(off) for key, off in [("w2370", 0x2370), ("w2374", 0x2374), ("flags", 0x273C), ("status", 0x2E34),
                                            ("o2ba0", 0x2BA0), ("v257c", 0x257C)]}
    r = h.run(0x008D4840, ecx=S, scratch=bytes(img), allow={0x007F1890},
              poke={0x01996BC4: cid, 0x018CFE80: gl[0], 0x018CFE84: gl[1], 0x018CFE88: gl[2]})
    o = r["scratch"]; rd = lambda off: struct.unpack_from("<I", o, off)[0]
    cases.append({"owner": S + OWN, "cat": cat, "pending": pending, "globals": gl, "in": ins,
                  "out": {"b50": o[0x50], "o54": rd(0x54), "w58": rd(0x58), "w60": rd(0x60), "f64": rd(0x64),
                          "b6c": o[0x6C], "w70": rd(0x70), "o74": rd(0x74), "o2ba0": rd(OWN + 0x2BA0),
                          "status": rd(OWN + 0x2E34), "flags": rd(OWN + 0x273C),
                          "vel": [rd(OWN + 0x2530 + 4 * i) for i in range(4)], "acc": [rd(OWN + 0x2540 + 4 * i) for i in range(4)],
                          "w2574": struct.unpack_from("<H", o, OWN + 0x2574)[0], "v2578": rd(OWN + 0x2578),
                          "v257c": rd(OWN + 0x257C), "b5c7c": o[OWN + 0x5C7C]}})
json.dump(cases, open(f"{OUT}/goblin_action_vf05.json", "w")); print(len(cases))
