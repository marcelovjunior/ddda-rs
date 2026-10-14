"""L191 oracle: 0x01165B40 `Global::FUN_01165b40(index, a, flag)` (ECX = table
owner; stdcall-like `ret 0x10`; the probe search step). Its guest callees are
recorded stubs: a vtable method (slot 0x44 of the two child objects), the
lifted helpers 0x00DE5B80 / 0x00DE52A0 / 0x00DE5C10 / 0x00DE5BD0, the CRT-like
0x010A88A0 / 0x010A8BB0 (six stack words) and two indirect function pointers
from the state's table. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x01165B40, 0x200
STATE, OWNER, CHILD, TABLE, BASE, PBASE, EARR, ARG, VT, OBJ_A, OBJ_B, THISOBJ = (
    0x20000000, 0x20010000, 0x20020000, 0x20030000, 0x20040000, 0x20050000, 0x20060000,
    0x20070000, 0x20080000, 0x20090000, 0x200A0000, 0x200B0000)
SP = 0x0F008000
DE5B80, DE52A0, DE5C10, DE5BD0 = 0x00DE5B80, 0x00DE52A0, 0x00DE5C10, 0x00DE5BD0
CB_A, CB_B = 0x010A88A0, 0x010A8BB0
CALLBACK = 0x011658C0
VMETHOD = 0x30000100
FP1, FP2 = 0x30000500, 0x30000600


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def plain(o, regs, esp):
        r = o.rng
        if r.random() < 0.3:  # the callee changes the table's words
            o.poke(BASE + 4 * r.randrange(8), struct.pack('<I', r.randrange(1 << 32)))
        return r.randrange(1 << 32)

    def flagged(o, regs, esp):
        # a vtable method: the low byte of the result is the flag the caller tests
        return plain(o, regs, esp) & 0xFFFFFF00 if o.rng.random() < 0.8 else plain(o, regs, esp)

    stubs = {
        VMETHOD: (1, flagged),
        DE5B80: (0, plain), DE52A0: (1, plain), DE5C10: (4, plain), DE5BD0: (0, plain),
        CB_A: (6, plain), CB_B: (6, plain),
        FP1: (3, plain), FP2: (3, plain),
    }
    maps = [(STATE, 0x2000), (OWNER, 0x1000), (CHILD, 0x1000), (TABLE, 0x1000), (BASE, 0x1000),
            (PBASE, 0x1000), (EARR, 0x1000), (ARG, 0x1000), (VT, 0x1000), (OBJ_A, 0x1000),
            (OBJ_B, 0x1000), (THISOBJ, 0x2000), (0x0F000000, 0x10000)]
    oracle = Oracle(ENTRY, LENGTH, stubs, maps)
    uc = oracle.uc
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def small(choices=(0, 1, 2, 3)):
            return rng.choice(choices)

        for region, size in ((STATE, 0x2000), (OWNER, 0x1000), (CHILD, 0x1000), (TABLE, 0x1000),
                             (BASE, 0x1000), (PBASE, 0x1000), (EARR, 0x1000), (ARG, 0x1000),
                             (OBJ_A, 0x1000), (OBJ_B, 0x1000), (THISOBJ, 0x2000)):
            uc.mem_write(region, bytes(rng.randrange(256) for _ in range(size)))
        # the state: objects, slots, the indirect table
        w(STATE + 0x90, OBJ_A + 0x800)            # L18: an object with a vtable
        w(STATE + 0x98, CHILD)                    # B: flags at +0x30, pointer at +0x38
        w(STATE + 0xA0, OWNER)                    # A: +8 value, +0x54 table, +0x58 entries
        w(STATE + 0xB8, OBJ_B)                    # object with a vtable (C34 path)
        w(STATE + 0xA8, 0 if rng.random() < 0.5 else rng.randrange(1, 1 << 32))
        w(STATE + 0xE8, small())                  # L1C: a slot index
        w(STATE + 0x50, small((0, 1)))            # indirect table entry count
        w(STATE + 0x94, rng.randrange(1 << 32))
        w(STATE + 0xC0, rng.randrange(1 << 32))
        uc.mem_write(STATE + 0xB4, bytes([small((0, 1))]))
        w(STATE + 0, rng.randrange(1 << 32))      # the first word (vtable-like), compared to 0
        for off in (0x10, 0x28):
            w(STATE + off + 0x00, rng.choice((FP1, FP2, 0)))
        # the L18 and B8 objects share one vtable whose slot 0x44 is the method stub
        w(VT + 0x44, VMETHOD)
        w(OBJ_A + 0x800, VT)
        w(OBJ_B, VT)
        # the child object (B) and owner (A) and table
        w(CHILD + 0x30, small((0, 1, 2)))
        w(CHILD + 0x38, CHILD + 0x100)
        w(OWNER + 0x08, rng.randrange(1 << 32))
        w(OWNER + 0x54, TABLE)
        w(OWNER + 0x58, EARR)
        w(TABLE + 0x68, 2 if rng.random() < 0.7 else small((0, 1, 3)))
        w(TABLE + 0xB8, BASE)
        w(TABLE + 0xBC, PBASE)
        # the argument object with two small indices (its +4 and +8 words)
        a_obj = ARG + 0x100 if rng.random() < 0.8 else 0
        w(ARG + 0x104, small())
        w(ARG + 0x108, small())
        for k in range(4):
            w(THISOBJ + 0x0C0 + 4 * k, rng.randrange(1 << 32))
            w(THISOBJ + 0x124 + 4 * k, rng.randrange(1 << 32))
        # the record entries at the owner's array (E0 = array + 64 * index)
        for k in range(4):
            uc.mem_write(EARR + 64 * k + 0xC, bytes([rng.choice((1,) * 9 + (0,))]))
            uc.mem_write(EARR + 64 * k + 0x1F, bytes([rng.choice((0,) * 9 + (1,))]))
        for k in range(4):
            w(EARR + 64 * k + 0x0, rng.randrange(1 << 32))
        # the table's slots and the 80-byte records
        for k in range(16):
            w(BASE + 4 * k, rng.choice((0, rng.randrange(1 << 32))))
        uc.mem_write(PBASE, bytes(rng.randrange(256) for _ in range(0x1000)))
        idx = small()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THISOBJ
        cases.append(oracle.run(seed, regs, esp, [STATE, idx, a_obj, rng.randrange(256) if rng.random() < 0.5 else rng.randrange(2)]))
    finish(out, '0x01165B40', cases)


main()
