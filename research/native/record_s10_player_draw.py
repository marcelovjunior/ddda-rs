"""L184 s10 oracle: 0x00B8A7F0 `uPlayerBase::vf25(this, model)` (thiscall,
ret 4): prepare the player's model for drawing. Branches: plain model path
(this+0x56d0 bit 2 clear), skin-resource binding (4 state slots with undo
log), joint matrix palette (SSE float math, 12 outputs per joint), lighting
resource, constants block allocation and copy (CRT memcpy 0x012EFEA0 is
stubbed with the real copy and recorded; x87 FLD/FSTP of four floats runs in
Unicorn). Seven guest callees are stubs; they also rewrite the model's undo
flag/count and state slots so the original's re-reads show.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00B8A7F0, 0xC33
THIS, MODEL, DT, G1, SKIN, RES, JOINTS, MATS, SLOTS = 0x20000000, 0x20100000, 0x20200000, 0x20300000, 0x20400000, 0x20500000, 0x20600000, 0x20700000, 0x20800000
MODEL2, SKEL, SRC, HEAP, VT = 0x20900000, 0x20901000, 0x20A00000, 0x20B00000, 0x20C00000
SP = 0x0F008000
GENERATION, CONSUMERS, DEVICE = 0x018D2CE8, 0x018D2C80, 0x018D08F4
STUB198 = 0x30000198
CALLEES = {0x00FFC9F0: 1, 0x00EA5B50: 1, 0x00FE46C0: 1, 0x00BFC7D0: 1, 0x00FE4780: 2, 0x00FED890: 1, STUB198: 1}
MEMCPY = 0x012EFEA0
SPECIAL = (0x7FC00000, 0x7F800000, 0xFF800000, 0x00000001, 0x80000000, 0x7F7FFFFF, 0xFFC00000)


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    state = {}

    def common(o, regs, esp):
        r = o.rng
        model = MODEL
        if r.random() < 0.3:
            o.poke(model + 0x5c, bytes([r.choice((0, 0x80, 0xff, 0x7f))]))
        if r.random() < 0.2:
            o.poke(model + 0x6c, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.2:
            o.poke(model + 0x30a0, struct.pack('<I', r.choice(state['values'])))
        if r.random() < 0.2:
            o.poke(model + 0x2840, struct.pack('<I', r.choice(state['values'])))
        if r.random() < 0.2:
            o.poke(model + 0x1ad8, struct.pack('<I', r.choice(state['resources'])))
        if r.random() < 0.15:
            o.poke(model + 0x154, struct.pack('<I', r.randrange(1 << 32)))
        return r.choice((0, 1, r.randrange(1 << 32)))

    def get_slots(o, regs, esp):
        common(o, regs, esp)
        return 0 if o.rng.random() < 0.3 else SLOTS

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        common(o, regs, esp)
        if r.random() < 0.8:
            o.poke(MODEL + 0x24, struct.pack('<I', HEAP + 0x1000 * r.randrange(4)))
            o.poke(MODEL + 0x28, struct.pack('<I', HEAP + 0x1000 * r.randrange(4) + size + r.randrange(2) * 0x10000))
        else:
            o.poke(MODEL + 0x24, struct.pack('<I', 0))
        return 0

    def memcpy(o, regs, esp):
        dst, src, n = o.u32(esp + 4), o.u32(esp + 8), o.u32(esp + 12)
        o.remember(src, n)
        data = bytes(o.uc.mem_read(src, n))
        o.poke(dst, data)
        return dst

    stubs = {a: (n, common) for a, n in CALLEES.items()}
    stubs[0x00FE46C0] = (1, get_slots)
    stubs[0x00FED890] = (1, grow)
    stubs[MEMCPY] = (0, memcpy, 3)
    oracle = Oracle(ENTRY, LENGTH, stubs, [(THIS, 0x8000), (MODEL, 0x40000), (DT, 0x4000), (G1, 0x40000), (SKIN, 0x1000), (RES, 0x10000), (JOINTS, 0x2000),
                                           (MATS, 0x2000), (SLOTS, 0x2000), (MODEL2, 0x2000), (SRC, 0x1000), (HEAP, 0x20000), (VT, 0x1000),
                                           (0x0F000000, 0x10000), (0x018D0000, 0x4000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        resources = [0] + [RES + 0x40 * k for k in range(8)]
        state['resources'] = resources
        values = [rng.randrange(1 << 32) for _ in range(3)]
        state['values'] = values
        # the device table
        for off in (0x2ed8, 0x2ee0, 0x2ee8, 0x2648, 0x2668, 0x2650):
            w(DT + off, rng.choice(values))
        w(MODEL + 0x20, DT)
        w(MODEL + 0x18, MODEL2)
        w(MODEL2 + 0x8C4, SKEL)
        words = rng.choice((0, 1, 3, 4, 5, rng.randrange(1, 40)))
        uc.mem_write(SKEL + 0x18, struct.pack('<H', words))
        w(SKEL + 0x24, SRC if rng.random() < 0.9 else HEAP + 0x1000)
        uc.mem_write(SRC, bytes(rng.randrange(256) for _ in range(0x100)))
        w(THIS, VT)
        w(VT + 0x198, STUB198)
        a_path = rng.random() < 0.3
        uc.mem_write(THIS + 0x56D0, bytes([rng.choice((0, 1, 3)) if a_path else rng.choice((4, 5, 7, 0xff))]))
        uc.mem_write(MODEL + 0x16A, struct.pack('<H', rng.choice((0, 0x30C, 1, 4, 0x100, rng.randrange(1 << 16)))))
        skinned = rng.random() < 0.4
        w(THIS + 0xF0, SKIN if skinned or rng.random() < 0.3 else 0)
        w(SKIN + 0x220, rng.choice((0, 1, 5)) if skinned else rng.choice((0, 0, 1)))
        uc.mem_write(MODEL + 0x168, bytes([rng.choice((1, 2, 3, 3, 9))]))
        n = rng.choice((0, 1, 2, 3, 4, 6))
        w(THIS + 0x4D34, n)
        for i in range(8):
            uc.mem_write(THIS + 0x4D40 + i, bytes([rng.randrange(6)]))
        w(THIS + 0x364, JOINTS)
        w(THIS + 0x5700, MATS)
        w(THIS + 0xD8C, rng.randrange(1 << 32))
        w(THIS + 0xD80, rng.choice((0, 1, 5)))
        w(THIS + 0x108, rng.choice((0x20000000, 0, rng.randrange(1 << 32))))
        for k in range(4):
            w(THIS + 0xE1C + 4 * k, rng.choice((rng.randrange(1 << 32), 0x7FC00001, 0x3F800000)))
        w(THIS + 0xE34, rng.choice(resources))
        w(THIS + 0xE3C, rng.choice(resources))
        w(THIS + 0xE38, rng.choice(resources))
        for k in range(5):
            w(THIS + 0xE2C + 4 * k, rng.choice(resources))
        w(DEVICE, G1)
        w(G1 + 0xFC, rng.choice((0, 2, 3, 100)))
        w(G1 + 0x33DB8, rng.randrange(5))
        w(CONSUMERS, rng.randrange(1 << 32))
        w(GENERATION, rng.randrange(1 << 32))
        for k in range(8):
            w(RES + 0x40 * k + 0xC, rng.randrange(1 << 20))
            w(RES + 0x40 * k + 4, rng.randrange(1 << 32))
        # model state
        uc.mem_write(MODEL + 0x5C, bytes([rng.choice((0x80, 0, 0x80, 0xff))]))
        w(MODEL + 0x6C, rng.randrange(20))
        w(MODEL + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        for slot in (0x30A0, 0x2840, 0x2848, 0x30D8):
            w(MODEL + slot, rng.choice(values))
            w(MODEL + slot + 4, rng.randrange(1 << 32))
        for slot in (0x1AD8, 0x1AF0, 0x1AE8, 0x1D18):
            w(MODEL + slot, rng.choice(resources))
            w(MODEL + slot + 4, rng.choice((0, rng.randrange(1 << 32))))
        cursor = HEAP + 0x1000 * rng.randrange(4)
        w(MODEL + 0x24, cursor)
        w(MODEL + 0x28, cursor + rng.choice((0, 8, 0x40, 0x100, 0x10000)))
        w(MODEL + 0x1380, rng.randrange(1 << 32))
        # floats; at most one special value per case (NaN-with-NaN operand order is not specified)
        special = [rng.random() < 0.03]

        def rf():
            if special[0] and rng.random() < 0.02:
                special[0] = False
                return rng.choice(SPECIAL)
            return struct.unpack('<I', struct.pack('<f', rng.uniform(-10, 10) * rng.choice((1, 1, 0.001, 1000))))[0]

        for j in range(6):
            for off in range(0x10, 0x4C, 4):
                w(JOINTS + 0xA0 * j + off, rf())
            for off in range(0, 0x40, 4):
                w(MATS + 0x40 * j + off, rf())
        uc.mem_write(SLOTS, bytes(rng.randrange(256) for _ in range(0x200)))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, [MODEL]))
    finish(out, '0x00B8A7F0', cases)


main()
