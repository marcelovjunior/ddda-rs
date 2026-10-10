"""Vectors for cPlAction::vf05 (0x00AC4F70). Private output.
The action's owner (+0x44) and player (+0x58) are the same object, as in
the game. The adjust getter ([player+0x3348] vtable +0x3C) is a scratch
fixture: mov eax,[esp+4]; fld dword [eax*4+TABLE]; ret 4 (quiet values
only: Unicorn does not quiet x87 signalling NaNs)."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
OUT = "E:/ddda-re/notes/native/vectors"; S = h.SCRATCH; rnd = random.Random(41)
P, ADJ, VT, CODE, TABLE = 0x1000, 0x100, 0x140, 0x180, 0x8000
f2b = lambda f: struct.unpack("<I", struct.pack("<f", f))[0]
FL = [0.0, -0.0, 1.0, -2.5, 120.5, 1e-38, float("inf")]
cases = []
for k in range(200):
    img = bytearray(rnd.getrandbits(8) for _ in range(0x9000))
    struct.pack_into("<I", img, 0x44, S + P); struct.pack_into("<I", img, 0x58, S + P)
    struct.pack_into("<I", img, P + 0x3348, S + ADJ)
    struct.pack_into("<I", img, ADJ, S + VT); struct.pack_into("<I", img, VT + 0x3C, S + CODE)
    img[CODE:CODE + 14] = bytes.fromhex("8b442404") + bytes([0xD9, 0x04, 0x85]) + struct.pack("<I", S + TABLE) + bytes.fromhex("c20400")
    d3, d4 = f2b(rnd.choice(FL)), f2b(rnd.choice(FL))
    struct.pack_into("<I", img, TABLE + 0xD3 * 4, d3); struct.pack_into("<I", img, TABLE + 0xD4 * 4, d4)
    vals = {"cat": rnd.choice([0, 2, 0xFFFFFFFD, 0xFFFFFFFF]), "flags": rnd.getrandbits(32), "saved": rnd.getrandbits(32),
            "pending": rnd.choice([0, 0x40, 0x400, 0x440, rnd.getrandbits(32)]), "window": rnd.choice([0, 0, 1, 0x80]),
            "w3558": rnd.choice([0xFFFFFFFF, 0x08000000, 0x08FFFFFF, 0x07000000, 0x11000000, 0x88000000, rnd.getrandbits(32)]),
            "v2654": f2b(rnd.choice(FL))}
    for key, off in [("cat", 0x206C), ("flags", 0x273C), ("saved", 0x2740), ("pending", 0x2E38), ("w3558", 0x3558), ("v2654", 0x2654)]:
        struct.pack_into("<I", img, P + off, vals[key])
    img[P + 0x4BE1] = vals["window"]
    gl = [f2b(rnd.choice(FL)) for _ in range(3)]
    ins = {k2: struct.unpack_from("<I", img, P + off)[0] for k2, off in
           [("w2370", 0x2370), ("w2374", 0x2374), ("status", 0x2E34)]}
    r = h.run(0x00AC4F70, ecx=S, scratch=bytes(img), allow={0x007F1890},
              poke={0x018CFE80: gl[0], 0x018CFE84: gl[1], 0x018CFE88: gl[2]})
    o = r["scratch"]; rd = lambda off: struct.unpack_from("<I", o, off)[0]
    cases.append({**vals, **ins, "d3": d3, "d4": d4, "globals": gl,
                  "out": {"b50": o[0x50], "a5c": rd(0x5C), "a60": rd(0x60), "a64": rd(0x64),
                          **{k2: rd(P + off) for k2, off in [("w2370", 0x2370), ("w2374", 0x2374), ("flags", 0x273C),
                             ("status", 0x2E34), ("w35e8", 0x35E8), ("w2fb0", 0x2FB0), ("w3068", 0x3068), ("f306c", 0x306C),
                             ("w3064", 0x3064), ("rank", 0x1DF0), ("v0", 0x1AC0), ("v1", 0x1AC4), ("v2", 0x1AC8), ("v3", 0x1ACC),
                             ("h1e68", 0x1E68), ("r31b4", 0x31B4), ("r1e6c", 0x1E6C)]},
                          "b49c0": o[P + 0x49C0], "b31b3": o[P + 0x31B3]}})
json.dump(cases, open(f"{OUT}/player_action_vf05.json", "w")); print(len(cases))
