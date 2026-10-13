"""L184 s5 oracle: 0x00FE4F30 `state->set_byte_constant(byte)` (stdcall: stack
arguments state, byte; `ret 8`) run as the original's code in Unicorn. The
frame allocator it may call, 0x00FED890 (thiscall, one stack argument), is a
stub recorded as an event whose effects move the state's allocation pointers
(or hand out a null slot). Constants cover exact quantisation boundaries,
NaN, infinities and huge values (the x87 FISTP "integer indefinite").
Run from research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe;
output is private (E:/ddda-re/notes/native-L184).
"""
import math
import struct
import sys

import cns_oracle as o

ENTRY, LENGTH = 0x00FE4F30, 0x120
ALLOCATOR = 0x00FED890
STATE, OWNER, DESC, BUF, FRESH = 0x20000000, 0x22000000, 0x22001000, 0x24000000, 0x26000000


def quantised(value):
    p = value * 255.0
    if math.isnan(p) or math.isinf(p) or abs(p) >= 2 ** 63:
        return 0
    return int(p) & 0xFFFFFFFF  # int() truncates toward zero


def allocator_stub(self, esp):
    rng = self.rng
    state = self.uc.reg_read(o.x.UC_X86_REG_ECX)
    slot = 0 if rng.random() < 0.1 else FRESH + 0x10 * rng.randrange(0, 64)
    self.stub_write(state + 0x24, struct.pack('<I', slot))
    self.stub_write(state + 0x28, struct.pack('<I', slot + 0x10000))
    return 1, rng.randrange(1 << 32), []


def setup(self, rng):
    w = self.uc.mem_write
    f = o.f32
    value = rng.choice((0.0, 1.0, 0.5, rng.random(), rng.uniform(-2, 3), 1e30, -1e30, float('nan'), float('inf'),
                        rng.randrange(256) / 255.0, rng.randrange(256) / 255.0, (rng.randrange(256) + 0.999999) / 255.0, 1.0 + rng.random(), 1.5))
    value = struct.unpack('<f', struct.pack('<f', value))[0] if abs(value) < 3e38 else value
    pointer = 0 if rng.random() < 0.1 else BUF + 0x10 * rng.randrange(0, 8)
    flagged = pointer | (rng.randrange(16) if pointer else 0)
    if pointer:
        w(pointer, f(value))
    w(STATE + 0x11C8, struct.pack('<I', flagged))
    w(STATE + 0x11CC, struct.pack('<I', rng.randrange(1 << 32)))
    # the change log record flag (bit 7 of the low byte) and its index
    w(STATE + 0x5C, struct.pack('<I', rng.choice((0, 0x80, 0x181, rng.randrange(1 << 32)))))
    index = rng.randrange(0, 4)
    w(STATE + 0x6C, struct.pack('<I', index))
    w(STATE + 0x30C7C + 12 * index, struct.pack('<III', rng.randrange(1 << 32), 0, 0))
    w(STATE + 0x18, struct.pack('<I', OWNER))
    w(OWNER + 0x7E8, struct.pack('<I', DESC))
    w(DESC + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 16, 17, rng.randrange(0, 200)))))
    used = FRESH + 0x10 * rng.randrange(0, 64) if rng.random() < 0.9 else 0
    w(STATE + 0x24, struct.pack('<I', used))
    w(STATE + 0x28, struct.pack('<I', rng.choice((used + 0x10000, used, used + 0x20, 0, 0xFFFFFFFF))))
    w(STATE + 0x154, struct.pack('<I', rng.randrange(1 << 32)))
    cur = quantised(value) if pointer else 0
    byte = cur & 0xFF if (cur < 256 and rng.random() < 0.4) else rng.randrange(256)
    if 256 <= cur < 512 and rng.random() < 0.6:
        byte = cur - 256   # same low byte, different value: the comparison is on the whole word
    junk = rng.randrange(1 << 24) << 8
    esp = o.SP - 12 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32) for _ in range(8)]
    regs[4] = esp
    return regs, [STATE, junk | byte]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(STATE, 0x32000), (OWNER, 0x1000), (DESC, 0x1000), (BUF, 0x1000), (FRESH, 0x20000)]
    oracle = o.Oracle([(ENTRY, LENGTH)], arenas, stubs={ALLOCATOR: allocator_stub})
    oracle.record('0x00FE4F30', ENTRY, setup, out, n)


main()
