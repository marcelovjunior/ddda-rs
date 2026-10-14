"""L172 P2b (s9) oracle: the sPrimitive pools' state-stack helpers run as the
original's code in Unicorn on random pool/owner objects: push_state
0x0111D610 and pop_state 0x0111D650 (with the owner helpers 0x01192260 /
0x01192290 as real code), the owner helpers alone, and the kind-3 job
thunk 0x010B7440 (a tail jump through the vtable slot +0x3C, to a stub
recorded as a call). Output is the abi_cases format of
crates/mtcore/tests/shadow_primitive.rs.
Usage: record_state_stack.py <out.json> <n> <push|pop|owner_push|owner_pop|thunk>
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

KINDS = {'push': (0x0111D610, 0x3A), 'pop': (0x0111D650, 0x1F), 'owner_push': (0x01192260, 0x2D),
         'owner_pop': (0x01192290, 0x30), 'thunk': (0x010B7440, 0x8)}
REAL = ((0x0111D610, 0x3A), (0x0111D650, 0x1F), (0x01192260, 0x2D), (0x01192290, 0x30), (0x010B7440, 0x8))
SET, OWNER, VT, STUB = 0x20000000, 0x21000000, 0x22000000, 0x30000040
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, kind):
        self.kind = kind
        self.entry, _ = KINDS[kind]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()
        for address, size in ((0x0111D000, 0x1000), (0x01192000, 0x1000), (0x010B7000, 0x1000), (SET, 0x1000), (OWNER, 0x1000),
                              (VT, 0x1000), (0x30000000, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            for page in range(address, address + size, 0x1000):
                if page not in mapped:
                    uc.mem_map(page, 0x1000)
                    mapped.add(page)
        for a, n in REAL:
            uc.mem_write(a, h.code(a, n))
        uc.mem_write(STUB, b'\xc3')
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
        if address == STUB:
            esp = uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(esp, 12)
            window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 12))]
            before = dict(regs=self.regs(), memory=window)
            back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
            uc.reg_write(x.UC_X86_REG_EAX, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, esp + 8)   # the unit consumer pops one argument
            uc.reg_write(x.UC_X86_REG_EIP, back)
            self.events.append(dict(target=STUB, before=before, after=dict(regs=self.regs(), memory=[])))
            return
        assert any(a <= address < a + n for a, n in REAL), hex(address)
        self.visited.add(address)

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base in (SET, OWNER, VT):
            uc.mem_write(base, rng.randbytes(0x1000))
        self.w32(SET + 4, OWNER)
        self.w32(SET + 0x54, rng.choice((0, 1, 5, 6, 7, rng.randrange(0, 9))))
        self.w32(OWNER + 0x64, rng.choice((0, 1, 5, 6, 7, rng.randrange(0, 9))))
        self.w32(OWNER, VT)
        self.w32(VT + 0x3C, STUB)
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        this = {'push': SET, 'pop': SET, 'owner_push': OWNER, 'owner_pop': OWNER, 'thunk': OWNER}[self.kind]
        args = {'push': [rng.getrandbits(32)], 'thunk': [rng.getrandbits(32)]}.get(self.kind, [])
        uc.mem_write(esp, struct.pack('<%dI' % (1 + len(args)), h.RET, *args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = this, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        self.remember(esp, 4 * (1 + len(args)))
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=100000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET, 'instruction limit'
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n, kind = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    oracle = Oracle(kind)
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    json.dump(dict(function=hex(oracle.entry), cases=cases), open(out, 'w'))
    print(f'{kind}: {n} cases, {len(visited)} instruction addresses visited')


main()
