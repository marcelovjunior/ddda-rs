"""Differential vectors for crates/ddnative (player_action, model).
Runs the ORIGINAL functions in Unicorn and writes inputs + outputs to
E:/ddda-re/notes/native/vectors/*.json (private, derived from the exe).
Run: E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/vectors_player_action.py
"""
import itertools, json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h

OUT = "E:/ddda-re/notes/native/vectors"
os.makedirs(OUT, exist_ok=True)
FLOATS = [0.0, -0.0, 1e-30, -1e-30, 1.0, -2.5, float("inf"), float("-inf")]
# Quiet NaNs only: Unicorn (QEMU softfloat) keeps a signalling NaN through
# fld/fstp, real x87 hardware quiets it (checked on the maintainer's CPU,
# 2026-10-10). Signalling NaNs are covered by ddnative's hardware test.
NANS = [0x7FC00001, 0xFFC00000]
f2b = lambda f: struct.unpack("<I", struct.pack("<f", f))[0]
FBITS = [f2b(f) for f in FLOATS] + NANS


def vf06():
    act, ply = h.SCRATCH, h.SCRATCH + 0x1000
    cases = []
    rnd = random.Random(7)
    for flags, dead, window, v2534, has_fx in itertools.product(
            [0, 1, 4, 5, 0xFFFFFFFA, 0xFFFFFFFF], [0, 1, 0x80], [0, 1], FBITS, [0, 1]):
        v2654, v4b20 = rnd.choice(FBITS), rnd.getrandbits(32)
        fx = rnd.getrandbits(32) | 1 if has_fx else 0
        mem = bytearray(0x1000 + 0x4C20)
        struct.pack_into("<I", mem, 0x58, ply)
        struct.pack_into("<I", mem, 0x1000 + 0x273C, flags)
        struct.pack_into("<I", mem, 0x1000 + 0x2534, v2534)
        struct.pack_into("<I", mem, 0x1000 + 0x2654, v2654)
        mem[0x1000 + 0x31EA] = dead
        struct.pack_into("<I", mem, 0x1000 + 0x4B20, v4b20)
        mem[0x1000 + 0x4BE1] = window
        struct.pack_into("<I", mem, 0x1000 + 0x4C18, fx)
        case = {"flags": flags, "v2534": v2534, "v2654": v2654, "dead": dead,
                "v4b20": v4b20, "window": window, "effect": fx}
        try:
            r = h.run(0x00AC5100, ecx=act, scratch=bytes(mem))
            out = r["scratch"]
            assert r["esp_delta"] == 0, "cdecl/thiscall ret without args"
            case["out"] = {"v4b20": struct.unpack_from("<I", out, 0x1000 + 0x4B20)[0],
                           "window": out[0x1000 + 0x4BE1],
                           "effect": struct.unpack_from("<I", out, 0x1000 + 0x4C18)[0],
                           "action60": struct.unpack_from("<I", out, 0x60)[0]}
        except RuntimeError as e:
            # the effect-release callee is not owned here: record that the
            # original reached it (and with which handle) instead
            assert "0x00427560" in str(e), e
            case["out"] = {"release": fx}
        cases.append(case)
    return cases


def vf85():
    cases = []
    for row in itertools.product(FBITS[:4] + NANS[:1], repeat=2):
        vals = [row[0], row[1], FBITS[4], NANS[0]]
        mem = bytearray(0x200)
        for i, b in enumerate(vals):
            struct.pack_into("<I", mem, 0xE0 + 4 * i, b)
        r = h.run(0x004C6B50, ecx=h.SCRATCH, args=[h.SCRATCH + 0x100], scratch=bytes(mem))
        assert r["esp_delta"] == 4
        cases.append({"row": vals, "out": list(struct.unpack_from("<4I", r["scratch"], 0x100))})
    return cases


for name, fn in [("player_action_vf06", vf06), ("model_vf85", vf85)]:
    cases = fn()
    json.dump(cases, open(f"{OUT}/{name}.json", "w"))
    print(name, len(cases), "cases,", sum("release" in c["out"] for c in cases if "out" in c), "reach the release callee")
