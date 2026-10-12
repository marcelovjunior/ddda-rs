"""Vectors for the readable frame-system ports (L39, crates/mtframe).
Private output.

    PY research/native/vectors_frame.py lift   # -> vectors/frame_lift.json
    PY research/native/vectors_frame.py edge   # -> vectors/frame_edge.json

`lift` copies the lifted program's own vectors (vectors/lift.json) for every
ported address.

`edge` runs the original in Unicorn on built objects. Every object lives in
a memory window filled with random garbage; the fields the code reads are
set to chosen values (special floats, NaNs, flags, counts, pointers to other
built objects). Globals the code reads (the sMain and sEffect pointers, ...)
are poked and recorded as windows too. The case records all general
registers, all xmm lanes and every window on entry and exit.

Callees:
- virtual calls go to tiny stubs on a page of their own (`ret N`,
  `mov eax, X; ret N`, `fld dword [X]; ret N`);
- direct calls to other code of the exe are faked at the callee's entry:
  the return value is chosen, ecx, edx and all xmm lanes (volatile in the
  MSVC x86 convention) become random, the arguments are popped;
- the platform models (memset, memcpy, the node pool) run as in
  harness.run_sparse; ported callees run as the original (the replay runs
  their ports).
Each stub and fake call is recorded with the machine at the callee's entry
and at its return (and the float it leaves in st0); the Rust test replays
it.

Unicorn applies the x87 NaN rule where SSE hardware returns the first
operand's NaN, quieted (Intel SDM vol. 1, 4.8.3.5): as in vectors_math.py,
an SSE add/sub/mul/div that meets two NaNs gets the hardware's lanes before
the next instruction ("nan_fixups"). MXCSR 0x1F80; the x87 control word is
0x037F unless a case chooses another precision. Reads the user's own exe
at run time; nothing from it is committed.
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

SCRATCH, SCRATCH_LEN = h.SCRATCH, 0x4000  # the main window; big objects get windows of their own
STUBS, STUBS_LEN = 0x0D000000, 0x1000  # stub code: a page of its own, never written by the code under test
ESP = h.STACK + h.STACK_SIZE - 0x1000
BELOW, ABOVE = 0x400, 0x40
HEAP_BASE, HEAP_LEN = 0x0C000000, 0x1000  # mem::HEAP_BASE, the models' bump allocator

S_MAIN = 0x018D0B28    # sMain*
S_EFFECT = 0x018D2818  # sEffect*

# address: (kind, size of the function in bytes)
PORTS = {
    0x00FB64D0: ("effect_update", 110),
    0x011E48E0: ("element_time", 58),
    0x00E6C260: ("emitter_time", 75),
    0x00A36DF0: ("lunge", 346),
}
# ports of other crates run as the original when called (their ports run in the replay)
FOREIGN_PORTS = {0x00402530, 0x004025C0, 0x00402620, 0x00CF5E20}
MODELS = {0x00CF73F0: "pool40_alloc", 0x012EFE20: "memset", 0x012EFEA0: "memcpy"}

NANS = [0x7FC00000, 0xFFC00000, 0x7FC12345, 0x7F800001, 0xFFA00007]
SPECIAL = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x00000001, 0x80400000, 0x007FFFFF,
           0x7F7FFFFF, 0xFF7FFFFF, 0x7F800000, 0xFF800000, 0x3DCCCCCD, 0x447A0000, 0x40000000,
           0x3F000000, 0x41F00000, 0x42700000, 0x3E800000]


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def is_nan(b):
    return (b >> 23) & 0xFF == 0xFF and b & 0x7FFFFF


def floats(rnd, nan=True):
    """A float bit pattern: special values, NaNs, or a random finite float."""
    k = rnd.random()
    if k < 0.35:
        return rnd.choice(SPECIAL)
    if k < 0.42 and nan:
        return rnd.choice(NANS)
    if k < 0.8:
        return f2b(rnd.uniform(-10.0, 10.0))
    return f2b(rnd.choice([-1, 1]) * 10 ** rnd.uniform(-40, 38))


class Dropped(Exception):
    pass


class Scratch:
    """Memory windows filled with random garbage (the main one at SCRATCH,
    with a bump allocator for small objects), and stubs at STUBS."""

    def __init__(self, rnd):
        self.rnd = rnd
        self.windows = {SCRATCH: bytearray(rnd.randbytes(SCRATCH_LEN))}
        self.top = SCRATCH + 0x40
        self.stub_top = STUBS
        self.code = bytearray([0xCC] * STUBS_LEN)
        self.st_returns = {}  # stub address -> the float bits it leaves in st0
        self.fakes = {}       # exe address -> (argument bytes popped, eax or None)

    def window(self, base, length):
        """An extra window (inside the SCRATCH mapping, above the main one)."""
        assert SCRATCH + SCRATCH_LEN <= base and base + length <= h.SCRATCH + h.SCRATCH_SIZE
        self.windows[base] = bytearray(self.rnd.randbytes(length))
        return base

    def alloc(self, n, align=4):
        self.top = (self.top + align - 1) & ~(align - 1)
        a = self.top
        self.top += n
        assert self.top < SCRATCH + SCRATCH_LEN, "scratch full"
        return a

    def _at(self, a, n):
        for base, b in self.windows.items():
            if base <= a and a + n <= base + len(b):
                return b, a - base
        raise AssertionError(f"{a:#x} is in no window")

    def u32(self, a, v):
        b, o = self._at(a, 4)
        struct.pack_into("<I", b, o, v & 0xFFFFFFFF)

    def u64(self, a, v):
        b, o = self._at(a, 8)
        struct.pack_into("<Q", b, o, v & 0xFFFFFFFFFFFFFFFF)

    def f64(self, a, v):
        b, o = self._at(a, 8)
        struct.pack_into("<d", b, o, v)

    def u8(self, a, v):
        b, o = self._at(a, 1)
        b[o] = v & 0xFF

    def get32(self, a):
        b, o = self._at(a, 4)
        return struct.unpack_from("<I", b, o)[0]

    def stub(self, code):
        a = self.stub_top
        self.code[a - STUBS:a - STUBS + len(code)] = code
        self.stub_top += 16
        assert self.stub_top <= STUBS + STUBS_LEN, "stubs full"
        return a

    def ret_stub(self, pop):
        return self.stub(bytes([0xC2, pop & 0xFF, pop >> 8]))

    def value_stub(self, value, pop):
        return self.stub(bytes([0xB8]) + struct.pack("<I", value) + bytes([0xC2, pop & 0xFF, pop >> 8]))

    def float_stub(self, bits, pop):
        """`fld dword [cell]; ret pop`: returns `bits` in st0."""
        cell = self.alloc(4)
        self.u32(cell, bits)
        a = self.stub(bytes([0xD9, 0x05]) + struct.pack("<I", cell) + bytes([0xC2, pop & 0xFF, pop >> 8]))
        self.st_returns[a] = bits
        return a

    def vtable_object(self, slots, size=0x60):
        """An object whose vtable holds `slots` (slot -> stub address)."""
        vt = self.alloc(4 * (max(slots) + 1))
        for k, a in slots.items():
            self.u32(vt + 4 * k, a)
        obj = self.alloc(size)
        self.u32(obj, vt)
        return obj

    def fake(self, addr, pop, eax=None):
        """Fake the exe function at `addr` (see the module docs)."""
        self.fakes[addr] = (pop, eax)


def run(addr, size, regs, xmm, sc, args, globals_, fpcw=0x037F):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(h.TCG_BUFFER)
    base, secs = h.image()
    for va, b in secs:
        start = va & ~0xFFF
        uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    uc.mem_map(h.STACK, h.STACK_SIZE)
    uc.mem_map(h.SCRATCH, h.SCRATCH_SIZE)
    uc.mem_map(HEAP_BASE, HEAP_LEN)
    uc.mem_map(STUBS, STUBS_LEN)
    uc.mem_write(STUBS, bytes(sc.code))
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
    uc.reg_write(xc.UC_X86_REG_FPCW, fpcw)
    uc.reg_write(xc.UC_X86_REG_FPTAG, 0xFFFF)
    rnd = random.Random(regs["eax"] ^ addr)
    stack_window = (regs["esp"] - BELOW, BELOW + ABOVE)
    stack = bytearray(rnd.randbytes(stack_window[1]))
    frame = struct.pack("<I", h.RET) + b"".join(struct.pack("<I", a & 0xFFFFFFFF) for a in args)
    stack[BELOW:BELOW + len(frame)] = frame
    heap = bytes(rnd.randbytes(HEAP_LEN))
    uc.mem_write(stack_window[0], bytes(stack))
    for wa, wb in sc.windows.items():
        uc.mem_write(wa, bytes(wb))
    uc.mem_write(HEAP_BASE, heap)
    for ga, gb in globals_:
        uc.mem_write(ga, gb)
    for name, r in zip(GPR, UC_GPR):
        uc.reg_write(r, regs[name])
    for k, r in enumerate(UC_XMM):
        uc.reg_write(r, sum(v << (32 * i) for i, v in enumerate(xmm[k])))
    read_xmm = lambda r: [(uc.reg_read(r) >> (32 * k)) & 0xFFFFFFFF for k in range(4)]  # noqa: E731
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    decoded = {}

    def machine():
        return {"regs": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)}, "xmm": [read_xmm(r) for r in UC_XMM]}

    calls, inside, ported, bad, heap_top = [], [], [], [], [HEAP_BASE]
    fixups, fixed = [], [0]

    def alloc(n):
        a = heap_top[0]
        heap_top[0] = (a + max(n, 1) + 15) & ~15
        if heap_top[0] > HEAP_BASE + HEAP_LEN:
            bad.append("heap full")
        return a

    def lanes(i, op):
        if op.type == 1:
            v = uc.reg_read(UC_XMM[int(i.reg_name(op.reg)[3:])])
            return [(v >> (32 * k)) & 0xFFFFFFFF for k in range(4)]
        ea = (uc.reg_read(UC_GPR[GPR.index(i.reg_name(op.mem.base))]) if op.mem.base else 0) + op.mem.disp
        if op.mem.index:
            ea += uc.reg_read(UC_GPR[GPR.index(i.reg_name(op.mem.index))]) * op.mem.scale
        n = 4 if i.mnemonic.endswith("ps") else 1
        return list(struct.unpack(f"<{n}I", bytes(uc.mem_read(ea & 0xFFFFFFFF, 4 * n))))

    def finish_call(uc_, st=None):
        calls[-1]["out"] = machine()
        calls[-1]["out"]["stack"] = bytes(uc_.mem_read(*stack_window)).hex()
        if st is not None:
            calls[-1]["out"]["st_push"] = st

    def hook(uc_, a, size_, _):
        while fixups:
            reg, want = fixups.pop()
            v = uc_.reg_read(reg)
            for lane, bits in want.items():
                v = (v & ~(0xFFFFFFFF << (32 * lane))) | (bits << (32 * lane))
            uc_.reg_write(reg, v)
        esp = uc_.reg_read(xc.UC_X86_REG_ESP)
        if inside:
            back, entry_esp, stub = inside[-1]
            if a == back and esp > entry_esp:
                inside.pop()
                finish_call(uc_, sc.st_returns.get(stub))
            return
        if STUBS <= a < STUBS + STUBS_LEN:
            back = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            calls.append({"target": a, "in": machine()})
            inside.append((back, esp, a))
            return
        if a in sc.fakes:
            pop, eax = sc.fakes[a]
            back = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            calls.append({"target": a, "in": machine()})
            uc_.reg_write(xc.UC_X86_REG_EAX, rnd.getrandbits(32) if eax is None else eax)
            uc_.reg_write(xc.UC_X86_REG_ECX, rnd.getrandbits(32))
            uc_.reg_write(xc.UC_X86_REG_EDX, rnd.getrandbits(32))
            for r in UC_XMM:
                uc_.reg_write(r, rnd.getrandbits(128))
            uc_.reg_write(xc.UC_X86_REG_ESP, esp + 4 + pop)
            uc_.reg_write(xc.UC_X86_REG_EIP, back)
            finish_call(uc_)
            return
        if a in MODELS:
            name = MODELS[a]
            arg = lambda k: struct.unpack("<I", uc_.mem_read(esp + 4 + 4 * k, 4))[0]  # noqa: E731
            eax = uc_.reg_read(xc.UC_X86_REG_EAX)
            if name == "pool40_alloc":
                eax = alloc(0x28)
            else:
                dst, x, n = arg(0), arg(1), arg(2)
                if n > 0x2000:
                    bad.append(f"{name} of {n:#x}")
                    uc_.emu_stop()
                    return
                uc_.mem_write(dst, bytes([x & 0xFF]) * n if name == "memset" else bytes(uc_.mem_read(x, n)))
                eax = dst
            ret = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            uc_.reg_write(xc.UC_X86_REG_EAX, eax)
            uc_.reg_write(xc.UC_X86_REG_ESP, esp + 4)
            uc_.reg_write(xc.UC_X86_REG_EIP, ret)
            return
        if ported:  # inside another crate's port, run as the original
            back, entry_esp = ported[-1]
            if not (a == back and esp > entry_esp):
                return
            ported.pop()
        if a in FOREIGN_PORTS:
            back = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            ported.append((back, esp))
            return
        if not (addr <= a < addr + size):
            bad.append(f"ran into {a:#x}")
            uc_.emu_stop()
            return
        i = decoded.get(a)
        if i is None:
            i = decoded[a] = next(md.disasm(bytes(uc_.mem_read(a, size_)), a))
        if i.mnemonic[:3] in ("add", "sub", "mul", "div") and i.mnemonic.endswith(("ss", "ps")):
            x, y = lanes(i, i.operands[0]), lanes(i, i.operands[1])
            want = {k: p | 0x400000 for k, (p, q) in enumerate(zip(x, y)) if is_nan(p) and is_nan(q)}
            if want:
                fixups.append((UC_XMM[int(i.reg_name(i.operands[0].reg)[3:])], want))
                fixed[0] += 1

    uc.hook_add(UC_HOOK_CODE, hook)
    try:
        uc.emu_start(addr, h.RET, count=400000)
    except UcError as e:
        raise Dropped(str(e))
    if bad:
        raise Dropped(bad[0])
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET or inside:
        raise Dropped("did not return")
    windows = [*[(wa, len(wb)) for wa, wb in sc.windows.items()], stack_window, (HEAP_BASE, HEAP_LEN),
               *[(ga, len(gb)) for ga, gb in globals_]]
    ins = [*[bytes(wb) for wb in sc.windows.values()], bytes(stack), heap, *[gb for _, gb in globals_]]
    return {
        "addr": addr,
        "fpcw": fpcw,
        "regs_in": regs, "xmm_in": xmm,
        "mem_in": [[w[0], b.hex()] for w, b in zip(windows, ins)],
        "regs_out": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)},
        "xmm_out": [read_xmm(r) for r in UC_XMM],
        "mem_out": [[w[0], bytes(uc.mem_read(w[0], w[1])).hex()] for w in windows],
        "fpcw_out": uc.reg_read(xc.UC_X86_REG_FPCW),
        "fptag_out": uc.reg_read(xc.UC_X86_REG_FPTAG),
        "calls": calls,
        "nan_fixups": fixed[0],
    }


# ---------------------------------------------------------------- builders

def s_main(sc, rnd):
    """An sMain with chosen pause and step fields (+0x00..+0xA0)."""
    obj = sc.alloc(0xA0, 16)
    sc.u32(obj + 0x68, rnd.choice([0, 0, 1, 2, rnd.getrandbits(32)]))
    sc.u8(obj + 0x6C, rnd.choice([0, 0, 1, 0x80]))
    sc.u32(obj + 0x70, floats(rnd))
    sc.u32(obj + 0x38, rnd.choice([0x41F00000, 0x42700000, floats(rnd)]))
    return obj


def far_object(sc, rnd, base, ranges):
    """An object at `base` (above the main window) of which only `ranges`
    ((offset, length) pairs) are mapped as windows."""
    for off, n in ranges:
        sc.window(base + off, n)
    return base


FAR = SCRATCH + 0x10000  # big objects (their used ranges are windows)


def build(kind, rnd):
    """(regs, scratch, args, globals) for one case of `kind`."""
    sc = Scratch(rnd)
    regs = {r: rnd.getrandbits(32) for r in GPR}
    regs["esp"] = ESP - 4 * rnd.randrange(4)
    args, g = [], []
    main = s_main(sc, rnd)
    effects = sc.alloc(0x180, 16)
    sc.u32(effects + 0x74, rnd.choice([0, 1, 0xFFFFFFFF, rnd.getrandbits(32)]))
    g += [(S_MAIN, struct.pack("<I", main)), (S_EFFECT, struct.pack("<I", effects))]
    if kind == "effect_update":
        regs["ecx"] = effects
        sc.u8(effects + 0x150, rnd.choice([0, 1, 1, 0x80]))
        sc.u8(effects + 0x151, rnd.choice([0, 1]))
        sc.u32(effects + 0x154, floats(rnd))
        sc.fake(0x011D4F30, 8, eax=rnd.choice([sc.alloc(0x10), rnd.getrandbits(32)]))
        sc.fake(0x00FB5E90, 4)
    elif kind in ("element_time", "emitter_time"):
        param = sc.alloc(8)
        sc.u8(param + 3, rnd.getrandbits(8))
        unit = sc.alloc(0x200, 16)
        if kind == "element_time":
            owner = sc.alloc(0x100)
            sc.u32(unit, owner)
            args = [param]
        else:
            sc.u32(unit + 0x1D0, 0 if rnd.random() < 0.4 else sc.alloc(0x110))
            args = [param, sc.alloc(0x90)]
        regs["ecx"] = unit
    elif kind == "lunge":
        action = sc.alloc(0x80)
        regs["ecx"] = action
        matrix = sc.alloc(0x40, 16)
        for k in range(16):
            sc.u32(matrix + 4 * k, floats(rnd))
        if rnd.random() < 0.15:
            sc.u32(matrix + 0x20, rnd.choice([0, 0x80000000]))
            sc.u32(matrix + 0x28, rnd.choice([0, 0x80000000]))
        sc.u32(action + 0x44, sc.vtable_object({0x58 // 4: sc.value_stub(matrix, 4)}))
        enemy = far_object(sc, rnd, FAR, [(0x40, 0x10), (0x380 + 0x4C, 4), (0xF0C, 4), (0x2360, 0x18),
                                         (0x74D0, 0x14), (0x7590, 8)])
        sc.u32(action + 0x74, 0 if rnd.random() < 0.1 else enemy)
        frame = rnd.choice([0x41EFFFFF, 0x41F00000, 0x41F00001, 0x42340000, 0x426FFFFF, 0x42700000,
                            0x42700001, 0x40000000, 0x7FC00000, floats(rnd)])
        sc.u32(enemy + 0x380 + 0x4C, frame if rnd.random() < 0.9 else f2b(rnd.uniform(30, 60)))
        for off in (0x40, 0x44, 0x48, 0x74E0, 0x7590, 0x7594):
            sc.u32(enemy + off, floats(rnd))
        sc.fake(0x00A00BF0, 4)
        sc.fake(0x010570F0, 0, eax=rnd.choice([0, 1, 0x100, 0xFFFFFF00, rnd.getrandbits(32)]))
    return regs, sc, args, g


def edge_cases(addr, kind, size, n, seed):
    rnd = random.Random(seed)
    cases, tries, why = [], 0, {}
    while len(cases) < n and tries < n * 20:
        tries += 1
        regs, sc, args, g = build(kind, rnd)
        xmm = [[rnd.getrandbits(32) for _ in range(4)] for _ in range(8)]
        try:
            cases.append(run(addr, size, regs, xmm, sc, args, g))
        except Dropped as e:
            why[str(e)[:40]] = why.get(str(e)[:40], 0) + 1
            continue
    if why:
        print(f"  dropped: {why}")
    return cases


def lift():
    want = set(PORTS)
    cases = json.load(open(f"{OUT}/lift.json"))
    sub = [c for c in cases if c["addr"] in want]
    missing = want - {c["addr"] for c in sub}
    if missing:
        raise SystemExit(f"no lifted vectors for {sorted(hex(a) for a in missing)}")
    json.dump(sub, open(f"{OUT}/frame_lift.json", "w"))
    print(f"frame_lift.json: {len(sub)} cases for {len(want)} functions")


def edge():
    only = {int(a, 16) for a in sys.argv[2:]}
    path = f"{OUT}/frame_edge.json"
    old = json.load(open(path)) if only and os.path.exists(path) else []
    cases = [c for c in old if c["addr"] not in only]
    for addr, (kind, size) in sorted(PORTS.items()):
        if only and addr not in only:
            continue
        print(f"0x{addr:08X} {kind} ...", flush=True)
        got = edge_cases(addr, kind, size, 400, addr)
        print(f"0x{addr:08X} {kind}: {len(got)} cases, {sum(len(c['calls']) > 0 for c in got)} with callouts, "
              f"{sum(c['nan_fixups'] > 0 for c in got)} with two-NaN fixups")
        cases += got
    json.dump(cases, open(path, "w"))
    print(f"frame_edge.json: {len(cases)} cases")


if __name__ == "__main__":
    {"lift": lift, "edge": edge}[sys.argv[1]]()
