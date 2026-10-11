"""Vectors that check the platform models (crates/ddnative/src/platform.rs)
against the original CRT routines they stand for: memset 0x012EFE20 and
memcpy 0x012EFEA0, run in Unicorn over a random 0x1000-byte buffer, with
sizes on both sides of the SSE threshold (0x80), the CRT's SSE2 flag
(0x019AA078) on and off, and overlapping copies in both directions.

    python vectors_platform.py      # E:/ddda-re/notes/native/vectors/platform.json

Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402

OUT = "E:/ddda-re/notes/native/vectors/platform.json"
SSE2_FLAG = 0x019AA078


def main():
    rnd = random.Random(61)
    S = h.SCRATCH
    cases = []
    for name, addr in (("memset", 0x012EFE20), ("memcpy", 0x012EFEA0)):
        for k in range(160):
            buf = bytes(rnd.getrandbits(8) for _ in range(0x1000))
            n = rnd.choice([0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 33, 64, 0x7F, 0x80, 0x81, 0x100, 0x1F3, 0x400, rnd.randrange(0x800)])
            dst = S + rnd.randrange(0x1000 - n) if n < 0x1000 else S
            if name == "memset":
                args = [dst, rnd.getrandbits(32), n]
            else:
                src = S + rnd.randrange(0x1000 - n) if n < 0x1000 else S
                if k % 4 == 0 and n:  # overlap, either direction
                    src = max(S, min(S + 0x1000 - n, dst + rnd.randrange(-min(n, 64), min(n, 64) + 1)))
                args = [dst, src, n]
            sse = rnd.random() < 0.5
            r = h.run(addr, args=args, scratch=buf, poke={SSE2_FLAG: int(sse)})
            cases.append({"model": name, "args": args, "sse2": sse, "scratch_base": S,
                          "scratch_in": buf.hex(), "scratch_out": r["scratch"][:0x1000].hex(), "eax": r["eax"]})
    json.dump(cases, open(OUT, "w"))
    print(len(cases), "cases")


if __name__ == "__main__":
    main()
