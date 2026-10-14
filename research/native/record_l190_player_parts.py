"""L190 oracle: 0x00B89790 `uPlayerBase::vf208(this, context, record)`
(thiscall, ret 8): the player unit draw. Built from record_p2c_unit_draw.py:
0x00EA1EE0 / 0x00EA1F40 / 0x00EA1FD0 run as REAL original code; their guest
calls and the part calls (0x00FED940, virtual +0x64 / +0x198 / the parts'
+0x24, 0x00FFC350, 0x007FFCC0, 0x00B89FE0, 0x00B8A770, 0x00FED890, memcpy)
are recorded stubs. Floats are finite so NaN payload order is not tested.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00B89790, 0x84A
THIS, VT, L1, L2, VTL, CTX, DT, MODEL2, SKEL, HEAP = (
    0x20000000, 0x20010000, 0x20012000, 0x20013000, 0x20014000, 0x20100000, 0x20200000, 0x20300000, 0x20301000, 0x20400000)
SP = 0x0F008000
S64, S198, S24, S68X = 0x30000064, 0x30000198, 0x30000024, 0x30000068
S30, MGR3C = 0x30000030, 0x30000003
PASS, MGR, MVT, REC = 0x20500000, 0x20600000, 0x20601000, 0x20700000
PARTS, GLB, VIEW, MESH, PASSOBJ, PVT, EMBED_VT = 0x20800000, 0x20900000, 0x20A00000, 0x20B00000, 0x20C00000, 0x20D00000, 0x20D01000
FED480, FED8D0, FED5D0, FED890, MEMCPY, FED940 = 0x00FED480, 0x00FED8D0, 0x00FED5D0, 0x00FED890, 0x012EFEA0, 0x00FED940
FF0780, FF0AD0, FFC350, S7FFCC0, B89FE0, B8A770 = 0x00FF0780, 0x00FF0AD0, 0x00FFC350, 0x007FFCC0, 0x00B89FE0, 0x00B8A770


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

    def manager(o, regs, esp):
        common(o, regs, esp)
        return o.rng.choice((0, 1, 0x100, 0xFF01))

    def record(o, regs, esp):
        r = o.rng
        o.poke(REC, b''.join(struct.pack('<I', r.choice((0x3F800000, r.randrange(1 << 32) & 0x3FFFFFFF))) for _ in range(24)))
        return REC

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

    def view(o, regs, esp):
        return VIEW

    def drawable(o, regs, esp):
        common(o, regs, esp)
        return o.rng.choice((0, 1, 0x100, 0xFF01, 0x7F))

    stubs = {FF0780: (1, mask), FF0AD0: (2, mask), S30: (1, link), FED890: (1, grow), FED480: (1, common), MGR3C: (2, manager),
             FED8D0: (0, record), FED5D0: (1, common), MEMCPY: (0, memcpy, 3), FED940: (0, view),
             S64: (1, common), S198: (1, common), S24: (0, drawable), FFC350: (6, common), S7FFCC0: (6, common),
             B89FE0: (8, common), B8A770: (2, common)}
    oracle = Oracle(ENTRY, LENGTH, stubs,
                    [(THIS, 0x6000), (VT, 0x1000), (L1, 0x1000), (L2, 0x1000), (VTL, 0x1000), (CTX, 0x40000), (DT, 0x4000), (MODEL2, 0x2000),
                     (HEAP, 0x20000), (0x0F000000, 0x10000), (0x01561000, 0x1000), (PASS, 0x1000), (MGR, 0x1000), (MVT, 0x1000), (REC, 0x1000),
                     (0x018D2000, 0x1000), (PARTS, 0x4000), (GLB, 0xD0000), (VIEW, 0x1000), (MESH, 0x1000), (PASSOBJ, 0x1000), (PVT, 0x2000),
                     (0x01521000, 0x1000), (0x0161C000, 0x1000), (0x018FA000, 0x1000), (0x018D1000, 0x1000)],
                    extra_code=[(0x00EA1EE0, 0x5C), (0x00EA1F40, 0x8B), (0x00EA1FD0, 0x41D)])
    oracle.uc.mem_write(0x01521690, h.code(0x01521690, 4))
    oracle.uc.mem_write(0x0161C280, h.code(0x0161C280, 4))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def rf():
            return struct.unpack('<I', struct.pack('<f', rng.uniform(-100, 100)))[0]

        oracle.values = [rng.randrange(1 << 32) for _ in range(3)]
        limit = struct.unpack('<f', h.code(0x01521690, 4))[0]
        w(THIS, VT)
        w(VT + 0x64, S64); w(VT + 0x198, S198)
        w(THIS + 8, rng.choice((0, 4, 8, 0xC, 0x100, 0x300, 0x104, 0x10C, 0x304, rng.randrange(1 << 12))))
        w(CTX + 0x33A0, rng.choice(oracle.values)); w(CTX + 0x33A4, rng.randrange(1 << 32))
        w(CTX + 0x1D4, rng.choice((0, PASS, PASS)))
        uc.mem_write(PASS + 0x10, bytes([rng.choice((0, 0, 4, 1))]))
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0, 1, 0x20, 0x21, 0x101, 0x104, 0x105, 0x8, 0x200, 0x208, 0x209, 0x20D, rng.randrange(1 << 16)))))
        w(0x018D2C80, MGR); w(MGR, MVT); w(MVT + 0x3C, MGR3C)
        w(CTX + 0x1430, rng.randrange(1 << 32)); w(CTX + 0x1434, rng.randrange(1 << 32))
        w(MODEL2 + 0x91C, SKEL)
        for off in (0x31B0, 0x31C0, 0x31A8):
            w(DT + off, rng.choice(oracle.values))
        w(THIS + 0xF0, MESH)
        w(THIS + 0xF8, rng.randrange(1 << 32))
        w(THIS + 0x108, rng.choice((0, 1, 2, 0x3F, 0x40000000, 0x40000001, 0x40000003, 0x4000003F, rng.randrange(1 << 32), 1 << 9, 0xE00, 0x1200, 0x1E00)))
        # intensity: sometimes equal to the constant
        w(THIS + 0x158, rng.choice((0, 0, rf(), rf(), rf(), 0x80000000, struct.unpack('<I', struct.pack('<f', limit))[0])))
        for off in (0x160, 0x164, 0x168, 0x16C, 0x170):
            w(THIS + off, rf())
        depth_limit = rng.choice((rf(), 0x7F7FFFFF, 0))
        w(THIS + 0xD84, depth_limit)
        w(THIS + 0x154, rng.choice((0, 5, 100, 0x7FFF, 0xFFFFFFF0, rng.randrange(1 << 32))))
        w(THIS + 0x5820, rng.choice((0, 1, 2, 3)))
        # parts
        for base, n in ((0x5738, 8), (0x5798, 6)):
            for k in range(n):
                obj = PARTS + 0x100 * (k + (8 if base == 0x5798 else 0))
                w(THIS + base + 4 * k, rng.choice((0, obj, obj)))
        for k in range(4):
            w(THIS + 0x57D0 + 4 * k, rng.choice((0, PARTS + 0x100 * (16 + k))))
        w(THIS + 0x5788, rng.choice((0, 1, 3, 8, rng.randrange(0, 9))))
        w(THIS + 0x57B0, rng.choice((0, 1, 3, 6, rng.randrange(0, 7))))
        w(THIS + 0x578C, rng.randrange(256)); w(THIS + 0x57B4, rng.randrange(256))
        w(THIS + 0x5140, EMBED_VT)
        w(EMBED_VT + 0x24, S24)
        w(PVT + 0x24, S24)
        for k in range(20):
            obj = PARTS + 0x100 * k
            w(obj, PVT)
            w(obj + 0x1C, rng.randrange(1 << 32)); w(obj + 0x20, rng.randrange(1 << 32))
        w(CTX + 0x1410, rng.choice((0, PASSOBJ + rng.choice((0, 5, 8, 12)), PASSOBJ + 8)))
        w(PASSOBJ, rng.choice((0, 1)))
        for off in (0x184, 0x188, 0x18C):
            w(CTX + off, rf())
        for off in (8, 0x18, 0x28, 0x38):
            w(VIEW + off, rf())
        w(MESH + 0xE0, rng.choice((0, 5, 50, 100, rng.randrange(0, 300)))); w(MESH + 0xE4, rng.choice((0, 5, 100, 200, rng.randrange(0, 400))))
        w(0x018D1D20, GLB + 0xC8000); w(GLB + 0xC8000 + 0x38, rng.choice((0, 1, 2, 3)))
        w(0x018FA4BC, GLB); w(GLB + 0xBE448, rng.choice((0, 1, 2))); w(GLB + 0xB8844, rng.choice((0, 0, 0x2000000, 0x4000000, 0x6000000, 0x1000000)))
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
        try:
            cases.append(oracle.run(seed, regs, esp, [CTX, rng.choice((0, 1, 1, 0x100, 0xFF01))], pre_remember=[]))
        except Exception as e:
            from unicorn.x86_const import UC_X86_REG_EIP
            print('seed', seed, e, hex(uc.reg_read(UC_X86_REG_EIP)), [hex(r) for r in oracle.regs()])
            raise
    finish(out, '0x00B89790', cases)


main()
