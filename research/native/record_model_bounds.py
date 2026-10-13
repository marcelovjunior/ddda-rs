"""L184 s3 oracle: 0x00FFA620 `model->update_bounds(visible_only)` run as the
original code in Unicorn on randomized synthetic models (joint tables,
radii, parents, basis rows with NaN/inf/zero, both the skinned and the plain
variant). The only callee, 0x00EA3D00 (joint count 0), is a recorded stub.
Output format: crates/mtcore/tests/semantic_s3.rs.
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

ENTRY, LENGTH = 0x00FFA620, 0xAD2  # through RET 4 at 0x00FFB0EF
CALLEE = 0x00EA3D00
MODEL, MODEL_SIZE = 0x20000000, 0x1000
RES, RADII, JOINTS = 0x21000000, 0x21001000, 0x21010000
CONSTANT_RANGES = ((0x01629560, 0x90), (0x01406CE0, 0x20))
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
NAN = struct.unpack('<f', struct.pack('<I', 0x7FC00000))[0]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((ENTRY, LENGTH), (CALLEE, 1), (MODEL, MODEL_SIZE), (RES, 0x100), (RADII, 0x1000),
                              (JOINTS, 0x2000), (STACK, 0x10000), (h.RET, 0x1000)) + CONSTANT_RANGES:
            mapping(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(CALLEE, b'\xc3')
        for address, size in CONSTANT_RANGES:
            uc.mem_write(address, h.code(address, size))
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

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address != CALLEE:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4))])
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, self.rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    def number(self, rng, spread=3.0):
        # Unicorn (QEMU softfloat) picks the NaN of an operation with two NaN
        # operands by the x87 rule (larger payload), not SSE's (first operand),
        # so a case holds either +NaN inputs or infinities (whose default NaN
        # is the negative one), never both kinds.
        if self.mode == 'zero':  # signed zeros against the strict/non-strict comparisons
            return rng.choice((0.0, -0.0, 0.0, -0.0, 1.0, -1.0))
        r = rng.random()
        if r < 0.02 and self.mode == 'nan':
            return NAN
        if r < 0.04 and self.mode == 'inf':
            return rng.choice((float('inf'), float('-inf')))
        if r < 0.12:
            return rng.choice((0.0, -0.0, 1.0, -1.0, 1e-40))
        return rng.uniform(-spread, spread)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((MODEL, MODEL_SIZE), (RES, 0x100), (RADII, 0x1000), (JOINTS, 0x2000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def wf(a, v):
            uc.mem_write(a, struct.pack('<f', v))

        count = rng.choice((0, 1, 1, 2, 3, 4, 5, 6, 6))
        w32(MODEL + 0xF0, 0 if rng.random() < 0.05 else RES)
        w32(MODEL + 0x360, count)
        w32(MODEL + 0x364, JOINTS)
        uc.mem_write(MODEL + 0x108, bytes([rng.choice((0x40, 0x40, 0, 0x41, rng.randrange(256)))]))
        w32(RES + 0x60, RADII)
        self.mode = rng.choice(('nan', 'inf', 'none', 'zero'))
        rough = self.mode in ('nan', 'inf', 'zero') and rng.random() < 0.6
        for k in range(count):
            wf(RADII + 4 + 0x18 * k, abs(self.number(rng, 2.0)) if rng.random() < 0.9 else self.number(rng))
            joint = JOINTS + 0xA0 * k
            for off in range(0x10, 0x40, 4):  # basis rows
                wf(joint + off, self.number(rng, 2.0) if rough else rng.uniform(-2, 2))
            for off in (0x40, 0x44, 0x48):
                wf(joint + off, self.number(rng, 10.0) if rough else rng.uniform(-10, 10))
            uc.mem_write(joint + 0x91, bytes([rng.choice((0xFF, rng.randrange(count)))]))
            uc.mem_write(joint + 0x96, bytes([rng.randrange(256)]))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(1, 5) - 4
        visible_only = rng.choice((0, 1, 5))
        uc.mem_write(esp, struct.pack('<II', h.RET, visible_only))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = MODEL, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00FFA620', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}; original instructions per case: mean {sum(c["insns"] for c in cases) / n:.0f}')


main()
