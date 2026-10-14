"""L191 oracle: 0x00886880 `uObjModel::vf11(arg)` (thiscall, ret 4; void).
Flag 0x40000000 of the model at +0x108 follows a float compare of +0x2514
with the constant at 0x015319B8; the state at +0x1F1C may call 0x007A6640
(one argument, ret 4), then tail-calls 0x00FFE5C0 (one argument, ret 4).
Both callees are recorded stubs. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00886880, 0x60
THIS, CONST = 0x20000000, 0x01531000
SP = 0x0F008000
CALL, TAIL = 0x007A6640, 0x00FFE5C0


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def callee(o, regs, esp):
        r = o.rng
        if r.random() < 0.5:  # the callee writes some of the state
            o.poke(THIS + 0x1F1C + 0x24, struct.pack('<I', r.choice((0, 1, r.randrange(1 << 32)))))
        return r.randrange(1 << 32)

    stubs = {CALL: (1, callee), TAIL: (1, callee)}
    oracle = Oracle(ENTRY, LENGTH, stubs, [(THIS, 0x4000), (CONST, 0x1000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x4000)))
        level = rng.choice((0.0, 1.0, -1.0, 0.5, rng.uniform(-2, 2), float('nan')))
        w(THIS + 0x2514, struct.unpack('<I', struct.pack('<f', level))[0])
        threshold = rng.choice((0.0, 0.5, 1.0, rng.uniform(-2, 2)))
        w(CONST + 0x9B8, struct.unpack('<I', struct.pack('<f', threshold))[0])
        base = THIS + 0x1F1C
        w(base + 0x24, rng.choice((0, 1, rng.randrange(1 << 32))))
        w(base + 0x28, rng.choice((0, 1, rng.randrange(1 << 32))))
        uc.mem_write(base + 0x5A, bytes([rng.choice((0, 0, 1, rng.randrange(256)))]))
        w(THIS + 0x108, rng.randrange(1 << 32))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [rng.randrange(1 << 32)]))
    finish(out, '0x00886880', cases)


main()
