"""L172 P2d oracle: 0x00EE2C90 shadow->draw(ctx) (thiscall, ret 4) and 0x00DB7620 unit->run_job(arg)
(thiscall, ret 4; a tail jmp to 0x00BFC7D0) run as the original's code in Unicorn on random inputs.
Callees are stubs recorded in the format of the `abi_cases_full` harness (ECX = their object).
usage: record_shadow_wrappers.py draw|job OUT [N]. Output is private (E:/ddda-re/notes/native-L172-p2d)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

MODES = {
    # entry, length, stub address -> stack words popped (args)
    'draw': (0x00EE2C90, 0x00EE2CBC - 0x00EE2C90, {0x00EDE9C0: 1, 0x31000060: 1}),
    'job': (0x00DB7620, 0x00DB7650 - 0x00DB7620, {0x00DA7DB0: 0, 0x31000020: 1, 0x00BFC7D0: 1}),
}
THIS, CTX, VTABLE, SCHED = 0x20000000, 0x21000000, 0x22000000, 0x23000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, mode):
        self.mode = mode
        self.entry, self.length, self.stubs = MODES[mode]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(self.stubs) + [self.entry, self.entry + self.length, 0x018D08F4]}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((THIS, 0x1000), (CTX, 0x1000), (VTABLE, 0x1000), (SCHED, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(self.entry, h.code(self.entry, self.length))
        for a in self.stubs:
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

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in self.stubs:
            assert self.entry <= address < self.entry + self.length, hex(address)
            self.hit.add(address)
            return
        nargs = self.stubs[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        eax = self.rng.choice((0, 1, 0x100, 0x1FF, self.rng.randrange(1 << 32))) if address == 0x00DA7DB0 else self.rng.randrange(1 << 32)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, b'\0' * 0x1000)
        uc.mem_write(CTX, b'\0' * 0x1000)
        uc.mem_write(STACK + 0x8000 - 0x100, bytes(rng.randrange(256) for _ in range(0x100)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        w32(THIS, VTABLE)
        w32(VTABLE + 0x60, 0x31000060)
        w32(VTABLE + 0x2C, 0x31000020)
        w32(0x018D08F4, SCHED)
        uc.mem_write(CTX + 0x16A, bytes([rng.choice((0, 1, 1, 3, 0xFF)), rng.randrange(256)]))
        uc.mem_write(THIS + 0x78, bytes([rng.choice((0, 0, 1, 7))]))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        arg = CTX if self.mode == 'draw' else rng.randrange(1 << 32)
        uc.mem_write(esp, struct.pack('<II', h.RET, arg))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    oracle = Oracle(mode)
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function=hex(oracle.entry), cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(oracle.entry, oracle.length), oracle.entry) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{mode}: {n} cases -> {out}', by, f'{len(unhit)} unhit', unhit[:20])


main()
