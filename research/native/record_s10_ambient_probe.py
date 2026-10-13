"""L184 s10 oracle: 0x00FE66A0 `scene->gather_ambient_probe(model,
light_mask, flags, position)` (thiscall, ret 0x10): the lighting gather of
a model draw. Four object ranges in one pointer list, x87 float returns
from virtual +0x4c (a native `FLD [ecx+0x400]; RET 4` stub: the hook only
records the call), 36-float contributions through virtual +0x4c(model,
position, buffer), the nested search 0x011787C0 (stub that rewrites the
accumulator and the hit byte through the context it is given), 0x00D13600
tests and the constants block (SSE spherical-harmonics scaling with the
game's own constants, x87 FLD/FSTP copy of one float).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00FE66A0, 0x184E
THIS, LISTARR, OBJS, MODEL, DT, G1, RES, MODEL2, SKEL, HEAP, VT, POS = (
    0x20000000, 0x20002000, 0x20100000, 0x20400000, 0x20500000, 0x20600000, 0x20700000, 0x20800000, 0x20801000, 0x20900000, 0x20A00000, 0x20B00000)
STRIDE = 0x800
SP = 0x0F008000
D13600, NESTED, GROW = 0x00D13600, 0x011787C0, 0x00FED890
X87STUB, VECSTUB = 0x30000100, 0x30000200
CONSTANTS = [(0x01406CE0, 4), (0x01406CEC, 4), (0x01406CF0, 4), (0x01406CF4, 4), (0x01408CA8, 8), (0x0142A8F0, 4), (0x01434024, 4),
             (0x0143A4E0, 8), (0x0161C280, 4), (0x0161CA58, 8), (0x0161D290, 8), (0x0162ADB0, 4)]


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def rfloat(rng, special=0.0):
        if rng.random() < special:
            return rng.choice((0x7FC00000, 0x7F800000, 0xFF800000, 0x80000000))
        return struct.unpack('<I', struct.pack('<f', rng.uniform(-4, 4) * rng.choice((1, 1, 0.01, 100))))[0]

    def d13600(o, regs, esp):
        return 0 if o.rng.random() < 0.25 else o.rng.randrange(1, 256) | (o.rng.randrange(1 << 24) << 8)

    def x87(o, regs, esp):
        # real code runs on: FLD dword [ecx+0x400]; RET 4
        return dict(st0=o.u32(regs[1] + 0x400))

    def vector(o, regs, esp):
        r = o.rng
        buffer = o.u32(esp + 4 + 8)
        o.poke(buffer, b''.join(struct.pack('<I', rfloat(r, 0.01)) for _ in range(36)))
        return 0 if r.random() < 0.25 else r.randrange(1, 256) | (r.randrange(1 << 24) << 8)

    def nested(o, regs, esp):
        r = o.rng
        context = o.u32(esp + 4 + 12)
        accumulator = o.u32(context + 0x10)
        was = o.uc.mem_read(context + 8, 1)[0]
        now = was if was else r.choice((0, 1, 1, 7))
        # the callback fills every word on its first contribution, else adds into some
        for k in range(36):
            if (not was and now) or (was and r.random() < 0.5) or (not was and not now and r.random() < 0.3):
                o.poke(accumulator + 4 * k, struct.pack('<I', rfloat(r)))
        o.poke(context + 8, bytes([now]))
        return r.randrange(1 << 32)

    def nested_reads(o, regs, esp):
        # what the nested search reads from the caller's frame
        context = o.u32(esp + 4 + 12)
        sphere = o.u32(esp + 4)
        acc = o.u32(context + 0x10)
        windows = [(context, 8), (context + 8, 1), (context + 0xC, 8), (sphere, 12), (sphere + 0x10, 16)]
        if o.uc.mem_read(context + 8, 1)[0]:
            windows.append((acc, 144))
        return windows

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        if r.random() < 0.3:
            o.poke(MODEL + 0x6c, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.8:
            o.poke(MODEL + 0x24, struct.pack('<I', HEAP + 0x1000 * r.randrange(4)))
            o.poke(MODEL + 0x28, struct.pack('<I', HEAP + 0x1000 * r.randrange(4) + size + r.randrange(2) * 0x10000))
        else:
            o.poke(MODEL + 0x24, struct.pack('<I', 0))
        return 0

    stubs = {D13600: (0, d13600, 2), X87STUB: (1, x87), VECSTUB: (3, vector), NESTED: (6, nested, 0, nested_reads), GROW: (1, grow)}
    pages = {(a & ~0xFFF) for a, _ in CONSTANTS}
    maps = [(THIS, 0x1000), (LISTARR, 0x1000), (OBJS, STRIDE * 40), (MODEL, 0x40000), (DT, 0x4000), (G1, 0x1000), (RES, 0x1000), (MODEL2, 0x2000),
            (HEAP, 0x20000), (VT, 0x1000), (POS, 0x1000), (0x0F000000, 0x10000), (0x018CF000, 0x4000)] + [(p, 0x1000) for p in sorted(pages)]
    oracle = Oracle(ENTRY, LENGTH, stubs, maps)
    for a, n in CONSTANTS:
        oracle.uc.mem_write(a, h.code(a, n))
    # the x87 stub is real code (the hook records and lets it run)
    oracle.uc.mem_write(X87STUB, bytes([0xD9, 0x81, 0x00, 0x04, 0x00, 0x00, 0xC2, 0x04, 0x00]))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        class_value = rng.randrange(4)
        uc.mem_write(MODEL + 0x168, bytes([class_value]))
        class_bit = 1 << class_value
        for a in (0x018CFE58, 0x018CFE5C):
            w(a, rfloat(rng))
        for a in (0x018CFE80, 0x018CFE84, 0x018CFE88):
            w(a, rfloat(rng))
        w(0x018D2C80, G1)
        w(G1 + 0x2C0, RES + 0x100)
        w(0x018D2CE8, rng.randrange(1 << 32))
        w(0x018D08F4, G1)
        w(G1 + 0x11C, rng.choice((rfloat(rng), 0x7FC00001)))
        w(RES + 0x100 + 0xC, rng.randrange(1 << 20))
        w(RES + 0x100 + 4, rng.randrange(1 << 32))
        # the scene: four disjoint ranges of one list
        counts = [rng.choice((0, 1, 2, 3, 4, 5)) for _ in range(4)]
        starts, pos = [], 0
        for n in counts:
            starts.append(pos)
            pos += n
        w(THIS + 0x20, LISTARR)
        w(THIS + 0x3C, starts[0]); w(THIS + 0x40, counts[0])
        w(THIS + 0x34, starts[1]); w(THIS + 0x38, counts[1])
        w(THIS + 0x2C, starts[2]); w(THIS + 0x30, counts[2])
        w(THIS + 0x44, starts[3]); w(THIS + 0x48, counts[3])
        uc.mem_write(THIS + 0xB5C, bytes([rng.choice((0, 1))]))
        w(THIS + 0xB60, rng.choice((0, 0x1234)))
        light_mask = rng.choice((0xFFFFFFFF, 0x1, 0x6, rng.randrange(1 << 32)))
        for i in range(pos):
            obj = OBJS + STRIDE * i
            w(LISTARR + 4 * i, obj)
            role = 0 if i < starts[1] else (1 if i < starts[2] else (2 if i < starts[3] else 3))
            w(obj, VT + 0x10 * role)
            kind = rng.choice((0x4802,) * 9 + (0x4803, 0x0002, 0x4800))
            classes = rng.choice((class_bit, class_bit, class_bit, 0x3FF, 0, class_bit << 1))
            w(obj + 4, (classes << 16) | kind | (rng.randrange(1 << 8) << 8 & 0xB000 if rng.random() < .2 else 0))
            w(obj + 0x30, rng.choice((2, 2, 2, 0, 3, 1)))
            uc.mem_write(obj + 0x32, struct.pack('<H', rng.choice((0, 0, 1, 5))))
            w(obj + 0x34, rng.choice((0xFFFFFFFF, 1, 2, 0, rng.randrange(1 << 32))))
            for off in (0x50, 0x54, 0x58, 0x230, 0x234):
                w(obj + off, rfloat(rng))
            w(obj + 0x238, rng.choice((0, RES)))
            w(obj + 0x400, rfloat(rng, 0.05))
            w(RES + 0xCC, rng.choice((0, RES + 0x100, RES + 0x180)))
        w(VT + 0x4C, X87STUB); w(VT + 0x10 + 0x4C, VECSTUB); w(VT + 0x20 + 0x4C, VECSTUB); w(VT + 0x30 + 0x4C, VECSTUB)
        # model
        w(MODEL + 0x20, DT)
        w(DT + 0x26F8, rng.randrange(3)); w(DT + 0x2700, rng.randrange(3))
        w(MODEL + 0x28F0, rng.randrange(3)); w(MODEL + 0x28F4, rng.randrange(1 << 32))
        w(MODEL + 0x1CB8, rng.choice((0, RES + 0x100, RES + 0x180)))
        w(MODEL + 0x1CBC, rng.choice((0, rng.randrange(1 << 32))))
        w(MODEL + 0x11C0, rng.randrange(1 << 32)); w(MODEL + 0x11C4, rng.randrange(1 << 32))
        w(MODEL + 0x18, MODEL2)
        w(MODEL2 + 0x7E4, SKEL)
        uc.mem_write(SKEL + 0x18, struct.pack('<H', rng.choice((0, 1, 8, 36, 40, rng.randrange(1, 60)))))
        uc.mem_write(MODEL + 0x5C, struct.pack('<I', rng.choice((0x80, 0, 0x80, 0xFF, 0x82))))
        w(MODEL + 0x6C, rng.randrange(20))
        w(MODEL + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        cursor = HEAP + 0x1000 * rng.randrange(4)
        w(MODEL + 0x24, cursor)
        w(MODEL + 0x28, cursor + rng.choice((0, 8, 0x40, 0x100, 0x10000)))
        for k in range(4):
            w(POS + 4 * k, rfloat(rng))
        flags = rng.randrange(8)
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [MODEL, light_mask, flags, POS]))
    finish(out, '0x00FE66A0', cases)


main()
