"""L179 oracle: 0x005B31A0 `find(key, table) -> index` (cdecl), the binary
search over a table's entries through its virtual count (vtable +0x3c) and
entry(i) (vtable +0x40, ret 4), run as the original's code in Unicorn.

Half the tables use the real rAIStageActionParameter accessors
(0x00CA8D60 count = u16 at [this+0x60]+6, 0x00CAA160 entry(i) =
[this+0x60] + 8 + 0xec i), whose code runs inline; the others point both
slots at stubs that serve a differently laid-out table and are recorded as
guest calls (the format of crates/mtcore/tests/semantic.rs
`abi_cases_full`). Keys and entries are signed and include duplicates,
misses below/above/between, empty tables and negative values.
Reads the user's own exe; output is private (E:/ddda-re/notes/native-L179).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x005B31A0, 0x53  # through RET at 0x005B31F2
COUNT, COUNT_LEN = 0x00CA8D60, 8
ROW, ROW_LEN = 0x00CAA160, 0x14
STUB_COUNT, STUB_ROW = 0x30000000, 0x30000010
TABLE, VTABLES, ROWS = 0x20000000, 0x21000000, 0x22000000
SP = 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x1000), (COUNT & ~0xFFF, 0x1000), (ROW & ~0xFFF, 0x1000), (STUB_COUNT, 0x1000),
                              (TABLE, 0x1000), (VTABLES, 0x1000), (ROWS, 0x40000),
                              (0x0F000000, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(COUNT, h.code(COUNT, COUNT_LEN))
        uc.mem_write(ROW, h.code(ROW, ROW_LEN))
        uc.mem_write(STUB_COUNT, b'\xc3')
        uc.mem_write(STUB_ROW, b'\xc3')
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
        if address not in (STUB_COUNT, STUB_ROW):
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = 0 if address == STUB_COUNT else 1
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        if address == STUB_COUNT:
            eax = self.stub_count
        else:
            index = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
            eax = (ROWS + 0x20000 + 12 * index) & 0xFFFFFFFF  # the stub layout: 12-byte rows
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        real = seed % 2 == 0
        n = rng.choice((0, 1, 2, 3, rng.randrange(4, 40), rng.randrange(40, 300)))
        values = sorted(rng.randrange(-50, 50) * rng.choice((1, 1, 1000003)) for _ in range(n))
        key = rng.choice(values + [rng.randrange(-60, 60), -(1 << 31), (1 << 31) - 1]) if values else rng.randrange(-5, 5)
        vtable = VTABLES + 0x100 * (seed % 4)
        uc.mem_write(vtable + 0x3C, struct.pack('<II', COUNT, ROW) if real else struct.pack('<II', STUB_COUNT, STUB_ROW))
        uc.mem_write(TABLE, struct.pack('<I', vtable))
        uc.mem_write(TABLE + 0x60, struct.pack('<I', ROWS))
        if real:
            uc.mem_write(ROWS + 6, struct.pack('<H', n))
            for i, v in enumerate(values):
                uc.mem_write(ROWS + 8 + 0xEC * i, struct.pack('<i', v))
        else:
            self.stub_count = n
            for i, v in enumerate(values):
                uc.mem_write(ROWS + 0x20000 + 12 * i, struct.pack('<i', v))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12
        uc.mem_write(esp, struct.pack('<IiI', h.RET, key, TABLE))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    hits = sum(1 for c in cases if c['after']['regs'][0] != 0xFFFFFFFF)
    json.dump(dict(function='0x005B31A0', cases=cases), open(out, 'w'))
    print(f'{n} cases ({hits} found), {sum(len(c["events"]) for c in cases)} calls -> {out}')


main()
