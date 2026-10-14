"""L184 s5 oracle: the uCnsEdit providers that scale the Euler rotation by the
joint's own row lengths: 0x00867B70 (modes 9, 10), 0x0086D180 (mode 11) and
0x0086AFF0 (mode 15). Run as original code (sinf/cosf included, no stubs).

  python record_cns_scale.py <m9|m11|m15> <out.json> [cases]

Registers/arguments (usercall):
  0x00867B70: EDI = pointer to the 3 angles, ESI = out; stack this, joint, model; ret 0xc
  0x0086D180: EDI = joint, ESI = out; stack this, model, pointer to the angles; ret 0xc
  0x0086AFF0: EAX = pointer to the angles, ESI = out; stack this, joint; ret 8
Run from research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import struct
import sys

import cns_oracle as o

THIS, MODEL, JOINT, JOINTS, TABLE, ANGLES = 0x20000000, 0x22000000, 0x21000000, 0x23000000, 0x22001000, 0x0F004000
OUT = o.SP + 0x400
SPECS = {  # name: (entry, length)
    'm9': (0x00867B70, 0x48D),
    'm11': (0x0086D180, 0x418),
    'm15': (0x0086AFF0, 0x2E8),
}


def setup_for(name):
    def setup(self, rng):
        w = self.uc.mem_write
        f = o.f32
        w(THIS + 0xB0, bytes([rng.choice((0, 0, 1))]))
        w(THIS + 0xC0, b''.join(f(rng.choice((1.0, rng.uniform(0.1, 2), 0.0))) for _ in range(3)))
        w(MODEL + 0x60, b''.join(f(rng.choice((1.0, rng.uniform(0.1, 2), 0.0))) for _ in range(3)))
        # the joint: random, or with rows too short to scale by (near zero)
        kind = rng.choice((None, None, None, 'zero', 'sparse'))
        w(JOINT + 0x10, self.matrix(kind))
        w(JOINT + 0x93, bytes([rng.choice((0, 1, 2, 2, 3))]))
        # model tables for mode 15: this+0x54 = model, this+0xAC = reference joint index
        w(THIS + 0x54, struct.pack('<I', MODEL))
        w(THIS + 0xAC, struct.pack('<I', rng.choice((0xFF, 0xFF, 0, 1, 2, 3, 0x1FF))))
        w(MODEL + 0x364, struct.pack('<I', JOINTS))
        w(MODEL + 0x368, struct.pack('<I', TABLE))
        w(TABLE, bytes(rng.choice((0xFF, 0, 1, 2, 3)) for _ in range(256)))
        for k in range(4):
            w(JOINTS + 0xA0 * k + 0x10, self.matrix(rng.choice((None, None, 'sparse'))))
        w(ANGLES, b''.join(f(self.angle() * rng.choice((1, -1))) for _ in range(3)) + f(0.0))
        esp = o.SP - 16 - 4 * rng.randrange(0, 64)
        junk = [rng.randrange(1 << 32) for _ in range(8)]
        regs = list(junk)
        regs[4] = esp
        regs[6] = OUT + 0x40 * rng.randrange(4)
        if name == 'm9':
            regs[7] = ANGLES
            return regs, [THIS, JOINT, MODEL]
        if name == 'm11':
            regs[7] = JOINT
            return regs, [THIS, MODEL, ANGLES]
        regs[0] = ANGLES
        return regs, [THIS, JOINT]
    return setup


def main():
    name, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    entry, length = SPECS[name]
    arenas = [(THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x10000), (ANGLES, 0x100), (OUT, 0x100)]
    oracle = o.Oracle([(entry, length), (0x00421ED0, 0x2F), (0x012F0906, 0x2A7)], arenas)
    if name == 'm11':  # the port loads the per-axis weights at this+0xC0 up front; the original only when a row is long enough
        oracle.speculative = [(THIS + 0xC0, 12)]
    oracle.record('0x%08X' % entry, entry, setup_for(name), out, n)


main()
