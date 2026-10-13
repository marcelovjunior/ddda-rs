"""L179 oracle: 0x00DD42C0 `this->dispatch(event)` run as the original's
code in Unicorn on synthetic listener tables. Its two guest calls are
stubs: 0x00DCC3E0 `this->deliver(listener, event)` (ret 8) writes random
bytes into the listeners and sometimes changes the listener count, and
0x00F73B90 `owner->find(id)` (ret 4) returns 0 or a record whose i16 at
+0x1c is the parent id. Records the first value of every byte read, the
final value of every byte written (the stubs' writes included), the
registers before/after and, per call, the registers and argument window
before and the effects and registers after (the format of
crates/mtcore/tests/semantic.rs `abi_cases_full`).
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

ENTRY, LENGTH = 0x00DD42C0, 0x118  # through RET 0x4 at 0x00DD43D5
DELIVER, FIND = 0x00DCC3E0, 0x00F73B90
FLOOR = 0x01561738
THIS, THIS_SIZE = 0x20000000, 0x50000
EVENT = 0x21000000
RECORDS = 0x22000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
COUNT, LEVELS = THIS + 0x4CD48, THIS + 0x4CD50


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x2000), (DELIVER & ~0xFFF, 0x1000), (FIND & ~0xFFF, 0x1000),
                              (FLOOR & ~0xFFF, 0x1000), (THIS, THIS_SIZE), (EVENT, 0x1000),
                              (RECORDS, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(FLOOR, h.code(FLOOR, 4))
        uc.mem_write(DELIVER, b'\xc3')
        uc.mem_write(FIND, b'\xc3')
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
        if address not in (DELIVER, FIND):
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = 2 if address == DELIVER else 1
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        rng = self.rng
        if address == DELIVER:
            listener = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
            if rng.random() < 0.5:
                effects += self.stub_write(listener + 0x20 + rng.randrange(8), bytes([rng.randrange(256)]))
            if rng.random() < 0.2:
                count = struct.unpack('<I', uc.mem_read(COUNT, 4))[0]
                effects += self.stub_write(COUNT, struct.pack('<I', max(0, min(self.listeners, count + rng.choice((-1, 1))))))
            eax = rng.randrange(1 << 32)
        else:
            choice = rng.random()
            if choice < 0.25:
                eax = 0
            else:
                record = RECORDS + 0x40 * rng.randrange(8)
                ident = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
                parent = rng.choice((ident & 0xFFFF, 0xFFFF, rng.randrange(6), rng.randrange(0x10000)))
                effects += self.stub_write(record + 0x1C, struct.pack('<H', parent))
                eax = record
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, b'\0' * THIS_SIZE)
        uc.mem_write(EVENT, b'\0' * 0x1000)
        uc.mem_write(RECORDS, b'\0' * 0x1000)
        self.listeners = rng.randrange(0, 10)
        owners = [0x1111, 0x2222, 0x3333]
        for i in range(self.listeners):
            base = THIS + 0x30 + 0x6E0 * i
            weight = rng.choice((0.0, -1.0, 0.5, 2.0, float('nan'), 1e-30))
            fields = {4: rng.choice((0, 0, 7)), 8: rng.choice((0, 1, 2, 3, 5)), 0xC: rng.choice(owners),
                      0x14: rng.randrange(6), 0x18: rng.randrange(3)}
            for off, v in fields.items():
                uc.mem_write(base + off, struct.pack('<I', v))
            uc.mem_write(base + 0xC8, struct.pack('<f', weight))
        uc.mem_write(COUNT, struct.pack('<I', self.listeners))
        uc.mem_write(LEVELS, struct.pack('<I', rng.randrange(4)))
        uc.mem_write(EVENT + 4, struct.pack('<I', rng.choice((rng.randrange(6), 0xFFFFFFFF))))
        uc.mem_write(EVENT + 8, struct.pack('<I', rng.choice((rng.randrange(3), 0xFFFFFFFF))))
        uc.mem_write(EVENT + 0x30, struct.pack('<I', rng.choice(owners + [0])))
        uc.mem_write(EVENT + 0x2C, bytes([rng.choice((0, 1, 1))]))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<II', h.RET, EVENT))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        # the stack bytes the call wrote, as the caller left them
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        self.remember(FLOOR, 4)  # the port reads the floor once up front
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 1500
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00DD42C0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
