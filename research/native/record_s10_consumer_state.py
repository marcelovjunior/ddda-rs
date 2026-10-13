"""L184 s10 oracle: 0x00FE4780 `consumer->bind_for_model(model, packed)`
(thiscall, ret 8): bind the consumer's two resources (+0x8c, +0x90) into the
model's state slots (undo-logged), allocate and fill the model's 12-byte
transform block (x87 FILD/FMUL/FSTP) and bind the consumer's three buffer
ranges (+0x2a0, +0x2a4, stride +0x2a8) for the packed (cell, slot, count)
word. The model's allocator grow (0x00FED890) is the one guest call; it is a
stub that also rewrites the model's flags, undo count and state words.
The multiplier constants (0x0143A04C, 0x014343A8) are the game's own.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00FE4780, 0x541
CONSUMER, MODEL, DT, RES, MODEL2, SKEL, HEAP = 0x20000000, 0x20100000, 0x20200000, 0x20500000, 0x20900000, 0x20901000, 0x20B00000
SP = 0x0F008000
GROW = 0x00FED890


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    state = {}

    def grow(o, regs, esp):
        r = o.rng
        size = o.u32(esp + 4)
        if r.random() < 0.3:
            o.poke(MODEL + 0x5c, struct.pack('<I', r.choice((0, 0x80, 0x82, 0xff))))
        if r.random() < 0.2:
            o.poke(MODEL + 0x6c, struct.pack('<I', r.randrange(30)))
        if r.random() < 0.8:
            o.poke(MODEL + 0x24, struct.pack('<I', HEAP + 0x1000 * r.randrange(4)))
            o.poke(MODEL + 0x28, struct.pack('<I', HEAP + 0x1000 * r.randrange(4) + size + r.randrange(2) * 0x10000))
        else:
            o.poke(MODEL + 0x24, struct.pack('<I', 0))
        return 0

    oracle = Oracle(ENTRY, LENGTH, {GROW: (1, grow)},
                    [(CONSUMER, 0x1000), (MODEL, 0x40000), (DT, 0x4000), (RES, 0x10000), (MODEL2, 0x2000), (HEAP, 0x20000),
                     (0x0F000000, 0x10000), (0x018D0000, 0x4000), (0x0143A000, 0x1000), (0x01434000, 0x1000), (0x0161C000, 0x1000)])
    for a in (0x0143A04C, 0x014343A8, 0x0161C220):
        oracle.uc.mem_write(a, h.code(a, 4))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        resources = [0] + [RES + 0x40 * k for k in range(8)]
        w(0x018D2CE8, rng.randrange(1 << 32))
        w(CONSUMER + 0x8C, rng.choice(resources))
        w(CONSUMER + 0x90, rng.choice(resources))
        w(CONSUMER + 0x2A0, rng.choice((0, 0x4000, 0x80000, 0x1234560)))
        w(CONSUMER + 0x2A4, rng.choice((0, 0x4000, 0x80000, 0x1234560)))
        w(CONSUMER + 0x2A8, rng.choice((0, 0x20, 0x24, 0x240, 0x10000)))
        w(MODEL + 0x20, DT)
        for off in (0xFB0, 0xFB8, 0xFC0):
            w(DT + off, rng.choice((0, 0x1000, 0xABCDEF0)))
        w(MODEL + 0x18, MODEL2)
        w(MODEL2 + 0x7D4, SKEL)
        uc.mem_write(SKEL + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 5, rng.randrange(1, 40)))))
        for k in range(8):
            w(RES + 0x40 * k + 0xC, rng.randrange(1 << 20))
            w(RES + 0x40 * k + 4, rng.randrange(1 << 32))
        uc.mem_write(MODEL + 0x5C, struct.pack('<I', rng.choice((0x80, 0, 0x80, 0xFF, 0x82, rng.randrange(1 << 32)))))
        w(MODEL + 0x6C, rng.randrange(20))
        w(MODEL + 0x154, rng.randrange(1 << 32) if rng.random() < .3 else 0)
        for slot in (0x1B80, 0x1B88):
            w(MODEL + slot, rng.choice(resources))
            w(MODEL + slot + 4, rng.choice((0, rng.randrange(1 << 32))))
        # buffer ranges: sometimes already holding the value the call computes
        for slot in (0x11A0, 0x11A8, 0x11AC, 0x11B0, 0x11B4, 0x11B8, 0x11BC):
            w(MODEL + slot, rng.choice((0, rng.randrange(1 << 32), 0x1000, 0xABCDEF0)))
        cursor = HEAP + 0x1000 * rng.randrange(4)
        w(MODEL + 0x24, cursor)
        w(MODEL + 0x28, cursor + rng.choice((0, 8, 0x40, 0x100, 0x10000)))
        packed = rng.choice((rng.randrange(1 << 32), rng.randrange(7) << 24 | rng.randrange(1 << 24), rng.randrange(3) << 24 | rng.randrange(1 << 24), 0))
        # make the stored slot values match the computed ones now and then
        if rng.random() < 0.3:
            for slot, field in ((0x11A8, 0x2A0), (0x11B0, 0x2A4), (0x11B8, 0x2A0)):
                n = ((packed >> 24) * 0x24 + 0xF) >> 4
                base = (((packed & 0xFFF) * struct.unpack('<I', uc.mem_read(CONSUMER + 0x2A8, 4))[0] + struct.unpack('<I', uc.mem_read(CONSUMER + field, 4))[0] + ((((packed >> 12) & 0xFFF) * 9) << 6))) & 0xFFFFFFFF
                w(MODEL + slot, base | n)
                w(MODEL + slot + 4, 0)
        if rng.random() < 0.15:
            # ranges whose address wraps to 0: the device defaults are used
            stride = struct.unpack('<I', uc.mem_read(CONSUMER + 0x2A8, 4))[0]
            zero = (-((packed & 0xFFF) * stride + (((packed >> 12) & 0xFFF) * 9 << 6))) & 0xFFFFFFFF
            w(CONSUMER + 0x2A0, zero)
            w(CONSUMER + 0x2A4, zero)
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = CONSUMER
        cases.append(oracle.run(seed, regs, esp, [MODEL, packed]))
    finish(out, '0x00FE4780', cases)


main()
