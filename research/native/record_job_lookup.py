"""Original-instruction L158 shader lookup oracles; run through heavy.py.

Private vectors only. No game, decompiler export, or shard build. Includes
empty buckets, collisions, sign-bit indices, key-pair discrimination and
32-bit link-address wrap to zero. Inputs and stack are randomized.
"""
import hashlib
import json
from pathlib import Path
import random
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, SIZE, STACK, STACK_SIZE, SP
import harness as h

ENTRIES = {0x1008200: 0x50, 0x1008260: 0x69}


class LookupOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for base, size in ((0x1008000, 4096), (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 4096)):
            self.uc.mem_map(base, size)
        for entry, size in ENTRIES.items():
            self.uc.mem_write(entry, h.code(entry, size))
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        self.uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        assert self.entry <= address < self.entry + ENTRIES[self.entry], hex(address)
        self.visited.add(address)

    def run(self, entry, seed, count, hit, index, mode):
        self.entry = entry
        uc, rng = self.uc, random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        owner, table, nodes = BASE, BASE + 0x1000, BASE + 0x10000
        # Deliberately unaligned lookup tables/nodes, including page edges.
        if mode == 'unaligned':
            table += 1
            nodes += 4093
        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xffffffff))
        key = rng.getrandbits(32) or 1
        second = rng.getrandbits(32) or 1
        if mode == 'zero': key = second = 0
        if mode == 'zero-first': key = 0
        word(owner + 0x84, table)
        word(table + 0x18, nodes)
        word(table + 0x1c + (key & 0xfff)*4, nodes if count else 0)
        for i in range(count):
            word(nodes + i*16, key if i == hit or mode == 'second-mismatch' else key ^ 0x1000)
            word(nodes + i*16 + 4, second if i == hit else second ^ 1)
            word(nodes + i*16 + 8, i+1 if i+1 < count else 0)
            word(nodes + i*16 + 12, index)
        if mode == 'wrap' and count:
            word(nodes + 8, 0x10000000 - 1)
            word(table + 0x18, 16)
        uc.mem_write(SP, struct.pack('<3I', h.RET, key, second))
        for register in REGS: uc.reg_write(register, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX, owner)
        uc.reg_write(x.UC_X86_REG_ESP, SP)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1f80 | ((seed % 4) << 13))
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xcd5))
        self.initial, self.written, self.visited = {}, set(), set()
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(entry, h.RET, count=100000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        self.active = False
        after = self.snapshot()
        after['flag_mask'] = 0xcc5  # Last flag writer is AND/OR/XOR; AF undefined.
        return dict(entry=entry, seed=seed, count=count, hit=hit, mode=mode,
                    before=before, initial=sorted(self.initial.items()), after=after,
                    visited=sorted(self.visited))


def main():
    oracle, cases = LookupOracle(), []
    for entry in ENTRIES:
        for count in (0, 1, 2, 3, 8, 32, 64):
            for hit in sorted({-1, 0, count//2, count-1}):
                for index in (0, 1, 0x7fffffff, 0x80000000, 0xffffffff):
                    for mode in ('normal', 'unaligned', 'zero', 'zero-first', 'second-mismatch', 'wrap'):
                        # The wrap case must miss the first node to exercise addition wrap.
                        cases.append(oracle.run(entry, len(cases)+1, count,
                                                -1 if mode == 'wrap' else hit, index, mode))
    out = Path('E:/ddda-re/notes/native-L158/job_lookup.json')
    out.write_text(json.dumps(dict(code_sha256={hex(e): hashlib.sha256(h.code(e,n)).hexdigest()
                                              for e,n in ENTRIES.items()}, cases=cases)))
    print(f'{len(cases)} original returns; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}')


if __name__ == '__main__': main()
