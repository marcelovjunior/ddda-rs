"""Vectors for ddnative action/particle ports (batch 2). Private output."""
import itertools, json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h

OUT = "E:/ddda-re/notes/native/vectors"
S = h.SCRATCH
rnd = random.Random(11)
u32 = lambda m, o: struct.unpack_from("<I", m, o)[0]


def action_enter():
    cases = []
    bits = [0, 0x40, 0x400, 0x440, 0x4000040, 0xFFFFFFFF, 0x100400]
    for cat, pending in itertools.product([0, 2, 0xFFFFFFFD, 0xFFFFFFFF], bits + [rnd.getrandbits(32) for _ in range(6)]):
        o = {"cat": cat, "w70": rnd.getrandbits(32), "w74": rnd.getrandbits(32),
             "flags": rnd.getrandbits(32), "status": rnd.getrandbits(32), "pending": pending}
        m = bytearray(0x1000 + 0x2E40)
        struct.pack_into("<I", m, 0x44, S + 0x1000); m[0x50] = 0xAA
        for off, k in [(0x206C, "cat"), (0x2370, "w70"), (0x2374, "w74"), (0x273C, "flags"), (0x2E34, "status"), (0x2E38, "pending")]:
            struct.pack_into("<I", m, 0x1000 + off, o[k])
        r = h.run(0x007F1890, ecx=S, scratch=bytes(m)); out = r["scratch"]
        assert r["esp_delta"] == 0
        o["out"] = {"b50": out[0x50], **{k: u32(out, 0x1000 + off) for off, k in
                    [(0x2370, "w70"), (0x2374, "w74"), (0x273C, "flags"), (0x2E34, "status"), (0x2E38, "pending")]}}
        cases.append(o)
    return cases


def demo_state():
    cases = []
    for has_owner, b0e, has_work in itertools.product([0, 1], [0, 1, 0x80], [0, 1]):
        work = [rnd.getrandbits(32) for _ in range(9)]
        m = bytearray(0x800)
        if has_owner:
            struct.pack_into("<I", m, 4, S + 0x100)
            m[0x100 + 0x0E] = b0e
            if has_work:
                struct.pack_into("<I", m, 0x100 + 0x40, S + 0x400)
        struct.pack_into("<9I", m, 0x400 + 0x10, *work)
        # guard words just outside the block must survive
        struct.pack_into("<I", m, 0x400 + 0x0C, 0xA5A5A5A5); struct.pack_into("<I", m, 0x400 + 0x34, 0x5A5A5A5A)
        r = h.run(0x00572860, ecx=S, scratch=bytes(m)); out = r["scratch"]
        cases.append({"owner": has_owner, "b0e": b0e, "work": has_work, "in": work, "ret": r["eax"],
                      "outwork": list(struct.unpack_from("<9I", out, 0x400 + 0x10)),
                      "guards": [u32(out, 0x400 + 0x0C), u32(out, 0x400 + 0x34)]})
    return cases


def particle():
    cases = []
    for flags, seed, arg in itertools.product([0, 0x10000000, 0xFFFFFFFF, rnd.getrandbits(32)],
                                              [0, 0xFFFF, 0x10000, 0xFFFFFFFF, rnd.getrandbits(32)], [0, 1, 0xFF]):
        m = bytearray(0x100)
        struct.pack_into("<4I", m, 0x3C, flags, 0, 0, 0)
        struct.pack_into("<I", m, 0x44, 0x11111111); struct.pack_into("<I", m, 0x48, 0x22222222)
        struct.pack_into("<I", m, 0x4C, seed)
        r8 = h.run(0x010FA570, ecx=S, scratch=bytes(m))["scratch"]
        r10 = h.run(0x010FA5B0, ecx=S, args=[arg | (rnd.getrandbits(24) << 8)], scratch=bytes(m))
        cases.append({"flags": flags, "seed": seed, "arg": arg,
                      "c44": u32(r8, 0x44), "c48": u32(r8, 0x48), "flags10": u32(r10["scratch"], 0x3C),
                      "esp10": r10["esp_delta"]})
    return cases


for name, fn in [("action_enter", action_enter), ("demo_state", demo_state), ("particle_vf08_vf10", particle)]:
    c = fn()
    json.dump(c, open(f"{OUT}/{name}.json", "w"))
    print(name, len(c))
