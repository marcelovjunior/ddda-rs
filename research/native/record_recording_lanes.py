"""L172 P2a oracle: the recording-lane machinery of the parallel shadow
branch, run as the original code in Unicorn on synthetic contexts.

  init   0x00FED200  context->init_lanes(lanes, count)   (thiscall, ret 8)
  merge  0x00FED3B0  context->merge_lanes(lanes, count)  (thiscall, ret 8)

The CRT memset (0x012EFE20) and memcpy (0x012EFEA0) are not original code
here: they are performed by the hook (memcpy with memmove semantics) and
their effects are part of the recorded memory; the semantic ports do the
same natively and make no guest calls.
Output format: crates/mtcore/tests/semantic_p2a.rs.
Usage: record_recording_lanes.py MODE OUT.json [cases]
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; the output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

MODES = {'init': (0x00FED200, 0x1A5), 'merge': (0x00FED3B0, 0xCC)}
MEMSET, MEMCPY = 0x012EFE20, 0x012EFEA0
STRIDE = 0x33C84
THIS, THIS_SIZE = 0x20000000, 0x40000
LANES, LANES_SIZE = 0x21000000, 0x150000
BUFFERS = 0x23000000  # command buffers and nested arrays
GLOBAL_12CC, GLOBAL_08F4 = 0x018D12CC, 0x018D08F4
G12, RENDER = 0x24000000, 0x25000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, mode):
        self.mode = mode
        self.entry, length = MODES[mode]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((self.entry, length), (THIS, THIS_SIZE), (LANES, LANES_SIZE), (BUFFERS, 0x100000), (G12, 0x1000),
                              (RENDER, 0x434000), (GLOBAL_12CC, 4), (GLOBAL_08F4, 4), (MEMSET, 1), (MEMCPY, 1),
                              (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        uc.mem_write(self.entry, h.code(self.entry, length))
        uc.mem_write(MEMSET, b'\xc3')
        uc.mem_write(MEMCPY, b'\xc3')
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

    def put(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, bytes(data))
        self.written.update(range(address, address + len(data)))

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address in (MEMSET, MEMCPY):
            esp = uc.reg_read(x.UC_X86_REG_ESP)
            dst, a, n = struct.unpack('<III', uc.mem_read(esp + 4, 12))
            if address == MEMSET:
                self.put(dst, bytes([a & 0xFF]) * n)
            else:
                self.remember(a, n)
                self.put(dst, bytes(uc.mem_read(a, n)))
            uc.reg_write(x.UC_X86_REG_EAX, dst)
            uc.reg_write(x.UC_X86_REG_ESP, esp + 4)  # cdecl: `ret`, the caller pops the arguments
            uc.reg_write(x.UC_X86_REG_EIP, struct.unpack('<I', uc.mem_read(esp, 4))[0])
            return
        assert self.entry <= address < self.entry + 0x200, hex(address)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, THIS_SIZE), (LANES, LANES_SIZE), (BUFFERS, 0x100000), (G12, 0x1000), (RENDER, 0x434000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def noise(address, size):
            uc.mem_write(address, rng.randbytes(size))

        count = rng.randrange(1, 5)
        if self.mode == 'init':
            w32(THIS + 0x4C, rng.randrange(0, 5000))
            w32(THIS + 0x54, rng.randrange(1 << 32))
            commands = rng.randrange(0, 40)
            w32(THIS + 0x14, commands)
            noise(THIS + 0xB4, commands * 8 + 0x144)
            w32(GLOBAL_12CC, G12)
            for off in (0x24, 0x28, 0x34):
                w32(G12 + off, rng.randrange(1 << 32))
            w32(GLOBAL_08F4, RENDER)
            w32(RENDER + 0x433E04, rng.randrange(1 << 32))
            w32(RENDER + 0x433E08, rng.randrange(1 << 32))
            w32(RENDER + 0x33DBC, rng.randrange(1 << 32))
            for k in range(3):
                w32(RENDER + 0x433DEC + 4 * k, rng.randrange(1 << 32))
                w32(RENDER + 0x433DF8 + 4 * k, rng.randrange(1 << 32))
            for i in range(count):  # stale lane contents the init overwrites in part
                noise(LANES + i * STRIDE, 0x400)
                noise(LANES + i * STRIDE + 0x33000, 0xC00)
        else:
            merged_buffer = BUFFERS
            w32(THIS + 0x44, merged_buffer)
            w32(THIS + 0x48, rng.randrange(0, 20))
            w32(THIS + 0x4C, 0x2000 + rng.randrange(0, 1000))
            w32(THIS + 0x30C78, rng.randrange(0, 6))
            for k in range(8):
                w32(THIS + 0x30878 + 4 * k, rng.randrange(1 << 32))
            arrays = 0x40000  # nested arrays of the lanes
            for i in range(count):
                lane = LANES + i * STRIDE
                source = BUFFERS + 0x20000 + 0x2000 * i
                commands = rng.randrange(0, 40)
                w32(lane + 0x44, source)
                w32(lane + 0x48, commands)
                noise(source, commands * 8)
                nested = rng.randrange(0, 4)
                w32(lane + 0x30C78, nested)
                for k in range(nested):
                    record = BUFFERS + arrays
                    arrays += 0x100
                    length = rng.randrange(0, 12)
                    data = BUFFERS + arrays
                    arrays += 0x100
                    w32(lane + 0x30878 + 4 * k, record)
                    w32(record + 8, data)
                    w32(record + 0xC, length)
                    noise(data, length * 8)
        self.initial, self.written, self.insns = {}, set(), 0
        esp = SP - 4 * rng.randrange(0, 4) - 12
        uc.mem_write(esp, struct.pack('<III', h.RET, LANES, count))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=500000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, count=count, before=before, initial=sorted(self.initial.items()), events=[], after=after, insns=self.insns)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 1500
    oracle = Oracle(mode)
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function=hex(MODES[mode][0]), cases=cases), open(out, 'w'))
    print(f'{mode}: {n} cases -> {out}; instructions per case {sum(c["insns"] for c in cases) / n:.0f}')


main()
