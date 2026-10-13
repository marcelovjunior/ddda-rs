"""L184 oracle: 0x00E6D070 count_active_children (fastcall ECX=owner, plain ret)
run as the original's code in Unicorn on random child lists. The virtual
method 0x44 of each node is a stub (returns random AL), recorded in the format
of crates/mtcore/tests/semantic.rs `abi_cases_full` (ECX = node).
Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00E6D070, 0x00E6D0B4 - 0x00E6D070
OWNER, NODES, VTABLE, STUB = 0x20000000, 0x21000000, 0x22000000, 0x30000044
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for a, size in ((ENTRY & ~0xFFF, 0x1000), (STUB & ~0xFFF, 0x1000), (OWNER, 0x1000), (NODES, 0x2000), (VTABLE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(STUB, b'\xc3')
        uc.mem_write(VTABLE + 0x44, struct.pack('<I', STUB))
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
        if address != STUB:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4))])
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, self.rng.choice((0, 1, 0x100, self.rng.randrange(1 << 32))))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((OWNER, 0x1000), (NODES, 0x2000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x100, bytes(rng.randrange(256) for _ in range(0x100)))
        n = rng.choice((0, 1, 2, 3, 6, 20))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v))
        w32(OWNER + 0x1F0, NODES if n else 0)
        uc.mem_write(OWNER + 0x1E2, struct.pack('<H', rng.randrange(1 << 16)))
        for i in range(n):
            node = NODES + 0x40 * i
            w32(node, VTABLE)
            w32(node + 8, node + 0x40 if i + 1 < n else 0)
            uc.mem_write(node + 0xC, bytes([rng.choice((0, 1, 1, 2, 3, 0xFF))]))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 4
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = OWNER, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00E6D070', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    print(f'{n} cases, {sum(len(c["events"]) for c in cases)} calls -> {out}; unhit {unhit}')


main()
