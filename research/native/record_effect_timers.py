"""L184 oracle: 0x005D1AF0 `update(timers, unit)` (stdcall, ret 8), the
per-frame update of a unit's periodic effect timers, run as the original's
code in Unicorn on randomized timer tables and unit slots covering every
branch (the script prints the instructions no case reached).

The callee 0x004241F0 (effect-request initialiser) runs as original code;
its only guest-call-like callee 0x00429680 is a stub (ret 0xc) that returns a
random EAX, sometimes rewrites the slot's current time and the record's
next-fire time, and is recorded in the format of crates/mtcore/tests/
semantic.rs `abi_cases_full` plus a `check` list: the bytes of the 0x68-byte
request the original wrote (the callee reads them).
Output is private (E:/ddda-re/notes/native-L184). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe.
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

ENTRY, LENGTH = 0x005D1AF0, 0x2BB  # through RET 0x8 at 0x005D1DA8
INIT, INIT_LEN = 0x004241F0, 0xE5  # through RET at 0x004242D4
SINK = 0x00429680
GLOBALS = 0x018C0000  # 0x018CFE80.., 0x018D2818 and 0x01629510.., 0x0161C20C live in two pages
P1, RECS, P2 = 0x20000000, 0x20001000, 0x21000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
NAN, NEG_NAN = 0x7FC00000, 0xFFC12345
POOL = [0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, -1.0, -0.0]


def f(v):
    return struct.pack('<f', v)


def bits(rng, nan=False):
    """A float's bits: from the pool, or (nan) sometimes a quiet NaN."""
    if nan and rng.random() < 0.12:
        return rng.choice((NAN, NEG_NAN))
    return struct.unpack('<I', f(rng.choice(POOL)))[0]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x1000), (INIT & ~0xFFF, 0x1000), (SINK & ~0xFFF, 0x1000),
                              (0x018CF000, 0x4000), (0x01629000, 0x1000), (0x0161C000, 0x1000),
                              (P1, 0x1000), (RECS, 0x2000), (P2, 0x4000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(INIT, h.code(INIT, INIT_LEN))
        uc.mem_write(SINK, b'\xc3')
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        self.hit = set()

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
        if address != SINK:
            assert ENTRY <= address < ENTRY + LENGTH or INIT <= address < INIT + INIT_LEN, hex(address)
            self.hit.add(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 16))]
        before = dict(regs=self.regs(), memory=window)
        request = struct.unpack('<I', uc.mem_read(esp + 12, 4))[0]
        # the bytes of the request the original wrote (the callee reads them)
        check = [(a, uc.mem_read(a, 1)[0]) for a in range(request, request + 0x68) if a in self.written]
        before['check'] = check
        rng = self.rng
        effects = []
        if rng.random() < 0.3:  # the callee changes the unit's current time
            effects += self.stub_write(self.slot_now[self.index_now()], f(rng.choice(POOL)))
        if rng.random() < 0.2:
            record = uc.reg_read(x.UC_X86_REG_ESI)
            effects += self.stub_write(record + 0x6C, f(rng.choice(POOL)))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 16)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def index_now(self):
        # slot index of the current iteration: the request's +0x64 word
        esp = self.uc.reg_read(x.UC_X86_REG_ESP)
        request = struct.unpack('<I', self.uc.mem_read(esp + 12, 4))[0]
        return struct.unpack('<I', self.uc.mem_read(request + 0x64, 4))[0]

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((P1, 0x1000), (RECS, 0x2000), (P2, 0x4000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x800, bytes(rng.randrange(256) for _ in range(0x800)))
        count = rng.choice((0, 1, 1, 2, 3, 5, 7))
        slots = rng.choice((0, 1, 2, 3, 5, 8, 8, 9, 20, 0xFFFFFFFF))
        uc.mem_write(P1 + 4, struct.pack('<II', RECS, count))
        uc.mem_write(P2 + 0x374, struct.pack('<I', slots))
        range_on = rng.random() < 0.5
        uc.mem_write(P2 + 0x1FCC + 8, bytes([rng.choice((0, 1, 2)) if not range_on else 1, 1 if range_on else rng.choice((0, 1))]))
        uc.mem_write(P2 + 0x1FCC + 0x20, struct.pack('<II', rng.choice((0, 1, 2, 3, 0xFF, 7)), rng.choice((0, 1, 2, 3, 5))))
        uc.mem_write(GLOBALS + 0xFE80, struct.pack('<fff', 1.0, 2.0, 3.0))
        uc.mem_write(GLOBALS + 0xFE80 + 8, struct.pack('<f', rng.choice(POOL)))
        uc.mem_write(0x01629510, struct.pack('<fff', 0.25, 0.5, rng.choice(POOL)))
        uc.mem_write(0x0161C20C, f(-1.0))
        uc.mem_write(0x018D2818, struct.pack('<I', rng.randrange(1 << 32)))
        self.slot_now = {}
        types = (1, 2, 3)
        for s in range(8):
            slot = P2 + 0x380 + 0x140 * s
            now = bits(rng, nan=True)
            self.slot_now[s] = slot + 0x4C
            uc.mem_write(slot + 4, struct.pack('<H', rng.choice(types)) + bytes([rng.randrange(256), rng.randrange(256)]))
            uc.mem_write(slot + 0x4C, struct.pack('<I', now))
            uc.mem_write(slot + 0x58, struct.pack('<I', bits(rng, nan=True)))
        for r in range(count):
            rec = RECS + 0x70 * r
            state = rng.choice((0xFF, 0xFF, 0, 1, 2, 3, rng.randrange(8), 0x100))
            ident = rng.choice((1, 2, 3, 0x10001, 0x10002))
            uc.mem_write(rec + 4, struct.pack('<II', ident, state))
            uc.mem_write(rec + 0xC, struct.pack('<I', bits(rng)))  # start delay (no NaN: it is added)
            uc.mem_write(rec + 0x10, struct.pack('<I', bits(rng, nan=True)))
            uc.mem_write(rec + 0x14, struct.pack('<I', bits(rng)))  # period (added)
            uc.mem_write(rec + 0x18, bytes(rng.choice((0, 1, 2, 3)) for _ in range(5)))
            uc.mem_write(rec + 0x20, struct.pack('<III', rng.randrange(1 << 32), rng.randrange(1 << 32), rng.randrange(1 << 32)))
            for s in range(8):
                uc.mem_write(rec + 0x2C + 4 * s, struct.pack('<I', rng.choice((0, 0, 1, 5))))
                uc.mem_write(rec + 0x4C + 4 * s, struct.pack('<I', bits(rng, nan=True)))
            uc.mem_write(rec + 0x6C, struct.pack('<I', bits(rng, nan=True)))
        p2 = rng.choice((P2, P2, P2, P2, 0))
        p1 = P1
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12
        uc.mem_write(esp, struct.pack('<III', h.RET, p1, p2))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x005D1AF0', cases=cases), open(out, 'w'))
    code = h.code(ENTRY, LENGTH)
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(code, ENTRY) if i.address not in oracle.hit]
    print(f'{n} cases, {calls} calls ({sum(1 for c in cases if c["events"])} cases with calls) -> {out}')
    print('unhit instructions:', unhit)


main()
