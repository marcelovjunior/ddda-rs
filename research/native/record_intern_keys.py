"""L184 oracle: 0x00E10FC0 `this->intern_keys(owner, values, keys, extra,
crc_out) -> hash` (thiscall, `ret 0x14`) with its REAL callees run inline
(the expansion 0x00E0EC50 and the key hash 0x00E0E5E0) and the real CRC
table of the exe, on random dependency banks (record_main_thread.recipe).
No events: the port makes no guest call. Private output
E:/ddda-re/notes/native-L184/intern_keys.json. Run with the pyenv python.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from record_main_thread import recipe, BASE, SIZE, OWNER, BANK, VALUES, INDICES, OUTPUT  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY = 0x00E10FC0
SP = 0x0F008000
OWNER_WORDS, KEYS, CRC_OUT = BASE + 0x300, BASE + 0x400, BASE + 0x500
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]

uc = Uc(UC_ARCH_X86, UC_MODE_32)
for page in range(0xE0E000, 0xE12000, 0x1000):
    uc.mem_map(page, 0x1000)
    uc.mem_write(page, h.code(page, 0x1000))
uc.mem_map(0x01407000, 0x1000)
uc.mem_write(0x01407000, h.code(0x01407000, 0x1000))
uc.mem_map(BASE, SIZE)
uc.mem_map(0x0F000000, 0x10000)
uc.mem_map(h.RET, 0x1000)
initial, written = {}, set()


def remember(address, size):
    for i, b in enumerate(uc.mem_read(address, size)):
        initial.setdefault(address + i, b)


uc.hook_add(UC_HOOK_MEM_READ, lambda u, a, address, size, v, d: remember(address, size))


def write(u, a, address, size, v, d):
    remember(address, size)
    written.update(range(address, address + size))


uc.hook_add(UC_HOOK_MEM_WRITE, write)
uc.hook_add(UC_HOOK_CODE, lambda u, a, s, d: u.emu_stop(), begin=h.RET, end=h.RET)


def case(seed):
    rng = random.Random(seed)
    initial.clear()
    written.clear()
    depth = rng.choice((0, 1, 2, 3, 4))
    width = rng.choice((1, 2, 3))
    terminals = rng.choice((0, 1, 2, 4, 8))
    words, halves, packed, output = recipe(seed, depth, width, terminals)
    uc.mem_write(BASE, rng.randbytes(SIZE))
    for a, v in words.items():
        uc.mem_write(a, struct.pack('<I', v))
    for a, v in halves.items():
        uc.mem_write(a, struct.pack('<H', v))
    nodes = max(1, len([a for a in words if BANK <= a < VALUES and (a - BANK) % 8 == 0]))
    for i in range(6):
        v = 0 if rng.random() < 0.3 else (rng.randrange(nodes) | (rng.getrandbits(20) << 12 if rng.random() < 0.5 else 0))
        uc.mem_write(OWNER_WORDS + 4 * i, struct.pack('<I', v))
    for i in range(4):
        uc.mem_write(KEYS + 4 * i, struct.pack('<I', 0 if rng.random() < 0.3 else rng.getrandbits(32)))
    crc = rng.random() < 0.6
    esp = SP - 4 * rng.randrange(4)
    uc.mem_write(esp, struct.pack('<IIIIII', h.RET, OWNER_WORDS, VALUES, KEYS, rng.getrandbits(32), CRC_OUT if crc else 0))
    start = [rng.getrandbits(32) for _ in range(8)]
    start[4], start[1] = esp, OWNER
    for r, v in zip(REGS, start):
        uc.reg_write(r, v)
    remember(esp, 24)
    before = dict(regs=[uc.reg_read(r) for r in REGS])
    uc.emu_start(ENTRY, h.RET, count=400000)
    after = dict(regs=[uc.reg_read(r) for r in REGS], memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(written)])
    return dict(seed=seed, before=before, initial=sorted(initial.items()), events=[], after=after)


cases = [case(s) for s in range(1500)]
json.dump(dict(function='0x00E10FC0', cases=cases), open('E:/ddda-re/notes/native-L184/intern_keys.json', 'w'))
print(len(cases), 'cases')
