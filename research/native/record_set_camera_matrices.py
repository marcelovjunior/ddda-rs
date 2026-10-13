"""L184 s9 oracle: 0x00FF2EE0 `this->set_camera_matrices(view, projection,
viewport, bias)` run as the original's code in Unicorn on synthetic camera
matrices (typical, mirrored, singular, huge, tiny, NaN). Guest callees are
stubs with recorded effects: the viewport setup 0x00FF04C0 (ret 4), the CRT
log pair 0x012F45C4 (float in/out XMM0) and 0x012F439E (double in/out XMM0),
the page allocator 0x00DA7D70 (ret 4) and the CRT memcpy 0x012EFEA0. The
constants (1.0, -1.0, FLT_EPSILON, the identity table) are the exe's real
data. Events carry xmm0 for the register-argument callees. Format of
crates/mtcore/tests/semantic_s9.rs `abi_cases_full`.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import math
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import UC_HOOK_MEM_READ_UNMAPPED, Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00FF2EE0, 0x32A3  # through RET 0x10 at 0x00FF6180
VIEWPORT_SETUP, LOG_F, LOG_D, PAGES, MEMCPY = 0x00FF04C0, 0x012F45C4, 0x012F439E, 0x00DA7D70, 0x012EFEA0
ARGS = {PAGES: 1, MEMCPY: 3}
# the real code of these runs inside the oracle (the port folds them in natively)
REAL = ((VIEWPORT_SETUP, 0x2B2), (LOG_F, 0x179), (LOG_D, 0x226))
THIS, THIS_SIZE = 0x20000000, 0x2000
DESC_TABLE, R1, R2, SOURCE = 0x21000000, 0x21001000, 0x21002000, 0x21003000
MATS, VIEWPORT = 0x22000000, 0x22001000
ARENA, ARENA_SIZE = 0x23000000, 0x40000
STACK, SP = 0x0F000000, 0x0F008000
DATA_PAGES = (0x01561000, 0x01406000, 0x01407000, 0x0161C000, 0x01629000, 0x018D0000)
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
XMM0 = x.UC_X86_REG_XMM0
QNAN = 0x7FC00000


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def as_float(bits):
    return struct.unpack('<f', struct.pack('<I', bits & 0xFFFFFFFF))[0]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, sections = h.image()
        for va, bytes_ in sections:
            start = va & ~0xFFF
            uc.mem_map(start, ((va + len(bytes_) - start) + 0xFFF) & ~0xFFF)
            uc.mem_write(va, bytes_)
        mapped = set()
        pages = [(0, 0x1000), (THIS, THIS_SIZE), (DESC_TABLE, 0x4000), (MATS, 0x2000), (ARENA, ARENA_SIZE), (STACK, 0x10000), (h.RET, 0x1000)]
        for address, size in pages:
            for page in range(address, address + size, 0x1000):
                if page not in mapped:
                    uc.mem_map(page, 0x1000)
                    mapped.add(page)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x10\x00', h.code(ENTRY + LENGTH - 3, 3)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in (PAGES, MEMCPY):
            uc.mem_write(a, b'\xc3')
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        uc.hook_add(UC_HOOK_MEM_READ_UNMAPPED, lambda u, a, ad, sz, v, d: print('unmapped read', hex(ad), 'at', hex(u.reg_read(x.UC_X86_REG_EIP))) or False)
        self.data = {p: bytes(uc.mem_read(p, 0x1000)) for p in DATA_PAGES}

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

    def xmm0(self):
        v = self.uc.reg_read(XMM0)
        return [(v >> (32 * i)) & 0xFFFFFFFF for i in range(4)]

    def u32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def stub_write32(self, effects, address, value):
        data = struct.pack('<I', value & 0xFFFFFFFF)
        self.remember(address, 4)
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + 4))
        effects += [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in ARGS:
            if ENTRY <= address < ENTRY + LENGTH:
                self.visited.add(address)
            else:
                assert any(a <= address < a + n for a, n in REAL), hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = ARGS[address]
        self.remember(esp, 4 * (1 + nargs))
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        args = struct.unpack('<%dI' % nargs, uc.mem_read(esp + 4, 4 * nargs)) if nargs else ()
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        before = dict(regs=self.regs(), memory=window, xmm0=self.xmm0())
        effects = []
        eax, pop = rng.getrandbits(32), 4 + 4 * nargs
        xmm0 = None
        if address == MEMCPY:
            dst, src, n = args
            self.remember(src, n)
            data = bytes(uc.mem_read(src, n))
            self.remember(dst, n)
            uc.mem_write(dst, data)
            self.written.update(range(dst, dst + n))
            effects += [(dst + i, b) for i, b in enumerate(data)]
            eax, pop = dst, 4
        elif address == PAGES:
            eax = 0 if rng.random() < 0.25 else ARENA + 0x20000 + 16 * rng.randrange(0, 0x400)
        elif address == VIEWPORT_SETUP:
            this = uc.reg_read(x.UC_X86_REG_ECX)
            for _ in range(rng.randrange(0, 4)):
                self.stub_write32(effects, this + rng.choice((0xCC, 0xD0, 0xDC, 0xC90, 0xC94, 0x154)), rng.getrandbits(32))
            if rng.random() < 0.3:
                # the setup may rewrite a matrix word the caller still reads
                self.stub_write32(effects, rng.choice(self.matrix_words), rng.choice((f32(1.0), f32(-2.5), 0, rng.getrandbits(32) & 0x7F7FFFFF)))
        elif address == LOG_F:
            v = as_float(self.xmm0()[0])
            r = math.log(v) if v > 0 and not math.isinf(v) else (float('nan') if v != v or v < 0 else float('inf'))
            xmm0 = [f32(r), 0, 0, 0]
        elif address == LOG_D:
            lo, hi = self.xmm0()[0], self.xmm0()[1]
            v = struct.unpack('<d', struct.pack('<II', lo, hi))[0]
            r = math.log(v) if v > 0 else float('nan')
            lo, hi = struct.unpack('<II', struct.pack('<d', r))
            xmm0 = [lo, hi, 0, 0]
        if xmm0 is not None:
            uc.reg_write(XMM0, sum(v << (32 * i) for i, v in enumerate(xmm0)))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        for r in (x.UC_X86_REG_ECX, x.UC_X86_REG_EDX):
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects, xmm0=self.xmm0())))

    def random_matrix(self, kind):
        rng = self.rng
        def rot():
            a, b, c = (rng.uniform(-math.pi, math.pi) for _ in range(3))
            ca, sa, cb, sb, cc, sc = math.cos(a), math.sin(a), math.cos(b), math.sin(b), math.cos(c), math.sin(c)
            return [[ca * cb, ca * sb * sc - sa * cc, ca * sb * cc + sa * sc],
                    [sa * cb, sa * sb * sc + ca * cc, sa * sb * cc - ca * sc],
                    [-sb, cb * sc, cb * cc]]
        m = [0.0] * 16
        if kind == 'view':
            r = rot()
            for i in range(3):
                for j in range(3):
                    m[4 * i + j] = r[i][j]
            for j in range(3):
                m[12 + j] = rng.uniform(-50, 50)
            m[15] = 1.0
        elif kind == 'proj':
            near, far = rng.uniform(0.05, 1), rng.uniform(100, 5000)
            fx, fy = rng.uniform(0.5, 3), rng.uniform(0.5, 3)
            sign = rng.choice((1, 1, -1))
            m[0], m[5] = fx, fy * rng.choice((1, -1))
            m[10] = sign * far / (far - near)
            m[11] = sign
            m[14] = -near * far / (far - near)
        else:
            m = [rng.uniform(-4, 4) for _ in range(16)]
        return m

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, THIS_SIZE), (DESC_TABLE, 0x4000), (MATS, 0x2000), (ARENA, ARENA_SIZE), (STACK, 0x10000)):
            uc.mem_write(base, b'\0' * size)
        for p, content in self.data.items():
            uc.mem_write(p, content)
        uc.mem_write(SOURCE, rng.randbytes(0x1000))
        view = self.random_matrix(rng.choice(('view', 'view', 'view', 'random')))
        proj = self.random_matrix(rng.choice(('proj', 'proj', 'proj', 'random')))
        special = rng.random()
        if special < 0.08:
            proj = [0.0] * 16
        elif special < 0.14:
            row = rng.randrange(4)
            view[4 * row:4 * row + 4] = [0.0] * 4
        elif special < 0.20:
            proj[4:8] = proj[0:4]
        elif special < 0.27:
            scale = rng.choice((1e12, 1e9, 1e6, 1e-6))
            view = [v * scale for v in view]
        elif special < 0.35:
            proj[10] = rng.choice((0.0, -0.0, 1.0, -1.0, 1e-30))
        for i in range(16):
            uc.mem_write(MATS + 4 * i, struct.pack('<f', view[i]))
            uc.mem_write(MATS + 0x40 + 4 * i, struct.pack('<f', proj[i]))
        self.matrix_words = [MATS + 4 * rng.choice((0, 1, 5, 9, 12, 13, 14)), MATS + 0x40 + 4 * rng.choice((0, 5, 10, 14, 15)), MATS + 0x40 + 0x28]
        uc.mem_write(VIEWPORT, rng.randbytes(16))
        self.w32(THIS + 0x18, DESC_TABLE)
        self.w32(DESC_TABLE + 0x554, R1)
        self.w32(DESC_TABLE + 0x548, R2)
        for descriptor in (R1, R2):
            uc.mem_write(descriptor + 0x18, struct.pack('<H', rng.choice((0, 3, 8, 20, 40, rng.randrange(0x80)))))
            self.w32(descriptor + 0x24, SOURCE)
        # the viewport block: render target size, previous rect and block, descriptor
        self.w32(THIS + 0x10C, rng.choice((0, 0x04380780, 0x03000400, 0x00400040, rng.getrandbits(32))))
        self.w32(THIS + 0xC8, rng.getrandbits(32))
        self.w32(THIS + 0xC90, SOURCE)
        self.w32(DESC_TABLE + 0x54C, DESC_TABLE + 0x3000)
        uc.mem_write(DESC_TABLE + 0x3018, struct.pack('<H', rng.choice((0, 4, 8, 20, rng.randrange(0x80)))))
        uc.mem_write(VIEWPORT, struct.pack('<4i', *[rng.choice((0, 1, 100, 640, 1920, -20, rng.randrange(-500, 2000))) for _ in range(4)]))
        if rng.random() < 0.2:
            uc.mem_write(THIS + 0xCC, bytes(uc.mem_read(VIEWPORT, 16)))
        self.w32(THIS + 0x114, rng.getrandbits(32))
        self.w32(THIS + 0x154, rng.getrandbits(32))
        cursor = ARENA + 16 * rng.randrange(0, 0x400)
        self.w32(THIS + 0x24, cursor)
        self.w32(THIS + 0x28, cursor + rng.choice((0, 0x40, 0x300, 0x400, 0x10000)))
        self.w32(0x018D08F4, rng.getrandbits(32))
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        bias = f32(rng.choice((0.0, 1.5, -2.0, rng.uniform(-100, 100))))
        uc.mem_write(esp, struct.pack('<IIIII', h.RET, MATS, MATS + 0x40, VIEWPORT, bias))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        self.remember(esp, 20)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=2000000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET, 'instruction limit'
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2500
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00FF2EE0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {len(visited)} instruction addresses visited -> {out}')
    open(out + '.visited', 'w').write('\n'.join('%08x' % a for a in sorted(visited)))


main()
