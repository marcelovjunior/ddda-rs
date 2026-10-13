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
    'query': dict(entry=0x00DEA870, length=0xCC, stubs={0x0115F650: 5}),
}
REAL = {0x00FB38E0: 8, 0x00BFC7D0: 8, 0x012EFE20: 0x80, 0x011631F0: 0xB8}
FLOATS = [0x0162ADB0]
THIS, BLOB, STACK, SP = 0x20000000, 0x21000000, 0x0F000000, 0x0F008000
VTABLE = 0x23000000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, mode):
        self.cfg = cfg = MODES[mode]
        self.mode = mode
        self.entry, self.length, self.stubs = cfg['entry'], cfg['length'], cfg['stubs']
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {self.entry & ~0xFFF} | {a & ~0xFFF for a in list(self.stubs) + list(REAL) + FLOATS}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, s in ((THIS, 0x2000), (BLOB, 0x2000), (VTABLE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
        uc.mem_write(self.entry, h.code(self.entry, self.length))
        for a, n in REAL.items():
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
            ok = self.entry <= address < self.entry + self.length or any(a <= address < a + n for a, n in REAL.items())
            assert ok, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = self.stubs[address]
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        if self.mode == 'query':
            frame = self.frame
            before['objects'] = [(a, uc.mem_read(a, 1)[0]) for a in range(frame + 0xc, frame + 0x100) if a in self.written]
        effects = []
        if address == 0x00DED390:   # the shape's bounds method: writes into query + 0x70
            for k in range(rng.randrange(0, 6)):
                effects += self.stub_write(word(esp + 4) + k, [rng.randrange(256)])
        eax = rng.choice((0, 1, 2, 4, 8, rng.randrange(1 << 10), rng.randrange(1 << 32)))
        back = word(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for a, n in ((THIS, 0x2000), (BLOB, 0x2000), (VTABLE, 0x1000)):
            uc.mem_write(a, b'\0' * n)
        uc.mem_write(0x0162ADB0, struct.pack('<I', rng.randrange(1 << 32)))
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
        else:
            query, block, buffer = BLOB, BLOB + 0x400, BLOB + 0x800
            uc.mem_write(query + 0x94, struct.pack('<I', buffer))
            uc.mem_write(query + 0x98, struct.pack('<I', block))
            uc.mem_write(block + 0x14, struct.pack('<I', rng.choice((0, 4, 8, 12, 2, rng.randrange(1 << 32)))))
            uc.mem_write(block + 0xB8, bytes([rng.choice((0, 1))]))
            if rng.random() < 0.8:
                shape = BLOB + 0x200
                uc.mem_write(shape, struct.pack('<I', VTABLE))
                uc.mem_write(VTABLE + 0x1C, struct.pack('<I', 0x00DED390))
            else:
                shape = 0
            uc.mem_write(esp, struct.pack('<III', h.RET, shape, query))
            nbytes = 12
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, nbytes)
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=400000)
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
