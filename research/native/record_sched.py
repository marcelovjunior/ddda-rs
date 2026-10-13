"""L186 oracle: the job scheduler's three routines run as the original's code
in Unicorn: 0x00DBD8D0 run_jobs(worker) (cdecl), 0x00DBDC90 worker_thread
(stdcall, one argument) and 0x00DBE560 scheduler->run(mode) (thiscall,
`ret 4`).

The scheduler object (global 0x018D0B28) holds a random batch: job count,
counter, mode 0..5 (4 and 5 do nothing), job entries (object, function,
extra) calling stubs that pop what the original's call leaves for them
(0, 4, 4 or 8 bytes by mode), an optional profiler hook (pops 0x10). The
imports the routines call (SetEvent, ResetEvent, WaitForSingleObject,
WaitForMultipleObjects) are stubs recorded as events with their arguments;
InterlockedIncrement is emulated silently in place (the port does the same
and the memory shows it), and 0x00DC5C20 runs as the original's code. For
the join, 0x00DBD8D0 and the thread creation 0x00DBDF30 are stubs recorded
as events too. The wait stub sets the worker's stop flag after a random
number of waits so the thread procedure ends.
Output format: the one of crates/mtcore/tests/semantic_s6.rs (events with
before/after registers and memory), private.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

RUN, WORKER, JOIN = 0x00DBD8D0, 0x00DBDC90, 0x00DBE560
CREATE = 0x00DBDF30
GLOBAL = 0x018D0B28
# import slots -> (stub address, bytes popped, name)
IMPORTS = {
    0x0139D174: (0x30000010, 4, 'SetEvent'),
    0x0139D094: (0x30000020, 4, 'ResetEvent'),
    0x0139D1A4: (0x30000030, 8, 'WaitForSingleObject'),
    0x0139D240: (0x30000040, 16, 'WaitForMultipleObjects'),
    0x0139D0D0: (0x30000050, 4, 'InterlockedIncrement'),
}
INTERLOCKED = 0x30000050
HOOK = 0x30000060
JOBS = [0x30000100, 0x30000110, 0x30000120, 0x30000130]  # by batch mode
JOB_POP = {0x30000100: 0, 0x30000110: 4, 0x30000120: 4, 0x30000130: 8}
SCHED, WORKER_STRUCT, DATA = 0x20000000, 0x21000000, 0x26000000
SCHED_SIZE = 0x1D000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, entry):
        self.entry = entry
        self.covered = set()
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for va, data in h.image()[1]:
            lo, hi = va & ~0xFFF, (va + len(data) + 0xFFF) & ~0xFFF
            uc.mem_map(lo, hi - lo)
            uc.mem_write(va, data)
        uc.mem_map(0x30000000, 0x1000)
        for stub, _, _ in IMPORTS.values():
            uc.mem_write(stub, b'\xc3')
        for slot, (stub, _, _) in IMPORTS.items():
            uc.mem_write(slot, struct.pack('<I', stub))
        for base, size in ((SCHED, SCHED_SIZE), (WORKER_STRUCT, 0x1000), (DATA, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        uc.mem_write(GLOBAL, struct.pack('<I', SCHED))
        self.stubs = {s: (p, n) for s, p, n in IMPORTS.values()}
        self.stubs[HOOK] = (0x10, 'hook')
        for j in JOBS:
            self.stubs[j] = (JOB_POP[j], 'job')
        self.stubs[CREATE] = (0, 'create')
        if entry == JOIN:
            self.stubs[RUN] = (0, 'run_jobs')
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            a = address + i
            if a not in self.written:
                self.initial.setdefault(a, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def put(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def word(self, a):
        return struct.unpack('<I', self.uc.mem_read(a, 4))[0]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        if address == INTERLOCKED:
            ptr = self.word(esp + 4)
            self.remember(esp, 8)
            self.remember(ptr, 4)
            value = (self.word(ptr) + 1) & 0xFFFFFFFF
            uc.mem_write(ptr, struct.pack('<I', value))
            self.written.update(range(ptr, ptr + 4))
            uc.reg_write(x.UC_X86_REG_EAX, value)
            uc.reg_write(x.UC_X86_REG_EIP, self.word(esp))
            uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
            return
        if address not in self.stubs or (address == RUN and self.entry == RUN):
            self.covered.add(address)
            return
        pop, name = self.stubs[address]
        words = 1 + pop // 4
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * words))]
        if name == 'WaitForMultipleObjects':
            count, handles = self.word(esp + 4), self.word(esp + 8)
            window += [(handles + i, b) for i, b in enumerate(uc.mem_read(handles, 4 * count))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        effects = []
        eax = rng.choice((0, 1, 0x102, 0xFFFFFFFF)) if name != 'hook' else rng.randrange(1 << 32)
        if name == 'WaitForSingleObject':
            eax = 0 if rng.random() < 0.85 else rng.choice((0x102, 0xFFFFFFFF))
            self.waits_left -= 1
            if self.waits_left <= 0:
                effects += self.put(WORKER_STRUCT + 0x1C, bytes([rng.choice((1, 1, 0xFF))]))
        elif name == 'SetEvent' and rng.random() < 0.1:
            effects += self.put(WORKER_STRUCT + 0x1C, bytes([1]))
        elif name in ('job', 'hook', 'run_jobs', 'create'):
            for _ in range(rng.randrange(0, 3)):
                effects += self.put(DATA + rng.randrange(0, 0x100), bytes([rng.randrange(256)]))
            if name == 'job' and rng.random() < 0.05:
                # a job that changes the hook field of the batch (read again by the next job)
                effects += self.put(SCHED + 0x1C14C, struct.pack('<I', rng.choice((0, HOOK))))
            if name == 'run_jobs' and rng.random() < 0.5:
                effects += self.put(SCHED + 0x90, struct.pack('<I', rng.randrange(0, 5)))
        back = self.word(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(SCHED, b'\0' * SCHED_SIZE)
        uc.mem_write(WORKER_STRUCT, b'\0' * 0x1000)
        uc.mem_write(DATA, b'\0' * 0x1000)
        mode = rng.choice((0, 1, 2, 3, 0, 1, 2, 3, 4, 7))
        total = rng.choice((0, 1, 1, 2, 3, 5, 8, 12))
        counter = rng.choice((0, 0, 0, 1, total, total + 1, rng.randrange(0, 14)))
        uc.mem_write(SCHED + 0x8C, struct.pack('<I', total))
        uc.mem_write(SCHED + 0x90, struct.pack('<I', counter))
        uc.mem_write(SCHED + 0x94, struct.pack('<I', mode))
        stride = 8 if mode < 2 else 12
        for k in range(14):
            at = SCHED + 0xC0 + stride * k
            uc.mem_write(at, struct.pack('<I', 0x22000000 + 0x100 * k + rng.randrange(16)))
            uc.mem_write(at + 4, struct.pack('<I', JOBS[mode & 3]))
            if stride == 12:
                uc.mem_write(at + 8, struct.pack('<I', rng.randrange(1 << 32)))
        hook = rng.choice((0, HOOK, HOOK))
        uc.mem_write(SCHED + 0x1C14C, struct.pack('<I', hook))
        uc.mem_write(SCHED + 0x1C148, struct.pack('<I', 0x23000000 + rng.randrange(64)))
        # the worker records (join) and the worker's own struct (thread procedure)
        participants = rng.choice((1, 2, 3, 4, 5, 8, 8, 2))
        uc.mem_write(SCHED + 0x98, struct.pack('<I', participants))
        uc.mem_write(SCHED + 0x6D, bytes([rng.choice((0, 1, 1))]))
        for k in range(1, 8):
            record = SCHED + 0x1BDEC + 0x20 * (k - 1)
            uc.mem_write(record - 4, struct.pack('<I', 0x5000 + 8 * k))  # go event
            uc.mem_write(record, struct.pack('<I', 0x6000 + 8 * k))  # done event
            uc.mem_write(record + 0xC, bytes([rng.choice((0, 1, 1, 3, 2))]))
        uc.mem_write(WORKER_STRUCT + 8, struct.pack('<I', 0x7000 + rng.randrange(16)))
        uc.mem_write(WORKER_STRUCT + 0xC, struct.pack('<I', 0x7100 + rng.randrange(16)))
        uc.mem_write(WORKER_STRUCT + 0x10, struct.pack('<I', rng.randrange(0, 8)))
        stop_first = rng.random() < 0.1
        uc.mem_write(WORKER_STRUCT + 0x1C, bytes([1 if stop_first else 0]))
        self.waits_left = rng.choice((1, 2, 3, 5))
        self.initial, self.written, self.events = {}, set(), []
        entry = SP - 8 - 4 * rng.randrange(4)
        arg = WORKER_STRUCT if self.entry != JOIN else mode if rng.random() < 0.9 else rng.randrange(8)
        uc.mem_write(entry, struct.pack('<II', h.RET, arg))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = entry
        if self.entry == JOIN:
            start[1] = SCHED
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(entry, 8)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(self.entry, h.RET, count=400000)
        except Exception:
            print('seed', seed, 'eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()])
            raise
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if not (STACK <= a < esp)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    name, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    entry = {'run': RUN, 'worker': WORKER, 'join': JOIN}[name]
    oracle = Oracle(entry)
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    if len(sys.argv) > 4:
        starts = [int(l.split(':')[0], 16) for l in open(sys.argv[4]) if l[:8].strip() and l[8:9] == ':']
        missed = [hex(a) for a in starts if a not in oracle.covered]
        print(f'{len(starts) - len(missed)}/{len(starts)} instructions run; never run: {missed}')
    json.dump(dict(function=hex(entry), cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}', by)


main()
