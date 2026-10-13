"""Target description for record_generic.py: 0x0078A730 (ESI = actor object,
void, plain RET). Callees: the CRT math functions 0x012F0906/0x012F0A55
(sinf/cosf style, XMM0 lane 0 in and out), 0x012F0D4C (XMM0 lane 0) and
0x012F1109 (two doubles in XMM0/XMM1, one double out in XMM0)."""
import struct

ENTRY, LENGTH = 0x0078A730, 0xD8B  # through RET at 0x0078B4BA
OBJ, MODEL, TABLE, RECS = 0x20000000, 0x21000000, 0x22000000, 0x23000000
OBJECTS = [(OBJ, 0x400), (MODEL, 0x400), (TABLE, 0x100), (RECS, 0x800)]
STUBS = {
    0x012F0906: (0, True, [(0, 0)], [(0, 0)]),
    0x012F0A55: (0, True, [(0, 0)], [(0, 0)]),
    0x012F0D4C: (0, True, [(0, 0)], [(0, 0)]),
    0x012F1109: (0, True, [(0, 0), (0, 1), (1, 0), (1, 1)], [(0, 0), (0, 1)]),
}


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def rand_float(rng, scale=1.0):
    r = rng.random()
    if r < 0.12:
        return 0.0
    if r < 0.2:
        return rng.choice((1.0, -1.0, 0.5, -0.5, 2.0))
    if r < 0.3:
        return rng.uniform(-0.05, 0.05)
    return rng.uniform(-3, 3) * scale


def stub(o, address, rng):
    if address == 0x012F1109:
        v = struct.unpack('<Q', struct.pack('<d', rng.uniform(-3.2, 3.2)))[0]
        return {'xmm': [(0, 0, v & 0xFFFFFFFF), (0, 1, v >> 32)]}
    return {'xmm': [(0, 0, f32(rng.choice((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-4, 4), 0.0, 1.0, -1.0))))]}


def fill(o, rng):
    uc = o.uc

    def put(base, off, fmt, v):
        uc.mem_write(base + off, struct.pack(fmt, v))
    sparsity = rng.choice((0, 0, 0.3, 0.6, 0.85))

    def value():
        return 0.0 if rng.random() < sparsity else rand_float(rng)
    for off in range(0, 0x240, 4):
        put(OBJ, off, '<f', value())
    for k in range(6):
        for off in range(0, 0xA0, 4):
            put(RECS, 0xA0 * k + off, '<f', value())
    for off in range(0, 0x300, 4):
        put(MODEL, off, '<f', rng.choice((0.016, 0.5, 1.0, 5.0, 100.0, rand_float(rng))))
    put(OBJ, 0x1C4, '<I', MODEL)
    put(OBJ, 0x1E0, '<I', rng.choice((0, 1, 2, 3, 0x100, 0x102)))
    for off in (0x71, 0x101, 0x191, 0x20C, 0x230, 0x231, 0x232):
        put(OBJ, off, '<B', rng.choice((0, 1)))
    base = rng.choice((RECS, RECS, RECS, 0))
    put(MODEL, 0x364, '<I', base)
    put(MODEL, 0x368, '<I', TABLE)
    for k in range(8):
        put(TABLE, k, '<B', rng.choice((0xFF, 0, 1, 2, 3, 4, 5)) if base else rng.choice((0xFF, 0)))
    return {6: OBJ}, []
