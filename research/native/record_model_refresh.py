"""L184 oracle: 0x01054860 `model->refresh(flags)` (uModel::vf28, ret 4) run
as the original's code in Unicorn on synthetic models. Its guest callees
(0x00EA4110, 0x01054700 ret 0xc, 0x00FFA620 ret 4) are stubs that
sometimes rewrite model fields, recorded as events in the format of
crates/mtcore/tests/semantic.rs `abi_cases_full`.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
Usage: record_model_refresh.py out.json [cases]
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x01054860, 0x70  # through RET 0x4 at 0x010548CB
THIS = 0x20000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
STUBS = {0x00EA4110: 0, 0x01054700: 3, 0x00FFA620: 1}


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {ENTRY & ~0xFFF, (ENTRY + LENGTH) & ~0xFFF} | {a & ~0xFFF for a in STUBS}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for base, size in ((THIS, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, bytes([0xC3]))
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

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        nargs = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        effects = []
        if rng.random() < 0.4:
            off, n = rng.choice(((4, 4), (0xF0, 4), (0x360, 4), (0x36C, 4), (0x370, 4), (0xD80, 4)))
            effects += self.stub_write(THIS + off, bytes(rng.choice((0, 1, 2, rng.randrange(256))) for _ in range(n)))
        eax = rng.randrange(1 << 32)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, bytes(0x1000))

        def put(off, v):
            uc.mem_write(THIS + off, struct.pack('<I', v))
        put(4, rng.choice((2, 0x4802, 0x4802 | (rng.randrange(1 << 8) << 20), 0x4800, 0x2, 1, rng.randrange(1 << 32))))
        put(0xF0, rng.choice((0, 0, 1, rng.randrange(1 << 32))))
        put(0x360, rng.randrange(1 << 32))
        put(0x36C, rng.randrange(1 << 32))
        put(0x370, rng.randrange(1 << 32))
        put(0xD80, rng.randrange(1 << 32))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        flags = rng.choice((0, 1, 4, 5, 2, 3, 6, 7, rng.randrange(1 << 32)))
        uc.mem_write(esp, struct.pack('<II', h.RET, flags))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x01054860', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
