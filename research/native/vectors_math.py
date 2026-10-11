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
the first operand's NaN, quieted (Intel SDM vol. 1, 4.8.3.5; checked on the
host CPU by mtmath's scalar test). When an SSE add/sub/mul/div meets two
NaNs, the recorder overwrites those result lanes with the hardware's
answer before the next instruction runs, and counts it in the case's
"nan_fixups". min/max return their second operand on any NaN, which
Unicorn already does. Everything else is the emulator's.

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
ESP = h.STACK + h.STACK_SIZE - 0x1000  # minus 0, 4, 8 or 12: every frame alignment
BELOW, ABOVE = 0x200, 0x40  # the stack window around ESP (callees' frames included)
# Calls the ported functions make to code that is not ported. The original
# callee runs in Unicorn; the case records the machine at its entry and at
# its return (registers, all xmm lanes, the stack window), and the replay
# checks the entry and restores the return state (crates/mtmath/tests).
CALLOUTS = {0x012F1109, 0x012F0906, 0x012F0A55, 0x012F0D4C, 0x012F0BAC}  # CRT atan2 (doubles), sinf, cosf, asinf, tanf

# The ported functions: address -> (inputs kind, stack argument count)
PORTS = {
    0x00D00690: ("random_next", 0),
    0x00D006C0: ("random_seed", 1),
    0x00CFC770: ("projection", 4),
    0x00CFC810: ("projection", 4),
    0x00CFC8B0: ("projection", 4),
    0x00CFC940: ("projection", 4),
    0x007AD870: ("angle", 0),
    0x00CFD5E0: ("look_at", 3),
    0x00CFD890: ("look_at", 3),
    0x00CFDB10: ("reflect", 1),
    0x007AD9D0: ("yaw", 0),
    0x007AD7A0: ("yaw", 0),
    0x007AD8E0: ("rotate", 2),
    0x00CFC9E0: ("euler", 1),
    0x00CFCB00: ("euler", 1),
    0x00CFCC20: ("euler", 1),
    0x00CFCD40: ("euler", 1),
    0x00CFCE60: ("euler", 1),
    0x00CFCF80: ("euler", 1),
    0x00CFC350: ("fov", 4),
    0x00CFC4B0: ("fov", 6),
    0x00CFC620: ("fov", 4),
}
BASE_MATRIX = 0x016295C0  # the identity the field-of-view builders copy (poked, recorded)
MATRIX = h.SCRATCH + 0x200  # the input matrix of the Euler conversions
PIVOTS = {0x00CFC9E0: 2, 0x00CFCB00: 1, 0x00CFCC20: 6, 0x00CFCD40: 4, 0x00CFCE60: 8, 0x00CFCF80: 9}


def rotation(rnd):
    """A random rotation matrix (product of rotations about x, y, z)."""
    import math
    def rx(a): return [[1, 0, 0], [0, math.cos(a), math.sin(a)], [0, -math.sin(a), math.cos(a)]]
    def ry(a): return [[math.cos(a), 0, -math.sin(a)], [0, 1, 0], [math.sin(a), 0, math.cos(a)]]
    def rz(a): return [[math.cos(a), math.sin(a), 0], [-math.sin(a), math.cos(a), 0], [0, 0, 1]]
    def mul(a, b): return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    m = mul(mul(rx(rnd.uniform(-4, 4)), ry(rnd.uniform(-4, 4))), rz(rnd.uniform(-4, 4)))
    return [m[0] + [0], m[1] + [0], m[2] + [0], [rnd.uniform(-9, 9) for _ in range(3)] + [1]]
ROTATE_BASE = 0x018CFE80  # global vector read by 0x007AD8E0 (poked, recorded as a window)
VEC = [h.SCRATCH + 0x20, h.SCRATCH + 0x40, h.SCRATCH + 0x60, h.SCRATCH + 0x80]  # input vectors

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


def run(addr, regs, xmm, obj, args, globals_=()):
    """One call of the original; returns the recorded case. `globals_` are
    (address, bytes) written into the image first and recorded as extra
    memory windows."""
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
    rnd = random.Random(regs["eax"] ^ addr)
    stack_window = (regs["esp"] - BELOW, BELOW + ABOVE)
    stack = bytearray(rnd.randbytes(stack_window[1]))
    frame = struct.pack("<I", h.RET) + b"".join(struct.pack("<I", a) for a in args)
    stack[BELOW:BELOW + len(frame)] = frame
    uc.mem_write(stack_window[0], bytes(stack))
    uc.mem_write(OBJ_WINDOW[0], bytes(obj))
    for ga, gb in globals_:
        uc.mem_write(ga, gb)
    for name, r in zip(GPR, UC_GPR):
        uc.reg_write(r, regs[name])
    for k, r in enumerate(UC_XMM):
        uc.reg_write(r, sum(v << (32 * i) for i, v in enumerate(xmm[k])))
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    bad = []
    fixups = []   # [(register, {lane: bits})] to apply before the next instruction
    fixed = [0]
    calls, inside = [], []  # recorded callouts; [(return address, esp after return)]
    read_xmm = lambda r: [(uc.reg_read(r) >> (32 * k)) & 0xFFFFFFFF for k in range(4)]

    def machine():
        return {"regs": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)}, "xmm": [read_xmm(r) for r in UC_XMM]}

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
        while fixups:
            reg, want = fixups.pop()
            v = uc_.reg_read(reg)
            for lane, bits in want.items():
                v = (v & ~(0xFFFFFFFF << (32 * lane))) | (bits << (32 * lane))
            uc_.reg_write(reg, v)
        if inside:
            back, esp = inside[-1]
            if a == back and uc_.reg_read(xc.UC_X86_REG_ESP) == esp:
                inside.pop()
                calls[-1]["out"] = machine()
                calls[-1]["out"]["stack"] = bytes(uc_.mem_read(*stack_window)).hex()
            return  # the callee is recorded, not judged
        i = next(md.disasm(bytes(uc_.mem_read(a, size)), a))
        if i.mnemonic == "call":
            target = i.operands[0].imm & 0xFFFFFFFF if i.operands[0].type == 2 else None
            if target not in CALLOUTS:
                bad.append(f"call at {a:#x}")
                uc_.emu_stop()
                return
            calls.append({"target": target, "in": machine()})
            inside.append((a + size, uc_.reg_read(xc.UC_X86_REG_ESP)))
            return
        if i.mnemonic[:3] in ("add", "sub", "mul", "div") and i.mnemonic.endswith(("ss", "ps")):
            x, y = lanes(i, i.operands[0]), lanes(i, i.operands[1])
            want = {k: p | 0x400000 for k, (p, q) in enumerate(zip(x, y)) if is_nan(p) and is_nan(q)}
            if want:
                fixups.append((UC_XMM[int(i.reg_name(i.operands[0].reg)[3:])], want))
                fixed[0] += 1

    uc.hook_add(UC_HOOK_CODE, hook)
    try:
        uc.emu_start(addr, h.RET, count=100000)
    except UcError as e:
        raise Dropped(f"{e}")
    if bad:
        raise Dropped(bad[0])
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET or inside:
        raise Dropped("did not return")
    return {
        "addr": addr,
        "regs_in": regs, "xmm_in": xmm,
        "mem_in": [[OBJ_WINDOW[0], bytes(obj).hex()], [stack_window[0], bytes(stack).hex()],
                   *[[ga, gb.hex()] for ga, gb in globals_]],
        "regs_out": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)},
        "xmm_out": [read_xmm(r) for r in UC_XMM],
        "mem_out": [[w[0], bytes(uc.mem_read(w[0], w[1])).hex()] for w in (OBJ_WINDOW, stack_window, *[(ga, len(gb)) for ga, gb in globals_])],
        "nan_fixups": fixed[0],
        "calls": calls,
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
        regs["esp"] = ESP - 4 * rnd.randrange(4)
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
        elif kind == "yaw":
            regs["ecx"], regs["eax"] = VEC[0], VEC[1]
            for k in range(2):
                v = ([f2b(rnd.uniform(-100, 100)) for _ in range(4)] if rnd.random() < 0.5
                     else [floats(rnd) for _ in range(4)])
                struct.pack_into("<4I", obj, VEC[k] - OBJ_WINDOW[0], *v)
            if rnd.random() < 0.1:  # the same point twice
                obj[VEC[1] - OBJ_WINDOW[0]:VEC[1] - OBJ_WINDOW[0] + 16] = obj[VEC[0] - OBJ_WINDOW[0]:VEC[0] - OBJ_WINDOW[0] + 16]
        elif kind == "euler":
            pick = rnd.random()
            if pick < 0.5:
                words = [f2b(v) for row in rotation(rnd) for v in row]
            else:
                words = [floats(rnd) if rnd.random() < 0.3 else f2b(rnd.uniform(-1.5, 1.5)) for _ in range(16)]
            if 0.3 < pick < 0.6:  # at or beyond a pole
                words[PIVOTS[addr]] = rnd.choice([0x3F800000, 0xBF800000, 0x3F800001, 0xBF800001,
                                                  0x3F7FFFFF, 0xBF7FFFFF, 0x40000000, 0xC0000000])
            struct.pack_into("<16I", obj, MATRIX - OBJ_WINDOW[0], *words)
            regs["ecx"] = MATRIX
            args = [OBJ]
        elif kind in ("look_at", "reflect"):
            vecs = [[floats(rnd) for _ in range(4)] for _ in range(3)]
            pick = rnd.random()
            if kind == "look_at" and pick < 0.15:
                vecs[1][:3] = vecs[0][:3]  # at == eye: nothing to normalize
            elif kind == "look_at" and pick < 0.3:
                # up along the view direction: up x z is (nearly) zero
                eye = [rnd.uniform(-50, 50) for _ in range(3)]
                d = [rnd.choice([0.0, rnd.uniform(-5, 5)]) for _ in range(3)]
                s = rnd.choice([1.0, -3.0, 1e-9])
                vecs = [[f2b(v) for v in eye] + [0], [f2b(e + v) for e, v in zip(eye, d)] + [0],
                        [f2b(v * s) for v in d] + [0]]
            elif pick < 0.6:
                vecs = [[f2b(rnd.uniform(-100, 100)) for _ in range(4)] for _ in range(3)]
            for k, v in enumerate(vecs):
                struct.pack_into("<4I", obj, VEC[k] - OBJ_WINDOW[0], *v)
            args = VEC[:nargs]
        globals_ = ()
        if kind == "fov":
            args = [floats(rnd) if rnd.random() < 0.3 else f2b(rnd.uniform(-3, 3) * 10 ** rnd.uniform(-2, 3))
                    for _ in range(nargs)]
            if rnd.random() < 0.5:  # a plausible camera
                args[:4] = [f2b(rnd.uniform(0.01, 2)), f2b(rnd.uniform(10, 5000)), f2b(rnd.uniform(0.1, 3)),
                            f2b(rnd.uniform(0.5, 2.5))]
            ident = [0x3F800000 if i % 5 == 0 else 0 for i in range(16)]
            base = ident if rnd.random() < 0.5 else [rnd.getrandbits(32) for _ in range(16)]
            globals_ = [(BASE_MATRIX, struct.pack("<16I", *base))]
        if kind == "rotate":
            regs["esi"] = OBJ
            args = [floats(rnd) if rnd.random() < 0.4 else f2b(rnd.uniform(-20, 20)) for _ in range(2)]
            base = [0, 0] if rnd.random() < 0.4 else [floats(rnd), floats(rnd)]
            globals_ = [(ROTATE_BASE, struct.pack("<2I", *base))]
        try:
            cases.append(run(addr, regs, xmm, obj, args, globals_))
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
        print(f"0x{addr:08X} {kind}: {len(got)} cases, {sum(c['nan_fixups'] > 0 for c in got)} with two-NaN fixups")
        cases += got
    json.dump(cases, open(f"{OUT}/math_edge.json", "w"))
    print(f"math_edge.json: {len(cases)} cases")


if __name__ == "__main__":
    {"lift": lift, "edge": edge}[sys.argv[1]]()
