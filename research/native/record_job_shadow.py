"""L167 original FED480 instructions; private synthetic state-save corpus.

The external memcpy seam has explicit synthetic effects: ordered byte copy,
EAX=destination, other registers/flags preserved, ret. Call entry state is
recorded and verified independently. No claim to port memcpy is made.
"""
import hashlib
import json
from pathlib import Path
import random
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, SIZE, STACK, STACK_SIZE, SP
import harness as h

ENTRY, LENGTH, MEMCPY = 0xfed480, 0x148, 0x12efea0


class ShadowOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for base, size in ((0xfed000,4096),(0x1447000,4096),(0x12ef000,4096),
                           (BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096)):
            self.uc.mem_map(base,size)
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH))
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self, uc, address, size, data):
        if address == MEMCPY:
            self.calls.append(self.snapshot())
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp,16)
            ret,dst,src,count = struct.unpack('<4I',uc.mem_read(sp,16))
            for i in range(count):
                self.remember(src+i,1)
                self.remember(dst+i,1)
                self.written.add(dst+i)
                uc.mem_write(dst+i,bytes(uc.mem_read(src+i,1)))
            uc.reg_write(x.UC_X86_REG_EAX,dst)
            uc.reg_write(x.UC_X86_REG_ESP,sp+4)
            uc.reg_write(x.UC_X86_REG_EIP,ret)
            return
        assert ENTRY <= address < ENTRY+LENGTH, hex(address)
        self.visited.add(address)

    def run(self, seed, full, df, index, alignment, alias):
        uc, rng = self.uc, random.Random(seed)
        self.active = False
        uc.mem_write(BASE,rng.randbytes(SIZE))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        owner = BASE+0x1000+alignment
        # index=-1 wraps the destination to source; larger negative values
        # are excluded since they leave the synthetic object's mapped arena.
        def word(a,v): uc.mem_write(a,struct.pack('<I',v&0xffffffff))
        word(owner+0x30874,index)
        word(owner+0x14,seed%17)
        for slot in range(6): word(0x1447b44+slot*4,rng.getrandbits(32))
        sp = owner+0x180 if alias else SP
        word(sp,h.RET)
        word(sp+4,full | (rng.getrandbits(24)<<8))
        for r in REGS: uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,owner)
        uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80 | ((seed%4)<<13))
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0x8d5) | (df<<10))
        self.initial,self.written,self.visited,self.calls = {},set(),set(),[]
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(ENTRY,h.RET,count=100000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active = False
        after = self.snapshot()
        after['flag_mask'] = 0xcd5 # INC defines all arithmetic flags, preserves CF.
        return dict(entry=ENTRY,seed=seed,full=full,df=df,index=index,
                    alignment=alignment,alias=alias,before=before,calls=self.calls,
                    initial=sorted(self.initial.items()),after=after,visited=sorted(self.visited))


def main():
    oracle,cases = ShadowOracle(),[]
    for full in (0,1,255):
        for df in (0,1):
            for index in (-1,0,1,2,0x3fffffff,0x40000000,0x7fffffff,0x80000000,0xc0000000):
                for alignment in (0,1,3,0xf49):
                    for alias in (False,True):
                        cases.append(oracle.run(len(cases)+1,full,df,index,alignment,alias))
    out=Path('E:/ddda-re/notes/native-L167/job_shadow.json')
    out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} original returns; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}')


if __name__=='__main__': main()
