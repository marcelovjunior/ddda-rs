"""L184 s5 oracle: the uCnsEdit providers that read the parent bone's matrix:
0x00867420 (mode 8), 0x00868000 (mode 12), 0x00869260 (mode 13),
0x0086A100 (mode 14). Run as original code (sinf/cosf included); the model's
virtual accessor (vtable +0x58, takes -1) is a stub recorded as an event, and
so is any other helper the provider calls (see HELPERS).

  python record_cns_pm.py <m8|m12|m13|m14> <out.json> [cases]

Registers/arguments (usercall):
  0x00867420: EAX = pointer to the 3 angles, ESI = out; stack this, joint, model; ret 0xc
  0x00868000: ECX = model, ESI = joint, EDI = out; stack this; ret 4
  0x00869260: EAX = model, EDX = pointer to the angles, ESI = joint, EDI = out; stack this, model, angles; ret 0xc
  0x0086A100: ECX = pointer to the angles, EDX = model, ESI = joint, EDI = out; stack this, model, angles; ret 0xc
Run from research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import struct
import sys

import cns_oracle as o

THIS, MODEL, JOINT, JOINTS, PTRS, ANGLES = 0x20000000, 0x22000000, 0x21000000, 0x23000000, 0x24000000, 0x0F004000
VTABLE, VSTUB = MODEL + 0x800, 0x30000058
OUT = o.SP + 0x400
SPECS = {  # name: (entry, length)
    'm8': (0x00867420, 0x749),
    'm12': (0x00868000, 0x1251),
    'm13': (0x00869260, 0xE99),
    'm14': (0x0086A100, 0xEE3),
}
HELPERS = {}
SPECULATIVE = []


def vcall_stub(self, esp):
    dest = PTRS + 0x40 * self.rng.randrange(64)
    self.stub_write(dest, self.matrix())
    return 1, dest, []


def common(self, rng):
    w = self.uc.mem_write
    f = o.f32
    w(JOINT + 0x10, self.matrix())
    index = rng.choice((0xFF, 0xFF, rng.randrange(0, 256), rng.randrange(0, 4)))
    w(JOINT + 0x91, bytes([index]))
    w(MODEL, struct.pack('<I', VTABLE))
    w(MODEL + 0x364, struct.pack('<I', JOINTS))
    w(VTABLE + 0x58, struct.pack('<I', VSTUB))
    if index != 0xFF:
        w(JOINTS + index * 0xA0 + 0x10, self.matrix())
    w(ANGLES, b''.join(f(self.angle() * rng.choice((1, -1))) for _ in range(3)) + f(0.0))
    w(THIS + 0x90, b''.join(f(rng.choice((0.0, rng.uniform(-5, 5)))) for _ in range(3)))
    w(THIS + 0x84, f(rng.choice((1.0, 1.0, rng.uniform(-2, 2), 0.0, 0.5))))
    w(THIS + 0x88, f(rng.choice((1.0, 1.0, rng.uniform(-2, 2), 0.0))))
    w(THIS + 0x80, f(rng.choice((1.0, 1.0, rng.uniform(-2, 2), 0.0))))
    # mode 12: the joint's quaternion and scale
    w(JOINT + 0x60, b''.join(f(rng.uniform(-1, 1)) for _ in range(4)))
    w(JOINT + 0x80, b''.join(f(rng.choice((1.0, rng.uniform(0.2, 2)))) for _ in range(3)))
    # modes 13, 14: reference joint (this+0xAC of the model at this+0x54)
    w(THIS + 0x54, struct.pack('<I', MODEL))
    w(THIS + 0xAC, struct.pack('<I', rng.choice((0xFF, 0, 1, 2, 3, 0, 1, 0x1FF))))
    w(MODEL + 0x368, struct.pack('<I', MODEL + 0x1000))
    w(MODEL + 0x1000, bytes(rng.choice((0xFF, 0, 1, 2, 3)) for _ in range(256)))
    for k in range(4):
        w(JOINTS + 0xA0 * k + 0x10, self.matrix())
        SPECULATIVE.append((JOINTS + 0xA0 * k + 0x10, 64))
    SPECULATIVE.append((THIS + 0x80, 24))
    # the ports load fields the original reads only on some paths
    SPECULATIVE.append((JOINT + 0x10, 64))
    SPECULATIVE.append((THIS + 0x80, 12))
    SPECULATIVE.append((JOINT + 0x60, 16))
    SPECULATIVE.append((JOINT + 0x80, 12))
    if index != 0xFF:
        SPECULATIVE.append((JOINTS + index * 0xA0 + 0x10, 64))


def setup_for(name):
    def setup(self, rng):
        SPECULATIVE.clear()
        common(self, rng)
        self.speculative = list(SPECULATIVE)
        if name == 'm8':
            pass
        esp = o.SP - 16 - 4 * rng.randrange(0, 64)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[4] = esp
        out = OUT + 0x40 * rng.randrange(4)
        if name == 'm8':
            regs[0], regs[6] = ANGLES, out
            return regs, [THIS, JOINT, MODEL]
        if name == 'm12':
            regs[1], regs[6], regs[7] = MODEL, JOINT, out
            return regs, [THIS]
        if name == 'm13':
            regs[0], regs[2], regs[6], regs[7] = MODEL, ANGLES, JOINT, out
            return regs, [THIS, MODEL, ANGLES]
        regs[1], regs[2], regs[6], regs[7] = ANGLES, MODEL, JOINT, out
        return regs, [THIS, MODEL, ANGLES]
    return setup


def main():
    name, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    entry, length = SPECS[name]
    arenas = [(THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x10000), (PTRS, 0x10000), (ANGLES, 0x100), (OUT, 0x100)]
    stubs = {VSTUB: vcall_stub}
    stubs.update(HELPERS)
    code = [(entry, length), (0x00421ED0, 0x2F), (0x012F0906, 0x2A7), (0x007E4B60, 0x144)]  # 0x7E4B60: quaternion -> matrix, called by mode 12
    oracle = o.Oracle(code, arenas, stubs=stubs)
    oracle.record('0x%08X' % entry, entry, setup_for(name), out, n)


main()
