"""L172 P2c oracle: 0x00EA42E0 `model->refine_visibility_mask(culler, node,
mask)` (thiscall, ret 0xc). The culler tests 0x00FF0800 / 0x00FF0AD0 /
0x00FF0D20 and the two record fillers 0x00433B00 (64 bytes at ECX) and
0x005B19C0 (32 bytes at ECX) are recorded stubs. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00EA42E0, 0x103D
MODEL, NODE, BOUNDS = 0x20000000, 0x20001000, 0x20002000
SP = 0x0F008000
FF0800, FF0AD0, FF0D20, F433B00, F5B19C0 = 0x00FF0800, 0x00FF0AD0, 0x00FF0D20, 0x00433B00, 0x005B19C0


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def rf(r, special=0.0):
        if r.random() < special:
            return r.choice((0x7FC00000, 0x00000000, 0x80000000, 0x7F800000))
        return struct.unpack('<I', struct.pack('<f', r.uniform(-3, 3) * r.choice((1, 1, 0.01, 50))))[0]

    def verdict(o, regs, esp):
        return o.rng.choice((-1, 0, 1, 2, 5, 0x3F, 0x3F, 0x80000000, 7, 12))

    def filler(count_words):
        def f(o, regs, esp):
            r = o.rng
            o.poke(regs[1], b''.join(struct.pack('<I', rf(r, 0.02)) for _ in range(count_words)))
            return r.randrange(1 << 32)
        return f

    def reads(size):
        # the data the culler test reads through its first argument
        return lambda o, regs, esp: [(o.u32(esp + 4), size)]

    stubs = {FF0800: (2, verdict, 0, reads(16)), FF0AD0: (2, verdict, 0, reads(64)), FF0D20: (2, verdict, 0, reads(32)),
             F433B00: (1, filler(16)), F5B19C0: (1, filler(8))}
    oracle = Oracle(ENTRY, LENGTH, stubs, [(MODEL, 0x1000), (NODE, 0x1000), (BOUNDS, 0x1000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        mode = rng.choice((3, 3, 0, 1, 2, 4, 0x3F))
        extra = rng.choice((0, 0x40, 0x80, 0x100, 0x140, 0x200, 0xC0, 0x20))
        w(MODEL + 0x108, mode | extra | (rng.randrange(1 << 20) << 12 if rng.random() < .3 else 0))
        same = rng.random() < 0.25
        for off in range(0xB0, 0xF0, 4):
            w(MODEL + off, rf(rng, 0.01))
        if same:  # equal row lengths, so every comparison branch is reachable
            for off in range(0xC0, 0xE0, 4):
                w(MODEL + off, struct.unpack('<I', uc.mem_read(MODEL + 0xB0 + (off - 0xC0), 4))[0])
        w(NODE + 0x2C, BOUNDS)
        for off in range(0x10, 0x60, 4):
            w(BOUNDS + off, rf(rng, 0.01))
        mask = rng.choice((0x3F, 0x3F, 1, 3, 7, 0, 0x80000000, 0xFFFFFFFF, rng.randrange(64)))
        culler = 0x20003000 + 0x10 * rng.randrange(8)
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = MODEL
        cases.append(oracle.run(seed, regs, esp, [culler, NODE, mask]))
    finish(out, '0x00EA42E0', cases)


main()
