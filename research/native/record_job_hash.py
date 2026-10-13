"""L158 original-instruction E0E5E0 hash oracle, never a game/shard run.

Run in heavy.py with research Python. All exe bytes and vectors stay private.
Includes byte tails, unaligned/page-edge inputs, zero/full bit patterns,
random initial machine state, and inputs aliasing overwritten stack slots.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, SIZE, STACK, STACK_SIZE, SP
import harness as h

ENTRY, LENGTH = 0xE0E5E0, 0x6E


class HashOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for base, size in ((ENTRY & ~4095, 4096), (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 4096)):
            self.uc.mem_map(base, size)
        self.uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        self.uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert ENTRY <= address < ENTRY + LENGTH, hex(address)
        self.visited.add(address)

    def run(self, seed, length, pointer, pattern):
        uc, rng = self.uc, random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        if pattern is not None:
            uc.mem_write(pointer, bytes([pattern]) * length)
        uc.mem_write(SP, struct.pack('<4I', h.RET, pointer, length, rng.getrandbits(32)))
        for r in REGS:
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, SP)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1f80 | ((seed % 4) << 13))
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xcd5))
        self.initial, self.written, self.visited = {}, set(), set()
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(ENTRY, h.RET, count=100000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        self.active = False
        after = self.snapshot()
        # TEST leaves AF undefined on the zero-word path; CMP defines it.
        after['flag_mask'] = 0xcd5 if length >= 4 else 0xcc5
        return dict(seed=seed, length=length, pointer=pointer, before=before,
                    initial=sorted(self.initial.items()), after=after, visited=sorted(self.visited))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, default=Path('E:/ddda-re/notes/native-L158/job_hash.json'))
    args = p.parse_args()
    oracle, cases = HashOracle(), []
    for length in (*range(0, 33), 63, 64, 65, 127, 128, 255, 256, 1023, 1024, 4095, 4096):
        for pointer in (BASE, BASE + 1, BASE + 4093, SP - 12):
            for pattern in (None, 0, 255):
                cases.append(oracle.run(len(cases) + 1, length, pointer, pattern))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    raw = h.code(ENTRY, LENGTH)
    args.out.write_text(json.dumps(dict(entry=ENTRY, code_sha256=hashlib.sha256(raw).hexdigest(),
                                         cases=cases)), encoding='utf-8')
    print(f'{len(cases)} original returns; {args.out}; sha256={hashlib.sha256(args.out.read_bytes()).hexdigest()}')


if __name__ == '__main__':
    main()
