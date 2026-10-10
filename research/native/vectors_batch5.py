"""Vectors for ddnative batch 5: cPlActTired::vf09, uNpc::vf118,
cPlActWpnWandBase::vf10. Private output."""
import itertools, json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
OUT = "E:/ddda-re/notes/native/vectors"; S = h.SCRATCH; rnd = random.Random(31)
c = []
for msg in [0, 0x10, 0x11, 0x12, 0x13, 0xFFFFFFFF, 0x111]:
    m = bytearray(rnd.getrandbits(8) for _ in range(0x100))
    r = h.run(0x00ACC4A0, ecx=S, args=[msg, rnd.getrandbits(32), rnd.getrandbits(32), rnd.getrandbits(32)], scratch=bytes(m))
    assert r["esp_delta"] == 0x10
    c.append({"msg": msg, "b84": m[0x84], "b85": m[0x85], "ret": r["eax"], "o84": r["scratch"][0x84], "o85": r["scratch"][0x85]})
json.dump(c, open(f"{OUT}/tired_vf09.json", "w"))
c = []
for mark, fat, dead in itertools.product([0, 1, 2], [0, 1, 2], [0, 1, 2]):
    m = bytearray(0x6000); m[0x5851] = mark; m[0x31ED] = fat; m[0x31EA] = dead
    r = h.run(0x00BB2150, ecx=S, scratch=bytes(m))
    c.append({"mark": mark, "fat": fat, "dead": dead, "al": r["eax"] & 0xFF, "omark": r["scratch"][0x5851]})
json.dump(c, open(f"{OUT}/npc_vf118.json", "w"))
c = []
for _ in range(60):
    m = bytearray(rnd.getrandbits(8) for _ in range(0x800))
    case = {"refs": [], "states": []}
    for k, off in enumerate([0x74, 0x78]):
        ref = 0 if rnd.random() < 0.3 else S + 0x400 + 0x40 * k
        st = rnd.getrandbits(32)
        if ref: struct.pack_into("<I", m, ref - S + 4, st)
        struct.pack_into("<I", m, off, ref); case["refs"].append(ref); case["states"].append(st)
    t = 0 if rnd.random() < 0.25 else S + 0x600
    cnt = rnd.choice([0, 1, -1, 5, -0x80000000, 0x7FFFFFFF]); flag = rnd.choice([0, 1, 0x80])
    if t:
        struct.pack_into("<i", m, t - S + 0x50, cnt); m[t - S + 0x4C] = flag
    struct.pack_into("<I", m, 0x1E4, t)
    r = h.run(0x00B44FB0, ecx=S, scratch=bytes(m)); o = r["scratch"]
    case.update({"t": t, "cnt": cnt & 0xFFFFFFFF, "flag": flag,
                 "out": [struct.unpack_from("<I", o, x)[0] for x in (0x74, 0x78, 0x1E4)]})
    c.append(case)
json.dump(c, open(f"{OUT}/wand_base_vf10.json", "w")); print("ok")
