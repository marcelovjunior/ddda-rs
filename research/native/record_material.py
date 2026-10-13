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


def branches(entry):
    return [i for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(h.code(entry, LENGTHS[entry]), entry)
            if i.mnemonic.startswith('j') and i.mnemonic != 'jmp']


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', default='E:/ddda-re/notes/native-L176/material_lookup.json')
    a = p.parse_args()
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
