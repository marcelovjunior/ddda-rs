"""L187 oracle: the effect system's update routines run as the original's code
in Unicorn:

  update  0x00E0BED0 effect->update(dt)            (thiscall, `ret 4`, dt a float)
  draw    0x00E75EA0 effect->draw_children(context) (thiscall, `ret 4`; uEffect::vf11)

Every guest callee is a stub recorded as an event (target, registers and
argument window before; registers and effects after), the virtual methods
included (their vtable slots point at stubs 0x30001000 + slot). Two are
transparent so the x87 hand-off is the real one: 0x00E6BC60 (machine code
`fld dword [value]; ret 4`) and the CRT `_ftol` 0x012F05C0 (its original
code runs, FISTP and all); their events are completed at the return
address with the registers the real code left, and the draw event for _ftol
carries the float ST0 holds when it is called.
Output: the format of crates/mtcore/tests/semantic_s6.rs (+ "st0").
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private."""
import json
import math
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

UPDATE, DRAW = 0x00E0BED0, 0x00E75EA0
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
STACK, SP = 0x0F000000, 0x0F008000
THIS, VTABLE, CMD, TIMELINE, PARENT, GLOBAL_B, OBJECTS = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000
VALUE = 0x27000000  # the float e6bc60's machine code loads
# fixed-address stubs: address -> (name, stack argument words, bytes popped)
FIXED = {
    0x01077240: ('timeline_step', 1, 4), 0x010784B0: ('timeline_apply', 2, 8),
    0x00FDF6A0: ('reject', 1, 4),
    # 0x00E75B20 is a stub only when the global at 0x018D1D20 has queued transforms
    # (otherwise its code runs: it answers false); the other small callees of draw
    # run as the original's code (the port does them in place)
    0x00E75B20: ('transform_ok', 3, 12),
}
CONDITIONAL = 0x00E75B20
# virtual methods: slot -> (name, argument words)
VIRTUAL = {0x9C: ('current_command', 0), 0xA0: ('h0', 1), 0xA4: ('h1', 1), 0xAC: ('h2', 1), 0xBC: ('h3', 1), 0xC0: ('h4', 1),
           0xC4: ('h5', 1), 0x70: ('other', 1), 0x50: ('v50', 0), 0x54: ('v54', 0), 0x60: ('hidden', 0), 0x20: ('alloc', 2),
           0x24: ('free', 1), 0x5000: ('child', 3)}
ALLOC, ALLOC_VT, GOBJ, RECORD = 0x28000000, 0x28001000, 0x29000000, 0x2A000000


def slot_stub(slot):
    return 0x30001000 + slot


def f32(v):
    try:
        return struct.unpack('<f', struct.pack('<f', v))[0]
    except OverflowError:
        return math.copysign(math.inf, v)


def bits(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


class Oracle:
    def __init__(self, entry):
        self.entry = entry
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for va, data in h.image()[1]:
            lo, hi = va & ~0xFFF, (va + len(data) + 0xFFF) & ~0xFFF
            uc.mem_map(lo, hi - lo)
            uc.mem_write(va, data)
        uc.mem_map(0, 0x1000)  # a missing next command is read at address 4 (the original does)
        for page in (0x30000000, 0x30001000, 0x30002000, 0x30003000, 0x30004000, 0x30005000, 0x30006000):
            uc.mem_map(page, 0x1000)
        self.stubs = {}
        for address, (name, args, pop) in FIXED.items():
            self.stubs[address] = (name, args, pop)
            if address != CONDITIONAL:
                uc.mem_write(address, b'\xc3')
        for slot, (name, args) in VIRTUAL.items():
            pop = 4 * args
            self.stubs[slot_stub(slot)] = (name, args, pop)
            uc.mem_write(slot_stub(slot), b'\xc3')
        for base, size in ((THIS, 0x1000), (VTABLE, 0x1000), (CMD, 0x1000), (TIMELINE, 0x1000), (PARENT, 0x1000), (GLOBAL_B, 0x1000),
                           (OBJECTS, 0x10000), (VALUE, 0x1000), (ALLOC, 0x2000), (GOBJ, 0x1000), (RECORD, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        self.pending = {}
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            a = address + i
            if a not in self.written:
                self.initial.setdefault(a, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def word(self, a):
        return struct.unpack('<I', self.uc.mem_read(a, 4))[0]

    def put(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address in self.pending:
            event = self.pending.pop(address)
            event['after'] = dict(regs=self.regs(), memory=[])
            return
        if address not in self.stubs:
            return
        if address == CONDITIONAL:
            self.remember(0x018D1D20, 4)
            self.remember(self.word(0x018D1D20) + 0x1C, 4)
        if address == CONDITIONAL and self.word(self.word(0x018D1D20) + 0x1C) == 0:
            return  # the code answers false
        name, args, pop = self.stubs[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + args)))])
        back = self.word(esp)
        event = dict(target=address, before=before)
        rng = self.rng
        effects = []
        eax = rng.randrange(1 << 32)
        if name in ('hidden', 'reject', 'transform_ok', 'h0', 'h1', 'h2', 'h3', 'h4', 'h5'):
            eax = (rng.choice((0, 1, 1, 1, 0x100, 0x2, 0xFF01)) if name.startswith('h') and name != 'hidden'
                   else rng.choice((0, 1, 1, 0x100, 0x7F00, 0xAB01)))
        if name == 'current_command':
            eax = self.script.pop(0) if self.script else 0
        elif name == 'alloc':
            eax = rng.choice((0, OBJECTS + 0x6000, OBJECTS + 0x6100))
        elif name == 'transform_ok':
            local = self.word(esp + 12)
            if rng.random() < 0.7:
                size = rng.choice((16, 16, 16, 32, 0))
                effects += self.put(local + 8, struct.pack('<III', RECORD, RECORD + size, RECORD + 32))
                effects += self.put(RECORD, struct.pack('<IIII', rng.choice((0xFFFFFFFF, 0xFFFFFFFF, 5)), *[rng.randrange(1 << 32) for _ in range(3)]))
        for _ in range(rng.randrange(0, 3)):
            if name in ('child', 'free', 'timeline_step', 'timeline_apply', 'v50', 'v54'):
                effects += self.put(OBJECTS + rng.randrange(0, 0x100), bytes([rng.randrange(256)]))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(event, after=dict(regs=self.regs(), memory=effects)))

    # -------------------------------------------------------------------
    def setup_update(self, rng):
        uc = self.uc
        flags = rng.choice((1, 1, 1, 1, 0, 0x101, 0x401, 0x801, 0xC01, 0x405, 0x1, 0x5)) | rng.choice((0, 0, 0x100, 0x1000, 0x10000))
        uc.mem_write(THIS + 0x164, struct.pack('<I', flags))
        for off in (0x12C, 0x130, 0x134, 0x138):
            v = rng.choice((0.0, 1.0, -1.0, rng.uniform(-10, 10), 1e30, math.nan, math.inf))
            uc.mem_write(THIS + off, struct.pack('<f', v))
        uc.mem_write(THIS + 0x100, struct.pack('<I', rng.choice((0, TIMELINE, TIMELINE))))
        uc.mem_write(THIS + 0xFC, struct.pack('<I', rng.choice((0, PARENT))))
        uc.mem_write(PARENT + 0x70, struct.pack('<ff', rng.uniform(-5, 5), rng.uniform(-5, 5)))
        uc.mem_write(GLOBAL_B, b'\0' * 0x1000)
        uc.mem_write(0x018D08F4, struct.pack('<I', GLOBAL_B))
        uc.mem_write(GLOBAL_B + 0xB8, struct.pack('<ff', rng.uniform(-5, 5), rng.uniform(-5, 5)))
        count = rng.choice((0, 0, 1, 2, 4))
        uc.mem_write(THIS + 0x10C, struct.pack('<I', count))
        uc.mem_write(THIS + 0x118, struct.pack('<I', OBJECTS + 0x1000))
        for k in range(4):
            obj = rng.choice((0, OBJECTS + 0x2000 + 0x40 * k))
            uc.mem_write(OBJECTS + 0x1000 + 4 * k, struct.pack('<I', obj))
            uc.mem_write(OBJECTS + 0x2000 + 0x40 * k + 8, struct.pack('<I', rng.randrange(1 << 32)))
        # the command queue
        for k in range(8):
            uc.mem_write(CMD + 16 * k, struct.pack('<II', rng.choice((0, 1, 2, 5, 7, 0xFFFFFFFF)), rng.choice((0, 1, 2, 3, 4, 4, 5, 6, 7, 2))))
        self.script = [0 if rng.random() < 0.12 else CMD + 16 * rng.randrange(8) for _ in range(rng.randrange(0, 9))]

    def setup_draw(self, rng):
        uc = self.uc
        uc.mem_write(THIS + 0xF4, struct.pack('<I', rng.choice((0, OBJECTS, OBJECTS, OBJECTS))))
        uc.mem_write(OBJECTS + 0x58, struct.pack('<II', rng.randrange(1 << 32), rng.randrange(1 << 32)))
        uc.mem_write(THIS + 0xF0, struct.pack('<I', rng.choice((0, 0, 0, 0x80000, 0x7FFFF))))
        uc.mem_write(THIS + 0x210, bytes([rng.choice((0, 1, 1))]))
        ctx = OBJECTS + 0x4000
        uc.mem_write(ctx + 0x168, bytes([rng.randrange(256)]))
        uc.mem_write(ctx + 0x16A, bytes([rng.choice((0xFF, 0x01, 0x80, 0x0F, 0))]))
        uc.mem_write(ctx + 0x58, struct.pack('<I', rng.randrange(1 << 32)))
        # children
        n = rng.choice((0, 1, 2, 3, 5))
        head = 0
        for k in reversed(range(n)):
            at = OBJECTS + 0x5000 + 0x100 * k
            uc.mem_write(at + 8, struct.pack('<I', head))
            uc.mem_write(at + 0xC, struct.pack('<I', rng.choice((2, 2, 0, 3, 0xFFFFFFFD, 1))))
            uc.mem_write(at + 0x30, struct.pack('<I', rng.choice((0, OBJECTS + 0x8000 + 0x40 * k))))
            uc.mem_write(at + 0x40, bytes([rng.choice((0xFF, 0x01, 0x02, 0x80, 0))]))
            uc.mem_write(at + 0x90, struct.pack('<I', rng.randrange(1 << 32)))
            uc.mem_write(at, struct.pack('<I', VTABLE))
            head = at
        uc.mem_write(THIS + 0x1F0, struct.pack('<I', head))
        self.ctx = ctx
        # the effect's eight floats (the frame-scale source), the owner of the constants block
        for k in range(8):
            v = rng.choice((0.0, 1.0, 0.5, -1.0, rng.uniform(0, 4), rng.uniform(-2, 2), 1e9, 3e9, math.nan, math.inf, 1e-6))
            uc.mem_write(THIS + 0x180 + 4 * k, struct.pack('<f', v))
        uc.mem_write(0x018D2818, struct.pack('<I', rng.choice((0, OBJECTS + 0x7000))))
        uc.mem_write(OBJECTS + 0x7140, struct.pack('<II', rng.choice((0, 0x30000000)), rng.choice((0, 3, 0x10))))
        uc.mem_write(THIS + 0x1C0, struct.pack('<I', rng.choice((0, 5, 0x11, 0x4321))))
        # the occluder (when the effect has one) and its stamps
        occluder = OBJECTS + 0x9000
        uc.mem_write(THIS + 0x1D0, struct.pack('<I', rng.choice((0, occluder, occluder, occluder))))
        stamp = rng.randrange(1 << 32)
        uc.mem_write(THIS + 0x1D4, struct.pack('<I', stamp))
        uc.mem_write(occluder + 4, struct.pack('<I', rng.choice((1, 2, 2, 3, 0x4802, 0x4000, 0x0800, 0x4A02, 0xFFFF_FFFA, 0x4801))))
        uc.mem_write(occluder + 0x1D4, struct.pack('<I', stamp if rng.random() < 0.8 else stamp ^ 1))
        uc.mem_write(occluder + 0x1E0, struct.pack('<H', rng.choice((0, 1, 0x100, 0xFFFF))))
        # the shared block, its allocator and the queued-transform global
        uc.mem_write(0x018D188C, struct.pack('<I', rng.choice((0, OBJECTS + 0xA000, OBJECTS + 0xA000))))
        uc.mem_write(0x0187662C, struct.pack('<I', ALLOC))
        uc.mem_write(ALLOC, struct.pack('<I', ALLOC_VT))
        uc.mem_write(ALLOC_VT + 0x20, struct.pack('<II', slot_stub(0x20), slot_stub(0x24)))
        uc.mem_write(0x019AA078, struct.pack('<I', 1))  # the CRT's SSE2 flag, set at start-up
        uc.mem_write(0x018D1D20, struct.pack('<I', GOBJ))
        uc.mem_write(GOBJ + 0x1C, struct.pack('<I', rng.choice((0, 0, 0, 1, 7))))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x1000), (VTABLE, 0x1000), (CMD, 0x1000), (TIMELINE, 0x1000), (PARENT, 0x1000), (OBJECTS, 0x10000), (VALUE, 0x1000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(THIS, struct.pack('<I', VTABLE))
        for slot in VIRTUAL:
            if slot != 0x5000:
                uc.mem_write(VTABLE + slot, struct.pack('<I', slot_stub(slot)))
        if self.entry == DRAW:
            # the children's vtable is the same table; their +0x50 is the draw method
            uc.mem_write(VTABLE + 0x50, struct.pack('<I', slot_stub(0x5000)))
        self.script = []
        self.st0_bits = 0
        self.pending = {}
        if self.entry == UPDATE:
            self.setup_update(rng)
            arg = rng.choice((0.0, 0.016, 0.033, -0.5, 1.0, math.nan, math.inf, rng.uniform(0, 1)))
            argbits = bits(f32(arg))
        else:
            self.setup_draw(rng)
            argbits = self.ctx
        # the x87 stack is empty at entry
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        self.initial, self.written, self.events = {}, set(), []
        entry = SP - 8 - 4 * rng.randrange(4)
        uc.mem_write(entry, struct.pack('<II', h.RET, argbits))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, entry
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(entry, 8)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(self.entry, h.RET, count=400000)
        except Exception:
            print('seed', seed, 'eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()])
            raise
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        assert esp == entry + 8, (hex(esp), hex(entry))
        assert not self.pending, self.pending
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if not (STACK <= a < esp)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    name, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    entry = {'update': UPDATE, 'draw': DRAW}[name]
    oracle = Oracle(entry)
    covered = set()
    oracle.uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: covered.add(a))
    cases = [oracle.case(seed) for seed in range(n)]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    if len(sys.argv) > 4:
        starts = [int(l.split(':')[0], 16) for l in open(sys.argv[4]) if l[:8].strip() and l[8:9] == ':']
        print('never run:', [hex(a) for a in starts if a not in covered])
    json.dump(dict(function=hex(entry), cases=cases), open(out, 'w'))
    print(f'{n} cases, {sum(len(c["events"]) for c in cases)} calls -> {out}', by)


main()
