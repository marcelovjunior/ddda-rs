"""L184 oracle: 0x00E53D10 skeleton->build_joint_matrices(owner, joints, count) (thiscall, ret 0xC) run
as the original's code in Unicorn on random joint chains (prints instructions no case reached). The
owner's virtual method 0x58 (called when the root index byte is 0xFF) is a stub recorded in the format
of crates/mtcore/tests/semantic.rs `abi_cases_full` (ECX = owner, 1 arg, returns a pointer to a random
root matrix). Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00E53D10, 0x00E54BAD - 0x00E53D10  # through RET 0xC at 0x00E54BAA .. (3 bytes)
LENGTH = 0x00E558FB - ENTRY
VSTUB = 0x30000058
THIS, OWNER, VTABLE, TABLE, JOINTS, SKELETON, ROOT = 0x20000000, 0x21000000, 0x21001000, 0x22000000, 0x23000000, 0x24000000, 0x25000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for a, size in ((ENTRY & ~0xFFF, 0x4000), (VSTUB & ~0xFFF, 0x1000), (THIS, 0x6000), (OWNER, 0x2000), (TABLE, 0x2000), (JOINTS, 0x8000), (SKELETON, 0x1000), (ROOT, 0x1000), (STACK, 0x10000), (h.RET, 0x1000), (0x01406000, 0x1000), (0x01561000, 0x1000), (0x0161C000, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(VSTUB, b'\xc3')
        uc.mem_write(0x01406CE0, struct.pack('<f', 1.0))
        uc.mem_write(0x01561738, struct.pack('<f', 0.0))
        uc.mem_write(0x0161C280, struct.pack('<IIII', 0x80000000, 0x80000000, 0x80000000, 0x80000000))
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
        if address != VSTUB:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 8))])
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, ROOT)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=VSTUB, before=before, after=dict(regs=self.regs(), memory=[])))

    def floats(self, n):
        rng = self.rng
        return b''.join(struct.pack('<f', rng.uniform(-3, 3) if rng.random() > 0.05 else rng.choice((0.0, 1.0, -1.0))) for _ in range(n))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        uc.mem_write(THIS, self.floats(0x6000 // 4))
        uc.mem_write(JOINTS, self.floats(0x8000 // 4))
        uc.mem_write(TABLE, self.floats(0x2000 // 4))
        uc.mem_write(ROOT, self.floats(0x1000 // 4))
        uc.mem_write(THIS + 0xCFA, bytes([rng.choice((0, 0, 1, 7))]))
        w32(OWNER, VTABLE)
        w32(VTABLE + 0x58, VSTUB)
        w32(OWNER + 0x364, TABLE)
        w32(JOINTS + 0x220, SKELETON)
        uc.mem_write(SKELETON + 0x91, bytes([rng.choice((0xFF, 0xFF, 0, 1, 2, 3))]))
        count = rng.choice((0, 1, 1, 2, 3, 4, 0xFFFFFFFF))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 16
        uc.mem_write(esp, struct.pack('<IIII', h.RET, OWNER, JOINTS, count))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 16)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=500000)
        except Exception:
            print('fault at eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), 'seed', seed, [hex(uc.reg_read(r)) for r in REGS]); raise
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00E53D10', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    print(f'{n} cases -> {out}; {len(unhit)} unhit {unhit[:20]}')


main()
