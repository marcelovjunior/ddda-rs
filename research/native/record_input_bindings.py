"""L184 s5 oracle: 0x0076F570 `unit->copy_input_bindings()` (stdcall: stack
argument `unit`; `ret 4`) run as the original's code in Unicorn. The
controller's virtual method +0x10 (thiscall, no arguments, returns a class
descriptor) is a stub recorded as an event; it returns a descriptor chain
(+4 class id, +0x10 base) that does or does not reach the id at 0x018D2D10.
Run from research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe;
output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY, LENGTH = 0x0076F570, 0xEA
UNIT, CONTROLLER, VTABLE, CLASSES, ARRAYS, GLOBAL = 0x20000000, 0x22000000, 0x23000000, 0x24000000, 0x26000000, 0x018D2000
STUB = 0x30000010


def method_stub(self, esp):
    return 0, CLASSES + 0x40 * self.chain_start, []


def setup(self, rng):
    w = self.uc.mem_write
    wanted = rng.randrange(1, 1 << 32)
    w(GLOBAL + 0xD10, struct.pack('<I', wanted))
    # descriptor chain: node k has id at +4 and the next node at +0x10
    length = rng.randrange(1, 5)
    hit = rng.choice((None, None, 0, 1, 2, 3))
    for k in range(length):
        ident = wanted if hit == k else rng.randrange(1, 1 << 32)
        nxt = CLASSES + 0x40 * (k + 1) if k + 1 < length else 0
        w(CLASSES + 0x40 * k + 4, struct.pack('<I', ident))
        w(CLASSES + 0x40 * k + 0x10, struct.pack('<I', nxt))
    self.chain_start = 0
    w(VTABLE + 0x10, struct.pack('<I', STUB))
    w(CONTROLLER, struct.pack('<I', VTABLE))
    for g in range(8):
        table = CONTROLLER + 0x480 + 0x140 * g
        array = ARRAYS + 0x40 * g if rng.random() < 0.8 else 0
        w(table, struct.pack('<I', array))
        w(table + 0x10, struct.pack('<I', rng.choice((0, 0xFFFF, 0xFFFFFFFF, rng.randrange(1 << 32), 1 << rng.randrange(32)))))
        if array:
            w(array, b''.join(struct.pack('<H', rng.randrange(1 << 16)) for _ in range(16)))
    w(UNIT + 0xCD0, struct.pack('<I', 0 if rng.random() < 0.1 else CONTROLLER))
    w(UNIT + 0xD99, bytes([rng.choice((0, 1, 1, 0xFF))]))
    # pre-existing flags and bindings in the unit (a clear bit must leave the binding alone)
    w(UNIT + 0x72, bytes(rng.randrange(256) for _ in range(0x200)))
    w(UNIT + 0x70, bytes(rng.randrange(256) for _ in range(2)))
    esp = o.SP - 8 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32) for _ in range(8)]
    regs[4] = esp
    return regs, [UNIT]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(UNIT, 0x1000), (CONTROLLER, 0x1000), (VTABLE, 0x1000), (CLASSES, 0x1000), (ARRAYS, 0x1000), (GLOBAL, 0x1000)]
    oracle = o.Oracle([(ENTRY, LENGTH)], arenas, stubs={STUB: method_stub})
    oracle.record('0x0076F570', ENTRY, setup, out, n)


main()
