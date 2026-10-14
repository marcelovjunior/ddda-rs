"""L184 s5 oracle: 0x00DEFA60 `context->release_pending()` (fastcall, ECX =
context, plain `ret`) run as the original's code in Unicorn. Its six release
functions (thiscall, one stack argument, `ret 4`) are stubs recorded as events;
a stub sometimes rewrites the current bucket's element array pointer, which the
original re-reads before every call. Run from research/native with
E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY, LENGTH = 0x00DEFA60, 0x1FC
TARGETS = (0x01134A90, 0x01134CA0, 0x01134BD0, 0x01134E40, 0x00FB4BF0, 0x01131290)
CTX, DATA, DATA2 = 0x20000000, 0x24000000, 0x26000000
# (total offset, first bucket offset) of the six kinds
KINDS = ((0x10E10, 0x10C84), (0x11460, 0x112D4), (0x10FA4, 0x10E18), (0x115F4, 0x11468), (0x112CC, 0x11140), (0x11138, 0x10FAC))


def release_stub(self, esp):
    rng = self.rng
    if rng.random() < 0.15:
        edi = self.uc.reg_read(o.x.UC_X86_REG_EDI)   # the current bucket
        self.stub_write(edi + 8, struct.pack('<I', DATA2 + 0x100 * rng.randrange(0, 64)))
    return 1, rng.randrange(1 << 32), []


def setup(self, rng):
    w = self.uc.mem_write
    # element arrays: addresses into DATA (records) or pointer arrays
    for k, (total, buckets) in enumerate(KINDS):
        for b in range(25):
            count = 0 if rng.random() < 0.7 else rng.randrange(1, 4)
            base = DATA + 0x200 * (k * 25 + b)
            w(CTX + buckets + 16 * b, struct.pack('<IIII', count, rng.randrange(1 << 32), base, rng.randrange(1 << 32)))
            w(base, b''.join(struct.pack('<I', DATA2 + 0x10 * rng.randrange(0, 256)) for _ in range(8)))
        # the kind's total sits right after its 25 buckets (it overlaps the last bucket's last dword)
        w(CTX + total, struct.pack('<I', rng.choice((0, 1, 5, 0x7FFFFFFF, 0xFFFFFFFF, 0x80000000))))
    esp = o.SP - 4 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32) for _ in range(8)]
    regs[1], regs[4] = CTX, esp
    return regs, []


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(CTX, 0x12000), (DATA, 0x14000), (DATA2, 0x10000)]
    oracle = o.Oracle([(ENTRY, LENGTH)], arenas, stubs={t: release_stub for t in TARGETS})
    oracle.record('0x00DEFA60', ENTRY, setup, out, n)


main()
