"""L173 original instructions, synthetic ABI callees; private vectors only.

Run with the research Python through heavy.py. No game process/shard build.
The allocator returns a mapped arena, memcpy copies bytes, other calls return
controlled pointers/booleans and clobber volatile registers. These are explicit
test seams, not assertions about the implementations of those callees.
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

SIZES = {0xff1920: 0xaac, 0xff14d0: 0x438, 0xfefb90: 0x50e}
OWNER = BASE
BANK, META, MATERIAL, TEX, STATE, SHADER, KEYS, DRAW, RECORDS, ARENA, CALLBACK_OBJ, VTABLE = (
    BASE + o for o in (0x40000,0x49000,0x4a000,0x4b000,0x4c000,0x4d000,0x4e000,0x50000,0x54000,0x60000,0x4f000,0x4f100))
BEGIN, END = 0x1200000, 0x1200010
# Stack arities are from the call sites; cleanup from original callee RETs.
CALLS = {0xda7d70:(1,4),0xfeef80:(0,0),0x67fb90:(2,8),0xc6cbd0:(1,4),
         0xe132b0:(7,28),0x12efea0:(3,0),0xfee320:(2,8),BEGIN:(2,8),END:(2,8)}

class DrawOracle(Oracle):
    def __init__(self, entry):
        self.entry = entry
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        pages = set(range(entry & ~4095, (entry+SIZES[entry]+4095)&~4095,4096))
        pages.update(a&~4095 for a in CALLS)
        pages.update((0x18d0000,0x18d1000,0x18d2000,0x1447000,0x143a000,0x161c000,0x1406000,h.RET,0))
        for a in pages: self.uc.mem_map(a,4096)
        self.uc.mem_map(BASE,0x90000)
        self.uc.mem_map(STACK,STACK_SIZE)
        self.uc.mem_write(entry,h.code(entry,SIZES[entry]))
        for a in (0x1447000,0x143a000,0x161c000,0x1406000): self.uc.mem_write(a,h.code(a,4096))
        for a in CALLS: self.uc.mem_write(a,b'\xc3')
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def word(self,a,v): self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))
    def readword(self,a): return struct.unpack('<I',self.uc.mem_read(a,4))[0]

    def code(self,uc,address,size,data):
        if address in CALLS:
            argc,pop=CALLS[address]
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp,4+4*argc)
            args=[self.readword(sp+4+4*i) for i in range(argc)]
            state=self.snapshot()
            state['memory']=[(a,uc.mem_read(a,1)[0]) for a in sorted(self.initial)]
            self.events.append(dict(target=address,args=args,**state))
            effects=[]
            result=self.rng.getrandbits(32)
            if address==0xda7d70:
                result=0 if self.variant in (25,26,27) else ARENA+0x10000+self.allocations*0x2000
                self.allocations+=1
            elif address==0x67fb90: result=TEX
            elif address==0xc6cbd0: result=0 if self.variant==21 else MATERIAL
            elif address==0xe132b0: result=0 if self.variant==22 else SHADER
            elif address==BEGIN: result=(result&~255) | (0 if self.variant%5==1 else 1)
            elif address==0xfee320: result=(result&~255) | (0 if self.variant%3==1 else 1)
            elif address==0x12efea0:
                dst,src,count=args
                assert count<=4096
                self.remember(src,count)
                self.remember(dst,count)
                for k in range(count):
                    value=uc.mem_read(src+k,1)[0]
                    uc.mem_write(dst+k,bytes([value]))
                    self.written.add(dst+k)
                    effects.append((dst+k,value))
                result=dst
            for r in REGS[:3]: uc.reg_write(r,self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_EAX,result)
            uc.reg_write(x.UC_X86_REG_ESP,sp+4+pop)
            self.events.append(dict(effects=effects,**self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP,self.readword(sp))
            return
        assert self.entry<=address<self.entry+SIZES[self.entry],hex(address)
        self.visited.add(address)

    def run(self,seed,variant,count):
        self.active=False
        self.rng=rng=random.Random(seed)
        self.variant=variant
        self.allocations=0
        u=self.uc
        u.mem_write(BASE,rng.randbytes(0x90000))
        u.mem_write(STACK,rng.randbytes(STACK_SIZE))
        u.mem_write(0,rng.randbytes(4096))
        w=self.word
        for a,v in ((0x18d08f4,CALLBACK_OBJ),(0x18d12cc,CALLBACK_OBJ),(0x18d2ce8,seed)):
            w(a,v)
        for off,v in {0x18:META,0x1c:BANK,0x20:BANK,0x24:ARENA,0x28:ARENA+(0 if variant==20 else 0x10000),
                      0x44:DRAW,0x48:count,0x5c:0x80,0x60:0,0x64:0,0x68:3,0x6c:3,0x78:CALLBACK_OBJ,
                      0xb4:0,0xc4:0,0xc8:0x30,0x154:0x123458,0x164:11,0x169:0,0x178:123,
                      0x190:0x100,0x194:STATE,0x198:SHADER,0x1a0:STATE,0x1a4:0,0x1a8:0,0x1ac:0,
                      0x1cc:2,0x1d0:0xface,0x1d4:0,0x1d8:TEX,0x1dc:MATERIAL,0xcc8:STATE+0x100,
                      0xc90:STATE+0x200,0x2248:0,0x224c:2}.items(): w(OWNER+off,v)
        w(OWNER+0x114,127)
        w(STATE+12,MATERIAL)
        w(MATERIAL+0x10,3|0x8040)
        w(MATERIAL+0x14,variant%9)
        w(MATERIAL+0x18,64|(32<<16))
        w(MATERIAL+0x20,0)
        w(TEX+0x18,64|(32<<16))
        w(TEX+0x20,(variant%16)<<20)
        w(SHADER+0x20,variant%6)
        w(SHADER+0x1c,KEYS)
        for k in range(8): w(KEYS+4*k,k)
        w(META+0x568,META+0x800)
        w(META+0x54c,META+0x800)
        w(META+0x818,16)
        w(META+0x824,STATE+0x300)
        w(CALLBACK_OBJ,VTABLE)
        w(VTABLE+0x34,BEGIN)
        w(VTABLE+0x38,END)
        for k in range(count):
            w(DRAW+8*k,rng.getrandbits(32))
            w(DRAW+8*k+4,RECORDS+64*k)
            w(RECORDS+64*k,((k%4)<<16)|((k%3)<<20)|rng.randrange(65536))
        for k in range(3):
            w(OWNER+0x30c7c+12*k,k | (0x80000000 if k==1 else 0))
        if self.entry==0xff1920:
            if variant<8:
                off,value=[(0x5c,0x40),(0x5c,0x10),(0x1dc,0),(0x164,1),
                           (0xc8,0),(0x190,0),(0x164,13),(0x164,14)][variant]
                w(OWNER+off,value)
            if variant in (8,9): w(MATERIAL+0x10,variant-8)
            if variant>=10:
                w(OWNER+0x164,(11,12,15,16)[variant%4])
                w(OWNER+0x154,(0,0x100000,0x100004)[variant%3]|(127<<11))
            if variant==11: w(OWNER+0xc8,0x430)
            if variant in (12,13,14):
                w(OWNER+0x1a8,STATE+0x400);w(STATE+0x400,TEX)
                if variant>=13:w(OWNER+0x1ac,STATE+0x500)
                if variant>=14:w(OWNER+0x1a4,STATE+0x600)
            if variant in (18,19,21):w(OWNER+0x1d4,CALLBACK_OBJ)
            if variant in (24,25,26,27,28,29):
                w(OWNER+0x1a8,STATE+0x400);w(STATE+0x400,TEX)
                if variant!=24:w(OWNER+0x1ac,STATE+0x500)
                if variant>=26:w(OWNER+0x1a4,STATE+0x600)
                w(OWNER+0x28,ARENA)
            if variant==28:
                for k in range(count):w(RECORDS+64*k,0xf0000)
            if variant==29:w(OWNER+0x60,count)
            if variant==30:
                w(OWNER+0x154,0);w(OWNER+0x114,0);w(MATERIAL+0x10,3);w(OWNER+0x164,15)
                for k,v in enumerate((0.0,0.0,0.0,1.0/struct.unpack('<f',h.code(0x143a7f4,4))[0])):
                    u.mem_write(STATE+0x100+4*k,struct.pack('<f',v))
            if variant==31:w(OWNER+0x2248,self.readword(BANK+0x2050));w(OWNER+0x154,0)
            if 32<=variant<=35:
                w(OWNER+0x1a8,STATE+0x400);w(STATE+0x400,TEX)
                w(OWNER+0x1ac,STATE+0x500);w(OWNER+0x1a4,STATE+0x600);w(OWNER+0x28,ARENA)
                for k in range(count):w(RECORDS+64*k,(variant%4)<<16)
            if variant==36:u.mem_write(OWNER+0x169,b'\xff')
            if variant==37:
                w(OWNER+0x1a8,STATE+0x400);w(STATE+0x400,0);w(OWNER+0x1ac,STATE+0x500);w(OWNER+0x28,ARENA)
            if variant==38:w(OWNER+0x28,ARENA+192)
            # Unicorn leaves x87 signaling NaNs signaling on FSTP, unlike
            # hardware (and the unmodified lift). Use a quiet NaN here;
            # signaling-NaN validation requires a hardware oracle.
            if variant==39:w(OWNER+0x17c,0x7fc00001)
        elif self.entry==0xff14d0:
            if variant==0:w(OWNER+0x5c,0x40)
            if variant==1:w(OWNER+0x5c,0)
            if variant==2:w(OWNER+0x68,1)
            if variant==3:w(OWNER+0x68,2)
            if variant==24:
                for k in range(count):w(RECORDS+64*k,0xf0000)
            if variant in (25,26,27):w(OWNER+0x28,ARENA)
            if 32<=variant<=35:
                w(OWNER+0x28,ARENA)
                for k in range(count):w(RECORDS+64*k,(variant%4)<<16)
        w(SP,h.RET)
        args=[TEX,MATERIAL,1,rng.getrandbits(32)]
        if self.entry==0xfefb90:
            args=[TEX if variant%4!=0 else 0,MATERIAL if variant%4!=1 else 0,(0,1,2,0x20,0x40,0x43,0x1000,0xffffffff)[variant%8],rng.getrandbits(32)]
            w(MATERIAL+0x10,STATE+0x800);w(STATE+0x844,STATE+0x900);w(STATE+0x900,TEX if variant%2 else 0)
            w(MATERIAL+0x20,0x80 if variant%3 else 0)
            dimension_slot={0:0,1:1,4:2,12:3}[count]
            dimensions=(1,0,0xffff,64)[dimension_slot]|((1,0,0xffff,32)[dimension_slot]<<16)
            w(TEX+0x18,dimensions);w(MATERIAL+0x18,dimensions)
            w(TEX+0x20,((variant%16)<<20)|(0x80000000 if variant%2 else 0))
            if variant>=16:w(OWNER+0xb4,args[0])
            if variant in (0,1,2):
                w(OWNER+0xb4,args[0]);w(OWNER+0xc4,args[1]);w(OWNER+0xc8,(args[2]<<4)|1)
            width=dimensions&0xffff if args[0] or args[1] else 1
            height=(dimensions>>16)&(0x7fff if args[0] else 0xffff) if args[0] or args[1] else 1
            w(OWNER+0x10c,(width|(height<<16)) if variant%2 else 0x80018001)
            if variant in (20,25,26,27):w(OWNER+0x28,ARENA)
            if variant==37:w(OWNER+0x28,ARENA)
            if variant==39:w(OWNER+0x10c,0x80018001)
            for off in (0xcc,0xdc):
                values=(0,0,width,height) if variant%4==0 else (-23,17,1920,1080)
                if variant%4==2:values=(0x7fffffff,0x80000000,0x80000000,0x7fffffff)
                if variant%4==3:values=(0,0,0,0)
                for k,v in enumerate(values):w(OWNER+off+4*k,v)
        for k,value in enumerate(args):w(SP+4+4*k,value)
        for r in REGS:u.reg_write(r,rng.getrandbits(32))
        u.reg_write(x.UC_X86_REG_ECX,OWNER)
        u.reg_write(x.UC_X86_REG_ESP,SP)
        u.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (0x400 if variant==23 else 0))
        u.reg_write(x.UC_X86_REG_FPCW,0x37f | ((seed%4)<<10))
        u.reg_write(x.UC_X86_REG_FPSW,0)
        u.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        u.reg_write(x.UC_X86_REG_MXCSR,0x1f80 | ((seed%4)<<13))
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        before=self.snapshot(False)
        self.active=True
        u.emu_start(self.entry,h.RET,count=100000)
        assert u.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(entry=self.entry,seed=seed,variant=variant,count=count,before=before,
                    initial=sorted(self.initial.items()),events=self.events,after=self.snapshot(),visited=sorted(self.visited))

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--entry',type=lambda s:int(s,16),default=0xff1920)
    a=p.parse_args()
    oracle=DrawOracle(a.entry)
    cases=[oracle.run(1+v*4+k,v,n) for v in range(40) for k,n in enumerate((0,1,4,12))]
    out=Path(f'E:/ddda-re/notes/native-L173/{a.entry:08x}.json')
    out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(a.entry,SIZES[a.entry])).hexdigest(),cases=cases)))
    print(f'{out}: {len(cases)} original returns; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}',flush=True)

if __name__=='__main__':main()
