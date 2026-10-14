"""L184 oracle: 0x00E0E5E0 `hash_key_words(words, byte_length, basis)` (stdcall,
`ret 0xc`), the original's code in Unicorn on random word buffers (lengths 0..70
words, byte lengths that are not multiples of 4 included). Private output
E:/ddda-re/notes/native-L184/key_hash.json. Run with the pyenv python."""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00E0E5E0, 0x6D
BUF, SP = 0x20000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
uc = Uc(UC_ARCH_X86, UC_MODE_32)
uc.mem_map(ENTRY & ~0xFFF, 0x1000)
uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
uc.mem_map(BUF, 0x1000)
uc.mem_map(0x0F000000, 0x10000)
uc.mem_map(h.RET, 0x1000)
initial = {}


def read(uc_, access, address, size, value, data):
    for i, b in enumerate(uc.mem_read(address, size)):
        initial.setdefault(address + i, b)


uc.hook_add(UC_HOOK_MEM_READ, read)
uc.hook_add(UC_HOOK_CODE, lambda u, a, s, d: u.emu_stop(), begin=h.RET, end=h.RET)


def case(seed):
    rng = random.Random(seed)
    initial.clear()
    n = rng.choice((0, 1, 2, 3, rng.randrange(4, 70)))
    length = n * 4 + rng.choice((0, 0, 0, 1, 2, 3))
    uc.mem_write(BUF, bytes(rng.randrange(256) for _ in range(0x1000)))
    esp = SP - 4 * rng.randrange(4)
    uc.mem_write(esp, struct.pack('<IIII', h.RET, BUF, length, rng.choice((0x811C9DC5, rng.getrandbits(32)))))
    start = [rng.getrandbits(32) for _ in range(8)]
    start[4] = esp
    for r, v in zip(REGS, start):
        uc.reg_write(r, v)
    for i, b in enumerate(uc.mem_read(esp, 16)):
        initial.setdefault(esp + i, b)
    before = dict(regs=[uc.reg_read(r) for r in REGS])
    uc.emu_start(ENTRY, h.RET)
    return dict(seed=seed, before=before, initial=sorted(initial.items()), events=[],
                after=dict(regs=[uc.reg_read(r) for r in REGS], memory=[]))


cases = [case(s) for s in range(2000)]
json.dump(dict(function='0x00E0E5E0', cases=cases), open('E:/ddda-re/notes/native-L184/key_hash.json', 'w'))
print(len(cases), 'cases')
