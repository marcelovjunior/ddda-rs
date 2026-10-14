"""L191 oracle: 0x00885DC0 `uObjModel::vf08()` (thiscall, plain ret; void).
Reads the model's flags at +0x1F9C / +0x1F98; when +0x1F9C is clear it sets it
and calls vtable slot 0x80; when +0x1F98 is 1 it calls vtable slots 0x168,
0x16C and 0x170, then tail-calls slot 0x174. The vtable is synthetic: its
slots point at four recorded stubs (no arguments). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00885DC0, 0x70
THIS, VTABLE = 0x20000000, 0x20010000
SP = 0x0F008000
STUBS = (0x30000100, 0x30000200, 0x30000300, 0x30000400)
SLOTS = (0x80, 0x168, 0x16C, 0x170, 0x174)


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def stub(o, regs, esp):
        r = o.rng
        if r.random() < 0.5:  # a method changes some of the flags
            o.poke(THIS + 0x1F98, struct.pack('<I', r.choice((0, 1, 2, r.randrange(1 << 32)))))
        return r.randrange(1 << 32)

    oracle = Oracle(ENTRY, LENGTH, {s: (0, stub) for s in STUBS},
                    [(THIS, 0x8000), (VTABLE, 0x1000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x8000)))
        w(THIS, VTABLE)
        uc.mem_write(VTABLE, bytes(rng.randrange(256) for _ in range(0x1000)))
        for slot in SLOTS:
            w(VTABLE + slot, rng.choice(STUBS))
        w(THIS + 0x1F9C, rng.choice((0, 0, 1, rng.randrange(1 << 32))))
        w(THIS + 0x1F98, rng.choice((1, 1, 0, 2, rng.randrange(1 << 32))))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, []))
    finish(out, '0x00885DC0', cases)


main()
