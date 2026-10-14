"""L190 oracle: 0x00B8B800, the skin joint remap rebuild of uPlayerBase (a
helper of 0x00B8B430 with the model in ESI, plain RET, AL = 1). The CRT
memcpy / memset and the allocator's virtual free (+0x24) and allocate (+0x20)
are recorded stubs. Parent links are acyclic (the original loops forever on a
cycle). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00B8B800, 0x20A
THIS, MESH, NODES, SRC, HEAP, ALLOC, AVT = 0x20000000, 0x20010000, 0x20020000, 0x20040000, 0x20100000, 0x20200000, 0x20201000
SP = 0x0F008000
MEMCPY, MEMSET, S_FREE, S_ALLOC, GLOBAL = 0x012EFEA0, 0x012EFE20, 0x30000024, 0x30000020, 0x01876658


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def memcpy(o, regs, esp):
        dst, src, n = o.u32(esp + 4), o.u32(esp + 8), o.u32(esp + 12)
        o.remember(src, n)
        o.poke(dst, bytes(o.uc.mem_read(src, n)))
        return dst

    def memset(o, regs, esp):
        dst, val, n = o.u32(esp + 4), o.u32(esp + 8), o.u32(esp + 12)
        o.poke(dst, bytes([val & 0xFF]) * n)
        return dst

    def free(o, regs, esp):
        return o.rng.randrange(1 << 32)

    def alloc(o, regs, esp):
        return HEAP + 16 * o.rng.randrange(256)

    stubs = {MEMCPY: (0, memcpy, 3), MEMSET: (0, memset, 3), S_FREE: (1, free), S_ALLOC: (2, alloc)}
    oracle = Oracle(ENTRY, LENGTH, stubs,
                    [(THIS, 0x10000), (MESH, 0x1000), (NODES, 0xA000), (SRC, 0x10000), (HEAP, 0x20000), (ALLOC, 0x2000), (0x0F000000, 0x10000), (GLOBAL & ~0xFFF, 0x1000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        n = rng.choice((0, 1, 2, 3, 5, 8, 12, 20, 30, 40, 70, rng.randrange(1, 40), rng.randrange(1, 40), 255 if seed % 100 == 0 else 6))
        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x6000)))
        w(THIS + 0x360, n)
        w(THIS + 0x364, NODES)
        w(THIS + 0xF0, MESH)
        w(MESH + 0x6C, SRC)
        uc.mem_write(SRC, bytes(rng.randrange(256) for _ in range(0x4100)))
        w(GLOBAL, ALLOC)
        w(ALLOC, AVT)
        w(AVT + 0x20, S_ALLOC)
        w(AVT + 0x24, S_FREE)
        flag_p = rng.choice((0.2, 0.5, 0.8, 1.0))
        node_bytes = bytearray(0xA000)
        for i in range(min(n + 2, 256)):
            base = i * 0xA0
            for k in range(0xA0):
                node_bytes[base + k] = rng.randrange(256)
            node_bytes[base + 0x90] = (node_bytes[base + 0x90] & 0xFE) | (1 if rng.random() < flag_p else 0)
            r = rng.random()
            node_bytes[base + 0x91] = 0xFF if (r < 0.25 or i == 0) else rng.randrange(0, i) if r < 0.9 else rng.randrange(i, 256) if False else rng.randrange(0, i)
        uc.mem_write(NODES, bytes(node_bytes))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[6] = THIS
        cases.append(oracle.run(seed, regs, esp, []))
    finish(out, '0x00B8B800', cases)


main()
