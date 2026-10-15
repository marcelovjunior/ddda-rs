"""Oracles for two more leaf functions (no guest calls), one JSON per function in OUT_DIR:

  look_bits.json          0x01328F30  cdecl(reader, bits): peeks up to 32 bits of a little-endian bit reader
                          (the Ogg `look`); -1 when the stream is too short. The mask table is read from the image.
  dominant_row.json       0x011AE1F0  cdecl(out, matrix, vector): three dot products of the vector with the
                          matrix's rows (+0x10/+0x20/+0x30), and the row with the largest product is copied to `out`.

Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_leaves2.py OUT_DIR [N]"""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

SP = 0x0F008000
STACK = (0x0F000000, 0x10000)
REGION = 0x20000000
MASKS = 0x01470658


def run_flags(oracle, *args):
    """Oracle.run plus the EFLAGS the original leaves (arithmetic flags are compared by the tests)."""
    import unicorn.x86_const as ux
    case = oracle.run(*args)
    case['after']['eflags'] = oracle.uc.reg_read(ux.UC_X86_REG_EFLAGS)
    return case


def regs_for(r, **fixed):
    regs = [r.randrange(1 << 32) for _ in range(8)]
    for name, v in fixed.items():
        regs['eax ecx edx ebx esp ebp esi edi'.split().index(name)] = v
    return regs


def look_bits(out, count):
    table = h.code(MASKS, 33 * 4)
    oracle = Oracle(0x01328F30, 0x9F, {}, [(REGION, 0x1000), (MASKS & ~0xFFF, 0x2000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        uc.mem_write(MASKS, bytes(table))
        reader, data = REGION + 0x100, REGION + 0x400
        storage = r.choice((1, 2, 3, 5, 6, 8, 12))
        endbyte = r.choice((0, 0, 1, storage - 5 if storage > 5 else 0, storage - 1, storage - 4 if storage > 4 else 0, r.randrange(storage + 1)))
        endbit = r.randrange(8)
        uc.mem_write(data, bytes(r.randrange(256) for _ in range(64)))
        for off, v in ((0, endbyte), (4, endbit), (0xC, data + endbyte), (0x10, storage)):
            uc.mem_write(reader + off, struct.pack('<I', v))
        bits = r.choice((0, 1, 7, 8, 9, 15, 16, 17, 24, 25, 31, 32, r.randrange(33)))
        regs = regs_for(r)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), [reader, bits]))
    finish(out, '0x01328F30', cases)


def dominant_row(out, count):
    oracle = Oracle(0x011AE1F0, 0xA8, {}, [(REGION, 0x2000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        dest, matrix, vector = REGION + 0x100, REGION + 0x400, REGION + 0x800

        def val():
            k = r.random()
            if k < 0.04:
                return float('nan')
            return r.choice((0.0, 1.0, -1.0, 0.5, r.uniform(-4, 4)))
        uc.mem_write(matrix, b''.join(struct.pack('<f', v) for v in [0.0] * 4))      # row 0 (not read)
        # rows 1..3: random, with some ties so that t0 < t1 and max < t2 are exercised at equality
        rows = [[val() for _ in range(4)] for _ in range(3)]
        if r.random() < 0.3:
            rows[1] = list(rows[0])
        if r.random() < 0.3:
            rows[2] = list(rows[r.randrange(2)])
        for i, row in enumerate(rows):
            uc.mem_write(matrix + 0x10 * (i + 1), b''.join(struct.pack('<f', v) if v == v else struct.pack('<I', 0x7FC00000) for v in row))
        vec = [val() for _ in range(4)]
        uc.mem_write(vector, b''.join(struct.pack('<f', v) if v == v else struct.pack('<I', 0x7FC00000) for v in vec))
        uc.mem_write(dest, bytes(r.randrange(256) for _ in range(16)))
        regs = regs_for(r)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), [dest, matrix, vector]))
    finish(out, '0x011AE1F0', cases)


def main():
    out, count = Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    out.mkdir(parents=True, exist_ok=True)
    look_bits(out / 'look_bits.json', count)
    dominant_row(out / 'dominant_row.json', count)


main()
