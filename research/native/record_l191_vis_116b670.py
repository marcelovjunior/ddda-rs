"""L191 oracle: 0x0116B670 `Global::FUN_0116b670(arg)` (ECX = the owner `this`,
`ret 4`, arg = a request record). Its guest callees are recorded stubs: the
allocator-free helpers 0x00FB3790 and 0x001107CC0 / 0x001107E40 / 0x001107E30
(local lifted calls), the lifted 0x0057B8C0 (returns a vector pair), 0x001169A50
(three words), 0x011787C0 (six words; the local struct it reads is recorded as a
memory check), and an indirect method (nine words). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x0116B670, 0x220
THIS, ARG, PTR, BLOCK, VEC, ELEM, CONST, OBJ, EXTRA = (
    0x20000000, 0x20010000, 0x20020000, 0x20030000, 0x20040000, 0x20050000, 0x018D0000,
    0x20060000, 0x20070000)
SP = 0x0F008000
F0, F1, F2, F3, F4, F5, F6, F7 = (0xFB3790, 0x01107CC0, 0x0057B8C0, 0x01107E40, 0x01107E30,
                                  0x01169A50, 0x011787C0, 0x30000700)
CONST_ADDR = 0x018D0640
DATA_REGION = 0x0162ADB0
SENTINEL_TABLE = 0x01594890
OTHER = (0x0116AD60, 0x0116AE90)
FPTR = 0x30000800


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def plain(o, regs, esp):
        r = o.rng
        if r.random() < 0.3:  # the callee changes some words of the owner
            o.poke(THIS + 0x5F0 + 4 * r.randrange(2), struct.pack('<I', r.randrange(4)))
        return r.randrange(1 << 32)

    def flagged(o, regs, esp):
        # a flag-returning callee: low byte zero or not
        return plain(o, regs, esp) & 0xFFFFFF00 if o.rng.random() < 0.6 else plain(o, regs, esp)

    def small_count(o, regs, esp):
        # 0x01107E40 returns a count of loop iterations
        return o.rng.randrange(4)

    def vec_pointer(o, regs, esp):
        # 0x0057B8C0 returns a pair of vectors (the compare-and-mask test)
        r = o.rng
        # a poke records the write (a stub effect the port must replay)
        o.poke(VEC, b"".join(struct.pack("<f", r.choice((0.0, 1.0, -1.0, r.uniform(-4, 4)))) for _ in range(8)))
        return VEC

    def block_pointer(o, regs, esp):
        # 0x01107CC0 returns a pointer to a small block of words
        return BLOCK + 4 * o.rng.randrange(8)

    def v_checks(o, regs, esp):
        arg1 = o.u32(esp + 4)
        # words the original never wrote are garbage it reads: they become initial state
        o.garbage.update(a for a in range(arg1, arg1 + 0x40) if a not in o.written)
        return [(arg1, 0x40)]

    stubs = {
        F0: (0, plain), F1: (0, block_pointer), F2: (0, vec_pointer), F3: (0, small_count),
        F4: (1, plain), F5: (3, flagged), F6: (6, plain, 0, v_checks), F7: (9, plain),
        FPTR: (9, plain),
    }
    maps = [(THIS, 0x2000), (ARG, 0x1000), (PTR, 0x1000), (BLOCK, 0x1000), (VEC, 0x1000),
            (ELEM, 0x1000), (CONST, 0x1000), (OBJ, 0x1000), (EXTRA, 0x1000), (0x0F000000, 0x10000),
            (0x0162A000, 0x1000), (0x01594000, 0x1000)]
    oracle = Oracle(ENTRY, LENGTH, stubs, maps)
    uc = oracle.uc
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        for region, size in ((THIS, 0x2000), (ARG, 0x1000), (PTR, 0x1000), (BLOCK, 0x1000),
                             (ELEM, 0x1000), (OBJ, 0x1000), (EXTRA, 0x1000)):
            uc.mem_write(region, bytes(rng.randrange(256) for _ in range(size)))
        # the owner's fields
        uc.mem_write(THIS + 0x0C, bytes([rng.choice((0, 1, 1, 1))]))
        w(THIS + 0x5F0, rng.choice((0, 1, 2, 3)))
        w(THIS + 0xE44, rng.choice((0, 1, 2, 3)))
        w(THIS + 0x5FC, ELEM)
        w(THIS + 0xE50, ELEM + 0x100)
        for k in range(4):
            w(ELEM + 0x100 + 4 * k, ELEM + 0x200 + 0x40 * k)
            w(ELEM + 0x200 + 0x40 * k + 0xC, rng.choice((0, 1, rng.randrange(1, 1 << 32))))
            w(ELEM + 4 * k, rng.randrange(1 << 32))
        # the request record: +4 -> object X with a count at +0x10; +8, +0xC, +0x18
        x_obj = OBJ + 0x100
        w(ARG + 4, x_obj)
        w(x_obj + 0x10, rng.choice((0, 1, rng.randrange(1, 1 << 32))))
        w(ARG + 8, rng.choice((0, 1, rng.randrange(1, 1 << 32))))
        w(ARG + 0xC, FPTR)
        w(ARG + 0x18, rng.randrange(1 << 32))
        # the constant mask (a lane compare target)
        w(CONST_ADDR, 0xFFFFFFFF)
        for k in range(1, 4):
            w(CONST_ADDR + 4 * k, 0xFFFFFFFF if rng.random() < 0.85 else rng.randrange(1 << 32))
        # the block the first helper returns (its words are the state for the compares)
        for k in range(8):
            w(BLOCK + 4 * k, rng.choice((0, 1, 2, rng.randrange(1 << 32))))
        w(VEC + 0x0, 0)
        w(0x01594890, 0)
        w(DATA_REGION, rng.randrange(1 << 32))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        oracle.garbage = set()
        case = oracle.run(seed, regs, esp, [ARG])
        known = set(a for a, _ in case["initial"])
        for event in case["events"]:
            for a, b in event["before"].get("checks", []):
                if a in oracle.garbage and a not in known:
                    case["initial"].append((a, b))
                    known.add(a)
        case["initial"].sort()
        cases.append(case)
    finish(out, '0x0116B670', cases)


main()
