"""L191 oracle: 0x00886530 `uObjModel::vf09()` (thiscall, plain ret; void), the
per-model draw prep. Its guest callees are recorded stubs (vtable slots of the
model and of two child objects, the lifted helpers 0x0076D6B0, 0x004C6B50,
0x008871A0, ...). The double-precision atan2 0x012F1109 runs as the original's
code in Unicorn (its tables at 0x0144C640.. are mapped from the image), so the
port's native atan2 is checked bit for bit. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402
import s10_oracle as so  # noqa: E402

ENTRY, LENGTH = 0x00886530, 0x350
THIS, VT, VT2, OBJ, OBJ2, GLOBAL, PTR, ATAN2 = (0x20000000, 0x20010000, 0x20011000, 0x20020000,
                                                0x20030000, 0x018D0000, 0x20050000, 0x012F1109)
SHAPE = 0x0150929C
TABLES = (0x0144C000, 0x2000)
SP = 0x0F008000
STUB = {
    'query': 0x008871A0, 'eaa': 0x00EA1B20, 'lifted_a': 0x0076D6B0, 'lifted_b': 0x0076C540,
    'h1': 0x0076B300, 'h2': 0x0076B660, 'free': 0x005B0570, 'h3': 0x00887360, 'h4': 0x00888750,
    'h5': 0x00EA1CC0, 'h6': 0x00821750, 'bounds': 0x004C6B50,
}
VSLOTS = {0x3C: 0x30000100, 0x17C: 0x30000110, 0x180: 0x30000120, 0x184: 0x30000130}
OBJSLOT = 0x30000140
ARGS = {'query': 0, 'eaa': 0, 'lifted_a': 0, 'lifted_b': 5, 'h1': 1, 'h2': 1, 'free': 1, 'h3': 1,
        'h4': 0, 'h5': 1, 'h6': 0, 'bounds': 1}


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def f32_round(v):
    return struct.unpack('<f', struct.pack('<f', v))[0]


def tie_coordinate(k):
    """A float x with x * x == k in single precision (k = the threshold), or None."""
    import math
    if not (k > 0 and math.isfinite(k)):
        return None
    bits = struct.unpack('<I', struct.pack('<f', math.sqrt(k)))[0]
    for d in range(-8, 9):
        x = struct.unpack('<f', struct.pack('<I', bits + d))[0]
        if f32_round(x * x) == k:
            return x
    return None


SHAPE_VALUE = struct.unpack('<f', h.code(0x0150929C, 4))[0]


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def plain(o, regs, esp):
        r = o.rng
        if r.random() < 0.3:  # the callee changes a flag word of the model
            o.poke(THIS + 0x1F98, struct.pack('<I', r.choice((0, 1, 2, r.randrange(1 << 32)))))
        return r.randrange(1 << 32)

    def bounds(o, regs, esp):
        # a pointer to three floats (the object's bounds), random small values;
        # in a third of the cases the squared distance ties the threshold exactly
        r = o.rng
        tie = tie_coordinate(SHAPE_VALUE) if r.random() < 0.33 else None
        if tie is not None:
            o.uc.mem_write(PTR, struct.pack('<fff', tie, 0.0, 0.0))
            o.uc.mem_write(OBJ2 + 0x940, bytes(12))
            return PTR
        for k in range(3):
            o.uc.mem_write(PTR + 4 * k, struct.pack('<f', r.uniform(-8, 8)))
        return PTR

    def query(o, regs, esp):
        r = o.rng
        return r.randrange(1 << 32) & 0xFFFFFF00 if r.random() < 0.8 else r.randrange(1 << 32)

    stubs = {}
    for name, addr in STUB.items():
        stubs[addr] = (ARGS[name], bounds if name == "bounds" else query if name == "query" else plain)
    for addr in VSLOTS.values():
        stubs[addr] = (0, plain)
    stubs[OBJSLOT] = (0, plain)
    maps = [(THIS, 0x10000), (VT, 0x1000), (VT2, 0x1000), (OBJ, 0x1000), (OBJ2, 0x3000),
            (GLOBAL, 0x1000), (PTR, 0x1000), (0x0F000000, 0x10000), (SHAPE & ~0xFFF, 0x1000),
            (TABLES[0], TABLES[1])]
    oracle = Oracle(ENTRY, LENGTH, stubs, maps, extra_code=((ATAN2, 0x400),))
    uc = oracle.uc
    # the image's constants and tables (read-only data of the original)
    uc.mem_write(SHAPE, h.code(SHAPE, 4))
    uc.mem_write(TABLES[0], h.code(TABLES[0], TABLES[1]))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def fl(a, v):
            w(a, f32(v))

        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x10000)))
        for addr in (THIS + 0xE70, THIS + 0xE74, THIS + 0xE78, THIS + 0x40, THIS + 0x44, THIS + 0x48,
                     THIS + 0xE80, THIS + 0xE84, THIS + 0xE88, THIS + 0xE0, THIS + 0xE4, THIS + 0xE8):
            fl(addr, rng.uniform(-8, 8))
        w(THIS, VT)
        uc.mem_write(VT, bytes(rng.randrange(256) for _ in range(0x1000)))
        for slot, target in VSLOTS.items():
            w(VT + slot, target)
        w(THIS + 0x1EF8, OBJ if rng.random() < 0.8 else 0)
        w(THIS + 0x1EFC, OBJ2 if rng.random() < 0.8 else 0)
        for o in (OBJ, OBJ2):
            w(o, VT2)
        uc.mem_write(VT2, bytes(rng.randrange(256) for _ in range(0x1000)))
        w(VT2 + 0x24, OBJSLOT)
        w(GLOBAL + 0xB6C, OBJ2 + 0x800)
        w(OBJ2 + 0x800 + 0x34, OBJ2 + 0x900)
        for k in range(3):
            fl(OBJ2 + 0x900 + 0x40 + 4 * k, rng.uniform(-8, 8))
        uc.mem_write(THIS + 0x20FC, bytes([rng.choice((0, 0, 0, 0, 0, 0, 1))]))
        w(THIS + 0x2398, rng.choice((0, 0x10000, 0x80000, 0x100000, 0x200000, 0x400000, 0x4, rng.randrange(1 << 32))))
        w(THIS + 0x1F98, rng.choice((1, 1, 1, 1, 0, 2)))
        w(THIS + 0x1F80, rng.choice((0, rng.randrange(1, 1 << 32))))
        uc.mem_write(THIS + 0x206D, bytes([rng.randrange(256)]))
        uc.mem_write(THIS + 0x206E, bytes([rng.randrange(256)]))
        uc.mem_write(THIS + 0x1F76, bytes([rng.choice((0, 1, rng.randrange(256)))]))
        uc.mem_write(THIS + 0xEC0, bytes([rng.randrange(256)]))
        uc.mem_write(THIS + 0x20E4, bytes([rng.randrange(256)]))
        w(THIS + 0x1F40, rng.choice((0, 1)))
        w(THIS + 0x1F44, rng.choice((0, 1)))
        w(THIS + 0x23A4, rng.choice((0, 1, 2, 3, 4)))
        w(THIS + 0x23B4, rng.choice((0, 1)))
        uc.mem_write(THIS + 0x23B8, bytes([rng.randrange(256)]))
        w(THIS + 0x23B0, OBJ2 + 0x1000)
        for k in range(4):
            w(OBJ2 + 0x1000 + 4 * k, rng.choice((0, 0x20000000 + k, rng.randrange(1 << 32))))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        try:
            cases.append(oracle.run(seed, regs, esp, []))
        except Exception as e:
            print("fail", seed, hex(uc.reg_read(so.x.UC_X86_REG_EIP)), e)
            raise
    finish(out, '0x00886530', cases)


main()
