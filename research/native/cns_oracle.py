"""L184 s5: shared Unicorn oracle for the uCnsEdit matrix providers (0x00864660 ...).

Maps the original's code (functions, constant pages, CRT sinf/cosf) at their
real addresses, runs one function on a synthetic fixture and records, like
record_dispatch.py, the first value of every byte read, the final value of
every byte written, the registers before/after and each guest call to a stub
(`events`, the format of crates/mtcore/tests/semantic.rs `abi_cases_full`
plus `reads`). Scripts define the function, its fixture and its stubs.
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

REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
STACK, SP = 0x0F000000, 0x0F008000
# constant pages the providers read (identity matrix, epsilons, 0.5/2.0, sign mask, zero, one) and sinf/cosf with their tables
DATA_PAGES = (0x014F0000, 0x014F9000, 0x0150A000, 0x01521000, 0x01531000, 0x0161C000, 0x01629000, 0x012F0000, 0x0144B000)
IDENTITY = [1.0 if i % 5 == 0 else 0.0 for i in range(16)]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self, functions, arenas, stubs=None, data_pages=DATA_PAGES):
        """functions: [(address, length)] of real code; arenas: [(address, size)] of zero-filled RAM;
        stubs: {address: handler(oracle, esp) -> (nargs, eax, reads)} for guest calls recorded as events
        (the handler may call oracle.stub_write(address, bytes) to record the callee's memory effects)."""
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def map_range(address, size):
            for page in range(address & ~0xFFF, (address + size + 0xFFF) & ~0xFFF, 0x1000):
                if page not in mapped:
                    uc.mem_map(page, 0x1000)
                    mapped.add(page)
        for address, length in functions:
            map_range(address, length)
        for page in data_pages:
            map_range(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        for address, length in functions:
            uc.mem_write(address, h.code(address, length))
        self.functions = functions
        self.stubs = stubs or {}
        for address in self.stubs:
            map_range(address, 1)
            uc.mem_write(address, bytes([0xC3]))
        for address, size in list(arenas) + [(STACK, 0x10000), (h.RET, 0x1000)]:
            map_range(address, size)
        self.arenas = arenas
        self.speculative = []   # (address, size) the port may read although the original does not (fields of live objects)
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
        self.effects += [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
        elif address in self.stubs:
            esp = uc.reg_read(x.UC_X86_REG_ESP)
            nargs, eax, reads = self.stubs[address](self, esp)
            window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
            before = dict(regs=self.regs(), memory=window)
            back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
            uc.reg_write(x.UC_X86_REG_EAX, eax)
            uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
            uc.reg_write(x.UC_X86_REG_EIP, back)
            self.events.append(dict(target=address, before=before, reads=reads or [],
                                    after=dict(regs=self.regs(), memory=self.effects)))
            self.effects = []
        else:
            assert any(a <= address < a + n for a, n in self.functions), hex(address)

    def matrix(self, kind=None):
        rng = self.rng
        kind = kind or rng.choice(('rand', 'rand', 'rand', 'rot', 'zero', 'identity', 'sparse'))
        if kind == 'zero':
            v = [0.0] * 16
        elif kind == 'identity':
            v = IDENTITY
        elif kind == 'sparse':
            v = [rng.choice((0.0, 0.0, 1.0, -1.0, rng.uniform(-2, 2))) for _ in range(16)]
        elif kind == 'rot':
            import math
            a = rng.uniform(-3.2, 3.2)
            c, s = math.cos(a), math.sin(a)
            v = [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 1.0]
        else:
            v = [rng.choice((rng.uniform(-3, 3), rng.uniform(-3, 3), rng.uniform(-3, 3), 0.0, 1.0)) for _ in range(16)]
        return b''.join(f32(e) for e in v)

    def angle(self):
        """A radian angle: skipped (<= epsilon), small, ordinary, large (< 8192)."""
        rng = self.rng
        r = rng.random()
        if r < 0.15:
            return rng.choice((0.0, -0.5, 1e-8, 1.1920929e-07, 1.2e-07, -3.0))
        if r < 0.3:
            return rng.uniform(1e-6, 0.1)
        if r < 0.9:
            return rng.uniform(0.0, 6.5)
        return rng.uniform(0.0, 8190.0)

    def run_case(self, seed, setup, entry):
        """setup(oracle, rng) fills the arenas and returns (register list, stack words after the return address)."""
        rng = self.rng = random.Random(seed)
        for a, n in self.arenas:
            self.uc.mem_write(a, b'\0' * n)
        self.initial, self.written, self.events, self.effects = {}, set(), [], []
        regs, words = setup(self, rng)
        esp = regs[4]
        self.uc.mem_write(esp, struct.pack('<%dI' % (1 + len(words)), h.RET, *words))
        for reg, v in zip(REGS, regs):
            self.uc.reg_write(reg, v)
        self.remember(esp, 4 * (1 + len(words)))
        before = dict(regs=self.regs())
        self.uc.emu_start(entry, h.RET, count=300000)
        for a, n in self.speculative:
            self.remember(a, n)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, mode=0, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)

    def record(self, name, entry, setup, out, n):
        cases = [self.run_case(seed, setup, entry) for seed in range(n)]
        json.dump(dict(function=name, cases=cases), open(out, 'w'))
        print(f'{n} cases, {sum(len(c["events"]) for c in cases)} calls -> {out}', flush=True)
