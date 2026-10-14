"""L191 oracle: 0x0084C720 `uCharacterBase::vf09()` (thiscall, plain ret; void).
Sets bit 0x200000 of +0x1B58 from the byte at +0x2F84, calls 0x008586E0 (EAX =
the character) and 0x00886530 (the model draw prep), and when +0x23BC is set
calls the vtable slot 0x94 of the character (its result points at an object
whose byte +0x206D picks the mode), then the stdcall 0x00CA5930 with six
arguments. The vtable and the callees are recorded stubs. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x0084C720, 0xE0
THIS, VTABLE, OBJECTS = 0x20000000, 0x20010000, 0x20030000
SP = 0x0F008000
CALL_A, CALL_B, STDCALL, VMETHOD = 0x008586E0, 0x00886530, 0x00CA5930, 0x30000100


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000

    def plain(o, regs, esp):
        r = o.rng
        if r.random() < 0.5:  # the callee changes the character's state
            o.poke(THIS + 0x2ECC, struct.pack('<I', r.randrange(1 << 32)))
        return r.randrange(1 << 32)

    def method(o, regs, esp):
        r = o.rng
        return OBJECTS + r.randrange(0x1000) if r.random() < 0.8 else 0

    stubs = {CALL_A: (0, plain), CALL_B: (0, plain), STDCALL: (6, plain), VMETHOD: (0, method)}
    oracle = Oracle(ENTRY, LENGTH, stubs, [(THIS, 0x8000), (VTABLE, 0x1000), (OBJECTS, 0x8000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        rng = oracle.rng = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x8000)))
        w(THIS, VTABLE)
        uc.mem_write(VTABLE, bytes(rng.randrange(256) for _ in range(0x1000)))
        w(VTABLE + 0x94, VMETHOD)
        uc.mem_write(OBJECTS, bytes(rng.randrange(256) for _ in range(0x8000)))
        uc.mem_write(THIS + 0x2F84, bytes([rng.choice((0, 0, 1, rng.randrange(256)))]))
        w(THIS + 0x1B58, rng.randrange(1 << 32))
        w(THIS + 0x2360, rng.choice((0, 0x20000000, rng.randrange(1 << 32))))
        w(THIS + 0x23BC, rng.choice((0, 0, rng.randrange(1, 1 << 32))))
        w(THIS + 0x26A0, rng.randrange(1 << 32))
        w(THIS + 0x26A4, rng.randrange(1 << 32))
        w(THIS + 0x2ECC, rng.randrange(1 << 32))
        w(THIS + 0x2748, rng.randrange(1 << 32))
        esp = SP - 0x20 - 4 * rng.randrange(4)
        regs = [rng.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        cases.append(oracle.run(seed, regs, esp, []))
    finish(out, '0x0084C720', cases)


main()
