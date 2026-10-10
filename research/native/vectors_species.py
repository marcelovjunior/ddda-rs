"""Vectors for the species base entries (ddnative::species). Private output.
Each case stores the whole scratch image before and after the original ran
(the test compares every byte). getDTI is a scratch fixture; the species
owner is of the class unless the tail is null-safe and the case says not."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
N = "E:/ddda-re/notes/native"; S = h.SCRATCH; rnd = random.Random(53)
SPECIES = [  # addr, class global, kind, null-safe
    (0x00A4DB70, 0x0199F320, "em5800", False), (0x008FBDD0, 0x01997F64, "em0200", False),
    (0x00919DB0, 0x0199886C, "em0400", False), (0x0095A810, 0x0199A36C, "em0900", True),
    (0x00A2AE80, 0x0199EE38, "em5500t", True), (0x00A202E0, 0x0199E9D8, "em5500_00", True),
    (0x009C5890, 0x0199CEA4, "em5300", True), (0x009B4920, 0x0199C82C, "em5200", True),
    (0x009BD970, 0x0199CD44, "em5200", True)]
OWN, VT, DTIF, DTI, SIZE = 0x1000, 0x100, 0x180, 0x200, 0x9000
cases = []
for addr, glob, kind, null_safe in SPECIES:
    for k in range(8):
        img = bytearray(rnd.getrandbits(8) for _ in range(SIZE))
        struct.pack_into("<I", img, 0x44, S + OWN); struct.pack_into("<I", img, OWN, S + VT)
        struct.pack_into("<I", img, VT + 0x10, S + DTIF)
        img[DTIF:DTIF + 6] = bytes([0xB8]) + struct.pack("<I", S + DTI) + bytes([0xC3])
        is_class = (not null_safe) or k % 3 != 2
        cid = rnd.getrandbits(32) | 1
        struct.pack_into("<I", img, DTI + 4, cid if is_class else cid ^ 4); struct.pack_into("<I", img, DTI + 0x10, 0)
        struct.pack_into("<I", img, OWN + 0x206C, rnd.choice([0, 2, 0x80, 0x82, 0xFFFFFFFF]))
        struct.pack_into("<I", img, OWN + 0x2E38, rnd.choice([0, 0x40, 0x400, rnd.getrandbits(32)]))
        if kind.startswith("em5500"):
            sub = rnd.choice([0, S + 0x8800])
            struct.pack_into("<I", img, OWN + 0x7324, sub)
            if sub: img[0x8800 + 0x2D] = rnd.choice([0x41, 0xAF, 0x00, 0x42])
        if kind == "em5300":
            n = rnd.choice([0, 1, 3])
            struct.pack_into("<I", img, OWN + 0x60E8, n); struct.pack_into("<I", img, OWN + 0x60F4, S + 0x8A00)
            for j in range(n): struct.pack_into("<I", img, 0x8A00 + 4 * j, S + 0x8B00 + 0x100 * j)
        g = [rnd.getrandbits(32) & 0x7F7FFFFF for _ in range(3)]
        r = h.run(addr, ecx=S, scratch=bytes(img), allow={0x007F1890},
                  poke={glob: cid, 0x018CFE80: g[0], 0x018CFE84: g[1], 0x018CFE88: g[2]})
        cases.append({"addr": addr, "kind": kind, "is_class": is_class, "globals": g,
                      "in": img.hex(), "out": r["scratch"][:SIZE].hex()})
json.dump(cases, open(f"{N}/vectors/species.json", "w")); print(len(cases))
