"""Vectors for the readable MT core-library ports (L33, crates/mtcore).
Private output.

    PY research/native/vectors_core.py lift   # -> vectors/core_lift.json
    PY research/native/vectors_core.py edge   # -> vectors/core_edge.json

`lift` copies the lifted program's own vectors (vectors/lift.json) for every
ported address.

`edge` runs the original in Unicorn on built object graphs: descriptor trees
with names and hash chains, arrays with null and non-null elements,
property lists and node pools, the allocator registry, strings of every
short length. Every structure lives in a scratch area filled with random
garbage; globals the code reads (allocator table, DTI buckets, MtArray's
DTI, the node free list, the registry, the import slots) are poked with
chosen values and recorded as memory windows. The case records all
general registers, all xmm lanes, the scratch, stack, global and heap
windows on entry and exit.

Callees:
- virtual functions and imports point at tiny stubs in scratch (`ret 4`,
  `mov eax, X; ret 8`); each call is recorded with the machine at the
  stub's entry and at its return, and the Rust test replays it;
- the platform models (node pool, memset, memcpy) run as in
  harness.run_sparse, allocating from mem::HEAP_BASE as the Rust models do;
- ported callees run as the original (the replay runs their ports).

MXCSR 0x1F80, x87 control 0x037F. Reads the user's own exe at run time;
nothing from it is committed.
"""
import json
import os
import random
import struct
import sys

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

SCRATCH, SCRATCH_LEN = h.SCRATCH, 0x2000
STUBS, STUBS_LEN = 0x0D000000, 0x1000  # stub code: a page of its own, never written by the code under test
ESP = h.STACK + h.STACK_SIZE - 0x1000
BELOW, ABOVE = 0x200, 0x40
HEAP_BASE, HEAP_LEN = 0x0C000000, 0x1000  # mem::HEAP_BASE, the models' bump allocator

ALLOCATORS = 0x01876628   # 64 allocator pointers
BUCKETS = 0x018768A0      # 256 DTI hash buckets
ARRAY_DTI = 0x01877D24
FREE_LIST = 0x01877DA0
REGISTRY, REGISTRY_LEN = 0x01876730, 0x01876834
IAT_ENTER, IAT_LEAVE = 0x0139D160, 0x0139D164
NEW_ALLOCATOR = 0x0187662C

PORTS = {
    0x00CF5E20: "alloc_of", 0x00CF55B0: "find_id", 0x00D01B60: "crc", 0x004038F0: "obj_dtor",
    0x00CF5C00: "new", 0x00CF7340: "plist_ctor", 0x004027A0: "plist_add", 0x00CF74E0: "plist_clear",
    0x00CF76C0: "plist_dtor", 0x004025C0: "arr_clear", 0x00402620: "arr_reserve", 0x00402530: "arr_dtor",
    0x00CF5450: "reg_add", 0x00CF54A0: "reg_remove", 0x00CF5E40: "set_alloc", 0x00CF5750: "propagate",
    0x00CF55E0: "find_child", 0x00CF5870: "find_name",
}
MODELS = {0x00CF73F0: "pool40_alloc", 0x012EFE20: "memset", 0x012EFEA0: "memcpy"}


class Dropped(Exception):
    pass


class Scratch:
    """The scratch window: random garbage, a bump allocator for structures,
    and stubs at STUBS."""

    def __init__(self, rnd):
        self.rnd = rnd
        self.b = bytearray(rnd.randbytes(SCRATCH_LEN))
        self.top = SCRATCH + 0x40
        self.stub_top = STUBS
        self.code = bytearray([0xCC] * STUBS_LEN)

    def alloc(self, n, align=4):
        self.top = (self.top + align - 1) & ~(align - 1)
        a = self.top
        self.top += n
        assert self.top < SCRATCH + SCRATCH_LEN, "scratch full"
        return a

    def u32(self, a, v):
        struct.pack_into("<I", self.b, a - SCRATCH, v & 0xFFFFFFFF)

    def u8(self, a, v):
        self.b[a - SCRATCH] = v & 0xFF

    def bytes(self, a, data):
        self.b[a - SCRATCH:a - SCRATCH + len(data)] = data

    def cstr(self, s):
        a = self.alloc(len(s) + 1, 1)
        self.bytes(a, s + b"\0")
        return a

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

    def vtable_object(self, slots):
        """An object whose vtable holds `slots` (slot -> stub address)."""
        vt = self.alloc(4 * (max(slots) + 1))
        for k, a in slots.items():
            self.u32(vt + 4 * k, a)
        obj = self.alloc(0x60)
        self.u32(obj, vt)
        return obj


def run(addr, regs, xmm, scratch, args, globals_):
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
    uc.mem_write(STUBS, bytes(scratch.code))
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
    uc.reg_write(xc.UC_X86_REG_FPCW, 0x037F)
    uc.reg_write(xc.UC_X86_REG_FPTAG, 0xFFFF)
    rnd = random.Random(regs["eax"] ^ addr)
    stack_window = (regs["esp"] - BELOW, BELOW + ABOVE)
    stack = bytearray(rnd.randbytes(stack_window[1]))
    frame = struct.pack("<I", h.RET) + b"".join(struct.pack("<I", a & 0xFFFFFFFF) for a in args)
    stack[BELOW:BELOW + len(frame)] = frame
    heap = bytes(rnd.randbytes(HEAP_LEN))
    uc.mem_write(stack_window[0], bytes(stack))
    uc.mem_write(SCRATCH, bytes(scratch.b))
    uc.mem_write(HEAP_BASE, heap)
    for ga, gb in globals_:
        uc.mem_write(ga, gb)
    for name, r in zip(GPR, UC_GPR):
        uc.reg_write(r, regs[name])
    for k, r in enumerate(UC_XMM):
        uc.reg_write(r, sum(v << (32 * i) for i, v in enumerate(xmm[k])))
    read_xmm = lambda r: [(uc.reg_read(r) >> (32 * k)) & 0xFFFFFFFF for k in range(4)]

    def machine():
        return {"regs": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)}, "xmm": [read_xmm(r) for r in UC_XMM]}

    calls, inside, bad, heap_top = [], [], [], [HEAP_BASE]

    def alloc(n):
        a = heap_top[0]
        heap_top[0] = (a + max(n, 1) + 15) & ~15
        if heap_top[0] > HEAP_BASE + HEAP_LEN:
            bad.append("heap full")
        return a

    def hook(uc_, a, size, _):
        esp = uc_.reg_read(xc.UC_X86_REG_ESP)
        if inside:
            back, entry_esp = inside[-1]
            if a == back and esp > entry_esp:
                inside.pop()
                calls[-1]["out"] = machine()
                calls[-1]["out"]["stack"] = bytes(uc_.mem_read(*stack_window)).hex()
            return
        if STUBS <= a < STUBS + STUBS_LEN:
            back = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            calls.append({"target": a, "in": machine()})
            inside.append((back, esp))
            return
        if a in MODELS:
            name = MODELS[a]
            arg = lambda k: struct.unpack("<I", uc_.mem_read(esp + 4 + 4 * k, 4))[0]  # noqa: E731
            eax = uc_.reg_read(xc.UC_X86_REG_EAX)
            if name == "pool40_alloc":
                eax = alloc(0x28)
            else:
                dst, x, n = arg(0), arg(1), arg(2)
                if n > 0x1000:
                    bad.append(f"{name} of {n:#x}")
                    uc_.emu_stop()
                    return
                uc_.mem_write(dst, bytes([x & 0xFF]) * n if name == "memset" else bytes(uc_.mem_read(x, n)))
                eax = dst
            ret = struct.unpack("<I", uc_.mem_read(esp, 4))[0]
            uc_.reg_write(xc.UC_X86_REG_EAX, eax)
            uc_.reg_write(xc.UC_X86_REG_ESP, esp + 4)
            uc_.reg_write(xc.UC_X86_REG_EIP, ret)

    uc.hook_add(UC_HOOK_CODE, hook)
    try:
        uc.emu_start(addr, h.RET, count=200000)
    except UcError as e:
        raise Dropped(str(e))
    if bad:
        raise Dropped(bad[0])
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET or inside:
        raise Dropped("did not return")
    windows = [(SCRATCH, SCRATCH_LEN), stack_window, (HEAP_BASE, HEAP_LEN), *[(ga, len(gb)) for ga, gb in globals_]]
    ins = [bytes(scratch.b), bytes(stack), heap, *[gb for _, gb in globals_]]
    return {
        "addr": addr,
        "regs_in": regs, "xmm_in": xmm,
        "mem_in": [[w[0], b.hex()] for w, b in zip(windows, ins)],
        "regs_out": {n: uc.reg_read(r) for n, r in zip(GPR, UC_GPR)},
        "xmm_out": [read_xmm(r) for r in UC_XMM],
        "mem_out": [[w[0], bytes(uc.mem_read(w[0], w[1])).hex()] for w in windows],
        "calls": calls,
        "heap_used": heap_top[0] - HEAP_BASE,
    }


# ---------------------------------------------------------------- builders

def name_bytes(rnd):
    pool = [b"uPlayer", b"uPlay", b"uPlayerBase", b"cAction", b"cActionBase", b"MtObject", b"a", b"",
            b"uEm0100", b"uEm010", b"zz", b"\xff\x80", b"ab", b"abc"]
    return rnd.choice(pool) if rnd.random() < 0.8 else bytes(rnd.randrange(1, 256) for _ in range(rnd.randrange(0, 9)))


def dti_tree(sc, rnd, depth=3, names=True):
    """A random descriptor tree; returns (root, all nodes)."""
    nodes = []

    def make(parent, d):
        n = sc.alloc(0x20)
        nodes.append(n)
        sc.u32(n + 0x04, sc.cstr(name_bytes(rnd)) if names and rnd.random() < 0.95 else 0)
        sc.u32(n + 0x10, parent)
        sc.u32(n + 0x18, rnd.getrandbits(32))
        sc.u32(n + 0x08, 0)
        sc.u32(n + 0x0C, 0)
        if d > 0:
            prev = 0
            for _ in range(rnd.choice([0, 0, 1, 2, 3])):
                c = make(n, d - 1)
                if prev:
                    sc.u32(prev + 0x08, c)
                else:
                    sc.u32(n + 0x0C, c)
                prev = c
        return n

    return make(0, depth), nodes


def allocator_globals(sc, rnd, alloc_result=None):
    """Allocator objects with stubbed alloc (+0x20) and free (+0x24), the
    allocator table, and MtArray's DTI selecting one of them."""
    table = [rnd.getrandbits(32) for _ in range(64)]
    for i in range(64):
        if rnd.random() < 0.25 or i in (0, 1):
            result = alloc_result if alloc_result is not None else sc.alloc(0x40, 16)
            table[i] = sc.vtable_object({8: sc.value_stub(result, 8), 9: sc.ret_stub(4)})
    dti = bytearray(rnd.randbytes(0x20))
    return table, dti


def build(kind, rnd):
    """(regs, scratch, args, globals) for one case of `kind`."""
    sc = Scratch(rnd)
    regs = {r: rnd.getrandbits(32) for r in GPR}
    regs["esp"] = ESP - 4 * rnd.randrange(4)
    args, g = [], []
    obj = sc.alloc(0x60, 16)
    regs["ecx"] = obj
    if kind == "alloc_of":
        d = sc.alloc(0x20)
        args = [d]
        g = [(ALLOCATORS, struct.pack("<64I", *[rnd.getrandbits(32) for _ in range(64)]))]
    elif kind == "find_id":
        buckets = [0] * 256
        ids = []
        for _ in range(rnd.randrange(0, 12)):
            d = sc.alloc(0x20)
            i = rnd.getrandbits(31) if rnd.random() < 0.6 else rnd.choice([0x05, 0x105, 0x205, 0x7FFFFF05])
            ids.append(i)
            sc.u32(d + 0x1C, i)
            sc.u32(d + 0x14, buckets[i & 0xFF])
            buckets[i & 0xFF] = d
        q = rnd.choice(ids) if ids and rnd.random() < 0.6 else rnd.choice([rnd.getrandbits(32), 0x305, 0])
        args = [q]
        g = [(BUCKETS, struct.pack("<256I", *buckets))]
    elif kind == "crc":
        s = bytes(rnd.randrange(1, 256) for _ in range(rnd.choice([0, 0, 1, 2, 3, 5, 8, 17, 40])))
        args = [sc.cstr(s), rnd.choice([0xFFFFFFFF, 0, rnd.getrandbits(32)])]
    elif kind == "new":
        a = sc.vtable_object({8: sc.value_stub(sc.alloc(0x40, 16), 8)})
        args = [rnd.choice([0, 1, 0x10, 0x5A0, rnd.getrandbits(32)])]
        g = [(NEW_ALLOCATOR, struct.pack("<I", a))]
    elif kind in ("plist_add",):
        head = 0 if rnd.random() < 0.3 else sc.alloc(0x28)
        sc.u32(obj + 4, head)
        prop = sc.alloc(0x28)
        args = [prop]
    elif kind in ("plist_clear", "plist_dtor"):
        nodes = [sc.alloc(0x28) for _ in range(rnd.choice([0, 1, 2, 5]))]
        for k, n in enumerate(nodes):
            sc.u32(n + 0x24, nodes[k + 1] if k + 1 < len(nodes) else 0)
        sc.u32(obj + 4, nodes[0] if nodes else 0)
        free = 0 if rnd.random() < 0.4 else sc.alloc(0x28)
        g = [(FREE_LIST, struct.pack("<I", free)),
             (IAT_ENTER, struct.pack("<II", sc.ret_stub(4), sc.ret_stub(4)))]
    elif kind in ("arr_clear", "arr_reserve", "arr_dtor"):
        table, dti = allocator_globals(sc, rnd)
        index = rnd.choice([0, 1] + [i for i in range(64) if table[i] and rnd.random() < 0.1] or [0])
        struct.pack_into("<I", dti, 0x18, (index << 23) | rnd.getrandbits(23) | (rnd.getrandbits(3) << 29))
        g = [(ALLOCATORS, struct.pack("<64I", *table)), (ARRAY_DTI, bytes(dti))]
        count = rnd.choice([0, 1, 2, 3, 6])
        cap = count + rnd.choice([0, 1, 4])
        data = 0 if (count == 0 and rnd.random() < 0.4) else sc.alloc(4 * max(cap, 1))
        dstub = sc.ret_stub(4)
        for k in range(count):
            e = 0 if rnd.random() < 0.3 else sc.vtable_object({0: dstub})
            sc.u32(data + 4 * k, e)
        sc.u32(obj + 4, count)
        sc.u32(obj + 8, cap)
        sc.u8(obj + 0xC, rnd.choice([0, 1, 0x80]))
        sc.u32(obj + 0x10, data)
        if kind == "arr_clear":
            args = [rnd.choice([0, 1, 0x100, 0x101, rnd.getrandbits(32)])]
        elif kind == "arr_reserve":
            args = [count + rnd.choice([0, 1, 7])]
    elif kind in ("reg_add", "reg_remove"):
        length = rnd.choice([0, 1, 2, 5, 9])
        table = [rnd.getrandbits(32) for _ in range(65)]
        a = sc.alloc(0x60)
        for i in range(1, length):
            table[i] = 0 if rnd.random() < 0.3 else (a if kind == "reg_remove" and rnd.random() < 0.3 else sc.alloc(0x60))
        args = [a]
        g = [(REGISTRY, struct.pack("<65I", *table)), (REGISTRY_LEN, struct.pack("<I", length))]
    elif kind in ("set_alloc", "propagate"):
        root, nodes = dti_tree(sc, rnd, names=False)
        regs["ecx"] = root
        if kind == "set_alloc":
            args = [root, rnd.choice([0, 1, 5, 63, 64, rnd.getrandbits(32)])]
    elif kind in ("find_child", "find_name"):
        root, nodes = dti_tree(sc, rnd)
        regs["ecx"] = root
        pick = rnd.random()
        if pick < 0.5 and nodes:
            n = rnd.choice(nodes)
            nm = struct.unpack_from("<I", sc.b, n + 4 - SCRATCH)[0]
            name = sc.cstr(bytes(sc.b[nm - SCRATCH:sc.b.index(0, nm - SCRATCH)])) if nm else sc.cstr(b"x")
        else:
            name = sc.cstr(name_bytes(rnd))
        args = [name] if kind == "find_child" else [0 if rnd.random() < 0.1 else name, root]
    return regs, sc, args, g


def edge_cases(addr, kind, n, seed):
    rnd = random.Random(seed)
    cases, tries = [], 0
    while len(cases) < n and tries < n * 20:
        tries += 1
        regs, sc, args, g = build(kind, rnd)
        xmm = [[rnd.getrandbits(32) for _ in range(4)] for _ in range(8)]
        try:
            cases.append(run(addr, regs, xmm, sc, args, g))
        except Dropped:
            continue
        except AssertionError:
            continue
    return cases


def lift():
    want = set(PORTS)
    cases = json.load(open(f"{OUT}/lift.json"))
    sub = [c for c in cases if c["addr"] in want]
    missing = want - {c["addr"] for c in sub}
    if missing:
        raise SystemExit(f"no lifted vectors for {sorted(hex(a) for a in missing)}")
    json.dump(sub, open(f"{OUT}/core_lift.json", "w"))
    print(f"core_lift.json: {len(sub)} cases for {len(want)} functions")


def edge():
    cases = []
    for addr, kind in sorted(PORTS.items()):
        print(f"0x{addr:08X} {kind} ...", flush=True)
        got = edge_cases(addr, kind, 300, addr)
        print(f"0x{addr:08X} {kind}: {len(got)} cases, {sum(len(c['calls']) > 0 for c in got)} with callouts")
        cases += got
    json.dump(cases, open(f"{OUT}/core_edge.json", "w"))
    print(f"core_edge.json: {len(cases)} cases")
    # the exe's CRC table, which hash::TABLE must equal
    json.dump({"crc_table": h.code(0x014075C0, 0x400).hex()}, open(f"{OUT}/core_tables.json", "w"))


if __name__ == "__main__":
    {"lift": lift, "edge": edge}[sys.argv[1]]()
