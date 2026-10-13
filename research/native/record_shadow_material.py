"""Original EA5B50 caller oracle with explicit external-call ABI fixtures.

Private synthetic vectors; no game run. Allocation and lighting/material
callees are seams, not reimplementations. The original caller instructions
execute in Unicorn. All 112 lifted basic-block starts are exercised.
"""
import hashlib
import json
from pathlib import Path
import random
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, STACK, STACK_SIZE, SP
import harness as h

ENTRY,LENGTH = 0xea5b50,0xcef
OWNER,TABLE,DESCRIPTORS,DESC,RESOURCES,DEFAULTS,BUFFER = [BASE+v for v in
    (0x40000,0x50000,0x60000,0x65000,0x70000,0x75000,0x80000)]
SEAMS = {0xfed890:4,0xfe66a0:16,0xff1310:4,0x12efea0:0,0xe100000:16}


class MaterialOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        pages={0xea5000,0xea6000,0x18d2000,0x1406000,h.RET}
        pages.update(a&~4095 for a in SEAMS if not BASE <= a < BASE+0x100000)
        for page in pages: self.uc.mem_map(page,4096)
        self.uc.mem_map(BASE,0x100000)
        self.uc.mem_map(STACK,STACK_SIZE)
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH))
        self.uc.mem_write(0x1406ce0,h.code(0x1406ce0,4))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self,uc,address,size,data):
        if address in SEAMS:
            self.calls.append(dict(target=address,**self.snapshot()))
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp,4+max(SEAMS[address],12 if address==0x12efea0 else 0))
            ret=struct.unpack('<I',uc.mem_read(sp,4))[0]
            def word(a):
                self.remember(a,4)
                return struct.unpack('<I',uc.mem_read(a,4))[0]
            def store(a,v):
                self.remember(a,4);self.written.update(range(a,a+4))
                uc.mem_write(a,struct.pack('<I',v&0xffffffff))
            if address==0xfed890:
                owner=uc.reg_read(x.UC_X86_REG_ECX)
                count=(word(sp+4)+4095)&~4095
                self.allocations+=1
                dest=BUFFER+self.allocations*0x1000
                store(owner+0x24,dest);store(owner+0x28,dest+count)
                uc.reg_write(x.UC_X86_REG_EAX,dest)
            elif address==0x12efea0:
                dst,src,count=word(sp+4),word(sp+8),word(sp+12)
                for i in range(count):
                    self.remember(src+i,1);self.remember(dst+i,1)
                    self.written.add(dst+i);uc.mem_write(dst+i,bytes(uc.mem_read(src+i,1)))
                uc.reg_write(x.UC_X86_REG_EAX,dst)
            # Lighting seams deliberately preserve caller state, ret N.
            uc.reg_write(x.UC_X86_REG_ESP,sp+4+SEAMS[address])
            uc.reg_write(x.UC_X86_REG_EIP,ret)
            return
        assert ENTRY <= address < ENTRY+LENGTH, hex(address)
        self.visited.add(address)

    def run(self,seed,mode='normal'):
        uc,rng=self.uc,random.Random(seed)
        self.active=False
        uc.mem_write(BASE,rng.randbytes(0x100000))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        def word(a,v): uc.mem_write(a,struct.pack('<I',v&0xffffffff))
        def half(a,v): uc.mem_write(a,struct.pack('<H',v))
        word(BASE+0x18,DESCRIPTORS);word(BASE+0x20,TABLE)
        word(BASE+0x24,BUFFER);word(BASE+0x28,BUFFER+(0 if seed&1 else 4096))
        word(BASE+0x5c,0x80 if seed&2 else 0)
        word(BASE+0x6c,seed%3)
        flags=(0x80000000 if seed&4 else 0)|(0x40000000 if seed&8 else 0)|(rng.randrange(16)<<25)
        word(OWNER+0x108,flags)
        word(OWNER+0x30,RESOURCES+0x3000 if seed&128 else 0)
        half(BASE+0x16a,(0,1,2,0x10,0x200,0x800,0xffff)[seed%7])
        word(BASE+0x1d4,RESOURCES if seed&16 else 0)
        for i in range(4096): word(DESCRIPTORS+i*4,DESC)
        half(DESC+0x18,(3,12,16,32)[seed%4]);word(DESC+0x24,DEFAULTS)
        for offset in (0x210,0x214,0x218,0x21c,0x264):
            word(RESOURCES+offset,RESOURCES+0x1000 if seed&32 else 0)
        singleton=RESOURCES+0x2000
        word(0x18d2c80,singleton);word(0x18d2ce8,rng.getrandbits(32))
        word(singleton,singleton+0x1000);word(singleton+0x1000+0x34,0xe100000)
        word(singleton+0x2cc,RESOURCES+0x1000 if seed&64 else 0)
        # Independently choose cached/mismatched state, including pointer nulls.
        for index,(dest,source) in enumerate(((0x2868,0x2670 if flags&0x80000000 else 0x2678),
                   (0x30a0,0x2ea8),(0x2eb0,0x2cc8),(0x30d8,0x2ee0),(0x2248,0x2050))):
            if rng.randrange(2): word(BASE+dest,struct.unpack('<I',uc.mem_read(TABLE+source,4))[0])
        for dest,offset in ((0x1c78,0x210),(0x1c80,0x214),(0x1cf0,0x218),(0x1aa0,0x264)):
            if rng.randrange(2): word(BASE+dest,RESOURCES+0x1000 if seed&16 and seed&32 else 0)
        if mode != 'normal':
            # Exercise legal aliasing between the allocation cursor and
            # context fields, including repeated state comparisons.
            word(OWNER+0x108,0xc0000000)
            half(BASE+0x16a,0)
            cursor=BUFFER
            if mode=='alias-state': cursor=BASE+0x2eb0
            if mode=='alias-resource': cursor=BASE+0x1cf0
            word(BASE+0x24,cursor);word(BASE+0x28,cursor+4096)
            half(DESC+0x18,1024 if mode=='refill' else 12)
            if seed&1: word(DEFAULTS+4,0)
        word(SP,h.RET);word(SP+4,BASE)
        for r in REGS: uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,OWNER);uc.reg_write(x.UC_X86_REG_ESP,SP)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f);uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80|((seed%4)<<13))
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|(rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.allocations=0
        before=self.snapshot(False)
        self.active=True
        uc.emu_start(ENTRY,h.RET,count=100000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        after=self.snapshot();after['flag_mask']=0xcc5 if 0xea6816 in self.visited else 0xcd5
        return dict(entry=ENTRY,seed=seed,mode=mode,before=before,initial=sorted(self.initial.items()),
                    calls=self.calls,after=after,visited=sorted(self.visited))


def main():
    oracle=MaterialOracle()
    cases=[oracle.run(seed) for seed in range(1,513)]
    for mode in ('refill','alias-state','alias-resource'):
        cases.extend(oracle.run(seed,mode) for seed in range(513,529))
    cases.extend(oracle.run(seed,'alias-resource') for seed in range(577,593))
    out=Path('E:/ddda-re/notes/native-L167/shadow_material.json')
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} returns; {len(set().union(*(set(c["visited"]) for c in cases)))} instructions; '
          f'sha256={hashlib.sha256(out.read_bytes()).hexdigest()}')


if __name__=='__main__': main()
