"""Vectors for ddnative batch 3 (character/player/npc/action). Private output."""
import itertools, json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h

OUT = "E:/ddda-re/notes/native/vectors"
S = h.SCRATCH
rnd = random.Random(13)
u32 = lambda m, o: struct.unpack_from("<I", m, o)[0]
f2b = lambda f: struct.unpack("<I", struct.pack("<f", f))[0]
cases = {}

# uCharacterBase::vf111, NPC branch through the ORIGINAL uNpc::vf180 (0x00BB4730)
c = []
VT, BODY, INTEL = 0x200, 0x1000, 0x8000
for cat, a2360, f273c, prev, intel, flag in itertools.product(
        [0, 8, 0xF7, 0xFF], [0, 0x4000, 0xFFFFBFFF], [0, 0x10, 0xFFFFFFEF], [0, 1, 2], [0, 1], [0, 0x80, 0x7F]):
    m = bytearray(0xA000)
    struct.pack_into("<I", m, BODY, S + VT)                     # vtable pointer
    struct.pack_into("<I", m, VT + 0x2D0, 0x00BB4730)           # vf180 = original uNpc::vf180
    struct.pack_into("<I", m, BODY + 0x5854, S + INTEL if intel else 0)
    m[INTEL + 0x5C0] = flag if intel else 0
    m[BODY + 0x2738] = 0 if intel else flag                     # own flag word when no intel
    m[BODY + 0x206D] = cat
    struct.pack_into("<I", m, BODY + 0x2360, a2360); struct.pack_into("<I", m, BODY + 0x273C, f273c)
    m[BODY + 0x2EA0] = prev; m[BODY + 0x2EA1] = 0xAA; m[BODY + 0x2EA2] = 0xBB; m[BODY + 0x2EA3] = 0xCC
    r = h.run(0x00850100, ecx=S + BODY, scratch=bytes(m)); o = r["scratch"]
    c.append({"cat": cat, "a2360": a2360, "f273c": f273c, "prev": prev, "flag": flag,
              "out": list(o[BODY + 0x2EA0:BODY + 0x2EA4])})
cases["character_vf111"] = c

# uNpc::vf180
c = []
for intel in [0, 0x10, S + 0x100, 0xFFFFF000]:
    m = bytearray(0x3000); struct.pack_into("<I", m, 0x5854 - 0x3000 + 0x3000 if False else 0, 0)
    m = bytearray(0x6000); struct.pack_into("<I", m, 0x5854, intel)
    r = h.run(0x00BB4730, ecx=S, scratch=bytes(m))
    c.append({"intel": intel, "this": S, "ret": r["eax"]})
cases["npc_vf180"] = c

# uPlayerBase::vf18, vf207, vf187, vf100; cActionBase::vf08
c = []
for st in [0, 1, 2, 3, 0xFFFFFFFF, 0x101, 0x201]:
    m = bytearray(0x6000); struct.pack_into("<I", m, 0x5824, st)
    c.append({"state": st, "al": h.run(0x00B88AB0, ecx=S, scratch=bytes(m))["eax"] & 0xFF})
cases["player_vf18"] = c
c = []
for w, a in itertools.product([0, 1, 0x80000000], [0, 0x1000000, 0xFEFFFFFF, 0xFFFFFFFF]):
    m = bytearray(0x3000); struct.pack_into("<I", m, 0x2104, w); struct.pack_into("<I", m, 0x2360, a)
    c.append({"w": w, "a": a, "al": h.run(0x00B53190, ecx=S, scratch=bytes(m))["eax"] & 0xFF})
cases["player_vf207"] = c
c = []
GLOBAL = 0x018FA4BC
for state, val in itertools.product([0x259, 0x258, 0, 0x10259], [f2b(1.5), f2b(-0.0), 0x7FC00001]):
    m = bytearray(0x3000); struct.pack_into("<I", m, 0x207C, val)
    struct.pack_into("<I", m, 0x2800 + 0x34, state)            # fake scene object in scratch
    # point the exe's global at it: write into the mapped .data image of this run
    r = h.run(0x00B6AE40, ecx=S, args=[rnd.getrandbits(32)], scratch=bytes(m), poke={GLOBAL: S + 0x2800})
    c.append({"state": state, "val": val, "out": u32(r["scratch"], 0x207C), "esp": r["esp_delta"]})
cases["player_vf187"] = c
c = []
for row, arg in itertools.product([0, 1, 2, 3], [0, 1, 7, 0x80, 0xFF]):
    m = bytearray(0x6000); struct.pack_into("<I", m, 0x5720, row)
    r = h.run(0x00B531B0, ecx=S, args=[arg], scratch=bytes(m))
    c.append({"row": row, "arg": arg, "al": r["eax"] & 0xFF, "table": h.code(0x014F2AF0 + (row << 8) + arg, 1)[0]})
cases["player_vf100"] = c
c = []
for cat in [0, 2, 0xFFFFFFFD, 0xFFFFFFFF]:
    m = bytearray(0x4000); struct.pack_into("<I", m, 0x44, S + 0x1000)
    struct.pack_into("<I", m, 0x1000 + 0x206C, cat); struct.pack_into("<I", m, 0x1000 + 0x2E34, 0xDEADBEEF)
    r = h.run(0x0089EFE0, ecx=S, scratch=bytes(m))
    c.append({"cat": cat, "status": u32(r["scratch"], 0x1000 + 0x2E34)})
cases["action_vf08"] = c

for k, v in cases.items():
    json.dump(v, open(f"{OUT}/{k}.json", "w")); print(k, len(v))
