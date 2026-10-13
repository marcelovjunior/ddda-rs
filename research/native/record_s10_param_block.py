"""L184 s10 oracle: 0x00C85A00 `apply_param_block(block, target, extra)`
(stdcall, ret 0xc): if `target` (an MtObject; its vtable +0x10 returns the
class descriptor) is not derived from class A (0x019A6CC0 id) but is derived
from class B (0x019A6CE0 id), apply the parameter block's sections to it
through eight register-convention helpers. Half the cases use a trivial
descriptor getter (mov eax, imm; ret) that runs natively, the others stub it
(recorded as a guest call). Callees are stubs that also rewrite the block's
section words, so the original's re-reads are exercised.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish, M32  # noqa: E402

ENTRY, LENGTH = 0x00C85A00, 0x10A
G_A, G_B = 0x019A6CC0, 0x019A6CE0
CALLEES = {0x00C86390: 0, 0x00C86420: 1, 0x00C864D0: 0, 0x00C879D0: 0, 0x00C87A70: 0, 0x00C87B70: 0, 0x00C87C30: 0, 0x00C87CA0: 0}
GETTERS = [0x30000000 + 0x10 * k for k in range(4)]
D, TRIVIAL, SP = 0x20000000, 0x31000000, 0x0F008000
BLOCK, OBJ, VT, DTIS = D + 0x6000, D + 0x4000, D + 0x5000, D + 0x1000


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    stubs = {}
    state = {}

    def callee(o, regs, esp):
        blk = regs[3]
        r = o.rng
        if r.random() < 0.4:
            o.poke(blk + 4, bytes([r.randrange(0x40)]))
        for off in (0x18, 0x1c, 0x20):
            if r.random() < 0.2:
                o.poke(blk + off, struct.pack('<I', r.choice((0, 1, 2, 3, 4, r.randrange(1 << 32)))))
        return r.randrange(1 << 32)

    def getter(o, regs, esp):
        k = state['calls']
        state['calls'] += 1
        return state['plan'][min(k, len(state['plan']) - 1)]

    for a, n in CALLEES.items():
        stubs[a] = (n, callee)
    for a in GETTERS:
        stubs[a] = (0, getter)
    oracle = Oracle(ENTRY, LENGTH, stubs, [(D, 0x10000), (0x0F000000, 0x10000), (0x019A6000, 0x1000), (TRIVIAL, 0x1000)],
                    extra_code=())
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc
        ida, idb = rng.randrange(1, 1 << 32), rng.randrange(1, 1 << 32)
        uc.mem_write(G_A, struct.pack('<I', ida))
        uc.mem_write(G_B, struct.pack('<I', idb))
        node = [DTIS]

        def chain(kind):
            """A descriptor chain; returns the pointer of its first node."""
            n = rng.randrange(1, 5)
            ids = [rng.randrange(1, 1 << 32) for _ in range(n)]
            if kind == 'a':
                ids[rng.randrange(n)] = ida
            elif kind == 'b':
                ids[rng.randrange(n)] = idb
            if kind in ('b', 'n') and rng.random() < 0.2:
                ids[rng.randrange(n)] = ida if kind == 'a' else rng.randrange(1, 1 << 32)
            ptrs = [node[0] + 0x20 * i for i in range(n)]
            node[0] += 0x20 * n
            for i in range(n):
                uc.mem_write(ptrs[i] + 4, struct.pack('<I', ids[i]))
                uc.mem_write(ptrs[i] + 0x10, struct.pack('<I', ptrs[i + 1] if i + 1 < n else 0))
            return ptrs[0]

        kind = rng.choice('abbbbnn')
        trivial = rng.random() < 0.5
        plan = [chain(kind)]
        if not trivial:
            plan.append(chain(rng.choice('abbn')) if rng.random() < 0.3 else plan[0])
        state['calls'], state['plan'] = 0, plan
        if trivial:
            method = TRIVIAL + 0x10 * rng.randrange(16)
            uc.mem_write(method, b'\xb8' + struct.pack('<I', plan[0]) + b'\xc3')
        else:
            method = rng.choice(GETTERS)
        uc.mem_write(OBJ, struct.pack('<I', VT))
        uc.mem_write(VT + 0x10, struct.pack('<I', method))
        flags = rng.choice((rng.randrange(0x40), 0x3f, 0x2e, 0x0c))
        uc.mem_write(BLOCK + 4, struct.pack('<I', flags | (rng.randrange(1 << 24) << 8 if rng.random() < .5 else 0)))
        for off in (0x18, 0x1c, 0x20):
            uc.mem_write(BLOCK + off, struct.pack('<I', rng.choice((0, 1, 2, 3, 4, rng.randrange(1 << 32)))))
        extra = rng.randrange(1 << 32)
        esp = SP - rng.choice((0x10, 0x14, 0x18, 0x1c))
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        case = oracle.run(seed, regs, esp, [BLOCK, OBJ, extra],
                          pre_remember=[(method, 6)])
        cases.append(case)
    finish(out, '0x00C85A00', cases)


main()
