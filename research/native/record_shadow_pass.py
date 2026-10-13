"""EFD240 pass-setup original oracle; explicit synthetic callees, private data.

Exercises recording, shader-key match/mismatch, buffer refill/null outputs,
optional secondary pass, stack alignment and finite/quiet-NaN float inputs.
External callees have recorded volatile-register effects, not game behavior.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from record_main_thread import Oracle, REGS, BASE, STACK, STACK_SIZE, SP
import harness as h

ENTRY,LENGTH=0xEFD240,0x3d7
OWNER,DRAW,BANK,SHADERS,OUTPUT,PARAMS,VTABLE,SAVED,UNIT=(BASE+n for n in (0,0x10000,0x60000,0x70000,0x80000,0x90000,0xa0000,0xa1000,0xa2000))
V74,V64,V70,V68,V40=(0x1200000+i*16 for i in range(5))
POPS={0xFF1090:8,0xFED8F0:0,0xFED890:4,0xEDE880:28,0xEEA130:12,0xFEDC00:0,V74:8,V64:4,V70:8,V68:12,V40:8}


class PassOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        pages={ENTRY&~4095,0x18d0000,0x18d2000,0x1561000,h.RET}
        pages.update(a&~4095 for a in POPS)
        for page in pages:self.uc.mem_map(page,4096)
        self.uc.mem_map(BASE,0x120000)
        self.uc.mem_map(STACK,STACK_SIZE)
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH))
        self.uc.mem_write(0x1561738,h.code(0x1561738,4))
        for a in POPS:self.uc.mem_write(a,b'\xc3')
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def word(self,a,v):self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))

    def snapshot(self,memory=True):
        result=super().snapshot(memory)
        result['x87_executed']=getattr(self,'x87_executed',False)
        return result

    def code(self,uc,address,size,data):
        if address==h.RET:uc.emu_stop();return
        if address in POPS:
            state=self.snapshot()
            state['flag_mask']=0xcc5 if address in (0xFED8F0,V70) or (address==V64 and not self.matched) else 0xcd5
            self.events.append(dict(kind='call',target=address,**state))
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            ret=struct.unpack('<I',uc.mem_read(sp,4))[0]
            for reg in REGS[:3]:uc.reg_write(reg,self.rng.getrandbits(32))
            if address==0xFED8F0:uc.reg_write(x.UC_X86_REG_EAX,SAVED)
            uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|(self.rng.getrandbits(12)&0xcd5))
            uc.reg_write(x.UC_X86_REG_ESP,sp+4+POPS[address])
            self.events.append(dict(kind='return',effects=[],**self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP,ret)
            return
        assert ENTRY<=address<ENTRY+LENGTH,hex(address)
        self.visited.add(address)
        if address==0xEFD355:self.x87_executed=True

    def run(self,seed,recording,matched,variant,alignment):
        uc=self.uc;self.active=False;self.rng=rng=random.Random(seed)
        self.matched=matched;self.x87_executed=False
        uc.mem_write(BASE,rng.randbytes(0x120000))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        self.word(OWNER,VTABLE)
        for off,target in ((0x74,V74),(0x64,V64),(0x70,V70),(0x68,V68),(0x40,V40)):self.word(VTABLE+off,target)
        self.word(0x18d09e0,UNIT);self.word(UNIT,VTABLE)
        self.word(0x18d2ce8,(0xffffffff,0,0x7fffffff)[variant%3])
        self.word(DRAW+0x18,SHADERS)
        self.word(SHADERS+0x914,SHADERS+0x1000)
        self.word(SHADERS+0x910,SHADERS+0x1100)
        for off in (0x1018,0x1118):uc.mem_write(SHADERS+off,struct.pack('<H',(0,1,3,4,5,65535)[variant%6]))
        self.word(DRAW+0x20,BANK)
        self.word(DRAW+0x24,0 if variant==0 else OUTPUT)
        self.word(DRAW+0x28,0 if variant%2 else OUTPUT+0x10000)
        self.word(DRAW+0x5c,(rng.getrandbits(32)&~0x80)|(0x80 if recording else 0))
        self.word(DRAW+0x6c,variant%3)
        floats=(0,0x80000000,0x3f800000,0xbf800000,0x7f800000,0x7fc01234)
        self.word(OWNER+0xb4,floats[variant%6])
        self.word(OWNER+0xb8,0x41200000)
        self.word(OWNER+0xc4,0xc1200000)
        key=0x6cb if variant in (2,4) else 0x6cc
        desired=rng.getrandbits(32)
        self.word(BANK+key*8,desired)
        self.word(DRAW+0x3848,desired if matched else desired^1)
        for i in range(3):self.word(OWNER+0xdc+i*4,OWNER+0x1000+i*16)
        self.word(OWNER+0x64,4 if variant==1 else 0)
        self.word(OWNER+0x68,0 if variant==2 else 1)
        self.word(PARAMS+0xa8,1 if variant==3 else 0)
        sp=SP+alignment
        for off,value in ((0,h.RET),(4,DRAW),(8,PARAMS),(12,rng.getrandbits(32))):self.word(sp+off,value)
        for reg in REGS:uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,OWNER);uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i in range(8):uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        for i in range(8):uc.reg_write(x.UC_X86_REG_FP0+i,(0,0))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|(rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        before=self.snapshot(False);self.active=True
        uc.emu_start(ENTRY,h.RET,count=10000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(seed=seed,recording=recording,matched=matched,variant=variant,alignment=alignment,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L169/shadow_pass.json'))
    args=parser.parse_args();oracle=PassOracle();cases=[]
    for recording in (False,True):
        for matched in (False,True):
            for variant in range(6):
                for alignment in (0,1,4,15):cases.append(oracle.run(len(cases)+1,recording,matched,variant,alignment))
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(dict(entry=ENTRY,code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)),encoding='utf-8')
    print(f'{len(cases)} original returns; sha256={hashlib.sha256(args.out.read_bytes()).hexdigest()}',flush=True)


if __name__=='__main__':main()
