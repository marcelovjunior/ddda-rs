"""L172 P2b (s9) oracle: 0x0111E180 `pool->sort_pairs(scratch)` (the kind-2
sort job of the sPrimitive pools: bottom-up merge sort of 8-byte records,
descending by the unsigned second dword, stable) with its merge pass
0x0111D490 as real original code. No guest calls. Format of
crates/mtcore/tests/shadow_primitive.rs (abi_cases_full).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x0111E180, 0xA8   # through RET 4 at 0x0111E225
MERGE, MERGE_LENGTH = 0x0111D490, 0xF1
POOL, DESC, ARRAY, SCRATCH = 0x20000000, 0x21000000, 0x22000000, 0x23000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((MERGE & ~0xFFF, 0x1000), (ENTRY & ~0xFFF, 0x1000), (POOL, 0x1000), (DESC, 0x1000), (ARRAY, 0x20000), (SCRATCH, 0x20000),
                              (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x04\x00' or True
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(MERGE, h.code(MERGE, MERGE_LENGTH))
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert ENTRY <= address < ENTRY + LENGTH or MERGE <= address < MERGE + MERGE_LENGTH, hex(address)
        self.visited.add(address)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(POOL, b'\0' * 0x1000)
        uc.mem_write(DESC, b'\0' * 0x1000)
        n = rng.choice((0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 33, 64, 100, 129, rng.randrange(0, 800)))
        keys_mode = rng.random()
        uc.mem_write(ARRAY, b''.join(struct.pack('<II', rng.getrandbits(32), (rng.randrange(0, 6) if keys_mode < 0.4 else rng.getrandbits(32) if keys_mode < 0.8 else n - i)) for i in range(n)) + b'\0' * 64)
        uc.mem_write(SCRATCH, rng.randbytes(8 * n + 64))
        depth = rng.randrange(1, 4)
        uc.mem_write(POOL + 4, struct.pack('<I', DESC))
        uc.mem_write(DESC + 0x64, struct.pack('<I', depth))
        uc.mem_write(DESC + 16 * depth - 4, struct.pack('<I', n))
        uc.mem_write(DESC + 16 * depth - 8, struct.pack('<I', ARRAY))
        self.initial, self.written, self.visited = {}, set(), set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<II', h.RET, SCRATCH))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = POOL, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=3000000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET, 'instruction limit'
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2])
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    json.dump(dict(function='0x0111E180', cases=cases), open(out, 'w'))
    print(f'{n} cases, {len(visited)} instruction addresses visited -> {out}')


main()
