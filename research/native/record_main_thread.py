"""L130 E0EC50 oracle: original instructions, complete recursive execution.

Only maps the original function's code page and synthetic memory. Records
all touched initial bytes, every final written byte, and machine/memory
state at each recursive call AND return. Never runs the game. Private output.
Run with the research Python through heavy.py.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

ENTRY = 0xE0EC50
BASE, SIZE = 0xE000000, 0x40000
STACK, STACK_SIZE, SP = 0xF000000, 0x10000, 0xF008000
OWNER, BANK, VALUES, INDICES, OUTPUT = (BASE + v for v in (0, 0x1000, 0x5000, 0x9000, 0x20000))
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX','ECX','EDX','EBX','ESP','EBP','ESI','EDI')]


def recipe(seed, depth, width, terminals, alias=False):
    rng = random.Random(seed)
    words, halves = {OWNER+0x28:BANK}, {}
    next_node, next_index, next_value = 0, INDICES, 0

    def node(level):
        nonlocal next_node, next_index, next_value
        index = next_node
        next_node += 1
        children = width if level and next_node < 180 else 0
        count = terminals if index % 3 == 0 else rng.randrange(min(terminals, 4)+1)
        start = next_index
        next_index += 2*(children+count)
        words[BANK+8*index] = (children << 20) | (count << 26) | rng.randrange(1 << 20)
        words[BANK+8*index+4] = start
        for slot in range(children+count):
            value_index = next_value
            next_value += 1
            halves[start+slot*2] = value_index
            if slot < children:
                child = node(level-1)
                value = child | (rng.randrange(1 << 20) << 12 if rng.randrange(2) else 0)
            else:
                value = rng.getrandbits(32) if rng.randrange(3) else 0
            words[VALUES+8*value_index+4] = value
        return index

    root = node(depth)
    assert next_node < 0x800 and next_index < OUTPUT and next_value < 0x800
    packed = root | (0xfffff000 if seed % 2 else 0)
    # Output over the value table is legal: it changes subsequent reads.
    output = VALUES+32 if alias else OUTPUT
    return words, halves, packed, output


class Oracle:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        self.uc.mem_map(0xE0E000, 0x1000)
        self.uc.mem_write(ENTRY, h.code(ENTRY, 0x306))
        self.uc.mem_map(BASE, SIZE)
        self.uc.mem_map(STACK, STACK_SIZE)
        self.uc.mem_map(h.RET, 0x1000)
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        self.uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address+i, b)

    def read(self, uc, access, address, size, value, data):
        if self.active: self.remember(address,size)

    def write(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address,size)
            self.written.update(range(address,address+size))

    def snapshot(self, memory=True):
        uc = self.uc
        result = dict(regs=[uc.reg_read(r) for r in REGS], flags=uc.reg_read(x.UC_X86_REG_EFLAGS)&0xCD5,
                      xmm=[[(uc.reg_read(x.UC_X86_REG_XMM0+i) >> (32*j))&0xffffffff for j in range(4)] for i in range(8)],
                      mxcsr=uc.reg_read(x.UC_X86_REG_MXCSR),
                      x87=[uc.reg_read(getattr(x, 'UC_X86_REG_'+name)) for name in ('FPCW','FPSW','FPTAG')]+[uc.reg_read(x.UC_X86_REG_FP0+i) for i in range(8)])
        if memory: result['memory'] = [(a,uc.mem_read(a,1)[0]) for a in sorted(self.written)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert ENTRY <= address <= 0xE0EF53, hex(address)
        self.visited.add(address)
        if self.last is not None: self.edges.add((self.last,address))
        self.last=address
        if address == ENTRY:
            if self.started: self.events.append(dict(kind='call', **self.snapshot()))
            self.started = True
        elif address == 0xE0EDD6:
            self.events.append(dict(kind='return', **self.snapshot()))

    def run(self, seed, depth, width, terminals, alias=False):
        uc = self.uc
        self.active = False
        rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        words, halves, packed, output = recipe(seed,depth,width,terminals,alias)
        for a,v in words.items(): uc.mem_write(a,struct.pack('<I',v))
        for a,v in halves.items(): uc.mem_write(a,struct.pack('<H',v))
        uc.mem_write(SP,struct.pack('<4I',h.RET,packed,VALUES,output))
        regs=[rng.getrandbits(32),OWNER,rng.getrandbits(32),rng.getrandbits(32),SP,
              rng.getrandbits(32),rng.getrandbits(32),rng.getrandbits(32)]
        for r,v in zip(REGS,regs): uc.reg_write(r,v)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0)  # eight nonempty finite sentinels
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1 << 63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xCD5))
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        self.edges, self.last = set(), None
        before=self.snapshot(False)
        self.started=False
        self.active=True
        uc.emu_start(ENTRY,h.RET,count=2000000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET, 'instruction limit'
        self.active=False
        return dict(seed=seed,depth=depth,width=width,terminals=terminals,alias=alias,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited),edges=sorted(self.edges))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',default='E:/ddda-re/notes/native-L130/dependencies.json')
    a=p.parse_args()
    oracle=Oracle()
    cases=[]
    for depth in range(13):
        for width in (1,2):
            for terminals in (0,1,3):
                seed=len(cases)+1
                cases.append(oracle.run(seed,depth,width,terminals))
    for depth in range(5):
        for terminals in (0,1,63):
            cases.append(oracle.run(len(cases)+1,depth,1,terminals))
    # Deterministic output/value overlap without introducing cycles.
    for seed in range(201,217): cases.append(oracle.run(seed,0,1,63,True))
    for terminals in (0,1,3): cases.append(oracle.run(len(cases)+1,1,63,terminals))
    out=Path(a.out); out.parent.mkdir(parents=True,exist_ok=True)
    edges=set(tuple(e) for c in cases for e in c['edges'])
    branches=[i for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(h.code(ENTRY,0x306),ENTRY)
              if i.mnemonic.startswith('j') and i.mnemonic!='jmp']
    missing=[(hex(i.address),hex(target)) for i in branches
             for target in (i.address+i.size,int(i.op_str,16)) if (i.address,target) not in edges]
    assert not missing, ('uncovered conditional edges',missing)
    result=dict(source='DDDA.exe 2017-12-13 Steam',address=ENTRY,
                conditional_edges=2*len(branches),exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),cases=cases)
    out.write_text(json.dumps(result,separators=(',',':')))
    print(f'{len(cases)} cases, {sum(len(c["events"]) for c in cases)} boundaries, {len(set().union(*(set(c["visited"]) for c in cases)))} instructions -> {out}',flush=True)


if __name__=='__main__': main()
