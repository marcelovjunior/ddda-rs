"""L190 oracle: 0x00B8A770 (stdcall(this, context), ret 8; EDI = the part, a
list of {primary, alternate} state-block pointers). 0x00C85A00 is a recorded
stub (3 stack arguments). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00B8A770, 0x7A
THIS, PART, ARR1, ARR2, GLB, CTX = 0x20000000, 0x20010000, 0x20011000, 0x20012000, 0x20020000, 0x20100000
SP = 0x0F008000
C85A00, GLOBAL = 0x00C85A00, 0x018FA4BC


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def apply(o, regs, esp):
        return o.rng.randrange(1 << 32)

    oracle = Oracle(ENTRY, LENGTH, {C85A00: (3, apply)},
                    [(THIS, 0x6000), (PART, 0x1000), (ARR1, 0x1000), (ARR2, 0x1000), (GLB, 0xD0000), (CTX, 0x1000), (0x0F000000, 0x10000), (GLOBAL & ~0xFFF, 0x1000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        w(GLOBAL, GLB)
        w(GLB + 0xBE448, rng.choice((0, 1, 1, 2)))
        w(THIS + 0x22B8, rng.choice((0, 0x1234, 0x5678, rng.randrange(1 << 32))))
        w(THIS + 0x22BC, rng.choice((0, 1, 5)))
        w(THIS + 0x5820, rng.choice((0, 1, 2, 2, 3)))
        n = rng.choice((0, 1, 2, 3, 5, 8, 20))
        w(PART + 0x24, n)
        w(PART + 0x1C, ARR1)
        w(PART + 0x20, rng.choice((ARR2, ARR2, ARR2)))
        for k in range(24):
            w(ARR1 + 4 * k, rng.choice((0, 0, rng.randrange(1 << 32))))
            w(ARR2 + 4 * k, rng.choice((0, 0, rng.randrange(1 << 32))))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[7] = PART
        cases.append(oracle.run(seed, regs, esp, [THIS, CTX]))
    finish(out, '0x00B8A770', cases)


main()
