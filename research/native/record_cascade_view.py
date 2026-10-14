"""L184 s3b oracle: 0x00EE5CE0 (the shadow cascade view setup, thiscall,
`ret 0x18`, result in ST0) run as the original code in Unicorn on randomized
lights, cameras and matrices. Its seven guest callees are recorded stubs:

  0x00FED950, 0x00FED940  camera->matrix() -> EAX = a 4x4 float matrix
  0x00FED920              camera->scale()  -> ST0 (a float)
  0x012F2F20              powf-like: XMM0, XMM1 -> XMM0
  0x012F3690              tan-like: a double on the stack -> ST0
  0x00CFD5E0              source->corners(a, b, c): writes through three pointers
  0x00CFC8B0              target->set_range(f, f, f, f)

Executed-instruction coverage of the function is reported.
Output format: crates/mtcore/tests/semantic_s3b.rs.
Usage: record_cascade_view.py OUT.json [cases]
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

ENTRY, END = 0x00EE5CE0, 0x00EE8B20
# stub -> (stack argument words the callee pops, kind)
STUBS = {0x00FED950: (0, 'matrix'), 0x00FED940: (0, 'matrix'), 0x00FED920: (0, 'st0f'), 0x012F2F20: (0, 'pow'),
         0x012F3690: (0, 'st0d'), 0x00CFD5E0: (3, 'corners'), 0x00CFC8B0: (4, 'range')}
CONSTANT_PAGES = (0x01406000, 0x01407000, 0x01433000, 0x01561000, 0x0161C000, 0x01629000)
THIS, ARG8, ARGC, OUT, CAMERA, MATRICES, SCRATCH = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((ENTRY, END - ENTRY), (THIS, 0x1000), (ARG8, 0x1000), (ARGC, 0x1000), (OUT, 0x1000), (CAMERA, 0x1000),
                              (MATRICES, 0x2000), (SCRATCH, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for page in CONSTANT_PAGES:
            mapping(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        for a in STUBS:
            mapping(a, 1)
            uc.mem_write(a, b'\xc3')
        uc.mem_write(ENTRY, h.code(ENTRY, END - ENTRY))
        self.executed = set()
        self.trace = None
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

    def put(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, bytes(data))
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def number(self):
        rng = self.rng
        r = rng.random()
        # no explicit NaNs: Unicorn (QEMU softfloat) picks the NaN of an operation on
        # two NaNs by the x87 rule (larger payload), the hardware by the SSE rule (first
        # operand), so a case may only produce the one kind the hardware also produces (the
        # negative default NaN of inf - inf, 0 / 0)
        if r < 0.04:
            return rng.choice((0.0, -0.0, 1.0, -1.0, 1e-30, 1e10, -1e10))
        return rng.uniform(-8.0, 8.0)

    def code(self, uc, address, size, data):
        self.insns += 1
        if ENTRY <= address < END:
            self.executed.add(address)
            if self.trace is not None:
                uc = self.uc
                lanes = ' '.join('%08x' % (uc.reg_read(x.UC_X86_REG_XMM0 + k) & 0xFFFFFFFF) for k in range(8))
                self.trace.write(('T %x %08x %08x %08x %08x %08x | %s' + chr(10)) % (address, uc.reg_read(x.UC_X86_REG_EAX), uc.reg_read(x.UC_X86_REG_ECX), uc.reg_read(x.UC_X86_REG_EDX), uc.reg_read(x.UC_X86_REG_ESI), uc.reg_read(x.UC_X86_REG_EDI), lanes))
        if address == h.RET:
            uc.emu_stop()
            return
        if address == SCRATCH + 0x100:
            return
        if address not in STUBS:
            assert ENTRY <= address < END, hex(address)
            return
        rng = self.rng
        pop, kind = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + max(pop, 2))))],
                      xmm=[list(struct.unpack('<4I', uc.reg_read(x.UC_X86_REG_XMM0).to_bytes(16, 'little'))), list(struct.unpack('<4I', uc.reg_read(x.UC_X86_REG_XMM1).to_bytes(16, 'little')))])
        effects = []
        extra = {}
        eax = rng.randrange(1 << 32)
        mode = 'normal' if rng.random() < 0.8 else 'wild'
        if kind == 'matrix':
            record = MATRICES + 0x80 * rng.randrange(8)
            matrix = [self.number() if mode == 'wild' else v for v in self.random_matrix()]
            effects += self.put(record, b''.join(f32(v) for v in matrix))
            eax = record
        elif kind == 'st0f':
            value = self.number() if mode == 'wild' else rng.uniform(0.2, 4.0)
            extra['st0f'] = struct.unpack('<I', f32(value))[0]
        elif kind == 'st0d':
            value = rng.uniform(0.05, 3.0)
            extra['st0d'] = struct.unpack('<Q', struct.pack('<d', value))[0]
        elif kind == 'pow':
            value = rng.uniform(0.0, 10.0) if mode == 'normal' else self.number()
            extra['xmm0'] = struct.unpack('<I', f32(value))[0]
        elif kind == 'corners':
            pointers = struct.unpack('<III', uc.mem_read(esp + 4, 12))
            for p in pointers:
                effects += self.put(p, b''.join(f32(rng.uniform(-6, 6) if mode == 'normal' else self.number()) for _ in range(16)))
            if rng.random() < 0.4:  # the callee also fills other frame slots
                low = esp + 16
                effects += self.put(low, b''.join(f32(rng.uniform(-6, 6)) for _ in range(0x80)))
        elif kind == 'range':
            pass
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        if 'xmm0' in extra:
            uc.reg_write(x.UC_X86_REG_XMM0, extra['xmm0'])
        # the x87 results are pushed with a real fld so the FPU state is the callee's
        if 'st0f' in extra or 'st0d' in extra:
            self.remember(SCRATCH, 8)
            if 'st0f' in extra:
                uc.mem_write(SCRATCH, struct.pack('<I', extra['st0f']))
                self.push_fpu(b'\xd9\x05' + struct.pack('<I', SCRATCH))
            else:
                uc.mem_write(SCRATCH, struct.pack('<Q', extra['st0d']))
                self.push_fpu(b'\xdd\x05' + struct.pack('<I', SCRATCH))
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects), **extra))
        if self.pending_fpu is not None:
            self.uc.emu_stop()

    def push_fpu(self, code):
        """Stop here; the driver runs one `fld` so the x87 stack holds the callee's result."""
        self.uc.mem_write(SCRATCH + 0x100, code)
        self.pending_fpu = len(code)
        self.resume = self.uc.reg_read(x.UC_X86_REG_EIP)
        self.uc.emu_stop()

    def random_matrix(self):
        rng = self.rng
        return [rng.uniform(-2, 2) for _ in range(16)]

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x1000), (ARG8, 0x1000), (ARGC, 0x1000), (OUT, 0x1000), (MATRICES, 0x2000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def wf(a, v):
            uc.mem_write(a, f32(v))

        # the light (this)
        direction = [rng.uniform(-1, 1) for _ in range(3)]
        if rng.random() < 0.2:
            direction = [rng.choice((0.0, 1.0, -1.0, 0.0)) for _ in range(3)]
        for k, v in enumerate(direction):
            wf(THIS + 0x160 + 4 * k, v)
        wf(THIS + 0x48, rng.choice((0.0, 0.5, 1.0, rng.uniform(0, 3))))
        wf(THIS + 0x60, rng.uniform(-3, 3))
        w32(THIS + 0x68, rng.choice((0, 1, 1, 2)))
        w32(THIS + 0x74, rng.choice((1, 2, 4, 8, 0x80000000, rng.randrange(1, 100))))
        uc.mem_write(THIS + 0x178, bytes([rng.choice((0, 1))]))
        wf(THIS + 0x17C, rng.uniform(0, 1))
        uc.mem_write(THIS + 0x180, bytes([rng.choice((0, 1))]))
        wf(THIS + 0x184, rng.uniform(-2, 2))
        wf(THIS + 0x188, rng.uniform(-2, 2))
        # the source camera object read through the corners call
        for k in range(0, 0x40, 4):
            wf(ARG8 + k, rng.uniform(-3, 3))
        # the stack is zeroed: a slot the function reads before writing reads 0 in both runs
        uc.mem_write(SP - 0x3000, bytes(0x3100))
        out = rng.choice((0, OUT, OUT))
        selector = rng.choice((0, 1, 2, 3))
        count = rng.choice((0, 1, 2, 4, 0xFFFFFFFF, rng.randrange(1, 9)))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        args = [ARG8, ARGC, out, CAMERA, selector, count]
        esp = SP - 4 * rng.randrange(0, 4) - 4 * 8
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', a) for a in args))
        # small integers: they are pushed to the frame, and read back as denormals (never NaNs)
        start = [rng.randrange(1 << 16) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        self.remember(esp, 4 + 4 * len(args))
        before = dict(regs=self.regs())
        self.pending_fpu = None
        pc = ENTRY
        while True:
            uc.emu_start(pc, h.RET, count=400000)
            if self.pending_fpu is None:
                break
            length, self.pending_fpu = self.pending_fpu, None
            uc.emu_start(SCRATCH + 0x100, SCRATCH + 0x100 + length)
            pc = self.resume
        top = (uc.reg_read(x.UC_X86_REG_FPSW) >> 11) & 7
        mantissa, exponent = uc.reg_read(x.UC_X86_REG_FP0 + top)
        record = dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events,
                      after=dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)]),
                      st0=list(struct.pack('<QH', mantissa, exponent)), depth=sum(((uc.reg_read(x.UC_X86_REG_FPTAG) >> (2 * k)) & 3) != 3 for k in range(8)),
                      insns=self.insns)
        return record


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 1500
    oracle = Oracle()
    import os
    if os.environ.get('CASCADE_TRACE'):
        oracle.trace = open(os.environ['CASCADE_TRACE'], 'w')
        oracle.case(int(os.environ.get('CASCADE_CASE', '0')))
        oracle.trace.close()
        return
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00EE5CE0', cases=cases), open(out, 'w'))
    covered = len(oracle.executed)
    print(f'{n} cases -> {out}; instructions per case {sum(c["insns"] for c in cases) / n:.0f}; '
          f'executed {covered} distinct addresses')


main()
