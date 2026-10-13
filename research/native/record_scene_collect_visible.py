"""L184 s9 oracle: 0x00FEB1F0 `scene->collect_visible(ctx, mask, max, probe)`
(sScene::vf13) run as the original's code in Unicorn on synthetic scenes.
The sphere test 0x00D13600 runs as the real original code (the port folds it
in natively); the guest calls are stubs with recorded effects: the scene's
selection virtual (+0x64), each object's virtual +0x50, the spatial index
query 0x011787C0 and the final pass 0x00FE4F30. The slot id tables are the
exe's real data. Format of crates/mtcore/tests/semantic_s9.rs `abi_cases_full`.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00FEB1F0, 0x929  # through RET 0x10 at 0x00FEBB16
SPHERE, SPHERE_LENGTH = 0x00D13600, 0x54
FINAL, SPECIAL = 0x00FE4F30, 0x011787C0
VC64, VC50 = 0x25000000, 0x25000100
ARGS = {VC64: 6, VC50: 2, FINAL: 2, SPECIAL: 6}
CTX, CTX_SIZE = 0x20000000, 0x32000
DEFAULTS, BINDINGS = 0x21000000, 0x21010000
SCENE, LIST, SCENE_VT, OBJ_VT, POS = 0x22000000, 0x22001000, 0x22002000, 0x22003000, 0x22004000
OBJS, OBJ_SIZE, OBJ_COUNT = 0x23000000, 0x400, 24
STACK, SP = 0x0F000000, 0x0F008000
TABLES = {'A': 0x0143A61C, 'B': 0x0143A5FC, 'C': 0x0142EC54, 'D': 0x0142EC34, 'E': 0x0143A5DC}
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x2000), (SPHERE & ~0xFFF, 0x1000), (FINAL & ~0xFFF, 0x1000),
                              (SPECIAL & ~0xFFF, 0x1000), (VC64, 0x1000), (0x0142E000, 0x1000), (0x0143A000, 0x1000),
                              (0x0162A000, 0x1000), (CTX, CTX_SIZE), (DEFAULTS, 0x20000), (SCENE, 0x2000),
                              (SCENE_VT, 0x2000), (POS, 0x1000), (OBJS, OBJ_SIZE * OBJ_COUNT), (STACK, 0x10000),
                              (h.RET, 0x1000)):
            uc.mem_map(address, size)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x10\x00', h.code(ENTRY + LENGTH - 3, 3)
        assert h.code(SPHERE + SPHERE_LENGTH - 1, 1) == b'\xc3'
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(SPHERE, h.code(SPHERE, SPHERE_LENGTH))
        uc.mem_write(0x0142EC30, h.code(0x0142EC30, 0x50))
        uc.mem_write(0x0143A5D0, h.code(0x0143A5D0, 0x80))
        uc.mem_write(0x0162ADB0, h.code(0x0162ADB0, 4))
        for address in (FINAL, SPECIAL, VC64, VC50):
            uc.mem_write(address, b'\xc3')
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        self.tables = {k: [struct.unpack('<I', h.code(a + 4 * i, 4))[0] for i in range(9)] for k, a in TABLES.items()}

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

    def u32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def stub_write32(self, effects, address, value):
        data = struct.pack('<I', value & 0xFFFFFFFF)
        self.remember(address, 4)
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + 4))
        effects += [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in ARGS:
            assert ENTRY <= address < ENTRY + LENGTH or SPHERE <= address < SPHERE + SPHERE_LENGTH, hex(address)
            if address < SPHERE + SPHERE_LENGTH and address >= SPHERE:
                pass
            else:
                self.visited.add(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = ARGS[address]
        self.remember(esp, 4 * (1 + nargs))
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        args = struct.unpack('<%dI' % nargs, uc.mem_read(esp + 4, 4 * nargs))
        frame = esp + 4 + 4 * nargs
        extra = []
        if address == VC64:
            extra += [(args[3], 36), (args[4], 4)]
        if address == SPECIAL:
            frame = args[0] - 0x40
            extra += [(frame + o, 4) for o in (0x2C, 0x30, 0x34, 0x38, 0x3C, 0x40, 0x44, 0x48, 0x50, 0x54, 0x58, 0x5C)]
        for a, n in extra:
            self.remember(a, n)
            window += [(a + i, b) for i, b in enumerate(uc.mem_read(a, n))]
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        max_count = self.max_count
        pool = [OBJS + OBJ_SIZE * k for k in range(OBJ_COUNT)]
        if address in (VC64, SPECIAL):
            count_address = args[4] if address == VC64 else frame + 0x38
            array = args[3] if address == VC64 else self.u32(frame + 0x3C)
            count = self.u32(count_address)
            if rng.random() < 0.75:
                added = rng.randrange(0, 4)
                new = min(count + added, max_count, 8)
                for k in range(count, new):
                    self.stub_write32(effects, array + 4 * k, rng.choice(pool))
                if new < count:
                    new = count
                self.stub_write32(effects, count_address, new)
            elif address == SPECIAL:
                self.stub_write32(effects, count_address, count)
            if rng.random() < 0.25:
                self.stub_write32(effects, CTX + 0x154, rng.getrandbits(32))
        elif address == VC50:
            if rng.random() < 0.15:
                count_address = frame + 0x14
                count = self.u32(count_address)
                self.stub_write32(effects, count_address, rng.randrange(0, count + 1))
            if rng.random() < 0.3:
                table = rng.choice('ABCDE')
                slot = self.tables[table][rng.randrange(8)] & 0xFFF
                self.stub_write32(effects, CTX + 0x1F8 + slot * 8 + rng.choice((0, 4)), rng.choice((0, rng.getrandbits(32))))
        uc.reg_write(x.UC_X86_REG_EAX, rng.getrandbits(32))
        for r in (x.UC_X86_REG_ECX, x.UC_X86_REG_EDX):
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, frame)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((CTX, CTX_SIZE), (DEFAULTS, 0x20000), (SCENE, 0x2000), (SCENE_VT, 0x2000), (POS, 0x1000),
                           (OBJS, OBJ_SIZE * OBJ_COUNT), (STACK, 0x10000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(DEFAULTS, rng.randbytes(0x8000))
        uc.mem_write(BINDINGS, rng.randbytes(0x8000))
        # scene
        self.w32(SCENE, SCENE_VT)
        self.w32(SCENE_VT + 0x64, VC64)
        self.w32(OBJ_VT + 0x50 - 0x50, 0)  # keep the page defined
        uc.mem_write(OBJ_VT, b'\0' * 0x100) if False else None
        self.w32(SCENE + 0x20, LIST)
        first_count, second_count = rng.randrange(0, 13), rng.randrange(0, 13)
        self.w32(SCENE + 0x34, rng.randrange(0, 8))
        self.w32(SCENE + 0x38, first_count)
        self.w32(SCENE + 0x2C, rng.randrange(0, 8))
        self.w32(SCENE + 0x30, second_count)
        uc.mem_write(SCENE + 0xB5C, bytes([rng.choice((0, 0, 1, 7))]))
        self.w32(SCENE + 0xB60, rng.choice((0, 0x1234, 0xABCDE)))
        # objects
        passbit_index = rng.choice((0, 1, 2, 3, 5, 9, 9, 12, 31, 40))
        passbit = 1 << (passbit_index & 31)
        mask = rng.getrandbits(32) | rng.choice((0, 1))
        for k in range(OBJ_COUNT):
            obj = OBJS + OBJ_SIZE * k
            self.w32(obj, OBJ_VT)
            flags = rng.getrandbits(32)
            if rng.random() < 0.85:
                flags = (flags & ~7) | 2
            if rng.random() < 0.85:
                flags |= 0x4800
            if rng.random() < 0.85:
                flags |= (passbit & 0x3FF) << 16
            self.w32(obj + 4, flags)
            self.w32(obj + 0x34, rng.choice((mask, rng.getrandbits(32), 0, mask & rng.getrandbits(32))))
            uc.mem_write(obj + 0x32, struct.pack('<H', rng.choice((rng.randrange(8), rng.getrandbits(16)))))
            self.w32(obj + 0x1E0, rng.choice((0, rng.getrandbits(32), rng.randrange(0x40) << rng.choice((0, 4)), 0x1000 | rng.randrange(4))))
            center = [rng.uniform(-5, 5) for _ in range(3)] + [rng.choice((0.5, 1.0, 3.0, 0.0, 100.0))]
            if rng.random() < 0.03:
                center[rng.randrange(4)] = float('nan')
            uc.mem_write(obj + 0x210, struct.pack('<4f', *center))
        self.w32(OBJ_VT + 0x50, VC50)
        uc.mem_write(LIST, b''.join(struct.pack('<I', OBJS + OBJ_SIZE * rng.randrange(OBJ_COUNT)) for _ in range(32)))
        uc.mem_write(POS, struct.pack('<4f', *[rng.uniform(-5, 5) for _ in range(3)], rng.choice((0.0, 0.5, 2.0))))
        # context
        self.w32(CTX + 0x1C, BINDINGS)
        self.w32(CTX + 0x20, DEFAULTS)
        uc.mem_write(CTX + 0x168, bytes([passbit_index]))
        self.w32(CTX + 0x5C, (rng.getrandbits(32) & ~0x80) | rng.choice((0, 0x80, 0x80)))
        self.w32(CTX + 0x6C, rng.randrange(0, 5))
        self.w32(CTX + 0x154, rng.getrandbits(32))
        # slots: random, but often equal to the default (and bound consistently)
        slots = set()
        for table in self.tables.values():
            slots |= {v & 0xFFF for v in table}
        for slot in slots:
            default = self.u32(DEFAULTS + slot * 8)
            value = default if rng.random() < 0.5 else rng.choice((0, rng.getrandbits(32), default & 0xFFFFFFF0, default | 3))
            self.w32(CTX + 0x1F8 + slot * 8, value)
            self.w32(CTX + 0x1FC + slot * 8, rng.choice((0, 0, slot, rng.getrandbits(32))))
        for value in self.tables['D']:
            slot = value & 0xFFF
            if rng.random() < 0.92:
                tag = (rng.getrandbits(20) << 12) | slot
                self.w32(CTX + 0x1FC + slot * 8, tag)
                entry = (value >> 12) if rng.random() < 0.8 else rng.getrandbits(20)
                self.w32(BINDINGS + slot * 8, (rng.getrandbits(12) << 20) | entry)
        max_count = rng.choice((0, 1, 2, 3, 5, 8, 8))
        self.max_count = max_count
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<IIIII', h.RET, CTX, mask, max_count, POS))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = SCENE, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 20)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2500
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00FEB1F0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {len(visited)} instruction addresses visited -> {out}')
    open(out + '.visited', 'w').write('\n'.join('%08x' % a for a in sorted(visited)))


main()
