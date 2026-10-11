"""Vectors for the readable MT math ports (L24, crates/mtmath). Private output.

    PY research/native/vectors_math.py lift   # -> vectors/math_lift.json
    PY research/native/vectors_math.py edge   # -> vectors/math_edge.json

`lift` copies the lifted program's own vectors (vectors/lift.json) for every
ported address, so crates/ddnative/tests/math.rs can replay exactly the
cases that verified the lifted body without loading the whole file.

`edge` runs the original in Unicorn on chosen inputs: zero, signed zero,
denormals, huge values, infinities, quiet and signalling NaNs with several
payloads, and random values; random states for the generator. Objects are
filled with random garbage and the stack window is recorded too, so a
replay also proves which bytes the original leaves alone. Every general
register and all eight xmm registers (all lanes) are recorded on entry and
exit. MXCSR is 0x1F80 and the x87 control word 0x037F, as in run_sparse.

Unicorn applies the x87 NaN rule (larger payload) where SSE hardware returns
the first operand's NaN, so a case in which one SSE instruction meets two
NaNs that differ (after quieting) is dropped; a case with a single NaN
input, or the same NaN twice, is kept.

Reads the user's own exe at run time; nothing from it is committed.
"""
import json
import os
import random
import struct
import sys

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UcError
import unicorn.x86_const as xc

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402

DATA = os.environ.get("LIFT_DATA", "E:/ddda-re/notes/native")
OUT = f"{DATA}/vectors"

GPR = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
UC_GPR = [xc.UC_X86_REG_EAX, xc.UC_X86_REG_ECX, xc.UC_X86_REG_EDX, xc.UC_X86_REG_EBX,
          xc.UC_X86_REG_ESP, xc.UC_X86_REG_EBP, xc.UC_X86_REG_ESI, xc.UC_X86_REG_EDI]
UC_XMM = [getattr(xc, f"UC_X86_REG_XMM{k}") for k in range(8)]

OBJ = h.SCRATCH + 0x100       # `this`: 0x100 garbage bytes either side
OBJ_WINDOW = (h.SCRATCH, 0x300)
ESP = h.STACK + h.STACK_SIZE - 0x1000
STACK_WINDOW = (ESP - 0x40, 0x80)

# The ported functions: address -> (inputs kind, stack argument count)
PORTS = {
    0x00D00690: ("random_next", 0),
    0x00D006C0: ("random_seed", 1),
    0x00CFC770: ("projection", 4),
    0x00CFC810: ("projection", 4),
    0x00CFC8B0: ("projection", 4),
    0x00CFC940: ("projection", 4),
    0x007AD870: ("angle", 0),
}

NANS = [0x7FC00000, 0xFFC00000, 0x7FC12345, 0x7F800001, 0xFFA00007]
SPECIAL = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x00000001, 0x80400000, 0x007FFFFF,
           0x7F7FFFFF, 0xFF7FFFFF, 0x7F800000, 0xFF800000, 0x3DCCCCCD, 0x447A0000, 0x0DA24260,
           0x40490FDB, 0xC0490FDB, 0x40C90FDB, 0xC0C90FDB, 0x40490FDC, 0x3FC90FDB]


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def is_nan(b):
    return (b >> 23) & 0xFF == 0xFF and b & 0x7FFFFF


class Dropped(Exception):
    pass


def run(addr, regs, xmm, obj, args):
    """One call of the original; returns the recorded case."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(h.TCG_BUFFER)
    base, secs = h.image()
    for va, b in secs:
        start = va & ~0xFFF
        uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    uc.mem_map(h.STACK, h.STACK_SIZE)
    uc.mem_map(h.SCRATCH, h.SCRATCH_SIZE)
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
    uc.reg_write(xc.UC_X86_REG_FPCW, 0x037F)
    uc.reg_write(xc.UC_X86_REG_FPTAG, 0xFFFF)
    rnd = random.Random(regs["ecx"] ^ addr)
    stack = bytearray(rnd.randbytes(STACK_WINDOW[1]))
    frame = struct.pack("<I", h.RET) + b"".join(struct.pack("<I", a) for a in args)
    stack[0x40:0x40 + len(frame)] = frame
    uc.mem_write(STACK_WINDOW[0], bytes(stack))
    uc.mem_write(OBJ_WINDOW[0], bytes(obj))
    for name, r in zip(GPR, UC_GPR):
        uc.reg_write(r, regs[name])
    for k, r in enumerate(UC_XMM):
        uc.reg_write(r, sum(v << (32 * i) for i, v in enumerate(xmm[k])))
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    bad = []

    def lanes(i, op):
        if op.type == 1:
            v = uc.reg_read(UC_XMM[int(i.reg_name(op.reg)[3:])])
            return [(v >> (32 * k)) & 0xFFFFFFFF for k in range(4)]
        ea = (uc.reg_read(UC_GPR[GPR.index(i.reg_name(op.mem.base))]) if op.mem.base else 0) + op.mem.disp
        if op.mem.index:
            ea += uc.reg_read(UC_GPR[GPR.index(i.reg_name(op.mem.index))]) * op.mem.scale
        n = 4 if i.mnemonic.endswith("ps") else 1
        return list(struct.unpack(f"<{n}I", bytes(uc.mem_read(ea & 0xFFFFFFFF, 4 * n))))

    def hook(uc_, a, size, _):
        i = next(md.disasm(bytes(uc_.mem_read(a, size)), a))
        if i.mnemonic == "call":
            bad.append(f"call at {a:#x}")
            uc_.emu_stop()
        if i.mnemonic in h.SSE_BINARY and i.mnemonic.endswith(("ss", "ps")):
            x, y = lanes(i, i.operands[0]), lanes(i, i.operands[1])
            for p, q in zip(x, y):
                if is_nan(p) and is_nan(q) and (p | 0x400000) != (q | 0x400000):
                    bad.append(f"two different NaNs at {a:#x}")
                    uc_.emu_stop()

    uc.hook_add(UC_HOOK_CODE, hook)
    try:
        uc.emu_start(addr, h.RET, count=100000)
    except UcError as e:
        raise Dropped(f"{e}")
    if bad:
        raise Dropped(bad[0])
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET:
        raise Dropped("did not return")
    read_xmm = lambda r: [(uc.reg_read(r) >> (32 * k)) & 0xFFFFFFFF for k in range(4)]
    return {
        "addr": addr,
        "regs_in": regs, "xmm_in": xmm,
        "mem_in": [[OBJ_WINDOW[0], bytes(obj).hex()], [STACK_WINDOW[0], bytes(stack).hex()]],
        "regs_out": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)},
        "xmm_out": [read_xmm(r) for r in UC_XMM],
        "mem_out": [[w[0], bytes(uc.mem_read(w[0], w[1])).hex()] for w in (OBJ_WINDOW, STACK_WINDOW)],
    }


def floats(rnd):
    """A float bit pattern: special values, NaNs, or a random finite float."""
    k = rnd.random()
    if k < 0.45:
        return rnd.choice(SPECIAL)
    if k < 0.55:
        return rnd.choice(NANS)
    if k < 0.75:
        return f2b(rnd.uniform(-10.0, 10.0))
    return f2b(rnd.choice([-1, 1]) * 10 ** rnd.uniform(-40, 38))


def edge_cases(addr, kind, nargs, n, seed):
    rnd = random.Random(seed)
    cases = []
    tries = 0
    while len(cases) < n and tries < n * 20:
        tries += 1
        regs = {r: rnd.getrandbits(32) for r in GPR}
        regs["esp"] = ESP
        regs["ecx"] = OBJ
        xmm = [[rnd.getrandbits(32) for _ in range(4)] for _ in range(8)]
        obj = bytearray(rnd.randbytes(OBJ_WINDOW[1]))
        args = [rnd.getrandbits(32) for _ in range(nargs)]
        if kind == "random_next":
            choice = rnd.random()
            words = ([0, 0, 0, 0] if choice < 0.1 else [0xFFFFFFFF] * 4 if choice < 0.2
                     else [rnd.getrandbits(32) for _ in range(4)])
            struct.pack_into("<4I", obj, OBJ - OBJ_WINDOW[0], *words)
        elif kind == "random_seed":
            args = [rnd.choice([0, 1, 0xFFFFFFFF, 0x00AC9365, 0x80000000, rnd.getrandbits(32),
                                rnd.getrandbits(32)])]
        elif kind == "projection":
            args = [floats(rnd) for _ in range(4)]
        elif kind == "angle":
            xmm[0][0], xmm[1][0] = floats(rnd), floats(rnd)
        try:
            cases.append(run(addr, regs, xmm, obj, args))
        except Dropped:
            continue
    return cases


def lift():
    want = set(PORTS)
    cases = json.load(open(f"{OUT}/lift.json"))
    sub = [c for c in cases if c["addr"] in want]
    missing = want - {c["addr"] for c in sub}
    if missing:
        raise SystemExit(f"no lifted vectors for {sorted(hex(a) for a in missing)}")
    json.dump(sub, open(f"{OUT}/math_lift.json", "w"))
    print(f"math_lift.json: {len(sub)} cases for {len(want)} functions")


def edge():
    cases = []
    for addr, (kind, nargs) in sorted(PORTS.items()):
        got = edge_cases(addr, kind, nargs, 300, addr)
        print(f"0x{addr:08X} {kind}: {len(got)} cases")
        cases += got
    json.dump(cases, open(f"{OUT}/math_edge.json", "w"))
    print(f"math_edge.json: {len(cases)} cases")


if __name__ == "__main__":
    {"lift": lift, "edge": edge}[sys.argv[1]]()
