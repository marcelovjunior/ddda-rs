"""L176 oracles: nDraw material parameter lookup 0x01066590 and its
binding writer 0x01066B10, run as the original's code in Unicorn on
synthetic material tables.

0x01066590 `this->find(slot, key)` (thiscall, `ret 8`): the `count`
indices at `this+0x78+4*slot` (count at `this+0x88+4*slot`) name 12-byte
entries at `this+0x30` whose first word holds a 12-bit key in bits 20..31;
a linear search for up to two indices, a binary search above. The cases
cover sorted, duplicate and unsorted tables, hits and misses, key words
with high bits set, and the binary search's wrap below index 0 (it then
reads the word before the index list). 0x01066B10 `this->bind(key, value)`
looks up slot 0 and fills the found entry from the table at 0x018D12CC.

Records every initial byte read, every final written byte and the
registers before and after (crates/mtdraw/tests/material.rs compares at
the ABI level). Reads the user's own exe; never runs the game; the output
is private (E:/ddda-re/notes/native-L176).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h

FIND, BIND = 0x1066590, 0x1066B10
LENGTHS = {FIND: 0xA7, BIND: 0x45}
GLOBAL_TABLE = 0x18D12CC
BASE, SIZE = 0xE000000, 0x80000
STACK, STACK_SIZE, SP = 0xF000000, 0x10000, 0xF008000
OWNER, LIST, ENTRIES, BINDINGS = (BASE + v for v in (0, 0x1000, 0x10000, 0x40000))
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0x1066000, 0x1000), (GLOBAL_TABLE & ~0xFFF, 0x1000),
                              (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        for entry, length in LENGTHS.items():
            uc.mem_write(entry, h.code(entry, length))
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)
            self.written.update(range(address, address + size))

    def snapshot(self, memory=True):
        uc = self.uc
        result = dict(regs=[uc.reg_read(r) for r in REGS])
        if memory:
            result['memory'] = [(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert any(e <= address < e + n for e, n in LENGTHS.items()), hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, entry, seed, count, mode, target):
        """mode: sorted | duplicates | unsorted; target: hit | below | above | between | high"""
        uc, rng = self.uc, random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        uc.mem_write(GLOBAL_TABLE & ~0xFFF, rng.randbytes(0x1000))
        words = {}
        slot = 0 if entry == BIND else seed % 4
        words[OWNER + 0x30] = ENTRIES
        words[OWNER + 0x78 + 4 * slot] = LIST
        words[OWNER + 0x88 + 4 * slot] = count
        total = max(count, 16) + 16
        keys = sorted(rng.sample(range(1, 0xFFF), total))
        if mode == 'duplicates':
            keys = sorted(rng.choice(keys[:max(count // 2, 1)]) for _ in range(total))
        order = list(range(total))
        if mode == 'unsorted':
            rng.shuffle(order)
        chosen = sorted(rng.sample(range(total), count)) if mode != 'unsorted' else order[:count]
        for k in range(total):
            words[ENTRIES + 12 * k] = (keys[k] << 20) | rng.getrandbits(20)
        for i, e in enumerate(chosen):
            words[LIST + 4 * i] = e
        # the word before the list: an entry index, read by the wrapped search
        words[LIST - 4] = rng.randrange(total)
        present = [keys[e] for e in chosen]
        if target == 'hit' and present:
            key = rng.choice(present)
        elif target == 'below':
            key = rng.randrange(0, min(keys))
        elif target == 'above':
            key = rng.randrange(max(keys) + 1, 0x1000)
        elif target == 'between' and present:
            gaps = [k for k in range(min(present), max(present)) if k not in set(present)]
            key = rng.choice(gaps) if gaps else rng.randrange(0x1000)
        else:
            key = rng.randrange(0x1000)
        key |= rng.getrandbits(20) << 12 if target == 'high' or seed % 3 == 0 else 0
        if entry == FIND:
            stack = [h.RET, slot, key]
        else:
            stack = [h.RET, key, rng.getrandbits(32)]
            words[GLOBAL_TABLE] = BINDINGS
        for i, v in enumerate(stack):
            words[SP + 4 * i] = v
        for a, v in words.items():
            uc.mem_write(a, struct.pack('<I', v))
        regs = [rng.getrandbits(32), OWNER, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for r, v in zip(REGS, regs):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.edges, self.last = {}, set(), set(), None
        before = self.snapshot(False)
        self.active = True
        try:
            uc.emu_start(entry, h.RET, count=20000)
        except UcError:
            self.active = False
            return None  # the wrapped search left the synthetic memory
        self.active = False
        if uc.reg_read(x.UC_X86_REG_EIP) != h.RET:
            return None  # the wrapped search did not end within the limit
        return dict(entry=entry, seed=seed, count=count, mode=mode, target=target, before=before,
                    initial=sorted(self.initial.items()), events=[], after=self.snapshot(),
                    edges=sorted(self.edges))


APPLY, APPLY_END, APPLY_CALLEE = 0x10655A0, 0x10658A2, 0x1062F20
SERIAL, THRESHOLD, SCALE = 0x18D2CE8, 0x1406CE0, 0x161C298
CONTEXT, SOURCE, MATERIAL = BASE + 0x10000, BASE + 0x8000, BASE + 0x100
LENGTHS[APPLY] = APPLY_END - APPLY


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def component(rng, threshold):
    """A colour component: the threshold itself, around it, NaN,
    infinities, denormals and zeros, values the conversion overflows."""
    t = struct.unpack('<f', struct.pack('<I', threshold))[0]
    pick = rng.randrange(10)
    if pick == 0:
        return threshold
    if pick == 1:
        return f32(rng.choice([float('inf'), float('-inf')]))
    if pick == 2:
        return 0x7FC00000 | rng.getrandbits(22) | (rng.getrandbits(1) << 31)
    if pick == 3:
        return rng.getrandbits(23) | (rng.getrandbits(1) << 31)
    if pick == 4:
        return f32(rng.uniform(-3e9, 3e9))
    return f32(rng.uniform(-0.5, 1.5) * t)


class StateOracle(Oracle):
    """0x010655A0 `this->apply(context, value)` (thiscall, `ret 8`): the
    material's states into the draw context's caches, then 0x01062F20
    (a guest call: stubbed, the stub records what it is given and returns
    a random EAX)."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0x1065000, 0x1000), (APPLY_CALLEE & ~0xFFF, 0x1000),
                              (SERIAL & ~0xFFF, 0x1000), (THRESHOLD & ~0xFFF, 0x1000),
                              (SCALE & ~0xFFF, 0x1000), (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(APPLY, h.code(APPLY, APPLY_END - APPLY))
        uc.mem_write(APPLY_CALLEE, bytes.fromhex('c20400'))  # the stub's own effect: ret 4
        for page in (THRESHOLD & ~0xFFF, SCALE & ~0xFFF):  # the float constants, as the image has them
            uc.mem_write(page, h.code(page, 0x1000))
        self.threshold = struct.unpack('<I', h.code(THRESHOLD, 4))[0]
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def snapshot(self, memory=True):
        result = super().snapshot(memory)
        result['xmm'] = [(self.uc.reg_read(x.UC_X86_REG_XMM0 + i)) & 0xFFFFFFFF for i in range(3)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address == APPLY_CALLEE:
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp, 8)
            self.events.append(dict(kind='call', target=address, **self.snapshot()))
            resume = struct.unpack('<I', uc.mem_read(sp, 4))[0]
            for reg in REGS[:3]:
                uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 8)  # thiscall, one argument
            self.events.append(dict(kind='return', **self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            return
        assert APPLY <= address < APPLY_END, hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, seed, warm, pending, records, count):
        """warm: None (the context's caches random) or the cache fields to
        change after a first, untraced run (every other cache then already
        holds what the material sets); records: whether bit 0x80 of the
        context's +0x5C is set; count: the material's source entries."""
        uc = self.uc
        rng = self.rng = random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        uc.mem_write(SERIAL & ~0xFFF, rng.randbytes(0x1000))
        this, ctx = MATERIAL, CONTEXT

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def byte(a, v):
            uc.mem_write(a, bytes([v & 0xFF]))

        def read(a):
            return struct.unpack('<I', uc.mem_read(a, 4))[0]

        value = rng.choice([rng.getrandbits(8), rng.getrandbits(32)])
        index = rng.getrandbits(12) & (0x3F if seed % 2 else 0xFFF)
        word(this + 0x2C, rng.getrandbits(20) << 12 | index)
        for k in range(4):
            word(this + 0x40 + 4 * k, component(rng, self.threshold))
        word(this + 0x18, (rng.getrandbits(23) << 9) | count)
        word(this + 0x30, SOURCE)
        for k in range(count):
            word(SOURCE + 12 * k, (rng.randrange(0x400) << 20) | rng.getrandbits(20))
        shader, blend, mode, extra = read(this + 0x10), read(this + 0x28), read(this + 0x14) & 0xFF, read(this + 0x24)
        if warm is not None:
            # Run once untraced: the context's caches then hold exactly what
            # the material sets; `warm` names the fields to change after.
            word(ctx + 0x1C30, 0)  # no pending record: the warm run only fills the caches
            self.events, self.edges, self.last = [], set(), None  # (not the previous case's)
            for i, v in enumerate((h.RET, ctx, value)):
                word(SP + 4 * i, v)
            uc.reg_write(x.UC_X86_REG_ESP, SP)
            uc.reg_write(x.UC_X86_REG_ECX, this)
            uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
            uc.emu_start(APPLY, h.RET, count=100000)
            fields = {'shader': (0x1CC, 4, 1), 'value': (0x169, 1, 1), 'tag': (0x1D0, 4, 1),
                      'blend': (0x11C, 4, 1), 'mode': (0x114, 1, 1), 'texture': (0x118, 4, 1),
                      'colour': (0x110, 4, 1 << rng.randrange(32)), 'extra': (0x120, 4, 1),
                      'flag20': (0x154, 4, 1 << 20), 'nibble': (0x154, 4, 1 << 3),
                      'high': (0x154, 4, 1 << 11), 'flag19': (0x154, 4, 1 << 19)}
            for name in warm:
                offset, size, bit = fields[name]
                old = int.from_bytes(uc.mem_read(ctx + offset, size), 'little')
                uc.mem_write(ctx + offset, (old ^ bit).to_bytes(size, 'little'))
        word(ctx + 0x1C30, pending)
        word(ctx + 0x1C34, rng.choice([0, rng.getrandbits(32)]))
        byte(ctx + 0x5C, (rng.getrandbits(8) | 0x80) if records else rng.getrandbits(7))
        word(ctx + 0x6C, rng.randrange(64))
        for i, v in enumerate((h.RET, ctx, value)):
            word(SP + 4 * i, v)
        regs = [rng.getrandbits(32), this, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(APPLY, h.RET, count=100000)
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=APPLY, seed=seed, warm=warm, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))


def branches(entry):
    return [i for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(h.code(entry, LENGTHS[entry]), entry)
            if i.mnemonic.startswith('j') and i.mnemonic != 'jmp']


def record_state(out):
    oracle = StateOracle()
    warms = [None, [], ['shader'], ['value'], ['tag'], ['blend'], ['mode'], ['texture'], ['colour'],
             ['extra'], ['flag20'], ['nibble'], ['high'], ['flag19'], ['shader', 'colour', 'flag19']]
    plan = [(warm, pending, records, count) for warm in warms for pending in (0, 1, 0xDEAD)
            for records in (False, True) for count in (0, 1, 7, 64) for _ in range(3)]
    cases = [oracle.run(i + 1, *p) for i, p in enumerate(plan)]
    edges = {tuple(e) for c in cases for e in c['edges']}
    missing = [(hex(i.address), hex(t)) for i in branches(APPLY)
               for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
    assert not missing, missing
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} state cases, {sum(len(c["events"]) for c in cases)} call boundaries -> {out}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', default='E:/ddda-re/notes/native-L176/material_lookup.json')
    p.add_argument('--state', action='store_true', help='record 0x010655A0 (material_state.json beside --out)')
    a = p.parse_args()
    if a.state:
        record_state(Path(a.out).with_name('material_state.json'))
        return
    oracle = Oracle()
    cases, skipped, seed = [], 0, 0
    for entry in (FIND, BIND):
        for count in (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 31, 64, 100, 255, 400):
            for mode in ('sorted', 'duplicates', 'unsorted'):
                for target in ('hit', 'below', 'above', 'between', 'high'):
                    for _ in range(3):
                        seed += 1
                        case = oracle.run(entry, seed, count, mode, target)
                        if case is None:
                            skipped += 1
                        else:
                            cases.append(case)
    for entry in (FIND, BIND):
        edges = {tuple(e) for c in cases if c['entry'] == entry for e in c['edges']}
        missing = [(hex(i.address), hex(t)) for i in branches(entry)
                   for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
        # A zero count returns at the first test, so the short search's
        # own zero test (0x10665BA) never takes its branch.
        expected = [('0x10665ba', '0x10665ec')] if entry == FIND else []
        assert missing == expected, (hex(entry), missing)
    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} cases ({skipped} wrapped searches left the synthetic memory, skipped) -> {out}')


if __name__ == '__main__':
    main()
