"""L184 s5 oracle: 0x00860AC0 `uCnsEdit::vf24(joint, model)` (thiscall, ret 8)
run as the original's code in Unicorn on synthetic constraint objects, joints
and models. The constraint chooses (field +0xA4) one of 17 ways to build a
4x4 local matrix (six Euler orders computed here with the CRT sinf/cosf at
0x012F0906/0x012F0A55 run as original code; ten matrix providers called as
guest functions), optionally blends it toward identity (0x00CFDC80), then
multiplies it into the joint's matrix (+0x10), optionally conjugated by a
parent bone's matrix (field +0xA0).

Guest callees are stubs recorded as events in the format of
crates/mtcore/tests/semantic.rs `abi_cases_full`, plus the registers the
callee takes its inputs in (ESI/EDI/EAX/ECX: the providers use a custom
convention) and `reads`: the stack buffers the callee would read (the angle
vector the providers take a pointer to; the two matrices of 0x00CFDC80).

Angles stay below 8192 rad so sinf/cosf never reach their x87 range
reduction (that path is a guest call in the port and is tested separately
in record_cns_sincos.py). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe;
output is private (E:/ddda-re/notes/native-L184).
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, END = 0x00860AC0, 0x008645A8  # code through RET 8 at 0x00864560 plus the 17-entry jump table at 0x00864564
SINCOS = (0x012F0906, 0x012F0BAC)
BLEND, VSTUB, DECOMPOSE = 0x00CFDC80, 0x30000058, 0x00CFC9E0  # DECOMPOSE: rotation matrix -> three angles (stub; ECX = matrix, stack: output buffer)
# lengths of the providers that can run as the original's real code (argv[3]: comma-separated addresses)
REAL_LENGTH = {0x00864660: 0x1326, 0x00865990: 0x1a87, 0x00867420: 0x749, 0x00867B70: 0x48d, 0x00868000: 0x1251,
               0x00869260: 0xe99, 0x0086A100: 0xee3, 0x0086AFF0: 0x2e8, 0x0086B2E0: 0x1e9b, 0x0086D180: 0x418}
REAL = set()
# target: (stack arguments, output-buffer register, takes a pointer to the angle vector)
PROVIDERS = {
    0x00864660: (1, 'esi', True), 0x00865990: (3, 'esi', True), 0x00867420: (3, 'esi', True),
    0x00867B70: (3, 'esi', True), 0x00868000: (1, 'edi', False), 0x00869260: (3, 'edi', True),
    0x0086A100: (3, 'edi', True), 0x0086AFF0: (2, 'esi', True), 0x0086B2E0: (1, None, False),
    0x0086D180: (3, 'esi', True),
}
THIS, JOINT, MODEL, VTABLE, JOINTS, PTRS = 0x20000000, 0x21000000, 0x22000000, 0x22000800, 0x23000000, 0x24000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
NAMES = ('eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi')


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        # the function's page range holds the stub addresses too (their bytes stay RET)
        uc.mem_map(0x00860000, 0x10000)
        uc.mem_write(ENTRY, h.code(ENTRY, END - ENTRY))
        for target in PROVIDERS:
            uc.mem_write(target, h.code(target, REAL_LENGTH[target]) if target in REAL else bytes([0xc3]))
        uc.mem_map(0x00421000, 0x1000)
        uc.mem_write(0x00421ED0, h.code(0x00421ED0, 0x2F))  # joint accessor the aim provider calls
        for page in (0x014F0000, 0x014F9000, 0x0150A000, 0x00CFD000, 0x00CFE000, 0x012F0000, 0x0144B000, 0x01521000, 0x01531000, 0x0161C000, 0x01629000):
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.mem_write(BLEND, b'\xc3')  # stub (the page's real bytes are not needed)
        uc.mem_map(0x00CFC000, 0x1000)
        uc.mem_write(DECOMPOSE, bytes([0xC3]))
        uc.mem_map(0x30000000, 0x1000)
        uc.mem_write(VSTUB, b'\xc3')
        for address, size in ((THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x10000), (PTRS, 0x10000),
                              (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
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

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def matrix(self, kind=None):
        rng = self.rng
        kind = kind if kind is not None else rng.choice(('rand', 'rand', 'rand', 'rot', 'zero', 'identity', 'sparse'))
        if kind == 'zero':
            v = [0.0] * 16
        elif kind == 'identity':
            v = [1.0 if i % 5 == 0 else 0.0 for i in range(16)]
        elif kind == 'sparse':
            v = [rng.choice((0.0, 0.0, 1.0, -1.0, rng.uniform(-2, 2))) for _ in range(16)]
        elif kind == 'rot':
            import math
            a = rng.uniform(-3.2, 3.2)
            c, s = math.cos(a), math.sin(a)
            v = [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 1.0]
        else:
            v = [rng.choice((rng.uniform(-3, 3), rng.uniform(-3, 3), rng.uniform(-3, 3), 0.0, 1.0)) for _ in range(16)]
        return b''.join(f32(e) for e in v)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        spec = PROVIDERS.get(address)
        if (address in PROVIDERS and address not in REAL) or address in (BLEND, VSTUB, DECOMPOSE):
            return self.stub(uc, address, spec)
        assert ENTRY <= address < END or 0x421ED0 <= address < 0x421EFF or SINCOS[0] <= address < SINCOS[1] or any(a <= address < a + REAL_LENGTH[a] for a in REAL), hex(address)

    def stub(self, uc, address, spec):
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        rng = self.rng
        if address == BLEND:
            nargs = 3
        elif address in (VSTUB, DECOMPOSE):
            nargs = 1
        else:
            nargs = spec[0]
        frame = esp + 4 * (1 + nargs)  # the original's ESP after its prologue
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        reads = []
        effects = []
        eax = rng.randrange(1 << 32)
        if address == BLEND:
            for a in (frame + 0x30, frame + 0x140):
                self.remember(a, 64)
                reads.append((a, list(uc.mem_read(a, 64))))
            matrix = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
            effects += self.stub_write(matrix, self.matrix())
        elif address == VSTUB:
            dest = PTRS + 0x40 * rng.randrange(64)
            effects += self.stub_write(dest, self.matrix())
            eax = dest
        elif address == DECOMPOSE:
            ecx = uc.reg_read(x.UC_X86_REG_ECX)
            buf = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
            self.remember(ecx, 64)
            reads.append((ecx, list(uc.mem_read(ecx, 64))))
            angles = [rng.choice((0.0, rng.uniform(-3.2, 3.2), rng.uniform(-3.2, 3.2), rng.uniform(-100, 100))) for _ in range(3)]
            eax = buf if rng.random() < 0.7 else PTRS + 0x100 + 0x10 * rng.randrange(8)
            effects += self.stub_write(eax, b''.join(f32(a) for a in angles))
        else:
            _, out_reg, angles = spec
            if angles:
                self.remember(frame + 0x120, 16)
                reads.append((frame + 0x120, list(uc.mem_read(frame + 0x120, 16))))
            if out_reg is not None:
                r = rng.random()
                if r < 0.6:
                    dest = uc.reg_read(getattr(x, 'UC_X86_REG_' + out_reg.upper()))
                else:
                    dest = PTRS + 0x40 * rng.randrange(64)
                effects += self.stub_write(dest, self.matrix())
                eax = dest
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, reads=reads,
                                after=dict(regs=self.regs(), memory=effects)))

    def angle(self):
        rng = self.rng
        r = rng.random()
        if r < 0.45:
            return rng.uniform(-360, 360)
        if r < 0.55:
            return 0.0
        if r < 0.7:
            return rng.choice((90.0, 180.0, 270.0, -90.0, 360.0, 720.0, 45.0, -180.0, 30.0))
        if r < 0.8:
            return rng.uniform(-400000, 400000)  # < 8192 rad
        if r < 0.9:
            return rng.uniform(-1e-3, 1e-3)
        if r < 0.95:
            return rng.choice((1e-40, -1e-40, 1e-43, 1e-30, -0.0))
        return rng.uniform(-20, 20)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for a, n in ((THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x10000), (PTRS, 0x10000)):
            uc.mem_write(a, b'\0' * n)
        w = uc.mem_write
        w(THIS + 0x44, f32(rng.choice((1.0, 1.0, 1.0, rng.uniform(0, 1), rng.uniform(0, 1), 0.0, 1e-30, 2.5, -1.0, 0.5))))
        w(THIS + 0x61, bytes([0 if rng.random() < 0.04 else rng.choice((1, 1, 1, 2, 0xff))]))
        w(THIS + 0xB1, bytes([rng.choice((1, 2)) if rng.random() < 0.04 else 0]))
        w(THIS + 0x70, b''.join(f32(self.angle()) for _ in range(3)))
        r = rng.random()
        if r < 0.15:
            scale = (0.0, 0.0, 0.0)
        elif r < 0.3:
            scale = (1.0, 1.0, 1.0)
        elif r < 0.4:
            scale = tuple(rng.uniform(-6e-4, 6e-4) for _ in range(3))  # near the 1e-6 squared-length threshold
        elif r < 0.45:
            scale = (0.001, 0.001, 0.001)
        else:
            scale = tuple(rng.uniform(-2, 2) for _ in range(3))
        w(THIS + 0x80, b''.join(f32(v) for v in scale))
        w(THIS + 0x90, b''.join(f32(rng.choice((0.0, rng.uniform(-5, 5)))) for _ in range(3)))
        w(THIS + 0xA0, struct.pack('<I', rng.choice((0, 0, 1, 1, 2, 0x100, 0xFFFFFFFF))))
        mode = rng.randrange(17) if rng.random() < 0.97 else rng.choice((17, 18, 40, 0x80000000, 0xFFFFFFFF))
        w(THIS + 0xA4, struct.pack('<I', mode))
        w(JOINT + 0x10, self.matrix())
        index = rng.choice((0xFF, 0xFF, 0xFF, rng.randrange(0, 256), rng.randrange(0, 4)))
        w(JOINT + 0x91, bytes([index]))
        w(MODEL, struct.pack('<I', VTABLE))
        w(MODEL + 0x364, struct.pack('<I', JOINTS))
        w(VTABLE + 0x58 - 0x800 + 0x800, struct.pack('<I', VSTUB)) if False else None
        uc.mem_write(VTABLE + 0x58, struct.pack('<I', VSTUB))
        # tables the aim provider (mode 6) reads: joint count, index->joint bytes, constraint base index
        TABLE = MODEL + 0x1000
        w(MODEL + 0x360, struct.pack('<I', rng.choice((0, 3, 8, 40, 256))))
        w(MODEL + 0x368, struct.pack('<I', TABLE))
        w(TABLE, bytes(rng.randrange(4) for _ in range(256)))
        w(THIS + 0xA8, struct.pack('<I', rng.randrange(0, 5)))
        for k in range(4):
            if rng.random() < 0.9:
                w(JOINTS + 0xA0 * k + 0x10, self.matrix())
        if index != 0xFF:
            w(JOINTS + index * 0xA0 + 0x10, self.matrix())
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12 - 4 * rng.randrange(0, 64)
        uc.mem_write(esp, struct.pack('<III', h.RET, JOINT, MODEL))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for reg, v in zip(REGS, start):
            uc.reg_write(reg, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, mode=mode, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    if len(sys.argv) > 3:
        REAL.update(int(a, 16) for a in sys.argv[3].split(',') if a)
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = {}
    for c in cases:
        for e in c['events']:
            calls[e['target']] = calls.get(e['target'], 0) + 1
    modes = {}
    for c in cases:
        modes[c['mode']] = modes.get(c['mode'], 0) + 1
    json.dump(dict(function='0x00860AC0', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}; modes {sorted(modes.items())}; calls {[(hex(k), v) for k, v in sorted(calls.items())]}')


main()
