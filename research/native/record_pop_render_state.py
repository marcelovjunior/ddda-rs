"""L184 s9 oracle: 0x00FED5D0 `this->pop_state(full)` run as the original's
code in Unicorn on synthetic render-state stacks. Its only guest call, the
CRT memcpy at 0x012EFEA0 (cdecl), is a stub that copies bytes and returns
the destination. Format of crates/mtcore/tests/semantic.rs `abi_cases_full`.
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

ENTRY, LENGTH = 0x00FED5D0, 0x13E  # through RET 0x4 at 0x00FED70B
MEMCPY = 0x012EFEA0
GLOBALS = 0x01447B44
THIS, THIS_SIZE = 0x20000000, 0x50000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x1000), (MEMCPY & ~0xFFF, 0x1000), (GLOBALS & ~0xFFF, 0x1000),
                              (THIS, THIS_SIZE), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        end = ENTRY + LENGTH
        assert h.code(end - 3, 3) == b'\xc2\x04\x00', h.code(end - 3, 3)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(MEMCPY, b'\xc3')
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
        if address != MEMCPY:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 16))]
        before = dict(regs=self.regs(), memory=window)
        dst, src, n = struct.unpack('<III', uc.mem_read(esp + 4, 12))
        self.remember(src, n)
        data = bytes(uc.mem_read(src, n))
        self.remember(dst, n)
        uc.mem_write(dst, data)
        self.written.update(range(dst, dst + n))
        effects = [(dst + i, b) for i, b in enumerate(data)]
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, dst)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, rng.randbytes(THIS_SIZE))
        uc.mem_write(GLOBALS & ~0xFFF, rng.randbytes(0x1000))
        depth = rng.choice((1, 2, 3, 6, 1 + rng.randrange(5)))
        uc.mem_write(THIS + 0x30874, struct.pack('<I', depth))
        # slot indices: mostly small, sometimes with high bits (masked by 0xfff)
        for i in range(6):
            v = rng.choice((rng.randrange(0x1000), rng.getrandbits(32), 0, 0xFFF))
            uc.mem_write(GLOBALS + 4 * i, struct.pack('<I', v))
        uc.mem_write(THIS + 0x14, struct.pack('<I', rng.choice((0, 1, rng.randrange(40), rng.randrange(0x100)))))
        full = rng.choice((0, 0, 1))
        if rng.random() < 0.2:
            full = rng.randrange(256)
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<IB', h.RET, full) + b'\0\0\0')
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 1500
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00FED5D0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
