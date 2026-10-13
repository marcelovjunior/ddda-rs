"""L184 s5 oracle: 0x00CFDC80, the matrix blend uCnsEdit applies for a weight other
than 1 (usercall: ECX = the matrix, which is also the first stack argument; stack
arguments: matrix, other matrix, weight float; `ret 0xc`). The matrix is replaced by
the blend toward the other one (the identity in the game; any matrix here).
The original code runs. Run from research/native with
E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY, LENGTH = 0x00CFDC80, 0x97B
A, B = 0x20000000, 0x20001000


def setup(self, rng):
    w = self.uc.mem_write
    kind = rng.choice(('rand', 'rand', 'rot', 'identity', 'sparse', 'zero'))
    w(A, self.matrix(kind))
    w(B, self.matrix(rng.choice(('identity', 'identity', 'identity', 'rand', 'rot', 'sparse'))))
    weight = rng.choice((0.0, 1.0, 0.5, rng.uniform(0, 1), rng.uniform(0, 1), rng.uniform(-1, 2), 1e-30, 2.5))
    esp = o.SP - 16 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32) for _ in range(8)]
    regs[1], regs[4] = A, esp
    return regs, [A, B, struct.unpack('<I', o.f32(weight))[0]]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = o.Oracle([(ENTRY, LENGTH)], [(A, 0x2000)])
    oracle.record('0x00CFDC80', ENTRY, setup, out, n)


main()
