"""L184 oracle: 0x0084C0A0 uCharacterBase::vf92 (thiscall, plain ret), the
per-frame character update, run as the original's code in Unicorn on
randomized characters covering every branch (the script prints the
instructions no case reached).

All its callees are stubs (a bare RET), recorded in the format of
crates/mtcore/tests/semantic.rs `abi_cases_full`: the register-convention
helpers 0x00423B80 (ESI), 0x0081F280 / 0x0080B450 / 0x004219A0 /
0x004215E0 / 0x00CADE50 (EAX), 0x00B809D0 (ECX, EDX), 0x00886210 (ECX), the
tuning-value virtual method (slot 0x3C of the object at
[[0x018FA4BC]+0xB8754], returns a float in ST0: recorded as `st0` bits), and
the EnterCriticalSection / LeaveCriticalSection import targets. Each event
also has `check`, the bytes the callee reads that the original wrote (the
16-byte vector of 0x00CADE50).
Output is private (E:/ddda-re/notes/native-L184). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import math
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x0084C0A0, 0x4C0  # through RET at 0x0084C55F
# stub address -> (stack argument words the callee pops, kind)
STUBS = {
    0x00423B80: (0, 'mask'), 0x0081F280: (0, 'void'), 0x0080B450: (0, 'void'), 0x00886210: (0, 'update'),
    0x004219A0: (4, 'slots'), 0x004215E0: (3, 'void'), 0x00CADE50: (2, 'vector'), 0x00B809D0: (0, 'byte'),
    0x30000000: (1, 'float'), 0x30000010: (1, 'void'), 0x30000020: (1, 'void'),
}
VSTUB, ENTER, LEAVE = 0x30000000, 0x30000010, 0x30000020
THIS, RUNTIME, TUNING, VTABLE = 0x20000000, 0x23000000, 0x24000000, 0x24001000
INSTANCE, TABLE, LIST, SLOTS, OBJECT = 0x25000000, 0x26000000, 0x26001000, 0x20003000, 0x27000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
POOL = [0.0, 0.5, 1.0, 1.5, 2.0, 0.25, 0.95, -1.0, 3.0]
SPEEDS = [0.0, 10.0, 24.0, 25.0, 26.0, 30.0, 50.0, 100.0, -5.0]
TUNES = [0.0, 10.0, 25.0, 30.0, 50.0, 100.0, 24.0, -5.0]


def f(v):
    return struct.pack('<f', v)


def fp80(v):
    """(mantissa64, sign+exponent16) of a finite float."""
    if v == 0:
        return (0, 0x8000 if math.copysign(1, v) < 0 else 0)
    mant, exp = math.frexp(abs(v))
    return (int(mant * (1 << 64)), (0x8000 if v < 0 else 0) | (exp - 1 + 16383))


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {ENTRY & ~0xFFF}
        for a in list(STUBS) + [0x018D0B6C, 0x018FA4BC, 0x0139D160, 0x015319B8, 0x014FA314, 0x0161D590]:
            pages.add(a & ~0xFFF)
        pages.add(0x018D0BB0 & ~0xFFF)
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((THIS, 0x4000), (RUNTIME, 0xC0000), (TUNING, 0x2000), (INSTANCE, 0x2000), (TABLE, 0x2000),
                        (OBJECT, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_map(0x018D1000, 0x1000)  # [0x018D0B6C] and [0x018D0BB0] may straddle nothing; harmless
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
        uc.mem_write(0x015319B8, f(1.0))
        uc.mem_write(0x014FA314, f(25.0))
        uc.mem_write(0x0161D590, struct.pack('<d', 0.95))
        uc.mem_write(0x0139D160, struct.pack('<II', ENTER, LEAVE))
        uc.mem_write(0x018D0B6C, struct.pack('<I', INSTANCE))
        uc.mem_write(0x018FA4BC, struct.pack('<I', RUNTIME))
        uc.mem_write(VTABLE + 0x3C, struct.pack('<I', VSTUB))
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        self.hit = set()

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

    def push_st0(self, value):
        """x87 push of a float (Unicorn: set TOP, tag and the register)."""
        uc = self.uc
        fpsw = uc.reg_read(x.UC_X86_REG_FPSW)
        top = ((fpsw >> 11) & 7) - 1 & 7
        uc.reg_write(x.UC_X86_REG_FPSW, (fpsw & ~0x3800) | (top << 11))
        uc.reg_write(x.UC_X86_REG_FPTAG, uc.reg_read(x.UC_X86_REG_FPTAG) & ~(3 << (2 * top)) & 0xFFFF)
        uc.reg_write(x.UC_X86_REG_FP0 + top, fp80(value))

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        nargs, kind = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        effects, eax, st0 = [], rng.randrange(1 << 32), None
        if kind == 'vector':
            ptr = struct.unpack('<I', uc.mem_read(esp + 8, 4))[0]
            before['check'] = [(ptr + i, b) for i, b in enumerate(uc.mem_read(ptr, 16))]
        elif kind == 'mask':
            eax = rng.choice((0, 0xC002000, 0x2000, 0x4000000, 0x8000000, rng.randrange(1 << 32)))
        elif kind == 'byte':
            eax = rng.choice((0, 1, 0x100, 0x1FF, rng.randrange(1 << 32)))
        elif kind == 'float':
            st0 = rng.choice(TUNES)
            self.push_st0(st0)
        elif kind == 'update':
            if rng.random() < 0.5:  # the base update changes the attachment
                effects += self.stub_write(THIS + 0x2E74, struct.pack('<I', rng.choice((0, OBJECT))))
        elif kind == 'slots':
            if rng.random() < 0.3:  # an action slot handler changes the slot bits
                effects += self.stub_write(THIS + 0x494, struct.pack('<I', rng.randrange(1 << 11)))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        after = dict(regs=self.regs(), memory=effects)
        if st0 is not None:
            after['st0'] = struct.unpack('<I', f(st0))[0]
        self.events.append(dict(target=address, before=before, after=after))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x4000), (RUNTIME + 0xB8000, 0x1000), (TUNING, 0x2000), (INSTANCE, 0x2000), (TABLE, 0x2000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x800, bytes(rng.randrange(256) for _ in range(0x800)))
        uc.mem_write(0x018D0BB0, bytes([rng.choice((0, 0, 1))]))

        def put(offset, data):
            uc.mem_write(THIS + offset, data)

        def fl(offset, values=POOL):
            put(offset, f(rng.choice(values)))

        def u32(offset, value):
            put(offset, struct.pack('<I', value))

        fl(0x14, [0.0, 0.016, 0.033, 1.0, 2.0])
        u32(0x26A0, rng.choice((0, 0x800, 0x1000, 0x1800, 0x1801)))
        u32(0x26A4, rng.randrange(1 << 32))
        fl(0x2864)
        put(0x286D, bytes([rng.choice((0, 1, 2))]))
        u32(0x1DE0, rng.randrange(1 << 32))
        fl(0x27B0)
        put(0x206C, bytes([rng.choice((0, 0x80, 0x7F, 0xFF))]))
        put(0x206D, bytes([rng.randrange(8)]))
        put(0x206E, bytes([rng.choice((0, 0x10, 0xEF, 0xFF))]))
        u32(0x2DD4, rng.choice((0xE4, 0xE4, 0, 5)))
        u32(0x2368, rng.choice((0, 4, 0x1C, 0x800000, 0x80001C, 0x20, 0x80001C, 8, 0x10, 0x1000000)))
        fl(0x29B4, SPEEDS)
        fl(0x29A4)
        fl(0x29A0)
        u32(0x2B30, rng.choice((0, 1, 1)))
        put(0x2DAE, bytes([rng.choice((0, 1))]))
        fl(0x2DB0)
        u32(0x2E74, rng.choice((0, OBJECT)))
        u32(0x484, rng.choice((0, SLOTS, SLOTS)))
        u32(0x494, rng.randrange(1 << 11) if rng.random() < 0.7 else 0x780)
        u32(0x4A4, rng.randrange(1 << 11))
        u32(0x210C, rng.randrange(1 << 32))
        fl(0x2EA8, [-1.0, 0.0, 0.5, 2.0, 0.01])
        u32(0x2EAC, rng.randrange(1 << 32))
        put(0xE0, bytes(rng.randrange(256) for _ in range(16)))
        # u16 slot ids (small: they index the handler list)
        uc.mem_write(SLOTS, b''.join(struct.pack('<H', rng.randrange(64)) for _ in range(16)))
        # runtime tuning object, global instance and its handler list
        uc.mem_write(RUNTIME + 0xB8754, struct.pack('<I', rng.choice((0, TUNING, TUNING, TUNING))))
        uc.mem_write(TUNING, struct.pack("<I", VTABLE))
        uc.mem_write(VTABLE + 0x3C, struct.pack("<I", VSTUB))
        uc.mem_write(INSTANCE + 0x1C, bytes([rng.choice((0, 1))]))
        uc.mem_write(INSTANCE + 0xCE0, struct.pack('<I', rng.choice((0, 1, 1))))
        uc.mem_write(INSTANCE + 0xCF8, struct.pack('<I', rng.choice((0, TABLE, TABLE))))
        uc.mem_write(TABLE + 0x70, struct.pack('<I', LIST))
        uc.mem_write(LIST, b''.join(struct.pack('<I', rng.choice((0, OBJECT + 0x10 * i))) for i in range(64)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 4
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x0084C0A0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by_target = {}
    for c in cases:
        for e in c['events']:
            by_target[hex(e['target'])] = by_target.get(hex(e['target']), 0) + 1
    print(f'{n} cases, {calls} calls -> {out}')
    print('calls by target:', by_target)
    print('unhit instructions:', unhit)


main()
