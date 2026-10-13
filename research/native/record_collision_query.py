"""L184 s2 oracle for the collision query family, run as the original's code
in Unicorn:

  python record_collision_query.py cast_b out.json [n]   0x01166C00
  python record_collision_query.py cast_a out.json [n]   0x01166B10
  python record_collision_query.py query  out.json [n]   0x00DEA870

Guest callees are stubs that record their calls (registers and argument
window before, effects and registers after); 0x00BFC7D0, 0x012EFE20,
0x011631F0 and 0x00FB38E0 run their ORIGINAL code. At every stub call of
`query` the bytes the original has written to its own stack frame so far
are recorded ("objects"). Format: crates/mtcore/tests/semantic_s2.rs.
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

MODES = {
    'cast_b': dict(entry=0x01166C00, length=0xEC, stubs={0x00FB3790: 0, 0x00DED390: 1, 0x01166A60: 2, 0x011668A0: 3,
                                                          0x01165EF0: 3, 0x01164500: 3, 0x01165560: 3}),
    'cast_a': dict(entry=0x01166B10, length=0xEC, stubs={0x00FB3790: 0, 0x00DED390: 1, 0x011669B0: 2, 0x011667B0: 3,
                                                          0x01165E00: 3, 0x01164500: 3, 0x01165560: 3}),
    'deep_b': dict(entry=0x01166C00, length=0xEC, stubs={0x00FB3790: 0, 0x00DED390: 1, 0x01164500: 3, 0x01165560: 3, 0x01173DC0: 6, 0x011740A0: 6, 0x01173800: 6, 0x01173AE0: 6, 0x011731B0: 6, 0x01173520: 6, 0x01174380: 6, 0x01175340: 6, 0x01175620: 6, 0x01174D80: 6, 0x01175060: 6, 0x01174730: 6, 0x01174AA0: 6, 0x01175900: 6}, real={0x011669B0: 0xB4, 0x01166A60: 0xB4, 0x011667B0: 0xE8, 0x011668A0: 0xE8, 0x01165E00: 0xF4, 0x01165EF0: 0xF4, 0x011787C0: 0x3F0}, deep=True),
    'deep_a': dict(entry=0x01166B10, length=0xEC, stubs={0x00FB3790: 0, 0x00DED390: 1, 0x01164500: 3, 0x01165560: 3, 0x01173DC0: 6, 0x011740A0: 6, 0x01173800: 6, 0x01173AE0: 6, 0x011731B0: 6, 0x01173520: 6, 0x01174380: 6, 0x01175340: 6, 0x01175620: 6, 0x01174D80: 6, 0x01175060: 6, 0x01174730: 6, 0x01174AA0: 6, 0x01175900: 6}, real={0x011669B0: 0xB4, 0x01166A60: 0xB4, 0x011667B0: 0xE8, 0x011668A0: 0xE8, 0x01165E00: 0xF4, 0x01165EF0: 0xF4, 0x011787C0: 0x3F0}, deep=True),
    'query': dict(entry=0x00DEA870, length=0xCC, stubs={0x0115F650: 5}),
    'walk_single_forward': dict(entry=0x011669B0, length=0xB4, stubs={0x011787C0: 6}, args=2),
    'walk_single_backward': dict(entry=0x01166A60, length=0xB4, stubs={0x011787C0: 6}, args=2),
    'walk_low_forward': dict(entry=0x011667B0, length=0xE8, stubs={0x011787C0: 6}, args=3),
    'walk_low_backward': dict(entry=0x011668A0, length=0xE8, stubs={0x011787C0: 6}, args=3),
    'walk_high_forward': dict(entry=0x01165E00, length=0xF4, stubs={0x011787C0: 6}, args=3),
    'walk_high_backward': dict(entry=0x01165EF0, length=0xF4, stubs={0x011787C0: 6}, args=3),
    'node': dict(entry=0x011787C0, length=0x3F0, stubs={0x01173DC0: 6, 0x011740A0: 6, 0x01173800: 6, 0x01173AE0: 6, 0x011731B0: 6, 0x01173520: 6, 0x01174380: 6, 0x01175340: 6, 0x01175620: 6, 0x01174D80: 6, 0x01175060: 6, 0x01174730: 6, 0x01174AA0: 6, 0x01175900: 6}, args=6),
    'segcast': dict(entry=0x0115F650, length=0x790, stubs={0x011C3280: 5, 0x01166B10: 2, 0x00DE46F0: 1, 0x00DE6010: 1,
                                                           0x007C6F90: 0, 0x00DEF0C0: 2, 0x011C3A30: 0}),
}
REAL = {0x00FB38E0: 8, 0x00BFC7D0: 8, 0x012EFE20: 0x80, 0x011631F0: 0xB8, 0x00B16AF0: 3, 0x01170F60: 8}
FLOATS = [0x0162AC60, 0x018CFE80, 0x0162ADB0, 0x01406CE0, 0x01621124, 0x0162112C, 0x01629510, 0x01629514, 0x01629518, 0x0161C280]
THIS, BLOB, STACK, SP = 0x20000000, 0x21000000, 0x0F000000, 0x0F008000
VTABLE = 0x23000000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, mode):
        self.cfg = cfg = MODES[mode]
        self.mode = mode
        self.entry, self.length, self.stubs = cfg['entry'], cfg['length'], cfg['stubs']
        self.real = {**REAL, **cfg.get('real', {})}
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {self.entry & ~0xFFF} | {a & ~0xFFF for a in list(self.stubs) + list(self.real) + FLOATS}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, s in ((THIS, 0x20000), (BLOB, 0x4000), (VTABLE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
        uc.mem_write(self.entry, h.code(self.entry, self.length))
        for a, n in self.real.items():
            uc.mem_write(a, h.code(a, n))
        for a in self.stubs:
            uc.mem_write(a, b'\xc3')
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
        data = bytes(data)
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in self.stubs:
            ok = self.entry <= address < self.entry + self.length or any(a <= address < a + n for a, n in self.real.items())
            assert ok, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = self.stubs[address]
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        if self.mode == 'node':
            before['objects'] = [(a, uc.mem_read(a, 1)[0]) for a in range(self.frame + 0x10, self.frame + 0x40) if a in self.written]
        if self.mode.startswith('walk_high'):   # the cast context {target, query, slot} on its stack
            before['objects'] = [(a, uc.mem_read(a, 1)[0]) for a in range(self.entry_esp - 0xc, self.entry_esp) if a in self.written]
        if self.mode in ('query', 'segcast'):
            frame, top = self.frame, (0x100 if self.mode == 'query' else 0x2d0)
            before['objects'] = [(a, uc.mem_read(a, 1)[0]) for a in range(frame + 0xc, frame + top) if a in self.written]
        effects = []
        if address == 0x00DED390:   # the shape's bounds method: writes into query + 0x70
            for k in range(rng.randrange(0, 6)):
                effects += self.stub_write(word(esp + 4) + k, [rng.randrange(256)])
        if address == 0x01166B10:   # the cast routine: fills the result buffer behind query + 0x94
            buffer = word(esp + 8) and word(word(esp + 8) + 0x94)
            effects += self.stub_write(buffer + 0xCC, [rng.choice((0, 1, 1, 2))])
            effects += self.stub_write(buffer + 0xD0, struct.pack('<I', BLOB + 0x2000 + 16 * rng.randrange(4)))
            for k in range(0x180, 0x1B8, 4):
                effects += self.stub_write(buffer + k, struct.pack('<f', rng.uniform(-3, 3)))
        eax = rng.choice((0, 1, 2, 4, 8, rng.randrange(1 << 10), rng.randrange(1 << 32)))
        if address == 0x01166B10:
            eax = rng.choice((0, 1, 1))
        back = word(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for a, n in ((THIS, 0x20000), (BLOB, 0x4000), (VTABLE, 0x1000)):
            uc.mem_write(a, b'\0' * n)
        uc.mem_write(0x0162ADB0, struct.pack('<I', rng.randrange(1 << 32)))
        for a in (0x01621124, 0x0162112C):
            uc.mem_write(a, struct.pack('<I', rng.randrange(1 << 32)))
        for off in range(0, 0x200, 4):
            uc.mem_write(THIS + off, struct.pack('<I', rng.randrange(1 << 32)))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 24 - 4 * rng.randrange(0, 8)
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        if self.mode == 'query':
            a0, a1, a2, a3 = BLOB, rng.choice((0, 1, 0x100, rng.randrange(1 << 32))), BLOB + 0x40, BLOB + 0x1000
            uc.mem_write(a3 + 0x3C, bytes([rng.choice((0, 1, 7))]))
            uc.mem_write(esp, struct.pack('<IIIII', h.RET, a0, a1, a2, a3))
            self.frame = ((esp - 4) & ~15) - 0xF4 - 0xC
            nbytes = 20
        elif self.mode.startswith('walk'):
            query, block = BLOB, BLOB + 0x400
            uc.mem_write(query + 0x98, struct.pack('<I', block))
            uc.mem_write(query + 0xB4, bytes([rng.choice((0, 1))]))
            uc.mem_write(query + 0xB8, struct.pack('<I', rng.randrange(1 << 32)))
            uc.mem_write(query + 0xE8, struct.pack('<I', rng.randrange(1 << 32)))
            uc.mem_write(block + 8, struct.pack('<I', rng.choice((0xFFFFFFFF, rng.randrange(1 << 32), rng.randrange(1 << 32) & rng.randrange(1 << 32)))))
            uc.mem_write(block + 0x14, struct.pack('<I', rng.choice((0, 2, 3, 6, rng.randrange(1 << 32)))))
            for off in range(0, 0x2A00, 0x40):
                uc.mem_write(THIS + 0x1C0 + off + 4, struct.pack('<I', rng.choice((0, 0, 1, rng.randrange(1 << 32)))))
            shape, slot = rng.randrange(1 << 32), rng.randrange(1 << 32)
            words = [h.RET, shape, query] + ([slot] if self.cfg['args'] == 3 else [])
            uc.mem_write(esp, struct.pack('<' + 'I' * len(words), *words))
            self.entry_esp = esp
            nbytes = 4 * len(words)
        elif self.mode == 'node':
            target = BLOB + 0x100
            kind = rng.choice((2, 2, 3, 4, 5, 6, 7, 7, 8, 9, 0x10, 0x15, 0x16, 0, 1, rng.randrange(1 << 32)))
            uc.mem_write(target + 4, struct.pack('<I', kind))
            for k in range(0, 0x30, 4):
                uc.mem_write(target + 0x10 + k, struct.pack('<f', rng.uniform(-50, 50)))
            for k in range(0, 16, 4):
                uc.mem_write(0x0162AC60 + k, struct.pack('<I', rng.choice((0, 0xFFFFFFFF, 0xFFFFFFFF, rng.randrange(1 << 32)))))
                uc.mem_write(0x018CFE80 + k, struct.pack('<f', rng.uniform(-3, 3)))
            uc.mem_write(0x0162ADB0, struct.pack('<f', rng.uniform(0, 1)))
            words = [h.RET, target] + [rng.randrange(1 << 32) for _ in range(3)] + [rng.choice((0, 1, 0x100, rng.randrange(1 << 32))), rng.randrange(1 << 32)]
            uc.mem_write(esp, struct.pack('<' + 'I' * len(words), *words))
            self.entry_esp = esp
            self.frame = ((esp - 4) & ~15) - 0x40
            nbytes = 4 * len(words)
        elif self.mode == 'segcast':
            seg, out, query, block = BLOB, BLOB + 0xC00, BLOB + 0x400, BLOB + 0x800
            for k in range(8):
                uc.mem_write(seg + 4 * k, struct.pack('<f', rng.uniform(-30, 30)))
            if rng.random() < 0.3:
                uc.mem_write(seg + 0x10, bytes(uc.mem_read(seg, 12)))   # a point
            thr = rng.choice((0.0, 1e9, rng.uniform(0, 3000)))
            uc.mem_write(THIS + 0x115F8, struct.pack('<f', thr))
            for off in (0x44, 0x48, 0x4C):
                uc.mem_write(THIS + off, struct.pack('<I', rng.choice((0, 1, 2, 4, 8))))
            for a in FLOATS:
                uc.mem_write(a, struct.pack('<f', rng.uniform(-3, 3)))
            uc.mem_write(0x01406CE0, struct.pack('<f', 1.0))
            uc.mem_write(0x0161C280, struct.pack('<IIII', *([0x80000000] * 4)))
            for k in range(0, 0x40 * 4, 4):
                uc.mem_write(BLOB + 0x2000 + k, struct.pack('<f', rng.uniform(-2, 2)))
            uc.mem_write(query + 0x94, struct.pack('<I', rng.randrange(1 << 32)))
            uc.mem_write(query + 0x98, struct.pack('<I', block))
            flag = rng.choice((0, 1, rng.randrange(1 << 32)))
            out = out if rng.random() < 0.7 else 0
            uc.mem_write(esp, struct.pack('<IIIIII', h.RET, seg, flag, out, query, 0x01166B10))
            self.frame = ((esp - 4) & ~15) - 0x2C4 - 0xC
            nbytes = 24
        else:
            query, block, buffer = BLOB, BLOB + 0x400, BLOB + 0x800
            uc.mem_write(query + 0x94, struct.pack('<I', buffer))
            uc.mem_write(query + 0x98, struct.pack('<I', block))
            uc.mem_write(block + 0x14, struct.pack('<I', rng.choice((0, 4, 8, 12, 2, rng.randrange(1 << 32)))))
            uc.mem_write(block + 0xB8, bytes([rng.choice((0, 1))]))
            if rng.random() < 0.8 or self.cfg.get('deep'):
                shape = BLOB + 0x200
                uc.mem_write(shape, struct.pack('<I', VTABLE))
                uc.mem_write(VTABLE + 0x1C, struct.pack('<I', 0x00DED390))
            else:
                shape = 0
            if self.cfg.get('deep'):
                for off in range(0, 0x2A00, 0x40):
                    uc.mem_write(THIS + 0x1C0 + off + 4, struct.pack('<I', rng.choice((0, 1, 1, 1))))
                uc.mem_write(block + 8, struct.pack('<I', rng.choice((0xFFFFFFFF, rng.randrange(1 << 32)))))
                uc.mem_write(block + 0x14, struct.pack('<I', rng.choice((4, 6, 12, 14, 4 | rng.randrange(1 << 4)))))
                uc.mem_write(query + 0xB4, bytes([rng.choice((0, 1))]))
                other = BLOB + 0x300
                uc.mem_write(query + 0xB8, struct.pack('<I', other))
                uc.mem_write(query + 0xE8, struct.pack('<I', rng.randrange(1 << 32)))
                kinds = (2, 2, 3, 4, 5, 6, 7, 7, 8, 9, 0x10, 0x15, 0x16, 1)
                for obj in (BLOB + 0x200, other):
                    uc.mem_write(obj + 4, struct.pack('<I', rng.choice(kinds)))
                    for k in range(0, 0x30, 4):
                        uc.mem_write(obj + 0x10 + k, struct.pack('<f', rng.uniform(-50, 50)))
                if shape:
                    uc.mem_write(shape + 4, struct.pack('<I', rng.choice(kinds)))
                for k in range(0, 16, 4):
                    uc.mem_write(0x0162AC60 + k, struct.pack('<I', rng.choice((0, 0xFFFFFFFF))))
                    uc.mem_write(0x018CFE80 + k, struct.pack('<f', rng.uniform(-3, 3)))
            uc.mem_write(esp, struct.pack('<III', h.RET, shape, query))
            nbytes = 12
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, nbytes)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(self.entry, h.RET, count=400000)
        except Exception:
            print('fault at eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), 'seed', seed, [hex(r) for r in self.regs()])
            raise
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after, insns=self.insns)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 2000
    oracle = Oracle(mode)
    cases = [oracle.case(seed) for seed in range(n)]
    print(mode, n, 'cases,', sum(len(c['events']) for c in cases), 'calls; mean original insns',
          sum(c['insns'] for c in cases) / n)
    json.dump(dict(function=hex(MODES[mode]['entry']), cases=cases), open(out, 'w'))


main()
