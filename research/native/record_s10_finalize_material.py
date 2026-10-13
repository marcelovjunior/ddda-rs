"""L184 s10 oracle: 0x01062F20 `material->finalize(draw)` (thiscall, ret 4):
for each of the material's four group slots, give every bank slot of the
group a private copy of its constant block (arena allocation through
0x00FED890, CRT memcpy 0x012EFEA0). The original runs in Unicorn; the
allocator grow is a recorded stub, the memcpy a recorded stub that really
copies (memmove semantics; some sources overlap the destination).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x01062F20, 0x10B
MAT, X, T, GROUPS, ENTRIES, DRAW, BANK, DESC, SRCS, HEAP = (
    0x20000000, 0x20001000, 0x20002000, 0x20003000, 0x20004000, 0x20100000, 0x20200000, 0x20210000, 0x20300000, 0x20400000)
SP = 0x0F008000
GROW, MEMCPY = 0x00FED890, 0x012EFEA0


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        if True:
            base = HEAP + 0x1000 * r.randrange(4)
            o.poke(DRAW + 0x24, struct.pack('<I', base))
            o.poke(DRAW + 0x28, struct.pack('<I', base + size * r.choice((1, 2, 5)) + r.choice((0, 0x100))))
        else:
            o.poke(DRAW + 0x24, struct.pack('<I', 0))
            o.poke(DRAW + 0x28, struct.pack('<I', 0))
        return 0

    def memcpy(o, regs, esp):
        dst, src, n = o.u32(esp + 4), o.u32(esp + 8), o.u32(esp + 12)
        o.remember(src, n)
        data = bytes(o.uc.mem_read(src, n))
        o.poke(dst, data)
        return dst

    oracle = Oracle(ENTRY, LENGTH, {GROW: (1, grow), MEMCPY: (0, memcpy, 3)},
                    [(MAT, 0x1000), (X, 0x1000), (T, 0x1000), (GROUPS, 0x1000), (ENTRIES, 0x1000), (DRAW, 0x1000), (BANK, 0x1000), (DESC, 0x10000),
                     (SRCS, 0x10000), (HEAP, 0x20000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        w(MAT + 0x74, X)
        w(X + 0x10, T)
        groups = rng.randrange(1, 6)
        for k in range(groups):
            w(T + 4 + 4 * k, GROUPS + 0x100 * k)
            w(GROUPS + 0x100 * k + 4, (rng.choice((0, 1, 2, 3, 5)) << 18) | rng.choice((0, rng.randrange(1 << 18))))
            w(GROUPS + 0x100 * k + 8, ENTRIES + 0x40 * k)
            for j in range(8):
                w(ENTRIES + 0x40 * k + 4 * j, (rng.randrange(32)) | (rng.randrange(1 << 20) << 12 if rng.random() < .4 else 0))
        for s in range(4):
            idx = rng.randrange(groups)
            if rng.random() < 0.25:
                idx |= 0x8000
            uc.mem_write(MAT + 0x50 + 8 * s, struct.pack('<H', idx))
        w(DRAW + 0x18, BANK)
        for n in range(32):
            w(BANK + 4 * n, DESC + 0x40 * n)
            uc.mem_write(DESC + 0x40 * n + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 5, 9, 16, 40))))
        cursor = HEAP + 0x40 + 0x1000 * rng.randrange(4) + 0x10 * rng.randrange(4)
        w(DRAW + 0x24, cursor)
        w(DRAW + 0x28, cursor + rng.choice((0, 0x10, 0x60, 0x400, 0x10000)))
        uc.mem_write(SRCS, bytes(rng.randrange(256) for _ in range(0x2000)))
        uc.mem_write(HEAP, bytes(rng.randrange(256) for _ in range(0x2000)))
        for n in range(32):
            src = SRCS + 0x100 * n
            if rng.random() < 0.15:
                src = cursor - 4 * rng.randrange(0, 12)  # overlaps the block being written
            w(DRAW + 0x1F8 + 8 * n, src)
        w(DRAW + 0x154, rng.randrange(1 << 32) if rng.random() < .5 else 0)
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = MAT
        cases.append(oracle.run(seed, regs, esp, [DRAW]))
    finish(out, '0x01062F20', cases)


main()
