"""L184 oracle: 0x00B66B30 `player->post_update()` (vf97) run as the
original's code in Unicorn on synthetic player objects. Every guest callee
is a stub (recorded as an event in the format of crates/mtcore/tests/
semantic.rs `abi_cases_full`: before = regs + argument window, after =
effects + regs); the input source's float getter (vtable +0x3C) returns its
value in ST0 (event key "st0" holds the f32 bits). The anchor probe
0x007B0580 is cdecl and writes three floats at EDI. Tables and constants
are the exe's own bytes (mapped from the unpacked exe).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
Usage: record_player_post_update.py out.json [cases]
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00B66B30, 0xC79  # through RET at 0x00B677A8
B, VB = 0x20000000, 0x26000000
P, A, BB = 0x21000000, 0x22000000, 0x23000000
V, VV = 0x24000000, 0x25000000
H, VH = 0x27000000, 0x28000000
D = 0x29000000
T, RECS = 0x2A000000, 0x2B000000
VEC = 0x2C000000
G = 0x2D000000
STUB = 0x30000000
CELL = 0x30003000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]

# target -> (stack args, caller pops args)
DIRECT = {0x0084C800: 0, 0x00B66320: 1, 0x00B661F0: 0, 0x00B71B90: 1, 0x00B72360: 0, 0x00B72D30: 0, 0x008508C0: 0,
          0x01054860: 1, 0x0078A730: 0, 0x0078B4C0: 0, 0x00850440: 0, 0x00421ED0: 1, 0x00B85F70: 0, 0x007B0580: 3,
          0x00E4B700: 1, 0x0081F2D0: 0, 0x0081F8F0: 1, 0x0081FC60: 0, 0x0081FD10: 2, 0x00837980: 2, 0x00B67930: 0,
          0x00B836A0: 0}
CDECL = {0x007B0580}
B_SLOTS = {0x210: 1, 0x50: 0, 0x2E0: 0, 0x70: 1}
STUBS = dict(DIRECT)
for slot, n in B_SLOTS.items():
    STUBS[STUB + slot] = n
ST0_STUB = STUB + 0x1000 + 0x3C
STUBS[ST0_STUB] = 1
STUBS[STUB + 0x2000 + 0x54] = 0

EXE_PAGES = [0x00B66000, 0x00B67000, 0x01531000, 0x014EA000, 0x014F7000, 0x0161C000, 0x0150A000, 0x01521000,
             0x01520000, 0x018FA000]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for p in EXE_PAGES:
            uc.mem_map(p, 0x1000)
            uc.mem_write(p, h.code(p, 0x1000) if p != 0x018FA000 else b'\0' * 0x1000)
        stub_pages = {a & ~0xFFF for a in STUBS}
        for p in sorted(stub_pages - set(EXE_PAGES)):
            uc.mem_map(p, 0x1000)
        for base, size in ((B, 0x6000), (VB, 0x1000), (P, 0x1000), (A, 0x1000), (BB, 0x1000), (V, 0x1000), (VV, 0x1000),
                           (H, 0x1000), (VH, 0x1000), (D, 0x1000), (T, 0x1000), (RECS, 0x1000), (VEC, 0x1000),
                           (G, 0xC0000), (CELL & ~0xFFF, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
        uc.mem_write(ST0_STUB, b'\xd9\x05' + struct.pack('<I', CELL) + b'\xc2\x04\x00')
        for slot in B_SLOTS:
            uc.mem_write(VB + slot, struct.pack('<I', STUB + slot))
        uc.mem_write(VV + 0x3C, struct.pack('<I', ST0_STUB))
        uc.mem_write(VH + 0x54, struct.pack('<I', STUB + 0x2000 + 0x54))
        uc.mem_write(0x018FA4BC, struct.pack('<I', G))
        self.consts = {a: struct.unpack('<f', bytes(uc.mem_read(a, 4)))[0] for a in
                       (0x015319B8, 0x014EAE5C, 0x014F7270, 0x0150A088, 0x014EA574, 0x01521690, 0x01520BAC, 0x01531AE8)}
        self.hit = set()
        self.counts = {}
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    # --- random values -------------------------------------------------
    def fbits(self, near=None):
        """A random f32 bit pattern (quiet NaNs only): zero, signs, constants and
        their neighbours, and ordinary values."""
        rng = self.rng
        choices = [0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 100.0, -100.0, rng.uniform(-5, 5), rng.uniform(-5, 5),
                   rng.uniform(-0.1, 0.1), float('inf'), float('-inf'), 1e-40]
        for v in self.consts.values():
            choices += [v, -v, v + 0.01, v - 0.01, v * 2]
        if near is not None:
            choices += [near, near + 0.001, near - 0.001, near + 1, near - 1, -near]
        v = rng.choice(choices)
        bits = struct.unpack('<I', struct.pack('<f', v))[0]
        if rng.random() < 0.03:
            bits = 0x7FC00000
        if rng.random() < 0.02:
            bits = rng.randrange(1 << 32)
        if bits & 0x7F800000 == 0x7F800000 and bits & 0x7FFFFF:
            bits |= 0x400000
            if rng.random() < 0.5:
                bits = 0x7FC00000
        return bits

    def state(self):
        rng = self.rng
        special = [2, 5, 0x65, 0x66, 0x7A, 0x7F, 0x80, 0x82, 0x83, 0xE6, 0xE7, 0xE8, 0xE9, 0x1010020, 0x0206_0000,
                   0x0206_0001, 0x0206_0002, 0x0206_0003, 0xAA, 0xAB, 0xA9, 0xAC, 0x11, 0x101, 0x102, 0x93, 0x94,
                   0xAC, 0xAD, 0, 1, 0x53, 0x54, 0xFFFFFFFF]
        r = rng.random()
        if r < 0.45:
            return rng.choice(special)
        if r < 0.65:
            return rng.randrange(0, 0x60)
        if r < 0.85:
            return rng.randrange(0x10, 0x110)
        return rng.randrange(1 << 32) if rng.random() < 0.3 else rng.randrange(0x60, 0xF0)

    def flags(self):
        rng = self.rng
        v = rng.randrange(1 << 32)
        v = (v & ~1) | (1 if rng.random() < 0.65 else 0)
        v = (v & ~0xC) | rng.choice((0, 0, 0, 4, 8, 0xC))
        v = (v & ~0x30) | rng.choice((0, 0, 0x10, 0x20, 0x30))
        return v

    def mutate(self):
        rng = self.rng
        choices = [
            (0x273C, '<I', self.flags), (0x2DD4, '<I', self.state), (0x206D, '<B', lambda: rng.randrange(256)),
            (0x3CF2, '<B', lambda: rng.randrange(2)), (0x46B4, '<I', self.fbits), (0x49EC, '<I', self.fbits),
            (0x3D51, '<B', lambda: rng.randrange(2)), (0x4924, '<I', lambda: rng.randrange(2)),
        ]
        off, fmt, gen = rng.choice(choices)
        return self.stub_write(B + off, struct.pack(fmt, gen()))

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        self.hit.add(address)
        self.counts[address] = self.counts.get(address, 0) + 1
        if address in (ST0_STUB + 6,):
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        nargs = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        # the vectors the callee reads through its pointer argument
        pointer = None
        if address == 0x00E4B700:
            pointer = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
        elif address == 0x007B0580:
            pointer = uc.reg_read(x.UC_X86_REG_ESI)
        if pointer is not None:
            before['data'] = [(pointer + i, b) for i, b in enumerate(uc.mem_read(pointer, 16 if address == 0x007B0580 else 12))]
        rng = self.rng
        effects = []
        event = dict(target=address, before=before)
        if address == ST0_STUB:
            bits = self.fbits()
            uc.mem_write(CELL, struct.pack('<I', bits))
            after = list(before['regs'])
            after[4] = esp + 8
            event.update(after=dict(regs=after, memory=[]), st0=bits)
            self.events.append(event)
            return  # the stub's own fld/ret runs
        if rng.random() < 0.12:
            effects += self.mutate()
        if address == 0x007B0580:
            edi = uc.reg_read(x.UC_X86_REG_EDI)
            effects += self.stub_write(edi, struct.pack('<III', self.fbits(), self.fbits(), self.fbits()))
            eax = rng.choice((0, 1, 1, 1, rng.randrange(1 << 32)))
        elif address == 0x00421ED0:
            eax = VEC + 0x100 * rng.randrange(4)
        elif address == 0x00B85F70:
            eax = self.fbits()
        elif address in (0x00B66320,):
            eax = rng.choice((0, 1, 1, 2, rng.randrange(1 << 32)))
        else:
            eax = rng.choice((0, 1, 0x100, rng.randrange(1 << 32), rng.randrange(1 << 32)))
            if rng.random() < 0.5:
                eax = rng.choice((0, 1))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + (0 if address in CDECL else 4 * nargs))
        uc.reg_write(x.UC_X86_REG_EIP, back)
        event['after'] = dict(regs=self.regs(), memory=effects)
        self.events.append(event)
        if back == h.RET:
            uc.emu_stop()

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((B, 0x6000), (P, 0x100), (A, 0x100), (BB, 0x100), (V, 0x100), (H, 0x100), (D, 0x400),
                           (T, 0x10), (RECS, 0x800), (VEC, 0x400)):
            uc.mem_write(base, b'\0' * size)

        def put(base, off, fmt, v):
            uc.mem_write(base + off, struct.pack(fmt, v))
        put(B, 0, '<I', VB)
        put(V, 0, '<I', VV)
        put(H, 0, '<I', VH)
        put(B, 0x14, '<I', self.fbits(0.0166))
        for off in (0x40, 0x44, 0x48, 0xE4, 0x46B4, 0x49EC):
            put(B, off, '<I', self.fbits())
        put(B, 0x1C, '<I', rng.randrange(1 << 32))
        put(B, 0x206D, '<B', rng.choice((0, 2, 4, 6, 8, 0xE, rng.randrange(256))))
        put(B, 0x273C, '<I', self.flags())
        put(B, 0x2DD4, '<I', self.state())
        put(B, 0x384, '<H', rng.choice((0x905, 0x905, 0, rng.randrange(1 << 16))))
        for off in (0x3D51, 0x3CF1, 0x3CF2, 0x1EE4, 0x4051, 0x41B1, 0x40E1, 0x4421, 0x4281, 0x4351):
            put(B, off, '<B', rng.choice((0, 1, 1, rng.randrange(256))) if off not in (0x3CF1, 0x3CF2) else rng.choice((0, 0, 0, 1)))
        put(B, 0x4924, '<I', rng.choice((0, 1, 5)))
        put(B, 0x4920, '<B', rng.choice((0, 1)))
        put(B, 0x4760 + 0x1C0, '<B', rng.choice((0, 1)))
        put(B, 0x20D4, '<I', rng.randrange(1 << 32))
        put(B, 0x3348, '<I', V)
        put(B, 0x2E74, '<I', P)
        put(B, 0x3D6C, '<I', rng.choice((0, H, H)))
        put(B, 0x3DEC, '<I', rng.choice((0, D, D)))
        base = rng.choice((RECS, RECS, RECS, 0))
        put(B, 0x364, '<I', base)
        put(B, 0x368, '<I', T)
        put(T, 0, '<B', rng.choice((0xFF, 0, 1, 2, 3, 0)) if base else rng.choice((0, 0xFF)))
        for i in range(4):
            put(RECS, 160 * i + 0x44, '<I', self.fbits())
        put(P, 0xC, '<I', rng.choice((A, A, A, A, 0)))
        put(P, 0x10, '<I', rng.choice((BB, BB, BB, BB, 0)))
        put(P, 0x7D, '<B', rng.choice((0, 1, 1)))
        put(H, 0x28, '<B', rng.choice((0, 1, 1)))
        put(H, 0x29, '<B', rng.choice((0, 1, 1)))
        put(G, 0xB8844, '<I', rng.choice((0, 0x400000, rng.randrange(1 << 32))))
        put(G, 0x58, '<I', self.fbits())
        for i in range(4):
            for off in (0x40, 0x44, 0x48):
                put(VEC, 0x100 * i + off, '<I', self.fbits())
        # the locals the original reads before it writes them are not touched
        for off in (0x20, 0x24, 0x1C, 0x2C):
            pass
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = B, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=400000)
        except Exception:
            print('seed', seed, hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()])
            raise
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    from collections import Counter
    print(sorted(Counter(e['target'] for c in cases for e in c['events']).items()))
    # instruction coverage of the original
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    insns = [i.address for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY)]
    missed = [a for a in insns if a not in oracle.hit]
    print({hex(a): oracle.counts.get(a, 0) for a in (0xb672f6, 0xb673ae, 0xb672ca, 0xb672e2, 0xb67382, 0xb6739a)})
    print(f'{len(insns) - len(missed)}/{len(insns)} instructions covered; missed:', ' '.join(hex(a) for a in missed))
    json.dump(dict(function='0x00B66B30', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
