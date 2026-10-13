"""L184 s3 oracle: 0x00E12A20 `this->lookup(a, bank, slot, owner_values, keys,
extra, other_bank) -> entry` (thiscall, `ret 0x1c`), the shader/technique
lookup of every model primitive draw, run as the original code in Unicorn
together with ALL the code it calls on the common branch (no stubs):
0x00E10FC0 (interning), 0x00E0EC50 (expansion), 0x00E0E5E0 (hash),
0x00E0F3C0 (package key), 0x00E0F4B0 (package pair key), 0x01008200 and
0x01008260 (bucket lookups). Byte `this + 0x39` is always clear (the shader
creation branch is not covered). Synthetic data: dependency descriptors,
six shader packages with hash buckets and chains, an exclusion list. The
searched keys are computed here (an independent model of the interning) so
that every outcome is hit: package key, pair key, other bank, excluded.

Output format: crates/mtcore/tests/semantic_s3.rs.
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

ENTRY = 0x00E12A20
CODE = ((0x00E12A20, 0x890), (0x00E10FC0, 0x1E0), (0x00E0EC50, 0x310), (0x00E0E5E0, 0x70), (0x00E0F3C0, 0x90),
        (0x00E0F4B0, 0x70), (0x01008200, 0x60), (0x01008260, 0x70))
CRC_TABLE = 0x014075C0
THIS, THIS_SIZE = 0x20000000, 0xB0000
BANKS, BUCKETS = 0x21000000, 0x21010000
DESC, INDEX, VALUES, KEYS = 0x22000000, 0x22010000, 0x23000000, 0x24000000
PACKAGE = 0x26000000  # six packages, 0x10000 apart: object, table at +0x1000, node pool at +0x8000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
N_DESC = 6


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in CODE:
            mapping(address, size)
            uc.mem_write(address, h.code(address, size))
        mapping(CRC_TABLE, 0x400)
        uc.mem_write(CRC_TABLE, h.code(CRC_TABLE, 0x400))
        self.crc_table = struct.unpack('<256I', h.code(CRC_TABLE, 0x400))
        for address, size in ((THIS, THIS_SIZE), (BANKS, 0x4000), (BUCKETS, 0x4000), (DESC, 0x2000), (INDEX, 0x1000),
                              (VALUES, 0x1000), (KEYS, 0x1000), (PACKAGE, 6 * 0x10000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
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

    # --- an independent model of the interning (for the searched keys) ---
    def expand(self, packed):
        out = []
        high = packed & 0xFFFFF000
        if high:
            out.append(high)
        w0, indices = self.descriptors[packed & 0xFFF]
        children, terminals = (w0 >> 20) & 0x3F, w0 >> 26
        position = 0
        for _ in range(children):
            out += self.expand(self.values[indices[position]][1])
            position += 1
        for _ in range(terminals):
            v = self.values[indices[position]][1]
            position += 1
            if v:
                out.append(v)
        return out

    def intern(self, owner, keys, extra):
        words = []
        for slot in range(4):
            if keys[slot]:
                words.append((keys[slot] & 0xFFFFF000) | slot)
        words.append(extra)
        for field in (0, 3, 4, 2, 1, 5):
            if owner[field]:
                words += self.expand(owner[field])
        digest = 0x811C9DC5
        crc = 0xFFFFFFFF
        for w in words:
            salted = w ^ 0x14DA14DA
            for shift in (0, 8, 16, 24):
                digest = ((digest * 0x01000193) & 0xFFFFFFFF) ^ ((salted >> shift) & 255)
            for shift in (24, 16, 8, 0):
                crc = (crc >> 8) ^ self.crc_table[((w >> shift) ^ crc) & 0xFF]
        return digest, crc, len(words)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, THIS_SIZE), (BANKS, 0x4000), (BUCKETS, 0x4000), (DESC, 0x2000), (INDEX, 0x1000),
                           (VALUES, 0x1000), (KEYS, 0x1000), (PACKAGE, 6 * 0x10000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        # --- dependency descriptors and values (acyclic: a value's descriptor is above its referrer's)
        n_values = 12
        self.values = []
        for j in range(n_values):
            self.values.append((rng.randrange(1 << 32), (rng.choice((0, 0, rng.randrange(1 << 20) << 12)) | rng.choice((0, rng.randrange(N_DESC))))))
        self.descriptors = [None] * N_DESC
        for d in range(N_DESC - 1, -1, -1):
            above = [j for j in range(n_values) if (self.values[j][1] & 0xFFF) > d]
            children = rng.randrange(0, 3) if above else 0
            terminals = rng.randrange(0, 4)
            pool = list(range(n_values))
            indices = [rng.choice(above) for _ in range(children)] + [rng.choice(pool) for _ in range(terminals)]
            w0 = (children << 20) | (terminals << 26) | rng.randrange(1 << 20)
            self.descriptors[d] = (w0, indices)
        for d, (w0, indices) in enumerate(self.descriptors):
            w32(DESC + 8 * d, w0)
            w32(DESC + 8 * d + 4, INDEX + 0x40 * d)
            for k, v in enumerate(indices):
                uc.mem_write(INDEX + 0x40 * d + 2 * k, struct.pack('<H', v))
        for j, (a, b) in enumerate(self.values):
            w32(VALUES + 8 * j, a)
            w32(VALUES + 8 * j + 4, b)
        w32(THIS + 0x28, DESC)

        def owner_words():
            return [rng.choice((0, 0, rng.randrange(1 << 20) << 12 | rng.randrange(N_DESC))) for _ in range(6)]

        bank, slot = rng.randrange(0, 4), rng.randrange(0, 3)
        other = rng.choice((0, rng.randrange(1, 4), rng.randrange(1, 4)))
        w32(THIS + 0x24, BANKS)
        owners = {}
        for b in range(4):
            w32(BANKS + 4 * b, BUCKETS + 0x400 * b)
            for s in range(3):
                words = owner_words()
                owners[(b, s)] = words
                for k, v in enumerate(words):
                    w32(BUCKETS + 0x400 * b + s * 0x28 + 0x1C + 4 * k, v)
        keys = [rng.choice((0, rng.randrange(1 << 32))) for _ in range(4)]
        extra = rng.randrange(1 << 32)
        for k, v in enumerate(keys):
            w32(KEYS + 4 * k, v)
        first = self.intern(owners[(bank, slot)], keys, extra)
        second = self.intern(owners[(other, 0)], keys, extra) if other else None
        # --- packages: noise nodes, and entries for the searched keys
        scenario = rng.choice(('single', 'pair', 'other', 'none', 'excluded', 'single_flagged', 'excluded_pair', 'flagged_pair', 'flagged_pair', 'other_flagged'))
        targets = []  # (key, crc, index word)
        if scenario in ('single', 'excluded'):
            targets.append((first[0], rng.randrange(1 << 32), rng.randrange(64)))
        if scenario == 'single_flagged':
            targets.append((first[0], rng.randrange(1 << 32), 0x80000000 | rng.randrange(64)))
        if scenario == 'pair':
            targets.append((first[0], first[1], rng.randrange(64)))
            if rng.random() < 0.4:  # a hit by key alone is found first by the single-key search
                targets.append((first[0], rng.randrange(1 << 32), rng.randrange(64)))
        if scenario == 'excluded_pair':
            targets.append((first[0], first[1], rng.randrange(64) | rng.choice((0, 0x80000000))))
        if scenario == 'flagged_pair':  # the key-only search stops at the flagged node, the pair search goes on
            targets.append((first[0], rng.randrange(1 << 32), 0x80000000 | rng.randrange(64)))
            targets.append((first[0], first[1], rng.randrange(64) | rng.choice((0, 0x80000000))))
        if scenario in ('other', 'other_flagged') and second:
            targets.append((second[0], second[1], rng.randrange(64) | (0x80000000 if scenario == 'other_flagged' else 0)))
        package_pointers = []
        for p in range(6):
            base = PACKAGE + 0x10000 * p
            if rng.random() < 0.2:
                package_pointers.append(0)
                continue
            package_pointers.append(base)
            table = base + 0x1000
            pool = base + 0x8000
            w32(base + 0x84, table)
            w32(table + 0x18, pool)
            nodes = []
            for _ in range(rng.randrange(0, 12)):
                key = rng.randrange(1 << 32)
                if nodes and rng.random() < 0.5:
                    key = (rng.choice(nodes)[0] & ~0xFFF) | (key & 0xFFF)  # not used; keeps shape
                nodes.append((key, rng.randrange(1 << 32), rng.randrange(64) | rng.choice((0, 0, 0x80000000))))
                if rng.random() < 0.3:  # same bucket as the first node
                    nodes.append(((nodes[0][0] & 0xFFF) | (rng.randrange(1 << 20) << 12), rng.randrange(1 << 32), rng.randrange(64)))
            if scenario in ('flagged_pair',) and p == 2:
                position = rng.randrange(len(nodes) + 1)
                nodes[position:position] = targets
            elif targets and rng.random() < 0.7:
                nodes.insert(rng.randrange(len(nodes) + 1), rng.choice(targets))
            if targets and p == 0 and scenario != 'flagged_pair':
                nodes.append(targets[0])
            heads = {}
            for k, (key, crc, index) in enumerate(nodes, start=1):
                node = pool + 16 * k
                w32(node, key)
                w32(node + 4, crc)
                w32(node + 12, index)
                bucket = key & 0xFFF
                if bucket in heads:
                    # append at the end of the chain
                    tail = heads[bucket]
                    while True:
                        nxt = struct.unpack('<I', bytes(uc.mem_read(tail + 8, 4)))[0]
                        if nxt == 0:
                            break
                        tail = pool + 16 * nxt
                    w32(tail + 8, k)
                else:
                    heads[bucket] = node
                    w32(table + 0x1C + 4 * bucket, node)
        for i in range(6):
            w32(THIS + 0xAB104 + 4 * i, package_pointers[i])
        excluded = [rng.randrange(1 << 32) for _ in range(rng.randrange(0, 4))]
        if scenario in ('excluded', 'excluded_pair'):
            excluded.insert(rng.randrange(len(excluded) + 1), first[0])
        for i, v in enumerate(excluded):
            w32(THIS + 0xAB2A0 + 4 * i, v)
        w32(THIS + 0xAC2A0, len(excluded))
        uc.mem_write(THIS + 0x39, bytes([0]))
        uc.mem_write(THIS + 0xAB229, bytes([rng.choice((0, 0, 1))]))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        args = [rng.randrange(1 << 32), bank, slot, VALUES, KEYS, extra, other]
        esp = SP - 4 * rng.randrange(0, 4) - 4 * 8
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', a) for a in args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4 + 4 * len(args))
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, scenario=scenario, before=before, initial=sorted(self.initial.items()), events=[], after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    from collections import Counter
    json.dump(dict(function='0x00E12A20', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}')
    print(Counter((c['scenario'], c['after']['regs'][0] != 0) for c in cases))
    print('instructions per case:', sum(c['insns'] for c in cases) / n)


main()
