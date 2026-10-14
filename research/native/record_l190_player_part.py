"""L190 oracle: 0x00B89FE0 `FUN_00b89fe0(this, part, table, alt, level, a, b,
flag)` (stdcall, ret 0x20, ESI = the draw context): the player part draw
loop over the part's 0x30-byte pass records. Guest calls (0x00FF14D0,
0x00FED190, 0x00FED890, 0x00EA23F0, 0x00FED940, 0x00FEEF20, 0x00FF1920) are
recorded stubs that may change the context's flags, so every later branch is
reached. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00B89FE0, 0x786
CTX, DT, MODEL2, DESC, HEAP = 0x20100000, 0x20200000, 0x20300000, 0x20301000, 0x20400000
THIS, PART, SUB, RECS, TABLE, ALT, RES, JOINTS, OWNER, VIEW, SORT, DEFAULT = (
    0x20800000, 0x20900000, 0x20A00000, 0x20B00000, 0x20C00000, 0x20C10000, 0x20D00000, 0x20E00000, 0x20F00000, 0x21000000, 0x21100000, 0x21200000)
SP = 0x0F008000
FF14D0, FED190, FED890, EA23F0, FED940, FEEF20, FF1920 = 0x00FF14D0, 0x00FED190, 0x00FED890, 0x00EA23F0, 0x00FED940, 0x00FEEF20, 0x00FF1920
GROUPS = 12


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def common(o, regs, esp):
        r = o.rng
        if r.random() < 0.3:
            o.poke(CTX + 0x5C, struct.pack('<I', r.choice((0, 0x80, 0x100, 0x150, 0x1D0, 0x50, 0x10, 0x180, 0xD0, 0x2, 0x82))))
        if r.random() < 0.15:
            o.poke(CTX + 0x6C, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.1:
            o.poke(CTX + 0x30A0, struct.pack('<I', r.choice(o.values)))
        if r.random() < 0.1:
            o.poke(CTX + 0x1A88, struct.pack('<I', r.choice((0, DEFAULT, DEFAULT + 0x10, DEFAULT + 0x17, r.randrange(1 << 32)))))
        if r.random() < 0.1:
            o.poke(CTX + 0x1A8C, struct.pack('<I', r.choice((0, 1))))
        if r.random() < 0.2:
            o.poke(CTX + 0x164, struct.pack('<I', r.randrange(1 << 32)))
        return r.choice((0, 1, 5, r.randrange(1 << 32)))

    def view(o, regs, esp):
        return VIEW

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        base = HEAP + 0x1000 * r.randrange(8)
        o.poke(CTX + 0x24, struct.pack('<I', base))
        o.poke(CTX + 0x28, struct.pack('<I', base + size + r.choice((0, 0x100, 0x400))))
        return 0

    stubs = {FF14D0: (0, common), FED190: (2, common), FED890: (1, grow), EA23F0: (2, common), FED940: (0, view), FEEF20: (0, common),
             FF1920: (0, common)}
    oracle = Oracle(ENTRY, LENGTH, stubs,
                    [(CTX, 0x40000), (DT, 0x6000), (MODEL2, 0x1000), (DESC, 0x1000), (HEAP, 0x40000), (THIS, 0x2000), (PART, 0x1000), (SUB, 0x1000),
                     (RECS, 0x4000), (TABLE, 0x1000), (ALT, 0x1000), (RES, 0x2000), (JOINTS, 0x10000), (OWNER, 0x1000), (VIEW, 0x1000), (SORT, 0x4000),
                     (DEFAULT, 0x1000), (0x0F000000, 0x10000), (0x018D2000, 0x1000), (0x01531000, 0x1000)])
    oracle.uc.mem_write(0x015319B8, h.code(0x015319B8, 4))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def rf():
            return struct.unpack('<I', struct.pack('<f', rng.uniform(-100, 100)))[0]

        oracle.values = [rng.randrange(1 << 32) for _ in range(3)]
        uc.mem_write(CTX, bytes(0x40000))
        n = rng.choice((0, 1, 2, 3, 4, 6, 10))
        w(0x018D2CE8, rng.randrange(1 << 20))
        # this (the player model): visibility bitmap, owner, joints, the dirty count
        uc.mem_write(THIS + 0x110, bytes(rng.choice((0xFF, 0xFF, 0xF0, 0x0F, rng.randrange(256))) for _ in range(0x200)))
        w(THIS + 0xF0, OWNER)
        w(THIS + 0x154, rng.choice((0, 5, 200, 0x7FFF, 0xFFFFFFF0, rng.randrange(1 << 32))))
        w(THIS + 0x364, JOINTS)
        for off in (0x100, 0x104, 0x108, 0xA4):
            w(OWNER + off, rng.choice((rf(), 0x3F800000, 0, rf())))
        for k in range(0x10000 // 4):
            pass
        uc.mem_write(JOINTS, b''.join(struct.pack('<I', rf()) for _ in range(0x4000)))
        # part
        w(PART + 8, SUB)
        uc.mem_write(PART + 0x570, bytes([rng.choice((0, 1, 7))]))
        w(SUB + 0x70, RECS)
        w(SUB + 0x74, n)
        w(SUB + 0x9C, rng.choice((RES + 0x1000, RES + 0x1000, RES + 0x1100)))
        w(SUB + 0xA0, rng.choice((0, RES + 0x1200, RES + 0x1300)))
        for off in (0xA4, 0x100, 0x104, 0x108):
            w(SUB + off, rng.choice((rf(), 0x3F800000, rf(), 0x7FC00001)))
        # resources and tables
        for g in range(GROUPS):
            res = RES + 0x40 * g
            w(TABLE + 4 * g, rng.choice((res, res, res)))
            w(ALT + 4 * g, rng.choice((0, 0, RES + 0x40 * rng.randrange(GROUPS))))
            w(res + 0x14, rng.choice((0x10, 0xE, 0x10, 0xE, rng.randrange(0x20))) << 26 | rng.randrange(1 << 20))
        w(RES + 0x1004, rng.randrange(1 << 32))
        w(RES + 0x1104, rng.randrange(1 << 32))
        # records: dword0 flags mask, dword1 {bits 0..11 visibility index, 12..23 group, 24..31 level mask}
        for i in range(n):
            r = RECS + 0x30 * i
            w(r, rng.choice((0xFFFFFFFF, 0xFFFF, 1, 0x10000, rng.randrange(1 << 32))))
            g = rng.choice((0, 0, 1, 1, 2, rng.randrange(GROUPS)))
            w(r + 4, rng.choice((rng.randrange(0, 0x1000), rng.randrange(0, 64))) | g << 12 | rng.choice((0xFF, 0xFF, 0x03, 0x80, 0x55)) << 24)
            uc.mem_write(r + 8, bytes([rng.choice((1, 1, 1, 5, 0, 4, 0xFF))]))   # flags byte (p+4): bit0 visible, bit2 weighted
            uc.mem_write(r + 9, bytes([rng.randrange(256)]))                       # p+5
            uc.mem_write(r + 10, bytes([rng.randrange(256)]))                      # p+6
            w(r + 0x10, rng.randrange(1 << 32))                                    # p+0xc
            w(r + 0x18, rng.randrange(1 << 32))                                    # p+0x14
            w(r + 0x1C, rng.randrange(1 << 32))                                    # p+0x18
            w(r + 0x20, rng.randrange(1 << 32))                                    # p+0x1c
            w(r + 0x2C, SORT + 0x100 + 0x40 * i)                                   # p+0x28 weights
            w(SORT + 0x100 + 0x40 * i, rng.randrange(0, 40))                       # joint index
            for off in (0x10, 0x14, 0x18):
                w(SORT + 0x100 + 0x40 * i + off, rf())
        for off in (0x18, 0x28, 0x38, 0x8):
            w(VIEW + off, rf())
        # the context
        w(CTX + 0x20, DT)
        for off in (0x5268, 0x2EA8, 0x1890):
            w(DT + off, rng.choice(oracle.values + [DEFAULT]))
        w(CTX + 0x30A0, rng.choice(oracle.values)); w(CTX + 0x30A4, rng.randrange(1 << 32))
        w(CTX + 0x14C, rng.choice((0, RES + 0x1000, RES + 0x1100)))
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0xFFFF, 0xFFFF, 1, 0x8000, rng.randrange(1 << 16)))))
        uc.mem_write(CTX + 0x5C, struct.pack('<I', rng.choice((0x80, 0, 0xFF, 0x82, 0x100, 0x150, 0x1D0, 0x50, 0x180, 0x10, 0x90))))
        w(CTX + 0x6C, rng.randrange(20))
        w(CTX + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        w(CTX + 0x18, MODEL2); w(MODEL2 + 0xC44, DESC)
        uc.mem_write(DESC + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 5, 9, 40))))
        cursor = HEAP + 0x1000 * rng.randrange(8)
        w(CTX + 0x24, cursor)
        w(CTX + 0x28, cursor + rng.choice((0, 0x20, 0x40, 0x100, 0x300, 0x10000)))
        w(CTX + 0x1A80, rng.randrange(1 << 32)); w(CTX + 0x1A84, rng.randrange(1 << 32))
        w(CTX + 0x1A88, rng.choice((0, PART + 0x170, (PART + 0x170) | 3, DEFAULT, rng.randrange(1 << 32))))
        w(CTX + 0x1A8C, rng.choice((0, 0, 1)))
        w(CTX + 0x74, rng.randrange(1 << 32)); w(CTX + 0x164, rng.randrange(1 << 32)); w(CTX + 0x178, rng.randrange(1 << 32))
        for off in (0x194, 0x198, 0x19C, 0x1A0):
            w(CTX + off, rng.randrange(1 << 32))
        w(CTX + 0x44, SORT + 0x2000); w(CTX + 0x48, rng.randrange(0, 20))
        w(CTX + 0x124, rng.choice((0, RES + 0x1200, rng.randrange(1 << 32)))); uc.mem_write(CTX + 0x134, bytes([rng.randrange(256)])); w(CTX + 0x138, rng.randrange(1 << 32))
        esp = SP - 0x40 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[6] = CTX
        level = rng.choice((0xFF, 0xFF, 0x01, 0x80, 0x1FF, 0x55, 0))
        alt = rng.choice((ALT, ALT, 0))
        flag = rng.choice((0, 1, 1, 0x100, 0xFF01))
        args = [THIS, PART, TABLE, alt, level, rng.randrange(1 << 20), rng.randrange(1 << 20), flag]
        try:
            cases.append(oracle.run(seed, regs, esp, args))
        except Exception as e:
            from unicorn.x86_const import UC_X86_REG_EIP
            print('seed', seed, e, hex(oracle.uc.reg_read(UC_X86_REG_EIP)), [hex(r) for r in oracle.regs()])
            raise
    finish(out, '0x00B89FE0', cases)


main()
