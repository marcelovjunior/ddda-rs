"""L184 s5 oracle: 0x00865990, mode 7 of uCnsEdit::vf24 (usercall: EDI = joint,
ESI = output buffer; stack arguments `this`, `model`, pointer to the three
angles in radians; `ret 0xc`). It builds the Euler rotation of the angles in the
buffer; when the weight at this+0x84 is not 1 it also takes the joint's
rotation relative to its parent bone (joint byte +0x91: a joint of the model, or
0xFF for the matrix of the model's virtual +0x58(-1)), decomposes it into
angles with 0x00CFC9E0 and rotates by those angles scaled by the weights
(this+0x80, this+0x84), then scales row 0 by this+0x88 and adds the
translation this+0x90. The original code runs (sinf/cosf included); the
virtual accessor and 0x00CFC9E0 are stubs recorded as events. Run from
research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY = 0x00865990
THIS, JOINT, MODEL, VTABLE, JOINTS, PTRS = 0x20000000, 0x21000000, 0x22000000, 0x22000800, 0x23000000, 0x24000000
ANGLES = 0x0F004000
OUT = o.SP + 0x400
VSTUB, DECOMPOSE = 0x30000058, 0x00CFC9E0


def vcall_stub(self, esp):
    dest = PTRS + 0x40 * self.rng.randrange(64)
    self.stub_write(dest, self.matrix())
    return 1, dest, []


def decompose_stub(self, esp):
    rng = self.rng
    ecx = self.uc.reg_read(o.x.UC_X86_REG_ECX)
    buf = struct.unpack('<I', self.uc.mem_read(esp + 4, 4))[0]
    self.remember(ecx, 64)
    reads = [(ecx, list(self.uc.mem_read(ecx, 64)))]
    angles = [rng.choice((0.0, rng.uniform(-3.2, 3.2), rng.uniform(-3.2, 3.2), rng.uniform(-100, 100))) for _ in range(3)]
    dest = buf if rng.random() < 0.7 else PTRS + 0x100 + 0x10 * rng.randrange(8)
    self.stub_write(dest, b''.join(o.f32(a) for a in angles))
    return 1, dest, reads


def setup(self, rng):
    w = self.uc.mem_write
    r = rng.random
    w(THIS + 0x80, o.f32(rng.choice((1.0, rng.uniform(-2, 2), 0.0, 0.5))))
    w(THIS + 0x84, o.f32(rng.choice((1.0, 1.0, rng.uniform(-2, 2), 0.0, 0.5))))
    w(THIS + 0x88, o.f32(rng.choice((1.0, 1.0, rng.uniform(-2, 2), 0.0))))
    w(THIS + 0x90, b''.join(o.f32(rng.choice((0.0, rng.uniform(-5, 5)))) for _ in range(3)))
    w(JOINT + 0x10, self.matrix())
    index = rng.choice((0xFF, 0xFF, rng.randrange(0, 256), rng.randrange(0, 4)))
    w(JOINT + 0x91, bytes([index]))
    w(MODEL, struct.pack('<I', VTABLE))
    w(MODEL + 0x364, struct.pack('<I', JOINTS))
    w(VTABLE + 0x58, struct.pack('<I', VSTUB))
    if index != 0xFF:
        w(JOINTS + index * 0xA0 + 0x10, self.matrix())
    w(ANGLES, b''.join(o.f32(self.angle() * rng.choice((1, -1))) for _ in range(3)) + o.f32(0.0))
    esp = o.SP - 16 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32), rng.randrange(1 << 32), rng.randrange(1 << 32), rng.randrange(1 << 32), esp,
            rng.randrange(1 << 32), OUT + 0x40 * rng.randrange(4), JOINT]
    return regs, [THIS, MODEL, ANGLES]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x1000), (JOINTS, 0x10000), (PTRS, 0x10000), (ANGLES, 0x100), (OUT, 0x100)]
    oracle = o.Oracle([(ENTRY, 0x1a87), (0x012F0906, 0x2A7)], arenas, stubs={VSTUB: vcall_stub, DECOMPOSE: decompose_stub})
    oracle.record('0x00865990', ENTRY, setup, out, n)


main()
