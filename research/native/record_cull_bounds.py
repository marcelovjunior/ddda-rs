"""L184 oracle: 0x00EA42E0 node->cull_bounds(culler, owner, planes) (thiscall, ret 0xC) run as
the original's code in Unicorn on random nodes (all kinds: sphere / box / plain, with and without
the transform step; the script prints the instructions no case reached). Callees are stubs recorded
in the format of crates/mtcore/tests/semantic.rs `abi_cases_full` (ECX = culler except 0x433B00 /
0x5B19C0 which get a stack buffer they fill); events carry `check`: the bytes the callee reads of
what the original wrote (the sphere, the corner buffer, the combined matrix).
Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00EA42E0, 0x00EA531D - 0x00EA42E0
SPHERE, BOX, ORIENTED, BUILD, CORNERS = 0x00FF0800, 0x00FF0D20, 0x00FF0AD0, 0x00433B00, 0x005B19C0
STUBS = {SPHERE: 2, BOX: 2, ORIENTED: 2, BUILD: 1, CORNERS: 1}
NODE, OWNER, SHAPE, CULLER = 0x20000000, 0x21000000, 0x22000000, 0x23000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(STUBS) + [ENTRY, ENTRY + LENGTH]}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((NODE, 0x1000), (OWNER, 0x1000), (SHAPE, 0x1000), (CULLER, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
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

    def rfloat(self):
        rng = self.rng
        if rng.random() < 0.1:
            return rng.choice((0.0, 1.0, -1.0, 0.5, 2.0))
        return rng.uniform(-8, 8) * rng.choice((1, 1, 0.01, 100))

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        nargs = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        rng = self.rng
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        back, arg0 = word(esp), word(esp + 4)
        effects, eax = [], rng.randrange(1 << 32)
        if address in (SPHERE, BOX, ORIENTED):
            length = {SPHERE: 16, BOX: 0x20, ORIENTED: 64}[address]
            if arg0 >= STACK:  # a buffer the original built on its stack
                before['check'] = [(a, uc.mem_read(a, 1)[0]) for a in range(arg0, arg0 + length) if a in self.written]
            eax = rng.choice((0, 1, 2, 5, 0x3F, 0xFFFFFFFF, 0x80000000, rng.randrange(1 << 32), 0x20))
        elif address == BUILD:
            ecx = uc.reg_read(x.UC_X86_REG_ECX)
            effects += self.stub_write(ecx, b''.join(struct.pack('<f', self.rfloat()) for _ in range(16)))
        elif address == CORNERS:
            ecx = uc.reg_read(x.UC_X86_REG_ECX)
            effects += self.stub_write(ecx, b''.join(struct.pack('<f', self.rfloat()) for _ in range(8)))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base in (NODE, OWNER, SHAPE):
            uc.mem_write(base, b'\0' * 0x1000)
        uc.mem_write(STACK + 0x8000 - 0x400, bytes(rng.randrange(256) for _ in range(0x400)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v))
        low = rng.choice((3, 3, 3, 3, 1, 7, 0x2A, 0x23))
        bits = 0
        for b in (0x40, 0x80, 0x100):
            if rng.random() < 0.45:
                bits |= b
        w32(NODE + 0x108, low | bits | (rng.randrange(1 << 20) << 9 & ~0x1C0 & ~0x3F))
        for off in range(0xB0, 0xF0, 4):
            uc.mem_write(NODE + off, struct.pack('<f', self.rfloat()))
        w32(OWNER + 0x2C, SHAPE)
        for off in (0x10, 0x14, 0x18, 0x1C):
            uc.mem_write(SHAPE + off, struct.pack('<f', self.rfloat()))
        planes = rng.choice((0x3F, 1, 0x20, 0x15, rng.randrange(0x3F)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 16
        uc.mem_write(esp, struct.pack('<IIII', h.RET, CULLER, OWNER, planes))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = NODE, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 16)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 5000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00EA42E0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{n} cases -> {out}', by)
    print(len(unhit), 'unhit', unhit[:30])


main()
