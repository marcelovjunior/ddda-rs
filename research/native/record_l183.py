"""L183 oracles: three game-side searches the main thread runs each frame,
run as the original's code in Unicorn on synthetic inputs.

- 0x0049E920 `nearest(owner, skip, id, position, radius, height, id2)`
  (stdcall, `ret 0x1c`): its objects' type getters (vtable +0x10) are
  either real `mov eax, imm32; ret` code, executed (the port reads it
  instead of calling), or recorded guest calls. Up to ~300 objects, so
  the sort's overrun past its 256 entries is covered too (cases whose
  overrun moves the return address are skipped: the original crashes).
- 0x004172C0 `scan(this)` (stdcall, `ret 4`): the unlock 0x005B4800
  (`ret 8`, its state in EAX) is a recorded guest call whose stub sets
  the id's bit, rewrites keyed-table words or the player's values.
- 0x00D1B840 `hits(box, segment, t_out)` (cdecl): integer, random,
  axis-aligned, nearly axis-aligned, degenerate and non-finite inputs.

Records every initial byte read, every final written byte, the registers
before and after, and at each guest call the registers, the memory written
so far and the callee's effects (crates/mtcore/tests/game_scan.rs compares
at the ABI level). Reads the user's own exe; never runs the game; the
output is private (E:/ddda-re/notes/native-L183).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h

BASE, SIZE = 0xE000000, 0x80000
STACK, STACK_SIZE, SP = 0xF000000, 0x10000, 0xF008000
CODE_GETTERS, STUBS = 0xD100000, 0xD200000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]

NEAREST, NEAREST_END = 0x49E920, 0x49EC39
SCAN, SCAN_END = 0x4172C0, 0x41746C
SEGMENT, SEGMENT_END = 0xD1B840, 0xD1BC52
RANGES = {NEAREST: NEAREST_END, SCAN: SCAN_END, SEGMENT: SEGMENT_END}
UNLOCK = 0x5B4800


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def any_float(rng):
    return rng.choice([0x7FC00000 | rng.getrandbits(22), 0xFFC00000, 0x7F800000, 0xFF800000, 0x80000000, 0,
                       rng.getrandbits(32), 0x00000001, 0x80000001])


class Oracle:
    def __init__(self, entry, image_pages, stubs=()):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        self.entry, self.end = entry, RANGES[entry]
        for page in sorted({entry & ~0xFFF, (self.end - 1) & ~0xFFF, *image_pages}):
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        for address, size in ((BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000), (CODE_GETTERS, 0x1000), (STUBS, 0x1000)):
            uc.mem_map(address, size)
        self.stubs = dict(stubs)
        self.pops = {NEAREST: 0x1c, SCAN: 4, SEGMENT: 0}[entry]
        for address in self.stubs:
            if not STUBS <= address < STUBS + 0x1000:
                uc.mem_map(address & ~0xFFF, 0x1000)
            uc.mem_write(address, b'\xc3')
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)
            self.written.update(range(address, address + size))

    def snapshot(self, memory=True, keep=None):
        uc = self.uc
        result = dict(regs=[uc.reg_read(r) for r in REGS])
        if memory:
            result['memory'] = [(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if keep is None or keep(a)]
        return result

    def effect(self, a, v, effects):
        b = struct.pack('<I', v & 0xFFFFFFFF)
        self.remember(a, 4)
        self.uc.mem_write(a, b)
        self.written.update(range(a, a + 4))
        effects += [(a + i, b[i]) for i in range(4)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address in self.stubs:
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            pops = self.stubs[address]
            self.remember(sp, 4 + pops)
            self.remember(address, 6)  # the port looks at a getter's code
            # at a call only what the test compares: memory outside the scratch
            # below the function's returned ESP, and the words pushed
            done = SP + 4 + self.pops
            keep = lambda a: not (done - 0x1000 <= a < done) or sp <= a < sp + 4 + pops
            self.events.append(dict(kind='call', target=address, pushed=4 + pops, **self.snapshot(keep=keep)))
            resume = struct.unpack('<I', uc.mem_read(sp, 4))[0]
            effects = []
            eax = self.callee(address, sp, effects)
            uc.reg_write(x.UC_X86_REG_EAX, eax)
            for reg in REGS[1:3]:
                uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 4 + pops)
            self.events.append(dict(kind='return', effects=effects, **self.snapshot(False)))
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            self.last = None
            return
        if CODE_GETTERS <= address < CODE_GETTERS + 0x1000:
            if uc.mem_read(address, 1)[0] == 0xB8:
                self.remember(address, 6)
            return
        if not self.entry <= address < self.end:
            self.escaped = address  # the original left its body (a moved return address)
            uc.emu_stop()
            return
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def start(self, rng, args, ecx=None):
        uc = self.uc
        for i, v in enumerate([h.RET] + args):
            uc.mem_write(SP + 4 * i, struct.pack('<I', v & 0xFFFFFFFF))
        regs = [rng.getrandbits(32), rng.getrandbits(32) if ecx is None else ecx, rng.getrandbits(32),
                rng.getrandbits(32), SP, rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        self.escaped = None
        before = self.snapshot(False)
        self.active = True
        try:
            uc.emu_start(self.entry, h.RET, count=2_000_000)
        except UcError:
            self.active = False
            return None
        self.active = False
        if self.escaped is not None or uc.reg_read(x.UC_X86_REG_EIP) != h.RET:
            return None
        return dict(entry=self.entry, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))

    def missing_edges(self, cases):
        edges = {tuple(e) for c in cases for e in c['edges']}
        insns = Cs(CS_ARCH_X86, CS_MODE_32).disasm(h.code(self.entry, self.end - self.entry), self.entry)
        return [(hex(i.address), hex(t)) for i in insns if i.mnemonic.startswith('j') and i.mnemonic != 'jmp'
                for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]


# --- 0x00D1B840 ---------------------------------------------------------------------------

class SegmentOracle(Oracle):
    def __init__(self):
        super().__init__(SEGMENT, (0x1406000, 0x1407000, 0x1408000, 0x161C000, 0x162A000))

    def run(self, seed):
        uc, rng = self.uc, random.Random(seed)
        self.rng = rng
        uc.mem_write(BASE, rng.randbytes(0x1000))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        box, seg, out = BASE + 0x100, BASE + 0x200, BASE + 0x300
        kind = seed % 7

        def put(a, values):
            uc.mem_write(a, b''.join(struct.pack('<I', v & 0xFFFFFFFF) for v in values))

        if kind == 0:  # integers: equalities and zero components happen exactly
            lo = [rng.randrange(-6, 6) for _ in range(4)]
            hi = [v + rng.randrange(-1, 6) for v in lo]
            p0 = [rng.randrange(-9, 10) for _ in range(4)]
            p1 = [v + rng.choice([0, 0, rng.randrange(-8, 9)]) for v in p0]
            values = [*lo, *hi], [*p0, *p1]
            put(box, [f32(float(v)) for v in values[0]])
            put(seg, [f32(float(v)) for v in values[1]])
        elif kind == 1:  # degenerate: a point (or nearly)
            put(box, [f32(rng.uniform(-5, 0)) for _ in range(4)] + [f32(rng.uniform(0, 5)) for _ in range(4)])
            p0 = [rng.uniform(-6, 6) for _ in range(4)]
            put(seg, [f32(v) for v in p0] + [f32(v + rng.choice([0.0, 1e-5, -2e-4, 5e-4])) for v in p0])
        elif kind == 2:  # nearly axis-aligned directions
            put(box, [f32(rng.uniform(-5, 0)) for _ in range(4)] + [f32(rng.uniform(0, 5)) for _ in range(4)])
            p0 = [rng.uniform(-8, 8) for _ in range(4)]
            d = [rng.choice([0.0, 1e-5, -3e-4, rng.uniform(-10, 10)]) for _ in range(3)]
            if all(abs(v) < 1e-3 for v in d):
                d[rng.randrange(3)] = rng.choice([4.0, -7.0])
            put(seg, [f32(v) for v in p0] + [f32(p0[k] + d[k]) for k in range(3)] + [f32(p0[3])])
        elif kind == 3:  # non-finite and odd values
            vals = [any_float(rng) if rng.randrange(4) == 0 else f32(rng.uniform(-8, 8)) for _ in range(16)]
            put(box, vals[:8])
            put(seg, vals[8:])
        elif kind == 6 and seed % 2:  # a squared length of exactly epsilon (2^-23)
            p0 = [float(rng.randrange(-4, 4)) for _ in range(4)]
            e = 2.0 ** -12
            put(box, [f32(v) for v in (-5, -5, -5, 0, 5, 5, 5, 0)])
            put(seg, [f32(v) for v in p0] + [f32(p0[0] + e), f32(p0[1] + rng.choice([e, -e])), f32(p0[2]), f32(p0[3])])
        elif kind == 6:  # the x slab's entry +0 and exit -0, along x only
            y, z = float(rng.randrange(-4, 5)), float(rng.randrange(-4, 5))
            put(box, [0, f32(-5.0), f32(-5.0), 0, 0x80000000, f32(5.0), f32(5.0), 0])
            put(seg, [0, f32(y), f32(z), 0, f32(rng.choice([3.0, 7.0])), f32(y), f32(z), 0])
        elif kind == 5 and seed % 2:  # the x slab's empty interval: t1 NaN, t2 -inf
            put(box, [0x7FC00000, f32(-5.0), f32(-5.0), 0, 0xFF800000, f32(5.0), f32(5.0), 0])
            put(seg, [f32(rng.uniform(-3, 3)) for _ in range(4)] + [f32(rng.uniform(4, 9)), f32(rng.uniform(-3, 3)),
                                                                    f32(rng.uniform(-3, 3)), 0])
        else:  # random boxes and segments
            lo = [rng.uniform(-10, 5) for _ in range(4)]
            put(box, [f32(v) for v in lo] + [f32(v + rng.uniform(0, 10)) for v in lo])
            put(seg, [f32(rng.uniform(-20, 20)) for _ in range(8)])
        args = [box, seg, rng.choice([0, out, out])]
        case = self.start(rng, args)
        if case is not None:
            case.update(seed=seed)
        return case


# --- 0x004172C0 ---------------------------------------------------------------------------

ROWS, KEYED, LEVELS, COUNTS, PLAYERS = 0x153B6E8, 0x15231E0, 0x153B9C8, 0x153BF68, 0x18FA4EC
MANAGER, THIS, DATA, LISTS, BITSETS = (BASE + v for v in (0x100, 0x20000, 0x21000, 0x30000, 0x40000))
PLAYER = [BASE + 0x4000 * (p + 1) for p in range(3)]


class ScanOracle(Oracle):
    def __init__(self):
        super().__init__(SCAN, (0x14EA000, 0x1523000, 0x153B000, 0x153C000, 0x18FA000), stubs={UNLOCK: 8})

    def callee(self, address, sp, effects):
        rng, uc = self.rng, self.uc
        state = uc.reg_read(x.UC_X86_REG_EAX)
        unlock_id = struct.unpack('<I', uc.mem_read(sp + 4, 4))[0]
        r = rng.randrange(6)
        if r < 3 and unlock_id < 0x2840:  # sets the id's bit: the scan sees it next time
            bits = struct.unpack('<I', uc.mem_read(state + 8, 4))[0]
            a = bits + (unlock_id >> 5) * 4
            self.effect(a, struct.unpack('<I', uc.mem_read(a, 4))[0] | 1 << (unlock_id & 31), effects)
        elif r == 3:  # rewrites a keyed entry's key (the port must search again)
            self.effect(KEYED + 0x14 * rng.randrange(97), rng.choice(self.keys + [rng.getrandbits(32)]), effects)
        elif r == 4:  # rewrites an id in a list
            self.effect(LISTS + 0x20 * rng.randrange(97) + 4 * rng.randrange(5), rng.randrange(0x2900), effects)
        return rng.getrandbits(32)

    def run(self, seed):
        uc, rng = self.uc, random.Random(seed)
        self.rng = rng
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        self.keys = keys = rng.sample(range(1, 1 << 20), 12)
        word(PLAYERS, MANAGER)
        for p in range(3):
            player = PLAYER[p] if rng.randrange(6) else 0
            word(MANAGER + 0x9A0 + 4 * p, player)
            if not player:
                continue
            uc.mem_write(player + 0x206D, bytes([rng.getrandbits(8)]))
            # (the original reads [holder + 0x6C] unchecked: players always have one)
            holder = player + 0x3000
            word(player + 0x2E64, holder)
            if holder:
                state = holder + 0x100 if rng.randrange(8) else 0
                word(holder + 0x6C, state)
                if state:
                    uc.mem_write(state + 0x10, bytes([rng.choice([0, 0, 0, 1, 0x80])]))
                    word(state + 8, BITSETS + 0x800 * p)
                    density = rng.choice([0, 0.2, 0.8])
                    for k in range(0x2840 // 32):
                        word(BITSETS + 0x800 * p + 4 * k, sum(1 << b for b in range(32) if rng.random() < density))
            word(THIS + 0x20 + 4 * p, DATA + 0x400 * p if rng.randrange(6) else 0)
            for r in range(8):
                word(DATA + 0x400 * p + 4 * r, f32(rng.uniform(0, 120)) if rng.randrange(12) else any_float(rng))
                word(DATA + 0x400 * p + 0x120 + 4 * r, rng.choice([0, 1, 3, 10, rng.getrandbits(32)]))
        for i in range(0x5C):
            word(ROWS + 8 * i, rng.choice(keys + [rng.getrandbits(32)]))
            word(ROWS + 8 * i + 4, rng.randrange(8))
        for j in range(97):
            word(KEYED + 0x14 * j, rng.choice(keys[:8] + [rng.getrandbits(32)] * 4))
            word(KEYED + 0x14 * j + 4, LISTS + 0x20 * j)
            for k in range(5):
                v = rng.choice([0, rng.randrange(0x2840), rng.randrange(0x2840), rng.randrange(0x2840), 0x2840 + rng.randrange(64)])
                word(LISTS + 0x20 * j + 4 * k, v if rng.randrange(10) else 0)
        for k in range(40):
            word(LEVELS + 4 * k, f32(rng.choice([0.0, 0.5, 1.0, 2.0, 3.5, 4.0])) if rng.randrange(20) else any_float(rng))
            word(COUNTS + 4 * k, rng.choice([0, 0, 1, 2, 5]))
        case = self.start(rng, [THIS])
        case.update(seed=seed)
        return case


# --- 0x0049E920 ---------------------------------------------------------------------------

TYPE_GLOBALS = (0x19A5E30, 0x19A60B0, 0x19A60D0)
OWNER, LIST, OBJECTS, VTABLES, DTIS, POSITION = (BASE + v for v in (0x100, 0x1000, 0x4000, 0x60000, 0x61000, 0x200))


class NearestOracle(Oracle):
    def __init__(self):
        super().__init__(NEAREST, (0x14F9000, 0x161C000, 0x19A5000, 0x19A6000),
                         stubs={STUBS + 0x10 * k: 0 for k in range(8)})

    def callee(self, address, sp, effects):
        return self.stub_dti[address]

    def run(self, seed):
        uc, rng = self.uc, random.Random(seed)
        self.rng = rng
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        names = [rng.getrandbits(32) for _ in range(6)]
        for g, n in zip(TYPE_GLOBALS, names):
            word(g, n)
        # descriptors: a name, a parent chain
        dtis = []
        for k in range(12):
            d = DTIS + 0x20 * k
            word(d + 4, rng.choice(names))
            word(d + 0x10, rng.choice(dtis) if dtis and rng.randrange(3) else 0)
            dtis.append(d)
        # vtables: getters as code (read by the port) or as stubs (called)
        self.stub_dti = {}
        vtables = []
        for k in range(8):
            v = VTABLES + 0x20 * k
            d = rng.choice(dtis)
            if rng.randrange(2):
                getter = CODE_GETTERS + 0x10 * k
                uc.mem_write(getter, b'\xb8' + struct.pack('<I', d) + b'\xc3')
            else:
                getter = STUBS + 0x10 * k
                self.stub_dti[getter] = d
            word(v + 0x10, getter)
            vtables.append(v)
        big = seed % 10 == 0
        count = rng.randrange(257, 300) if big else rng.choice([0, 1, 2, 3, 5, 8, 16, 40, 80])
        # exact cases: integer positions, distances 5 and 10 (3-4-5 offsets)
        # against radii whose double is 5 or 10, and ties between objects
        exact = seed % 5 == 1 and not big
        px, py, pz = ((float(rng.randrange(-50, 50)) for _ in range(3)) if exact
                      else (rng.uniform(-50, 50) for _ in range(3)))
        for k, v in enumerate((px, py, pz)):
            word(POSITION + 4 * k, f32(v))
        radius = rng.choice([f32(rng.uniform(1, 30)), f32(rng.uniform(1, 30)), 0, f32(-3.0), any_float(rng)])
        height = rng.choice([f32(rng.uniform(1, 30)), f32(rng.uniform(1, 30)), any_float(rng)])
        if exact:
            radius, height = f32(rng.choice([2.5, 5.0])), f32(rng.choice([1.0, 2.0, 30.0]))
        ident, ident2 = rng.getrandbits(8), rng.choice([0, 0, rng.getrandbits(8)])
        if big:  # every object a candidate: the sort overruns its 256 entries
            radius, height = f32(1000.0), f32(1000.0)
        objects = []
        type_a = [d for d in dtis if self.is_a(uc, d, names[0])]
        typed = [v for v in vtables if self.is_a(uc, self.getter_dti(uc, v), names[0])]
        for k in range(count):
            o = OBJECTS + 0x80 * k
            objects.append(o)
            if big and type_a:
                vt = VTABLES + 0x20 * 7
                word(vt + 0x10, CODE_GETTERS + 0x70)
                uc.mem_write(CODE_GETTERS + 0x70, b'\xb8' + struct.pack('<I', type_a[0]) + b'\xc3')
                word(o, vt)
            else:
                word(o, rng.choice(typed) if typed and rng.randrange(3) else rng.choice(vtables))
            word(o + 4, rng.choice([2 | 0x400, 2 | 0x400 | rng.getrandbits(32) & ~7, rng.getrandbits(32)]) if not big
                 else 2 | 0x400)
            word(o + 0x2574, ident if rng.randrange(4) or big else rng.getrandbits(8))
            word(o + 0x2534, ident2 if rng.randrange(4) or big else rng.getrandbits(8))
            if exact:
                dx, dz = rng.choice([(3, 4), (4, 3), (0, 5), (5, 0), (-3, -4), (6, 8), (-8, 6), (0, 10), (0, 0), (1, 1)])
                for j, v in enumerate((px + dx, py + rng.choice([0, 1, -2, 2]), pz + dz)):
                    word(o + 0x40 + 4 * j, f32(float(v)))
                continue
            spread = rng.choice([1, 5, 30])
            for j, base in enumerate((px, py, pz)):
                word(o + 0x40 + 4 * j, f32(base + rng.uniform(-spread, spread)) if rng.randrange(30) else any_float(rng))
        # the getters' code changes between runs: drop Unicorn's translations of it
        uc.ctl_remove_cache(CODE_GETTERS, CODE_GETTERS + 0x1000)
        word(OWNER + 0x24, count)
        word(OWNER + 0x30, LIST)
        for k, o in enumerate(objects):
            word(LIST + 4 * k, o)
        skip = rng.choice(objects) if objects and rng.randrange(3) == 0 else rng.getrandbits(32)
        args = [OWNER, skip, ident, POSITION, radius, height, ident2]
        case = self.start(rng, args)
        if case is None:
            return None
        case.update(seed=seed, count=count)
        return case

    def getter_dti(self, uc, v):
        getter = struct.unpack('<I', uc.mem_read(v + 0x10, 4))[0]
        if getter in self.stub_dti:
            return self.stub_dti[getter]
        return struct.unpack('<I', uc.mem_read(getter + 1, 4))[0]

    @staticmethod
    def is_a(uc, d, name):
        while d:
            if struct.unpack('<I', uc.mem_read(d + 4, 4))[0] == name:
                return True
            d = struct.unpack('<I', uc.mem_read(d + 0x10, 4))[0]
        return False


def write(out, cases):
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))


def record(oracle, n, out, expected_missing=()):
    cases, skipped = [], 0
    for seed in range(1, n + 1):
        c = oracle.run(seed)
        if c is None:
            skipped += 1
        else:
            cases.append(c)
    missing = oracle.missing_edges(cases)
    assert sorted(missing) == sorted(expected_missing), (hex(oracle.entry), missing)
    write(out, cases)
    calls = sum(len(c['events']) for c in cases) // 2
    print(f'{oracle.entry:#x}: {len(cases)} cases ({skipped} skipped), {calls} guest calls -> {out}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dir', default='E:/ddda-re/notes/native-L183')
    p.add_argument('--only', choices=('segment', 'scan', 'nearest'))
    p.add_argument('-n', type=int, default=1500)
    a = p.parse_args()
    d = Path(a.dir)
    if a.only in (None, 'segment'):
        record(SegmentOracle(), a.n, d / 'segment.json')
    if a.only in (None, 'scan'):
        # never branch: the player counter (always <= 2), the slot counter
        # (always <= 4), the second null test of a player already known
        # non-null; and a player without a state holder, which the original
        # dereferences unchecked further on (players always have one)
        record(ScanOracle(), a.n, d / 'scan.json',
               [('0x4172fa', '0x417438'), ('0x417349', '0x417438'), ('0x417383', '0x417430'), ('0x417326', '0x417328')])
    if a.only in (None, 'nearest'):
        record(NearestOracle(), a.n, d / 'nearest.json')


if __name__ == '__main__':
    main()
