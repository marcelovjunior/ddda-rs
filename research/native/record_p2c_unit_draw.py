"""L172 P2c oracle: 0x00EA3130 `uBaseModel::vf11(this, context)` (thiscall,
ret 4): draw one model unit. Its helpers 0x00EA1EE0 (visibility mask) and
0x00EA1F40 (bounds) run as REAL original code in Unicorn; their guest calls
(0x00FF0780, 0x00FF0AD0, the chain end's virtual +0x30), 0x00EA1FD0, the
virtual +0x64 / +0x68, 0x00FED890 and the CRT memcpy are recorded stubs.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00EA3130, 0x34C
THIS, VT, L1, L2, VTL, CTX, DT, MODEL2, SKEL, HEAP = (
    0x20000000, 0x20001000, 0x20002000, 0x20003000, 0x20004000, 0x20100000, 0x20200000, 0x20300000, 0x20301000, 0x20400000)
SP = 0x0F008000
S64, S68, S30 = 0x30000064, 0x30000068, 0x30000030
FF0780, FF0AD0, EA1FD0, FED890, MEMCPY = 0x00FF0780, 0x00FF0AD0, 0x00EA1FD0, 0x00FED890, 0x012EFEA0


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def common(o, regs, esp):
        r = o.rng
        if r.random() < 0.2:
            o.poke(CTX + 0x5C, bytes([r.choice((0, 0x80, 0xFF))]))
        if r.random() < 0.15:
            o.poke(CTX + 0x6C, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.15:
            o.poke(CTX + 0x2E08, struct.pack('<I', r.choice(o.values)))
        if r.random() < 0.1:
            o.poke(CTX + 0x1D90, struct.pack('<I', r.choice((0, 0x1000))))
        return r.choice((0, 1, 5, r.randrange(1 << 32)))

    def mask(o, regs, esp):
        common(o, regs, esp)
        return o.rng.choice((-1, 0, 1, 3, 0x3F, 0x3F, 7, 5, 9, 0x80000000))

    def link(o, regs, esp):
        r = o.rng
        dst = o.u32(esp + 4)
        o.poke(dst, b''.join(struct.pack('<I', 0x3F800000 + r.randrange(1 << 20)) for _ in range(3)))
        return r.choice((0, 1, 0x100, 0xFF, r.randrange(1 << 32)))

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        base = HEAP + 0x1000 * r.randrange(4)
        o.poke(CTX + 0x24, struct.pack('<I', base))
        o.poke(CTX + 0x28, struct.pack('<I', base + size + r.choice((0, 0x100))))
        return 0

    def memcpy(o, regs, esp):
        dst, src, n = o.u32(esp + 4), o.u32(esp + 8), o.u32(esp + 12)
        o.remember(src, n)
        o.poke(dst, bytes(o.uc.mem_read(src, n)))
        return dst

    stubs = {FF0780: (1, mask), FF0AD0: (2, mask), EA1FD0: (1, common), S64: (1, common), S68: (6, common), S30: (1, link),
             FED890: (1, grow), MEMCPY: (0, memcpy, 3)}
    oracle = Oracle(ENTRY, LENGTH, stubs,
                    [(THIS, 0x1000), (VT, 0x1000), (L1, 0x1000), (L2, 0x1000), (VTL, 0x1000), (CTX, 0x40000), (DT, 0x4000), (MODEL2, 0x2000),
                     (HEAP, 0x20000), (0x0F000000, 0x10000), (0x01561000, 0x1000)],
                    extra_code=[(0x00EA1EE0, 0x5C), (0x00EA1F40, 0x8B)])
    oracle.uc.mem_write(0x01561738, h.code(0x01561738, 4))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def rf():
            return struct.unpack('<I', struct.pack('<f', rng.uniform(-100, 100)))[0]

        oracle.values = [rng.randrange(1 << 32) for _ in range(3)]
        w(THIS, VT)
        w(VT + 0x64, S64); w(VT + 0x68, S68)
        w(THIS + 0xF0, rng.choice((0x1234, 0x5000, 0x1234, 0)))
        w(THIS + 0xF8, rng.randrange(1 << 32))
        w(THIS + 0x108, rng.choice((0, 1, 2, 0x3F, 0x40000000, 0x40000001, 0x40000003, 0x40000002, 0x4000003F, rng.randrange(1 << 32))))
        w(THIS + 0x158, rng.choice((0x3F800000, rf(), rf(), rf(), 0, 0x80000000, 0x7FC00000)))
        for off in (0x160, 0x164, 0x168):
            w(THIS + off, rf())
        # the parent chain
        depth = rng.choice((0, 1, 2))
        if depth == 0:
            w(THIS + 0x30, 0)
        else:
            w(THIS + 0x30, L1)
            w(L1 + 0x30, L2 if depth == 2 else 0)
            w(L2 + 0x30, 0)
            end = L2 if depth == 2 else L1
            w(end, VTL)
            w(VTL + 0x30, S30)
            w(L1, VTL)
        # the context
        w(CTX + 0x20, DT)
        w(DT + 0x2C10, rng.choice(oracle.values)); w(DT + 0x2CC8, rng.choice(oracle.values))
        w(CTX + 0x2E08, rng.choice(oracle.values)); w(CTX + 0x2E0C, rng.randrange(1 << 32))
        w(CTX + 0x1D90, rng.choice((0, 0x1000))); w(CTX + 0x1D94, rng.choice((0, 5)))
        w(CTX + 0x2EB0, rng.choice(oracle.values)); w(CTX + 0x2EB4, rng.randrange(1 << 32))
        w(CTX + 0x1330, rng.randrange(1 << 32)); w(CTX + 0x1334, rng.randrange(1 << 32))
        uc.mem_write(CTX + 0x5C, struct.pack('<I', rng.choice((0x80, 0, 0xFF, 0x82))))
        w(CTX + 0x6C, rng.randrange(20))
        w(CTX + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        w(CTX + 0x18, MODEL2)
        w(MODEL2 + 0x89C, SKEL)
        uc.mem_write(SKEL + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 5, 9, 40))))
        w(SKEL + 0x24, HEAP + 0x10000)
        uc.mem_write(HEAP + 0x10000, bytes(rng.randrange(256) for _ in range(0x200)))
        cursor = HEAP + 0x1000 * rng.randrange(4)
        w(CTX + 0x24, cursor)
        w(CTX + 0x28, cursor + rng.choice((0, 8, 0x40, 0x100, 0x10000)))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [CTX], pre_remember=[]))
    finish(out, '0x00EA3130', cases)


main()
