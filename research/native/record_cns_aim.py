"""L184 s5 oracle: 0x00864660, mode 6 of uCnsEdit::vf24 (usercall: EAX = this,
ECX = model, EDI = joint, ESI = output buffer, stack argument = pointer to
the three angles in radians, `ret 4`). It builds a matrix in the buffer: an
aim rotation of the joint toward the joint two after `this+0xA8` (angle 0),
then rotations by angles 1 and 2. The original code runs, including
0x00421ED0 (joint lookup by index) and the CRT sinf/cosf. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe from research/native; output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY = 0x00864660
THIS, JOINTS, MODEL, TABLE, ANGLES = 0x20000000, 0x23000000, 0x22000000, 0x22001000, 0x0F004000
OUT = o.SP + 0x400
JOINT_RECORDS = 6


def setup(self, rng):
    w = self.uc.mem_write
    # model: joint count (+0x360), joint array (+0x364), index -> joint byte table (+0x368)
    w(MODEL + 0x360, struct.pack('<I', rng.choice((0, 3, 8, 40, 256))))
    w(MODEL + 0x364, struct.pack('<I', JOINTS))
    w(MODEL + 0x368, struct.pack('<I', TABLE))
    for k in range(256):
        w(TABLE + k, bytes([rng.randrange(JOINT_RECORDS)]))
    base = rng.randrange(0, 3) + rng.choice((0, 0, 1, 5))
    w(THIS + 0xA8, struct.pack('<I', base if rng.random() < 0.9 else rng.randrange(1 << 32)))
    joint = JOINTS + 0xA0 * JOINT_RECORDS  # the constrained joint's record, apart from the table's
    for k in range(JOINT_RECORDS):
        w(JOINTS + 0xA0 * k + 0x10, self.matrix())
    w(joint + 0x10, self.matrix('rand' if rng.random() < 0.85 else None))
    w(ANGLES, b''.join(o.f32(self.angle()) for _ in range(3)) + o.f32(0.0))
    model = 0 if rng.random() < 0.04 else MODEL
    esp = o.SP - 8 - 4 * rng.randrange(0, 64)
    regs = [THIS, model, rng.randrange(1 << 32), rng.randrange(1 << 32), esp,
            rng.randrange(1 << 32), OUT + 0x40 * rng.randrange(4), joint]
    return regs, [ANGLES]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(THIS, 0x1000), (JOINTS, 0x10000), (MODEL, 0x1000), (TABLE, 0x1000), (ANGLES, 0x100), (OUT, 0x100)]
    oracle = o.Oracle([(ENTRY, 0x1326), (0x00421ED0, 0x2F), (0x012F0906, 0x2A7)], arenas)
    oracle.record('0x00864660', ENTRY, setup, out, n)


main()
