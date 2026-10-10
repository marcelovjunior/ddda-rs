"""Vectors for ddnative batch 4: 957 action constructors, uPlayerBase::vf85,
uEnemy::vf195. Private output."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h

OUT = "E:/ddda-re/notes/native/vectors"
S = h.SCRATCH
rnd = random.Random(17)
fam = json.load(open(f"{OUT}/ctor104_family.json"))
c = []
for a, ns, vt in (fam if not os.path.exists(f"{OUT}/action_ctor_family.json") else []):
    garbage = bytes(rnd.getrandbits(8) for _ in range(0x60))
    r = h.run(a, args=[S, rnd.getrandbits(32)], scratch=garbage)
    rn = h.run(a, args=[0, 0])
    assert r["eax"] == S and rn["eax"] == 0 and r["esp_delta"] == 8 and rn["esp_delta"] == 8
    c.append({"addr": a, "vtable": vt, "in": garbage.hex(), "out": r["scratch"][:0x60].hex()})
if c: json.dump(c, open(f"{OUT}/action_ctor_family.json", "w")); print("ctor family", len(c))

f2b = lambda f: struct.unpack("<I", struct.pack("<f", f))[0]
FL = [0.0, -0.0, 1.0, -3.25, 1e30, -1e-30, 0.5]
c = []
for map3 in [0, 1, 7, 0xFE, 0xFF]:
    for joints in [S + 0x2000, 0, (-(map3 * 0xA0)) & 0xFFFFFFFF]:
        if joints == 0 and map3 not in (0, 0xFF):
            continue  # null joint array with a real index: the original reads near null (a crash in game too)
        m = bytearray(0x10000)
        struct.pack_into("<I", m, 0x368, S + 0x400); m[0x400 + 3] = map3
        struct.pack_into("<I", m, 0x364, joints)
        origin = [f2b(rnd.choice(FL)) for _ in range(4)]
        struct.pack_into("<4I", m, 0xE0, *origin)
        row = [f2b(rnd.choice(FL)) for _ in range(4)]
        if joints == S + 0x2000:
            struct.pack_into("<4I", m, 0x2000 + map3 * 0xA0 + 0x40, *row)
        r = h.run(0x00B84300, ecx=S, args=[S + 0xF000], scratch=bytes(m))
        assert r["esp_delta"] == 4
        c.append({"map3": map3, "joints": joints, "origin": origin, "row": row,
                  "out": list(struct.unpack_from("<4I", r["scratch"], 0xF000))})
json.dump(c, open(f"{OUT}/player_vf85.json", "w")); print("player vf85", len(c))

c = []
for n in [0, 1, 3, 5]:
    for null_src, null_tgt in [(0, 0), (1, 0), (0, 1)]:
        m = bytearray(0x8000)
        srcs = []
        for i in range(n):
            vals = [f2b(rnd.uniform(-100, 100)) for _ in range(4)]
            t = f2b(rnd.choice([0.0, 1.0, 0.25, rnd.random(), -0.5, 2.0]))
            struct.pack_into("<4I", m, 0x1000 + i * 0x20, *vals)
            struct.pack_into("<I", m, 0x800 + i * 0x18 + 4, S + 0x1000 + i * 0x20)
            struct.pack_into("<I", m, 0x800 + i * 0x18 + 0x14, t)
            srcs.append({"values": vals, "t": t})
        struct.pack_into("<I", m, 0x5E84 - 0x5000, 0)  # placeholder (object below)
        obj = bytearray(0x6000)
        struct.pack_into("<I", obj, 0x5E84, 0 if null_src else S + 0x800)
        struct.pack_into("<I", obj, 0x22B8, 0 if null_tgt else S + 0x2000)
        struct.pack_into("<I", obj, 0x22BC, n)
        img = bytearray(0x10000); img[:0x8000] = m; img[0x8000:0x8000 + 0x6000] = obj
        for i in range(n):
            struct.pack_into("<I", img, 0x2000 + i * 0x230 + 8, 0x77777777)
        # sources/targets addresses are relative to S; object at S+0x8000
        r = h.run(0x00AAEF50, ecx=S + 0x8000, scratch=bytes(img))
        out = r["scratch"]
        c.append({"n": n, "null_src": null_src, "null_tgt": null_tgt, "srcs": srcs,
                  "tgts": [list(struct.unpack_from("<I", out, 0x2000 + i * 0x230 + 8)) +
                           list(struct.unpack_from("<2I", out, 0x2000 + i * 0x230 + 0x114)) for i in range(n)]})
json.dump(c, open(f"{OUT}/enemy_vf195.json", "w")); print("enemy vf195", len(c))
