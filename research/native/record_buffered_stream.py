"""L185 s7 oracle: the loader's buffered input stream, run as the original's code
in Unicorn: 0x00CFEEC0 skip(count) (thiscall, ret 4), 0x00CFEC00 read(dst, count)
(thiscall, ret 8) and 0x00CFE9D0 refill() (fastcall). The underlying stream's
virtual methods (vtable +0x10 state object, +0x24 size, +0x30 read(ptr, count),
+0x44 tell) are stubs recorded as guest calls (format of
crates/mtcore/tests/semantic.rs abi_cases_full); the CRT memcpy runs as original code.

Usage: record_buffered_stream.py skip|read|refill OUT [N]
Output: E:/ddda-re/notes/native-L184/stream_<mode>.json (private).
"""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x

ENTRIES = {'skip': 0x00CFEEC0, 'read': 0x00CFEC00, 'refill': 0x00CFE9D0}
THIS, BUF, DST = 0x20000000, 0x20010000, 0x20020000
STREAM, VTABLE, STATE_OBJ = 0x22000000, 0x22001000, 0x22002000
STUBS = {0x10: 0x30000010, 0x24: 0x30000024, 0x30: 0x30000030, 0x44: 0x30000044}
RETRY_GLOBAL = 0x018D0230
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, mode):
        self.mode = mode
        self.entry = ENTRIES[mode]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, secs = h.image()
        for va, b in secs:
            s = va & ~0xFFF
            uc.mem_map(s, ((va + len(b) - s) + 0xFFF) & ~0xFFF)
            uc.mem_write(va, b)
        for a, s in ((THIS, 0x40000), (STREAM, 0x4000), (0x30000000, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
        for stub in STUBS.values():
            uc.mem_write(stub, b'\xc3')
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
        slot = next((k for k, v in STUBS.items() if v == address), None)
        if slot is None:
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = 2 if slot == 0x30 else 0
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        if slot == 0x30:
            ptr, count = struct.unpack('<II', uc.mem_read(esp + 4, 8))
            n = {'full': count, 'short': rng.randrange(0, count + 1), 'empty': 0}[self.stream_kind] if rng.random() < 0.85 else count
            n = min(n, count)
            if n:
                effects += self.stub_write(ptr, bytes(rng.randrange(256) for _ in range(n)))
            eax = n
        elif slot == 0x10:
            eax = STATE_OBJ
        elif slot == 0x44:
            self.tell = rng.randrange(1 << 20)
            eax = self.tell
        else:
            eax = self.tell if rng.random() < 0.4 else self.tell + rng.randrange(1, 100)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        self.tell = 0
        capacity = rng.choice((16, 64, 256, 1000, 4096))
        valid = 0 if rng.random() < 0.1 else rng.randrange(0, capacity + 1)
        position = rng.randrange(0, valid + 1)
        self.stream_kind = rng.choices(('full', 'short', 'empty'), (60, 30, 10))[0]
        uc.mem_write(BUF, bytes(rng.randrange(256) for _ in range(0x2000)))
        uc.mem_write(DST, bytes(rng.randrange(256) for _ in range(0x4000)))
        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x40)))
        uc.mem_write(THIS + 4, struct.pack('<IIIII', STREAM, BUF, position, valid, capacity))
        uc.mem_write(STREAM, struct.pack('<I', VTABLE))
        for slot, stub in STUBS.items():
            uc.mem_write(VTABLE + slot, struct.pack('<I', stub))
        g = rng.randrange(1 << 32)
        uc.mem_write(RETRY_GLOBAL, struct.pack('<I', g))
        uc.mem_write(STATE_OBJ + 0x1c, struct.pack('<I', g if rng.random() < 0.7 else g ^ 1))
        uc.mem_write(STACK, bytes(rng.randrange(256) for _ in range(0x10000)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 16 - 4 * rng.randrange(4)
        if self.mode == 'skip':
            count = rng.choice((0, 1, 2, rng.randrange(0, 40), rng.randrange(0, 600), rng.randrange(0, 3000)))
            if self.stream_kind == 'empty':
                count = min(count, 40)
            args = struct.pack('<I', count)
        elif self.mode == 'read':
            count = rng.choice((0, 1, rng.randrange(0, 40), rng.randrange(0, capacity + 100), rng.randrange(0, 3000)))
            args = struct.pack('<II', DST + rng.randrange(0, 64), count)
        else:
            args = b''
        uc.mem_write(esp, struct.pack('<I', h.RET) + args)
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4 + len(args))
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=5000000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    o = Oracle(mode)
    cases = [o.case(s) for s in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function=hex(ENTRIES[mode]), cases=cases), open(out, 'w'))
    print(f'{mode}: {n} cases, {calls} guest calls -> {out}')


main()
