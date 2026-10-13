"""L184 oracle: 0x00E516F0 `uCnsIK::init_chain(skeleton, nodes, count,
axis_selector, up_selector) -> ok` (thiscall, `ret 0x14`), run as the
original's code in Unicorn.

The IK chain set-up: for each of `count` 0x260-byte chain nodes it looks
up the node's joint, rebuilds the joint's world matrix from its local
rotation/scale/translation and its parent's, builds the node's frame from
the chain's axis/up selectors and stores the inverse-relative matrices;
then it measures the segment lengths. The frame builder 0x01130A00 (cdecl)
is part of the run (inline, no event); the only guest call recorded is the
skeleton's virtual `vtable[0x58](-1)` (the root matrix), a stub at
0x30000020 returning a pointer (thiscall, `ret 4`).

Reads the user's own exe; output is private (E:/ddda-re/notes/native-L184).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import os
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00E516F0, 0x15EE  # through RET 0x14 at 0x00E53CDB (the jump tables follow)
TABLES_END = 0x00E53D10
FRAME, FRAME_LEN = 0x01130A00, 0x1B1  # the frame builder, run inline
STUB_ROOT = 0x30000020
THIS, SKEL, VTABLE, JOINTS, MAP, ROOT = 0x20001000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000
NODES = THIS + 0xD0
SP = 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
CONST_PAGES = (0x01406000, 0x01407000, 0x0161C000, 0x01629000)


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x3000), (FRAME & ~0xFFF, 0x1000), (STUB_ROOT & ~0xFFF, 0x1000),
                              (0x20000000, 0x5000), (SKEL, 0x1000), (VTABLE, 0x1000), (JOINTS, 0x4000), (MAP, 0x1000),
                              (ROOT, 0x1000), (0x0F000000, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, TABLES_END - ENTRY))
        uc.mem_write(FRAME, h.code(FRAME, FRAME_LEN))
        uc.mem_write(STUB_ROOT, b'\xc3')
        for page in CONST_PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code, begin=STUB_ROOT, end=STUB_ROOT)
        uc.hook_add(UC_HOOK_CODE, self.finish, begin=h.RET, end=h.RET)
        self.covered = set()
        if os.environ.get('COVER'):
            uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: self.covered.add(a))
            uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: self.covered.add(a))

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

    def finish(self, uc, address, size, data):
        uc.emu_stop()

    def code(self, uc, address, size, data):
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 8))]
        before = dict(regs=self.regs(), memory=window)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, ROOT)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    # ---- synthetic inputs -------------------------------------------------
    def value(self, rng):
        r = rng.random()
        if r < 0.05:
            return 0.0
        if r < 0.1:
            return rng.choice((1.0, -1.0, 0.5, 2.0))
        return rng.uniform(-3, 3)

    def quat(self, rng):
        q = [rng.uniform(-1, 1) for _ in range(4)]
        if rng.random() < 0.7:
            n = sum(v * v for v in q) ** 0.5 or 1.0
            q = [v / n for v in q]
        return q

    def matrix(self, rng, scale_rows=True):
        """A world matrix: three axis rows (4 floats each) and a position row."""
        rows = []
        for _ in range(3):
            row = [self.value(rng) for _ in range(4)]
            r = rng.random()
            if r < 0.06:
                row = [rng.uniform(-1e-9, 1e-9) for _ in range(4)]  # shorter than the epsilon
            elif r < 0.07:
                row = [v * rng.choice((1e5, 1e-8)) for v in row]  # extreme lengths (a tiny axis with a long cross product)
            elif r < 0.08:
                row = [0.0] * 4
            elif r < 0.5:
                n = sum(v * v for v in row[:3]) ** 0.5 or 1.0
                row = [row[0] / n, row[1] / n, row[2] / n, 0.0]
            rows.append(row)
        rows.append([rng.uniform(-10, 10) for _ in range(3)] + [rng.choice((1.0, 1.0, 0.0, 0.5))])
        return [v for row in rows for v in row]

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        count = rng.choice((1, 2, 3, 4, 4, 4, 5, 3, 2, 0, -1))
        flag = rng.choice((0, 1))
        if rng.random() < 0.15:
            flag = rng.randrange(2, 256)
        joint_count = rng.randrange(max(count, 1) + 1, 12)
        # zero the writable regions
        uc.mem_write(THIS, bytes(0x4000))
        uc.mem_write(JOINTS, bytes(0x4000))
        uc.mem_write(MAP, bytes(0x100))
        uc.mem_write(ROOT, f32(0) * 16)
        # skeleton object
        uc.mem_write(SKEL, struct.pack('<I', VTABLE))
        uc.mem_write(SKEL + 0x364, struct.pack('<II', JOINTS, MAP))
        uc.mem_write(VTABLE + 0x58, struct.pack('<I', STUB_ROOT))
        uc.mem_write(ROOT, b''.join(f32(v) for v in self.matrix(rng)))
        # joints: parent index smaller than own (or 0xff), local rotation/scale/translation, world matrix
        for j in range(joint_count):
            a = JOINTS + 0xA0 * j
            parent = 0xFF if j == 0 or rng.random() < 0.15 else rng.randrange(j)
            uc.mem_write(a + 0x10, b''.join(f32(v) for v in self.matrix(rng)))
            uc.mem_write(a + 0x60, b''.join(f32(v) for v in self.quat(rng)))
            uc.mem_write(a + 0x70, b''.join(f32(abs(rng.uniform(0.2, 2.5)) if rng.random() < 0.9 else 1.0) for _ in range(3)) + f32(0))
            uc.mem_write(a + 0x80, b''.join(f32(rng.uniform(-5, 5)) for _ in range(3)))
            uc.mem_write(a + 0x90, bytes([rng.randrange(256)]) + bytes([rng.randrange(256)]) + bytes(14))
            uc.mem_write(a + 0x91, bytes([parent]))
        # id -> joint map; node ids: low byte is the id, upper bytes are noise
        ids = rng.sample(range(256), 12)
        for k in range(256):
            uc.mem_write(MAP + k, bytes([0xFF]))
        for k, i in enumerate(ids):
            uc.mem_write(MAP + i, bytes([k % joint_count if rng.random() > 0.03 else 0xFF]))
        # a chain mostly follows the parent links
        for n in range(max(count, 0)):
            a = NODES + 0x260 * n
            uc.mem_write(a + 0x1A0, struct.pack('<I', (rng.randrange(1 << 24) << 8) | ids[rng.randrange(len(ids))]))
            uc.mem_write(a + 8, f32(rng.uniform(-0.5, 2) if rng.random() < 0.9 else 0.0) + f32(rng.uniform(0.3, 2) if rng.random() < 0.9 else rng.choice((0.0, -1.0))))
            # junk the original overwrites (so that a missing write shows)
            uc.mem_write(a + 4, bytes(rng.randrange(256) for _ in range(4)))
            uc.mem_write(a + 0x190, bytes(rng.randrange(256) for _ in range(16)))
            uc.mem_write(a + 0x150, bytes(rng.randrange(256) for _ in range(0x40)))
            uc.mem_write(a + 0x244, bytes(rng.randrange(256) for _ in range(4)))
        if rng.random() < 0.1:
            # a tiny chain axis with a long up vector: the frame builder normalises the cross product only
            for j in range(joint_count):
                a = JOINTS + 0xA0 * j
                uc.mem_write(a + 0x10, b''.join(f32(rng.uniform(-1, 1) * 1e-8) for _ in range(3)) + f32(0))
                uc.mem_write(a + 0x20, b''.join(f32(rng.uniform(-1, 1) * 1e5) for _ in range(3)) + f32(0))
            tiny = True
        else:
            tiny = False
        uc.mem_write(THIS + 0xCFA, bytes([flag]))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 0x40 - 4 * rng.randrange(4)
        ax, up = rng.randrange(6), rng.randrange(6)
        if tiny:
            ax, up = rng.choice((0, 3)), rng.choice((1, 4))
        if rng.random() < 0.06 and not tiny:
            ax = rng.choice((6, 7, 255, 0xFFFFFFFF, 1 << 31))  # no choice: the original keeps its (uninitialised) stack locals
        if rng.random() < 0.06:
            up = rng.choice((6, 9, 255, 0xFFFFFFFF))
        uc.mem_write(esp, struct.pack('<I', h.RET) + struct.pack('<IIiII', SKEL, NODES, count, ax, up))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        start[1] = THIS
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 24)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=200000)
        except Exception as e:
            raise RuntimeError(f'seed {seed} eip {uc.reg_read(x.UC_X86_REG_EIP):x} esp {uc.reg_read(x.UC_X86_REG_ESP):x} count {count}') from e
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    ok = sum(1 for c in cases if c['after']['regs'][0] & 0xFF == 1)
    calls = sum(len(c['events']) for c in cases)
    if oracle.covered:
        # COVER=<file of "address: mnemonic" lines from Ghidra's disassembly of both functions>
        every = [int(line.split(':')[0], 16) for line in open(os.environ['COVER']) if line[:8].strip() and ':' in line[:10]]
        missing = [a for a in every if a not in oracle.covered]
        print(f'coverage {len(every) - len(missing)}/{len(every)} instructions; missing:', ' '.join(f'{a:x}' for a in missing[:80]))
    json.dump(dict(function='0x00E516F0', cases=cases), open(out, 'w'))
    print(f'{n} cases ({ok} ok), {calls} calls -> {out}')


main()
