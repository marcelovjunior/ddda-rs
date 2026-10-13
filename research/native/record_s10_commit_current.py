"""L184 s10 oracle: 0x00FEEF20 `draw->commit_current()` (thiscall, plain
ret): unless draw flag 0x40 (+0x5c), commit the primitive state of the
currently selected bank entry through 0x00FEE320 (a recorded stub whose
AL varies) and mark the packet cursor. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00FEEF20, 0x54
DRAW, BANK = 0x20000000, 0x20010000
SP = 0x0F008000
COMMIT = 0x00FEE320


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def commit(o, regs, esp):
        r = o.rng
        return r.choice((0, 1, 0x100, 0xFF00, r.randrange(1 << 32)))

    oracle = Oracle(ENTRY, LENGTH, {COMMIT: (2, commit)}, [(DRAW, 0x1000), (BANK, 0x10000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        w(DRAW + 0x5C, rng.choice((0, 0x40, 0x8, 0x10, rng.randrange(1 << 32))))
        w(DRAW + 0x1CC, rng.randrange(1 << 32))
        w(DRAW + 0x20, BANK)
        w(DRAW + 0x48, rng.randrange(1 << 32))
        w(DRAW + 0x60, rng.randrange(1 << 32))
        uc.mem_write(DRAW + 0x169, bytes([rng.choice((0, 1, 0xFF, rng.randrange(256)))]))
        for k in range(0x1000):
            w(BANK + 8 * k, rng.randrange(1 << 32))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = DRAW
        cases.append(oracle.run(seed, regs, esp, []))
    finish(out, '0x00FEEF20', cases)


main()
