"""L184 s7 oracle: 0x00F2FFC0 (thiscall, ret 0x18, float10 result in ST0): the
distance-falloff integral, run as the original's code in Unicorn (its calls to the
CRT expf 0x012F4D01 run as original code too). Records, besides registers and
memory, the final ST0 (64-bit mantissa, sign/exponent word) and the x87 depth.
Inputs: finite floats only; the step count is small and non-negative.
Output: E:/ddda-re/notes/native-L184/integrate_falloff.json. Pyenv python."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x

ENTRY = 0x00F2FFC0
THIS, POS, VEL = 0x20000000, 0x20001000, 0x20002000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
f32 = lambda v: struct.pack('<f', v)

class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, secs = h.image()
        for va, b in secs:
            start = va & ~0xFFF
            uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
            uc.mem_write(va, b)
        for a, s in ((THIS, 0x4000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
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

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x400)))
        steps = rng.choice((0, 1, 2, 3, 5, 8, 16, rng.randrange(1, 40), rng.randrange(1, 40)))
        uc.mem_write(THIS + 0x3AC, struct.pack('<I', steps))
        uc.mem_write(POS, f32(rng.uniform(-8, 8)) + f32(rng.uniform(-8, 8)))
        uc.mem_write(VEL, f32(rng.choice((0.0, rng.uniform(-6, 6)))) + f32(rng.uniform(-6, 6)))
        total = rng.choice((rng.uniform(0, 4), rng.uniform(-2, 2), 1.0, 0.0))
        radius = rng.choice((rng.uniform(0, 6), 0.0, 1.5))
        scale = rng.choice((rng.uniform(0.1, 10), 1.0, 0.5))
        uc.mem_write(STACK, bytes(rng.randrange(256) for _ in range(0x10000)))
        self.initial, self.written = {}, set()
        esp = SP - 28 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<I', h.RET) + struct.pack('<II', POS, VEL) + f32(total) + f32(radius) + f32(scale) + struct.pack('<I', rng.randrange(1 << 32)))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        self.remember(esp, 28)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=2000000)
        fpsw = uc.reg_read(x.UC_X86_REG_FPSW)
        st0 = uc.reg_read(x.UC_X86_REG_FP0 + ((fpsw >> 11) & 7))
        depth = (8 - ((fpsw >> 11) & 7)) & 7  # stack top from the status word: TOP=7 means one value
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)], st0=[st0[0], st0[1]], top=(fpsw >> 11) & 7)
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)

def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    o = Oracle()
    cases = [o.case(s) for s in range(n)]
    json.dump(dict(function='0x00F2FFC0', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}', cases[0]['after']['st0'], cases[0]['after']['top'])

main()
