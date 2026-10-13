"""L172 P2c oracle: 0x00EA1FD0 `model->select_shadow_state(context)` (thiscall,
ret 4). 0x00EA1EE0 (the visibility mask) runs as real original code; its
calls (0x00FF0780, 0x00FF0AD0), 0x00FED480, the manager's virtual +0x3c,
0x00FED890, 0x00FED8D0 (a 24-float record) and 0x00FED5D0 are recorded stubs.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00EA1FD0, 0x41D
THIS, CTX, DT, PASS, MODEL2, SKEL, HEAP, REC, MGR, MVT = (
    0x20000000, 0x20100000, 0x20200000, 0x20300000, 0x20400000, 0x20401000, 0x20500000, 0x20600000, 0x20700000, 0x20701000)
SP = 0x0F008000
FF0780, FF0AD0, FED480, FED890, FED8D0, FED5D0, MGR3C = 0x00FF0780, 0x00FF0AD0, 0x00FED480, 0x00FED890, 0x00FED8D0, 0x00FED5D0, 0x30000003


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def common(o, regs, esp):
        r = o.rng
        if r.random() < 0.2:
            o.poke(CTX + 0x5C, bytes([r.choice((0, 0x80, 0xFF))]))
        if r.random() < 0.15:
            o.poke(CTX + 0x6C, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.15:
            o.poke(CTX + 0x33A0, struct.pack('<I', r.choice(o.values)))
        return r.choice((0, 1, 5, r.randrange(1 << 32)))

    def mask(o, regs, esp):
        common(o, regs, esp)
        return o.rng.choice((-1, 0, 1, 3, 0x3F, 0x3F, 7, 5, 9, 0x80000000))

    def manager(o, regs, esp):
        common(o, regs, esp)
        return o.rng.choice((0, 1, 0x100, 0xFF01))

    def record(o, regs, esp):
        r = o.rng
        o.poke(REC, b''.join(struct.pack('<I', r.choice((0x3F800000, 0x7FC00001, r.randrange(1 << 32) & 0xFF7FFFFF))) for _ in range(24)))
        return REC

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        base = HEAP + 0x1000 * r.randrange(4)
        o.poke(CTX + 0x24, struct.pack('<I', base))
        o.poke(CTX + 0x28, struct.pack('<I', base + size + r.choice((0, 0x100))))
        return 0

    def block_alloc_fail(o, regs, esp):
        return common(o, regs, esp)

    stubs = {FF0780: (1, mask), FF0AD0: (2, mask), FED480: (1, common), MGR3C: (2, manager), FED890: (1, grow), FED8D0: (0, record),
             FED5D0: (1, common)}
    oracle = Oracle(ENTRY, LENGTH, stubs,
                    [(THIS, 0x1000), (CTX, 0x40000), (DT, 0x4000), (PASS, 0x1000), (MODEL2, 0x2000), (HEAP, 0x20000), (REC, 0x1000), (MGR, 0x1000),
                     (MVT, 0x1000), (0x0F000000, 0x10000), (0x018D2000, 0x1000)],
                    extra_code=[(0x00EA1EE0, 0x5C)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        oracle.values = [rng.randrange(1 << 32) for _ in range(4)]
        w(THIS + 8, rng.choice((0, 4, 8, 0xC, 0x100, 0x200, 0x300, 0x104, 0x10C, 0x304, 0x40C, rng.randrange(1 << 12))))
        w(THIS + 0x108, rng.choice((0, 1, 2, 0x3F, 0x15, rng.randrange(1 << 32))))
        for off in (0x160, 0x164, 0x168, 0x170):
            w(THIS + off, rng.randrange(1 << 32))
        w(CTX + 0x20, DT)
        for off in (0x31B0, 0x31C0, 0x31A8):
            w(DT + off, rng.choice(oracle.values))
        w(CTX + 0x33A0, rng.choice(oracle.values)); w(CTX + 0x33A4, rng.randrange(1 << 32))
        w(CTX + 0x1D4, rng.choice((0, PASS, PASS)))
        uc.mem_write(PASS + 0x10, bytes([rng.choice((0, 0, 4, 1))]))
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0, 1, 3, 0x100, 0x101, rng.randrange(1 << 16)))))
        w(0x018D2C80, MGR)
        w(MGR, MVT)
        w(MVT + 0x3C, MGR3C)
        w(CTX + 0x1430, rng.randrange(1 << 32)); w(CTX + 0x1434, rng.randrange(1 << 32))
        uc.mem_write(CTX + 0x5C, struct.pack('<I', rng.choice((0x80, 0, 0xFF, 0x82))))
        w(CTX + 0x6C, rng.randrange(20))
        w(CTX + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        w(CTX + 0x18, MODEL2)
        w(MODEL2 + 0x91C, SKEL)
        uc.mem_write(SKEL + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 24, 40))))
        cursor = HEAP + 0x1000 * rng.randrange(4)
        w(CTX + 0x24, cursor)
        w(CTX + 0x28, cursor + rng.choice((0, 8, 0x40, 0x100, 0x10000)))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [CTX]))
    finish(out, '0x00EA1FD0', cases)


main()
