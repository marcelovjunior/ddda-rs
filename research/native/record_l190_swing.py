"""L190 oracle: 0x00FA5860 `uSwingModel::vf11(this, context)` (thiscall, ret 4).
The base draw 0x00EA3130 is a recorded stub that may also poke state slots of
the context (so every later branch is reached). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00FA5860, 0x272
THIS, CTX = 0x20000000, 0x20100000
SP = 0x0F008000
EA3130 = 0x00EA3130
SLOTS = (0x1DD0, 0x1DD4, 0x1C0, 0x1C4, 0x1C8, 0x13C, 0x128, 0x140, 0x12C, 0x144, 0x130)


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def base(o, regs, esp):
        r = o.rng
        if r.random() < 0.3:  # the base draw changes some slots
            for off in SLOTS:
                if r.random() < 0.3:
                    o.poke(CTX + off, struct.pack('<I', r.choice((0, 0, 1, r.randrange(1 << 32)))))
        return r.randrange(1 << 32)

    oracle = Oracle(ENTRY, LENGTH, {EA3130: (1, base)}, [(THIS, 0x1000), (CTX, 0x40000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        uc.mem_write(CTX, bytes(0x40000))
        uc.mem_write(CTX + 0x30000, bytes(rng.randrange(256) for _ in range(0x1000)))
        for off in SLOTS:
            w(CTX + off, rng.choice((0, 0, 0, 1, rng.randrange(1 << 32))))
        for off in (0x135, 0x136, 0x137):
            uc.mem_write(CTX + off, bytes([rng.choice((0, 0, 0, 1, rng.randrange(256)))]))
        uc.mem_write(CTX + 0x5C, bytes([rng.choice((0, 0x80, 0x80, 0xFF, 0x7F))]))
        w(CTX + 0x6C, rng.randrange(20))
        w(CTX + 0x150, rng.randrange(1 << 32))
        w(CTX + 0x154, rng.randrange(1 << 32))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [CTX]))
    finish(out, '0x00FA5860', cases)


main()
