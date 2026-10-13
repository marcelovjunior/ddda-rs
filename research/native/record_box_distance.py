"""L184 s3 oracle: the closest-point-on-a-box family run as the original
code in Unicorn on randomized synthetic boxes and points.

  closest  0x00D53730  (box, point, out) -> ST0 = distance^2, out = clamped point, 0.0  (cdecl)
  wrapper  0x00D53F30  (box, point) -> ST0 = distance^2                                  (cdecl)
  touch    0x00D13AB0  (box, sphere{x,y,z,r}) -> EAX = 1 when r*r >= distance^2         (cdecl)

The callees of `wrapper` and `touch` are the real functions (no stubs).
ST0 is recorded as the 10 bytes of the extended value.
Usage: record_box_distance.py MODE OUT.json [cases]
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; the output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

CODE = {'closest': (0x00D53730,), 'wrapper': (0x00D53F30,), 'touch': (0x00D13AB0,)}
PARTS = ((0x00D53730, 0x103), (0x00D53F30, 0x22), (0x00D13AB0, 0x2F))
BOX, POINT, OUT = 0x20000000, 0x20001000, 0x20002000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
NAN = struct.unpack('<f', struct.pack('<I', 0x7FC00000))[0]
INF = float('inf')


def f32(v):
    return struct.pack('<f', v)


def bits_to_f32(b):
    return struct.unpack('<f', struct.pack('<I', b))[0]


class Oracle:
    def __init__(self, mode):
        self.mode = mode
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in PARTS + ((BOX, 0x3000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for address, size in PARTS:
            uc.mem_write(address, h.code(address, size))
        self.entry = CODE[mode][0]
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

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def number(self, rng, spread=10.0):
        r = rng.random()
        if r < 0.03:
            return NAN
        if r < 0.06:
            return rng.choice((INF, -INF))
        if r < 0.15:
            return rng.choice((0.0, -0.0, 1.0, -1.0, 1e-40, -1e-40, 3.0e-45))
        if r < 0.22:
            return rng.choice((1e30, -1e30, 1e-30, 1.5e19, 7e-20, 3e38))
        return rng.uniform(-spread, spread)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(BOX, b'\0' * 0x3000)
        # operating modes: ordinary numbers, or specials (NaN, inf, tiny, huge)
        special = rng.random() < 0.35
        num = (lambda s=10.0: self.number(rng, s)) if special else (lambda s=10.0: rng.uniform(-s, s))
        lo = [num() for _ in range(3)]
        hi = [num() for _ in range(3)]
        if rng.random() < 0.75:  # mostly ordered boxes
            lo, hi = [min(a, b) if a == a and b == b else a for a, b in zip(lo, hi)], [max(a, b) if a == a and b == b else b for a, b in zip(lo, hi)]
        box = lo + [0.0] + hi + [0.0]
        point = [num() for _ in range(3)]
        radius = abs(num(5.0)) if rng.random() < 0.9 else num(5.0)
        if rng.random() < 0.3 and not special:  # near-ties: the point on the axis, the radius its distance
            axis = rng.randrange(3)
            point = [rng.uniform(lo[i], hi[i]) if lo[i] <= hi[i] else lo[i] for i in range(3)]
            gap = rng.choice((rng.uniform(0.001, 3.0), 2.0 ** rng.randrange(-20, 6)))
            point[axis] = hi[axis] + gap if rng.random() < 0.5 else lo[axis] - gap
            radius = f32_round(gap) if rng.random() < 0.6 else bits_to_f32(struct.unpack('<I', f32(f32_round(gap)))[0] + rng.choice((-1, 1)))
        for i, v in enumerate(box):
            uc.mem_write(BOX + 4 * i, f32(v))
        for i, v in enumerate(point + [radius]):
            uc.mem_write(POINT + 4 * i, f32(v))
        uc.mem_write(OUT, struct.pack('<4I', *[rng.randrange(1 << 32) for _ in range(4)]))
        self.initial, self.written, self.insns = {}, set(), 0
        esp = SP - 4 * rng.randrange(0, 4)
        args = [BOX, POINT, OUT] if self.mode == 'closest' else [BOX, POINT]
        # `touch` stores into its own argument slot: leave room above
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', a) for a in args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4 + 4 * len(args))
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=10000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        record = dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after, insns=self.insns)
        if self.mode != 'touch':
            top = (uc.reg_read(x.UC_X86_REG_FPSW) >> 11) & 7
            mantissa, exponent = uc.reg_read(x.UC_X86_REG_FP0 + top)
            record['st0'] = list(struct.pack('<QH', mantissa, exponent))
        # the unit stack is balanced for `touch`; reset the FPU for the next case
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        return record


def f32_round(v):
    return struct.unpack('<f', struct.pack('<f', v))[0]


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 4000
    oracle = Oracle(mode)
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function=hex(CODE[mode][0]), cases=cases), open(out, 'w'))
    if mode == 'touch':
        ones = sum(c['after']['regs'][0] == 1 for c in cases)
        print(f'{n} cases -> {out}; EAX=1 in {ones}')
    else:
        print(f'{n} cases -> {out}')


main()
