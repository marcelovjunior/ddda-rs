"""L184 s7 oracle: 0x00D1B840 `bool segment_vs_box(box, segment, float *t)` (cdecl)
run as the original's code in Unicorn on random boxes and segments: hits,
misses, axis-parallel segments, zero-length segments (point test), null and
non-null out pointers. No guest calls. No NaN/inf inputs (Unicorn and real SSE
pick different NaNs when two meet).
Output: E:/ddda-re/notes/native-L184/segment_vs_box.json (format of
crates/mtcore/tests/semantic.rs abi_cases_full). Run with the pyenv python."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x

ENTRY, LENGTH = 0x00D1B840, 0x412  # through RET at 0x00D1BC51
BOX, SEG, OUT = 0x20000000, 0x20001000, 0x20002000
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
        for a, s in ((BOX, 0x4000), (STACK, 0x10000), (h.RET, 0x1000)):
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
        c = lambda: rng.uniform(-10, 10)
        lo = [c(), c(), c()]
        size = [rng.choice((0.5, 1, 3, 8)) * rng.random() + 0.01 for _ in range(3)]
        hi = [lo[k] + size[k] for k in range(3)]
        if rng.random() < 0.1:
            hi[rng.randrange(3)] = lo[0]  # flat box
        mode = rng.choices(('random', 'toward', 'parallel', 'point', 'inside', 'tiny'), (30, 30, 15, 8, 10, 7))[0]
        inside = [rng.uniform(lo[k], hi[k]) for k in range(3)]
        outside = [c() for _ in range(3)]
        if mode == 'random':
            a, b = [c() for _ in range(3)], [c() for _ in range(3)]
        elif mode == 'toward':
            a = [c() for _ in range(3)]
            b = [inside[k] + rng.uniform(-1, 1) * rng.choice((0.1, 1, 5)) for k in range(3)]
        elif mode == 'parallel':
            a = [c() for _ in range(3)]
            b = list(a)
            axis = rng.randrange(3)
            b[axis] += rng.uniform(-15, 15)
            if rng.random() < 0.6:
                a[(axis + 1) % 3] = inside[(axis + 1) % 3]
                b[(axis + 1) % 3] = a[(axis + 1) % 3]
                a[(axis + 2) % 3] = inside[(axis + 2) % 3]
                b[(axis + 2) % 3] = a[(axis + 2) % 3]
            if rng.random() < 0.3:
                b[(axis + 1) % 3] += rng.uniform(-1e-3, 1e-3)  # nearly parallel
        elif mode == 'point':
            a = inside if rng.random() < 0.5 else outside
            b = list(a)
        elif mode == 'inside':
            a = inside
            b = [inside[k] + rng.uniform(-0.3, 0.3) for k in range(3)]
        else:
            a = inside if rng.random() < 0.5 else outside
            b = [a[k] + rng.uniform(-2e-4, 2e-4) for k in range(3)]
        # box and segment: 16-byte rows (w pad = garbage)
        uc.mem_write(BOX, f32(lo[0]) + f32(lo[1]) + f32(lo[2]) + f32(rng.uniform(-3, 3)) + f32(hi[0]) + f32(hi[1]) + f32(hi[2]) + f32(rng.uniform(-3, 3)))
        uc.mem_write(SEG, f32(a[0]) + f32(a[1]) + f32(a[2]) + f32(rng.uniform(-3, 3)) + f32(b[0]) + f32(b[1]) + f32(b[2]) + f32(rng.uniform(-3, 3)))
        uc.mem_write(OUT, f32(rng.uniform(-1, 1)))
        out = 0 if rng.random() < 0.2 else OUT
        uc.mem_write(STACK, bytes(rng.randrange(256) for _ in range(0x10000)))
        self.initial, self.written = {}, set()
        esp = SP - 16 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<IIII', h.RET, BOX, SEG, out))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 16)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)

def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    o = Oracle()
    cases = [o.case(s) for s in range(n)]
    hits = sum(1 for c in cases if c['after']['regs'][0] & 0xff == 1)
    json.dump(dict(function='0x00D1B840', length=LENGTH, cases=cases), open(out, 'w'))
    print(f'{n} cases, {hits} hits -> {out}')

main()
