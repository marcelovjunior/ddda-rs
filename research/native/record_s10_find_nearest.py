"""L184 s10 oracle: 0x0049E920 `find_nearest_object(list, exclude, owner_id,
position, scale, max_dy, param)` (stdcall, ret 0x1c): scan the list's
objects (count +0x24, pointers +0x30), keep those of three classes
(virtual +0x10 returns the class descriptor, walked through parents at
+0x10) within a range, bubble-sort the hits (<= 256) by distance and return
the nearest. Half the objects use a trivial `mov eax, imm; ret` getter run
natively, the others stub it (recorded as guest calls). Positions repeat
and occasionally hold a NaN, so the sort's tie and unordered paths run.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x0049E920, 0x319
G_A, G_B, G_C = 0x019A5E30, 0x019A60B0, 0x019A60D0
LIST, OBJS, VT, DTIS, POS, TRIVIAL = 0x20000000, 0x20010000, 0x20008000, 0x20004000, 0x20300000, 0x31000000
STRIDE = 0x2600
SP = 0x0F008000
GETTERS = [0x30000000 + 0x10 * k for k in range(4)]


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    state = {'dti': {}}

    def getter(o, regs, esp):
        return state['dti'][regs[1]]

    stubs = {a: (0, getter) for a in GETTERS}
    oracle = Oracle(ENTRY, LENGTH, stubs, [(LIST, 0x10000), (OBJS, STRIDE * 40), (POS, 0x1000), (TRIVIAL, 0x200000), (0x0F000000, 0x10000),
                                           (0x019A5000, 0x1000), (0x019A6000, 0x1000), (0x0161C000, 0x1000), (0x014F9000, 0x1000)])
    for a, n in ((0x0161C20C, 4), (0x014F9E04, 4), (0x0161C330, 16)):
        oracle.uc.mem_write(a, h.code(a, n))
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def wf(a, v):
            uc.mem_write(a, struct.pack('<f', v))

        ids = [rng.randrange(1, 1 << 32) for _ in range(3)]
        w(G_A, ids[0]); w(G_B, ids[1]); w(G_C, ids[2])
        n = rng.choice((0, 1, 2, 5, 12, rng.randrange(1, 40)))
        w(LIST + 0x24, n)
        w(LIST + 0x30, LIST + 0x1000)
        owner_id = rng.randrange(4)
        param = rng.choice((0, 0, 7))
        state['dti'] = {}
        methods = []
        node = DTIS
        spots = [(rng.uniform(-50, 50), rng.uniform(-5, 5), rng.uniform(-50, 50)) for _ in range(4)]
        for i in range(n):
            obj = OBJS + STRIDE * i
            w(LIST + 0x1000 + 4 * i, obj)
            # class chain
            chain_len = rng.randrange(1, 4)
            chain_ids = [rng.choice(ids + [rng.randrange(1, 1 << 32)]) for _ in range(chain_len)]
            ptrs = [node + 0x20 * k for k in range(chain_len)]
            node += 0x20 * chain_len
            for k in range(chain_len):
                w(ptrs[k] + 4, chain_ids[k])
                w(ptrs[k] + 0x10, ptrs[k + 1] if k + 1 < chain_len else 0)
            state['dti'][obj] = ptrs[0]
            if rng.random() < 0.5:
                method = TRIVIAL + 0x10 * (seed * 40 + i)
                uc.mem_write(method, b'\xb8' + struct.pack('<I', ptrs[0]) + b'\xc3')
            else:
                method = GETTERS[i % 4]
            methods.append((method, 6))
            w(obj, VT + 0x100 * i)
            w(VT + 0x100 * i + 0x10, method)
            w(obj + 4, rng.choice((2, 2, 2, 0x402, 0x402, 0x402, 0x402, 0x400, 0x3, 0x42A)) if rng.random() < .9 else rng.randrange(1 << 16))
            x, y, z = rng.choice(spots) if rng.random() < .5 else (rng.uniform(-50, 50), rng.uniform(-5, 5), rng.uniform(-50, 50))
            if rng.random() < 0.04:
                x = float('nan')
            wf(obj + 0x40, x); wf(obj + 0x44, y); wf(obj + 0x48, z)
            w(obj + 0x2574, owner_id if rng.random() < 0.8 else owner_id + 1)
            w(obj + 0x2534, param if rng.random() < 0.7 or param == 0 else param + 1)
        wf(POS, rng.uniform(-50, 50)); wf(POS + 4, rng.uniform(-5, 5)); wf(POS + 8, rng.uniform(-50, 50))
        exact = n and rng.random() < 0.25
        if exact:
            # one object at distance exactly 5 with limit 2 * 2.5 = 5, the others far or equal
            wf(POS, 0.0); wf(POS + 8, 0.0)
            for k in range(n):
                if rng.random() < 0.5:
                    wf(OBJS + STRIDE * k + 0x40, 3.0); wf(OBJS + STRIDE * k + 0x48, 4.0)
        exclude = OBJS + STRIDE * rng.randrange(n) if n and rng.random() < 0.3 else 0
        scale = rng.choice((rng.uniform(0.1, 100), 1.0, 0.0, -1.0, float('nan'))) if rng.random() < .3 else rng.uniform(0.5, 200)
        if exact:
            scale = 2.5
        max_dy = rng.choice((0.0, 2.0, 100.0, rng.uniform(0, 10)))
        esp = SP - 0x40 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        f = lambda v: struct.unpack('<I', struct.pack('<f', v))[0]
        cases.append(oracle.run(seed, regs, esp, [LIST, exclude, owner_id, POS, f(scale), f(max_dy), param], pre_remember=methods))
    finish(out, '0x0049E920', cases)


main()
