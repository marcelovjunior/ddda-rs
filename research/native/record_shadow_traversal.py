"""L169 DB63E0 original-instruction oracle with explicit synthetic callees.

No game or shards. Original code/vectors stay private. Callees record full
entry state and random volatile returns; callbacks may mutate later nodes,
proving the traversal rereads their visibility masks.
Run with research Python through heavy.py.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, SIZE, STACK, STACK_SIZE, SP
import harness as h

ENTRY, LENGTH = 0xDB63E0, 0x35F
OWNER, DRAW, SCHED, LIST, NODES, VTABLE, FILTER = (BASE + n for n in (0, 0x2000, 0x4000, 0x21000, 0x24000, 0x30000, 0x31000))
CALLBACK = 0x1200000
# Callee stack cleanup from the original call sites and RET instructions.
POPS = {0x1147970: 4, 0xFED200: 8, 0x107FF00: 0, 0xDBE560: 4,
        0x107FF30: 0, 0xFED3B0: 8, 0x11479A0: 0, CALLBACK: 4}


class TraversalOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        pages = {ENTRY & ~4095, 0x18D0000, h.RET}
        pages.update(a & ~4095 for a in POPS)
        for address in pages: self.uc.mem_map(address, 4096)
        self.uc.mem_map(BASE, SIZE)
        self.uc.mem_map(STACK, STACK_SIZE)
        self.uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for address in POPS: self.uc.mem_write(address, b'\xc3')
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        self.uc.hook_add(UC_HOOK_CODE, self.code)

    def word(self, a, v): self.uc.mem_write(a, struct.pack('<I', v & 0xffffffff))

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address in POPS:
            state = self.snapshot()
            # OR at the virtual call leaves AF undefined. All other
            # boundaries follow CMP/ADD/SUB, defining arithmetic flags.
            state['flag_mask'] = 0xcc5 if address == CALLBACK else 0xcd5
            self.events.append(dict(kind='call', target=address, **state))
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            return_to = struct.unpack('<I', uc.mem_read(sp, 4))[0]
            changes = []
            if address == CALLBACK and self.mutate:
                node = uc.reg_read(x.UC_X86_REG_ECX)
                # Toggle visibility of the *next* node, not the saved next
                # pointer; the original must reread that node's mask.
                nxt = struct.unpack('<I', uc.mem_read(node + 12, 4))[0]
                if nxt:
                    a = nxt + 4
                    self.remember(a, 4)
                    value = struct.unpack('<I', uc.mem_read(a, 4))[0] ^ 0x800
                    self.word(a, value)
                    self.written.update(range(a, a + 4))
                    changes.extend((a + i, b) for i, b in enumerate(struct.pack('<I', value)))
            for reg in REGS[:3]: uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (self.rng.getrandbits(12) & 0xcd5))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 4 + POPS[address])
            self.events.append(dict(kind='return', effects=changes, **self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP, return_to)
            return
        assert ENTRY <= address < ENTRY + LENGTH, hex(address)
        self.visited.add(address)

    def run(self, seed, groups, width, mode, filter_kind, variant, mutate):
        uc = self.uc
        self.active = False
        self.rng = rng = random.Random(seed)
        self.mutate = mutate
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        self.word(0x18D0B28, SCHED)
        self.word(0x18D09E0, LIST)
        uc.mem_write(0x18D0BB0, bytes([mode == 1]))
        self.word(SCHED + 0x98, 1 if mode == 2 else 7)
        self.word(SCHED + 0x8c, seed % 3)
        self.word(LIST + 0x620, groups)
        self.word(VTABLE + 0x2c, CALLBACK)
        self.word(OWNER + 0xd38, 0x13579bdf)
        self.word(OWNER + 0xd3c, 0x2468ace0)
        shift = (0, 9, 10, 31, 32, 255)[variant % 6]
        uc.mem_write(DRAW + 0x168, bytes([shift]))
        uc.mem_write(DRAW + 0x16a, struct.pack('<H', 0x55aa))
        self.word(DRAW + 0x16c, 0x0008000f)
        for group in range(groups):
            enabled = group % 3 != 1 or variant == 0
            self.word(OWNER + 0x28 + group * 24, 4 if enabled else 0xfffffffB)
            first = NODES + group * 0x800
            self.word(LIST + 0x2c + group * 24, first if width else 0)
            uc.mem_write(FILTER + group, bytes([0 if filter_kind == 2 and group % 2 else 0xff]))
            for i in range(width):
                node = first + i * 32
                flags, layer, low, high = 0x03ff4802, 0x55a8, 0x13579bdf, 0x2468ace0
                rejection = (i + variant) % 9
                if rejection == 1: flags ^= 1
                if rejection == 2: flags ^= 0x800
                if rejection == 3: layer = 0x10000
                if rejection == 4: layer ^= 1
                if rejection == 5: flags &= 0xffff
                if rejection == 6: low ^= 0x80000000
                if rejection == 7: high ^= 1
                for off, value in ((0,VTABLE),(4,flags),(8,layer),(12,node+32 if i+1<width else 0),(24,low),(28,high)):
                    self.word(node+off,value)
        self.word(SP,h.RET)
        self.word(SP+4,DRAW)
        self.word(SP+8,FILTER if filter_kind else 0)
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,OWNER)
        uc.reg_write(x.UC_X86_REG_ESP,SP)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80 | ((seed%4)<<13))
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.events,self.visited = {},set(),[],set()
        before=self.snapshot(False)
        self.active=True
        uc.emu_start(ENTRY,h.RET,count=200000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET, 'instruction limit'
        self.active=False
        return dict(seed=seed,groups=groups,width=width,mode=mode,variant=variant,filter=filter_kind,mutate=mutate,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L169/shadow_traversal.json'))
    args=parser.parse_args()
    oracle,cases=TraversalOracle(),[]
    for mode in range(3):
        for groups,width in ((0,0),(1,0),(1,1),(1,9),(4,9),(4,32)):
            for filter_kind in range(3):
                for variant in range(6):
                    cases.append(oracle.run(len(cases)+1,groups,width,mode,filter_kind,variant,bool(variant%2)))
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(dict(entry=ENTRY,code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)),encoding='utf-8')
    print(f'{len(cases)} original returns; sha256={hashlib.sha256(args.out.read_bytes()).hexdigest()}',flush=True)


if __name__=='__main__': main()
