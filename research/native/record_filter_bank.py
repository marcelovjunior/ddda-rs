"""Oracle for 0x00DCDDE0 (sound engine, thiscall, `ret 0xC`): a bank of 4-lane biquad filters.
Stack: buffer pointer, rounds, lane (0..3). For each round, for each of [this+0x270] channels (records
0x40 apart), one direct-form-I sample is filtered in place through the lane's five coefficients
(this+0x70/0x80/0x90/0xA0/0xB0 + lane*4); results under the denormal threshold (0x0142ABE8, compared
with the abs mask at 0x0161C330) are flushed to zero. No guest calls. NaN inputs are not generated:
the port uses plain f32 arithmetic and does not reproduce NaN payload order.
Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_filter_bank.py OUT.json [N]"""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

ENTRY, LENGTH = 0x00DCDDE0, 0xF3
THIS, BUF = 0x20000000, 0x21000000
THRESHOLD, MASK = 0x0142ABE8, 0x0161C330
SP = 0x0F008000


def bits(f):
    return struct.unpack('<I', struct.pack('<f', f))[0]


def sample(r):
    k = r.random()
    if k < 0.08:
        return 0.0
    if k < 0.25:
        return r.uniform(-1e-9, 1e-9)       # around the flush threshold
    if k < 0.30:
        return struct.unpack('<f', struct.pack('<I', r.randrange(1, 1 << 23)))[0]  # denormal
    if k < 0.32:
        return r.choice((float('inf'), float('-inf')))
    return r.uniform(-4.0, 4.0)


def main():
    out, count = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle(ENTRY, LENGTH, {}, [(THIS, 0x4000), (BUF, 0x1000), (THRESHOLD & ~0xFFF, 0x1000), (MASK & ~0xFFF, 0x1000), (0x0F000000, 0x10000)])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc

        def w(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        uc.mem_write(THIS, bytes(r.randrange(256) for _ in range(0x4000)))
        channels = r.choice((0, 1, 1, 2, 3, 5))
        rounds = r.choice((0, 1, 1, 2, 3, 4))
        lane = r.randrange(4)
        w(THIS + 0x270, channels)
        for base in (0x70, 0x80, 0x90, 0xA0, 0xB0):
            for k in range(4):
                w(THIS + base + 4 * k, bits(r.uniform(-1.5, 1.5)))
        for ch in range(6):
            for k in range(4):
                for slot in (-0xC, -8, -4, 0):
                    w(THIS + 0xCC + k * 16 + 0x40 * ch + slot, bits(sample(r)))
        for k in range(rounds * max(channels, 1) + 2):
            w(BUF + 4 * k, bits(sample(r)))
        threshold = r.choice((1e-10, 1e-20, 1e-5, r.uniform(1e-12, 1e-3)))
        w(THRESHOLD, bits(threshold))
        if r.random() < 0.2:
            # boundary cases: pure gain (all taps and states zero, gain 1), the sample at, one step
            # below and one step above the flush threshold, either sign
            for base in (0x80, 0x90, 0xA0, 0xB0):
                for k in range(4):
                    w(THIS + base + 4 * k, 0)
            for k in range(4):
                w(THIS + 0x70 + 4 * k, bits(1.0))
            for ch in range(6):
                for k in range(4):
                    for slot in (-0xC, -8, -4, 0):
                        w(THIS + 0xCC + k * 16 + 0x40 * ch + slot, 0)
            t = bits(threshold)
            for k in range(rounds * max(channels, 1) + 2):
                v = r.choice((t - 1, t, t + 1)) | r.choice((0, 0x80000000))
                w(BUF + 4 * k, v)
        uc.mem_write(MASK, struct.pack('<IIII', 0x7FFFFFFF, 0x7FFFFFFF, 0x7FFFFFFF, 0x7FFFFFFF))
        regs = [r.randrange(1 << 32) for _ in range(8)]
        regs[1] = THIS
        esp = SP - 0x20 - 4 * r.randrange(4)
        cases.append(oracle.run(seed, regs, esp, [BUF, rounds, lane]))
    finish(out, '0x00DCDDE0', cases)


main()
