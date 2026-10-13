"""L184 s10 oracle: 0x007FFCC0 `submit_pass_records` (ESI = render context,
EDI = pass sequencer, six stack arguments, ret 0x18): walks the sequencer's
0x30-byte draw records and drives the context through 0x00FF14D0,
0x00FED190, 0x00EA23F0 and four sequencer virtual methods (+0x14, +0x18,
+0x1c, +0x20), all stubbed and recorded. The stubs also rewrite the
context's binding fields and pass word so the original's re-reads show.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x007FFCC0, 0x2A9
CTX, PASS, VT, RECS, VIS, TABLE, RES, OVR, BIND = 0x20000000, 0x20001000, 0x20002000, 0x20010000, 0x20020000, 0x20030000, 0x20040000, 0x20038000, 0x20050000
SP = 0x0F008000
GEN = 0x018D2CE8
V14, V18, V1C, V20 = 0x30000014, 0x30000018, 0x3000001C, 0x30000020
PLAIN = {0x00FF14D0: 0, 0x00FED190: 2, 0x00EA23F0: 2}


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000

    def stub(o, regs, esp):
        r = o.rng
        ctx = regs[6]
        if r.random() < 0.3:
            o.poke(ctx + 0x138, struct.pack('<I', r.choice((o.keys + [0])) if o.keys else 0))
        if r.random() < 0.2:
            o.poke(ctx + 0x124, struct.pack('<I', r.choice((0, o.binding, BIND + 0x40))))
        if r.random() < 0.2:
            o.poke(ctx + 0x134, bytes([r.choice((0, 1, 2, 3))]))
        if r.random() < 0.2:
            o.poke(ctx + 0x164, struct.pack('<I', r.randrange(1 << 32)))
        return r.choice((0, 1, 5, r.randrange(1 << 32)))

    stubs = {a: (n, stub) for a, n in PLAIN.items()}
    stubs.update({V14: (4, stub), V18: (1, stub), V1C: (4, stub), V20: (3, stub)})
    oracle = Oracle(ENTRY, LENGTH, stubs, [(CTX, 0x1000), (PASS, 0x1000), (VT, 0x1000), (RECS, 0x10000), (VIS, 0x1000),
                                           (TABLE, 0x1000), (RES, 0x10000), (OVR, 0x1000), (BIND, 0x1000), (0x0F000000, 0x10000), (0x018D2000, 0x1000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc
        w = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        n = rng.choice((0, 1, 2, 3, rng.randrange(4, 12)))
        groups = rng.randrange(1, 5)
        w(GEN, rng.randrange(1 << 32))
        w(VT + 0x14, V14); w(VT + 0x18, V18); w(VT + 0x1C, V1C); w(VT + 0x20, V20)
        w(PASS, VT)
        binding = BIND + 0x40 * rng.randrange(2) if rng.random() < 0.8 else 0
        oracle.binding = binding
        stage = BIND + 0x80 if rng.random() < 0.9 else 0
        w(PASS + 0xC, binding)
        w(PASS + 0x18, stage or BIND + 0x80)
        stage = stage or BIND + 0x80
        w(PASS + 0x20, OVR if rng.random() < 0.6 else 0)
        w(PASS + 0x50, RECS)
        w(PASS + 0x54, n)
        uc.mem_write(PASS + 0x570, bytes([rng.choice((0, 0, 1, 0x80))]))
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0xFFFF, rng.randrange(1 << 16), 0x30C))))
        w(CTX + 0x14C, stage if rng.random() < 0.4 else rng.choice((0, BIND + 0xC0)))
        w(CTX + 0x150, 0)
        w(CTX + 0x164, rng.randrange(1 << 32))
        w(CTX + 0x124, rng.choice((0, binding, BIND + 0x40)))
        uc.mem_write(CTX + 0x134, bytes([rng.randrange(4)]))
        w(CTX + 0x138, rng.randrange(4))
        # visible bitmap: slots 0..63
        for k in range(0x110, 0x110 + 8, 4):
            pass
        w(VIS + 0x110, rng.choice((0xFFFFFFFF, rng.randrange(1 << 32))))
        w(VIS + 0x114, rng.choice((0xFFFFFFFF, rng.randrange(1 << 32))))
        # per-group resources and overrides
        for g in range(8):
            kind = rng.choice((0x10, 0x0E, 0x01, 0x1F))
            w(RES + 0x100 * g + 0x14, (kind << 26) | rng.randrange(1 << 26))
            w(TABLE + 4 * g, RES + 0x100 * g)
            w(OVR + 4 * g, RES + 0x1000 + 0x100 * g if rng.random() < 0.5 else 0)
            w(RES + 0x1000 + 0x100 * g + 0x14, rng.randrange(1 << 32))
        keys = []
        for i in range(n):
            base = RECS + 0x30 * i
            w(base, rng.choice((0xFFFF, rng.randrange(1 << 32), 0x10000, 0x30C)))
            slot = rng.randrange(64)
            grp = rng.randrange(groups) if rng.random() < 0.8 else rng.randrange(8)
            layer = rng.choice((0xFF, 0x01, rng.randrange(256)))
            w(base + 4, slot | (grp << 12) | (layer << 24))
            w(base + 8, rng.choice((1, 5, 4, 0, rng.randrange(8))) | (rng.randrange(1 << 8) << 8 if rng.random() < .3 else 0))
            uc.mem_write(base + 9, bytes([rng.randrange(256)]))
            uc.mem_write(base + 10, bytes([rng.randrange(4)]))
            key = rng.randrange(4)
            w(base + 0x10, key)
            keys.append(key)
        oracle.keys = keys
        arg0, arg1 = VIS, TABLE
        arg2 = rng.choice((0xFF, 0x01, 0x80, rng.randrange(1 << 32)))
        arg3 = rng.choice((0, 9, 10, 99, rng.randrange(1 << 32), rng.randrange(1000)))
        arg4 = rng.choice((0, 1, rng.randrange(1 << 27), rng.randrange(1 << 32)))
        arg5 = rng.choice((0, 1, 0x100, rng.randrange(1 << 32)))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[6], regs[7] = CTX, PASS
        cases.append(oracle.run(seed, regs, esp, [arg0, arg1, arg2, arg3, arg4, arg5]))
    finish(out, '0x007FFCC0', cases)


main()
