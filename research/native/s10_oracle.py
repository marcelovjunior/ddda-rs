"""L184 s10: shared Unicorn oracle for the semantic ports of 0x00C85A00,
0x007FFCC0 and 0x00B8A7F0. Runs the ORIGINAL code on randomized synthetic
guest memory; guest callees are stubs at their real addresses, recorded as
events in the format of crates/mtcore/tests/semantic_s10.rs:

  case = dict(seed, before=dict(regs), initial=[(addr, byte)],
              events=[dict(target, before=dict(regs, memory), after=dict(regs, memory))],
              after=dict(regs, memory=[(addr, byte)]))

regs order: eax ecx edx ebx esp ebp esi edi. `initial` holds every byte the
run read (before any write to it). `after.memory` holds every byte written
(function and stubs). Output is private (E:/ddda-re/notes/native-L184).
"""
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
M32 = 0xFFFFFFFF


class Oracle:
    """`entry`/`length`: the function's bytes (through its final RET incl.
    immediate). `stubs`: {address: (stack_args, behaviour)}; behaviour(oracle,
    regs, esp) -> eax, may call oracle.poke(). `maps`: [(address, size)] of
    data regions. Code pages of the function and stubs are mapped here."""

    def __init__(self, entry, length, stubs, maps, extra_code=()):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.entry, self.length, self.stubs = entry, length, stubs
        pages = {entry & ~0xFFF, (entry + length) & ~0xFFF}
        for a in stubs:
            pages.add(a & ~0xFFF)
        for a, n in extra_code:
            pages.update(range(a & ~0xFFF, ((a + n - 1) & ~0xFFF) + 0x1000, 0x1000))
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, n in maps:
            uc.mem_map(a, n)
        uc.mem_map(h.RET, 0x1000)
        uc.mem_write(entry, h.code(entry, length))
        for a in stubs:
            uc.mem_write(a, b'\xc3')
        for a, n in extra_code:
            uc.mem_write(a, h.code(a, n))
        uc.hook_add(UC_HOOK_MEM_READ, self._read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self._write)
        uc.hook_add(UC_HOOK_CODE, self._code)
        self.initial, self.written, self.events = {}, set(), []
        self.rng = None

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def _read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def _write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def poke(self, address, data):
        """A stub's write to guest memory."""
        self.remember(address, len(data))
        self.uc.mem_write(address, bytes(data))
        self.written.update(range(address, address + len(data)))
        self._stub_writes.update(range(address, address + len(data)))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def u32(self, a):
        return struct.unpack('<I', self.uc.mem_read(a, 4))[0]

    def _code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        stub = self.stubs.get(address)
        if stub is None:
            return
        nargs, behaviour = stub[:2]
        # a cdecl callee (the caller pops) has stub[2] more argument words in its window only
        extra = stub[2] if len(stub) > 2 else 0
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs + extra)))]
        before = dict(regs=self.regs(), memory=window)
        # optional extra memory the callee reads (stub[3](oracle, regs, esp) -> [(address, length)])
        if len(stub) > 3:
            before['checks'] = [(a + i, b) for a, n in stub[3](self, self.regs(), esp) for i, b in enumerate(uc.mem_read(a, n))]
        self._stub_writes = set()
        result = behaviour(self, self.regs(), esp)
        if isinstance(result, dict):
            # a native stub (real code at the address runs on): only record the call
            r = self.regs()
            r[4] = esp + 4 + 4 * nargs
            ev = dict(target=address, before=before, after=dict(regs=r, memory=[]))
            ev.update(result)
            self.events.append(ev)
            return
        eax = result & M32
        back = self.u32(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        # scribble the caller-saved registers: a port must not rely on them
        uc.reg_write(x.UC_X86_REG_ECX, self.rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_EDX, self.rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        effects = [(a, uc.mem_read(a, 1)[0]) for a in sorted(self._stub_writes)]
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def run(self, seed, regs, esp, args_words, pre_remember=()):
        """Run the function once: `regs` the initial 8 registers (ESP is
        overwritten), the stack at `esp` = return address + `args_words`."""
        uc = self.uc
        self.initial, self.written, self.events = {}, set(), []
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', w & M32) for w in args_words))
        start = list(regs)
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v & M32)
        self.remember(esp, 4 + 4 * len(args_words))
        for a, n in pre_remember:
            self.remember(a, n)
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=2000000)
        esp_after = uc.reg_read(x.UC_X86_REG_ESP)
        # stack scratch below the returned ESP is not part of the ABI
        scratch = lambda a: 0x0F000000 <= a < esp  # below the entry frame
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if not scratch(a)])
        initial = sorted((a, b) for a, b in self.initial.items() if not scratch(a))
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def finish(out, function, cases):
    json.dump(dict(function=function, cases=cases), open(out, 'w'))
    print(f'{len(cases)} cases, {sum(len(c["events"]) for c in cases)} calls -> {out}')
