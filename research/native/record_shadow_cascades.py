"""L172 P2d oracle: 0x00EFDC70 shadow->draw_cascades(ctx) (thiscall, ret 4) run as the original's code
in Unicorn on random shadows (prints instructions no case reached). Callees are stubs recorded in the
format of crates/mtcore/tests/*.rs `abi_cases_full`: 0x00EE5CE0 (ECX = shadow, 6 args, ret 0x18, fills
its two matrices and an extra block; returns a float in ST0 which the original drops), 0x00EFD240 (3 args;
`check` = the bytes of its pass record the original wrote) and 0x00EEA950 (2 args).
Output is private (E:/ddda-re/notes/native-L172-p2d)."""
import json, math, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00EFDC70, 0x00EFDFCA - 0x00EFDC70  # through RET 0x4 at 0x00EFDFC7
BUILD, SETUP, FINISH = 0x00EE5CE0, 0x00EFD240, 0x00EEA950
STUBS = {BUILD: 6, SETUP: 3, FINISH: 2}
THIS, CTX = 0x20000000, 0x22000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def fp80(v):
    if v == 0:
        return (0, 0)
    mant, exp = math.frexp(abs(v))
    return (int(mant * (1 << 64)), (0x8000 if v < 0 else 0) | (exp - 1 + 16383))


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(STUBS) + [ENTRY, ENTRY + LENGTH]}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((THIS, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
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

    def push_st0(self, value):
        uc = self.uc
        fpsw = uc.reg_read(x.UC_X86_REG_FPSW)
        top = ((fpsw >> 11) & 7) - 1 & 7
        uc.reg_write(x.UC_X86_REG_FPSW, (fpsw & ~0x3800) | (top << 11))
        uc.reg_write(x.UC_X86_REG_FPTAG, uc.reg_read(x.UC_X86_REG_FPTAG) & ~(3 << (2 * top)) & 0xFFFF)
        uc.reg_write(x.UC_X86_REG_FP0 + top, fp80(value))

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
        back = word(esp)
        effects, st0 = [], None
        rnd = lambda n: b''.join(struct.pack('<f', rng.uniform(-4, 4)) for _ in range(n))
        if address == BUILD:
            a0, a1, a2 = word(esp + 4), word(esp + 8), word(esp + 12)
            effects += self.stub_write(a0, rnd(16))
            effects += self.stub_write(a1, rnd(16))
            effects += self.stub_write(a2, rnd(3))
            st0 = rng.choice((0.5, 1.0, 2.5, -3.0))
            self.push_st0(st0)
        elif address == SETUP:
            record = word(esp + 8)
            before['check'] = [(a, uc.mem_read(a, 1)[0]) for a in range(record, record + 0x130) if a in self.written]
        uc.reg_write(x.UC_X86_REG_EAX, rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        after = dict(regs=self.regs(), memory=effects)
        if st0 is not None:
            after['st0'] = struct.unpack('<I', struct.pack('<f', st0))[0]
        self.events.append(dict(target=address, before=before, after=after))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, b'\0' * 0x1000)
        uc.mem_write(STACK + 0x8000 - 0x800, bytes(rng.randrange(256) for _ in range(0x800)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        w32(THIS + 0x30, rng.choice((0, 0x22100000, 0x22100000, 0x22100000)))
        uc.mem_write(THIS + 0x7B, bytes([rng.choice((0, 1, 1, 1))]))
        uc.mem_write(THIS + 0x170, bytes([rng.choice((0, 1, 1))]))
        w32(THIS + 0x174, rng.choice((0, 1, 2, 3, 4)))
        w32(THIS + 0x3C, rng.randrange(1 << 32))
        w32(THIS + 0x74, rng.choice((0, 1, 5, 40, rng.randrange(1 << 20))))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<II', h.RET, CTX + rng.randrange(1 << 8) * 16))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=300000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00EFDC70', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{n} cases -> {out}', by, f'{len(unhit)} unhit', unhit[:20])


main()
