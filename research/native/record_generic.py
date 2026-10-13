"""L184 generic oracle for the mechanically translated ports (s4).

Runs one DDDA.exe function in Unicorn on randomised guest objects and
records the cases in the format of crates/mtcore/tests/semantic_s4.rs.
Guest callees are stubs (an address list with stack-argument counts and a
cleanup convention); math-library callees return in XMM0 and the recorded
events carry the XMM lanes they read ("xmm_in") and return ("xmm_out").
Absolute data the function reads (constants, tables) is mapped from the exe.

A target description is a Python module path given on the command line
defining: ENTRY, LENGTH (bytes through the final RET), STUBS {address:
(stack_args, cdecl, xmm_in [(reg, lane)], xmm_out [(reg, lane)])},
`fill(oracle, rng)` (build one case; returns (esp_extra_words list, regs
dict)), OBJECTS [(base, size)], plus optional `ret_words` (stack words
after the return address). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe:
    record_generic.py <target module file> out.json [cases]
"""
import importlib.util
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
import capstone  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
XMM = [getattr(x, f'UC_X86_REG_XMM{k}') for k in range(8)]
STACK, SP = 0x0F000000, 0x0F008000


class Oracle:
    def __init__(self, t):
        self.t = t
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def map_page(p):
            if p not in mapped:
                uc.mem_map(p, 0x1000)
                mapped.add(p)
        end = t.ENTRY + t.LENGTH
        for p in range(t.ENTRY & ~0xFFF, (end + 0xFFF) & ~0xFFF, 0x1000):
            map_page(p)
        for a in t.STUBS:
            map_page(a & ~0xFFF)
        md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        md.detail = True
        code = h.code(t.ENTRY, t.LENGTH)
        data_pages = set()
        for i in md.disasm(code, t.ENTRY):
            for op in i.operands:
                if op.type == capstone.x86.X86_OP_MEM and not op.mem.base and not op.mem.index and op.mem.disp >= 0x400000:
                    data_pages.add(op.mem.disp & ~0xFFF)
                    data_pages.add((op.mem.disp + 15) & ~0xFFF)
        for p in sorted(data_pages):
            if p not in mapped:
                map_page(p)
                uc.mem_write(p, h.code(p, 0x1000))
        for p in range(t.ENTRY & ~0xFFF, (end + 0xFFF) & ~0xFFF, 0x1000):
            pass
        uc.mem_write(t.ENTRY, code)
        for a in t.STUBS:
            uc.mem_write(a, bytes([0xC3]))
        for base, size in t.OBJECTS:
            uc.mem_map(base, (size + 0xFFF) & ~0xFFF)
        uc.mem_map(STACK, 0x10000)
        uc.mem_map(h.RET, 0x1000)
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        self.hit = set()
        self.insns = [i.address for i in md.disasm(code, t.ENTRY)]

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

    def lane(self, reg, lane):
        v = self.uc.reg_read(XMM[reg])
        return (v >> (32 * lane)) & 0xFFFFFFFF

    def set_lane(self, reg, lane, value):
        v = self.uc.reg_read(XMM[reg])
        v &= ~(0xFFFFFFFF << (32 * lane))
        v |= value << (32 * lane)
        self.uc.reg_write(XMM[reg], v)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        self.hit.add(address)
        if address not in self.t.STUBS:
            assert self.t.ENTRY <= address < self.t.ENTRY + self.t.LENGTH, hex(address)
            return
        spec = self.t.STUBS[address]
        nargs, cdecl, xin, xout = spec[:4]
        pointers = spec[4] if len(spec) > 4 else []
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        data = []
        for argument, length in pointers:
            pointer = struct.unpack('<I', uc.mem_read(esp + 4 + 4 * argument, 4))[0]
            if pointer:
                data += [(pointer + i, b) for i, b in enumerate(uc.mem_read(pointer, length))]
        if data:
            before['data'] = data
        if xin:
            before['xmm'] = [[r, l, self.lane(r, l)] for r, l in xin]
        rng = self.rng
        event = dict(target=address, before=before)
        out = self.t.stub(self, address, rng)
        effects = []
        for a, data_bytes in out.get('writes', []):
            self.remember(a, len(data_bytes))
            uc.mem_write(a, data_bytes)
            self.written.update(range(a, a + len(data_bytes)))
            effects += [(a + i, b) for i, b in enumerate(data_bytes)]
        xmm_after = []
        for r, l, v in out.get('xmm', []):
            self.set_lane(r, l, v)
            xmm_after.append([r, l, v])
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, out.get('eax', rng.randrange(1 << 32)))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + (0 if cdecl else 4 * nargs))
        uc.reg_write(x.UC_X86_REG_EIP, back)
        after = dict(regs=self.regs(), memory=effects)
        if xmm_after:
            after['xmm'] = xmm_after
        event['after'] = after
        self.events.append(event)
        if back == h.RET:
            uc.emu_stop()

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in self.t.OBJECTS:
            uc.mem_write(base, bytes(size))
        uc.mem_write(STACK, bytes(0x10000))
        self.initial, self.written, self.events = {}, set(), []
        regs, stack_words = self.t.fill(self, rng)
        esp = SP - 4 * (2 + len(stack_words)) - 4 * rng.randrange(4)
        self.entry_esp = esp
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', w) for w in stack_words))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for k, v in regs.items():
            start[k] = v
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        for r in XMM:
            uc.reg_write(r, rng.randrange(1 << 128))
        self.remember(esp, 4 + 4 * len(stack_words))
        before = dict(regs=self.regs())
        try:
            uc.emu_start(self.t.ENTRY, h.RET, count=2000000)
        except Exception:
            print('seed', seed, hex(uc.reg_read(x.UC_X86_REG_EIP)))
            raise
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    spec = importlib.util.spec_from_file_location('target', sys.argv[1])
    t = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(t)
    out = sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    o = Oracle(t)
    cases = [o.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    from collections import Counter
    print(sorted((hex(k), v) for k, v in Counter(e['target'] for c in cases for e in c['events']).items()))
    missed = [a for a in o.insns if a not in o.hit]
    print(f'{len(o.insns) - len(missed)}/{len(o.insns)} instructions covered; missed:', ' '.join(hex(a) for a in missed[:60]))
    json.dump(dict(function=hex(t.ENTRY), cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
