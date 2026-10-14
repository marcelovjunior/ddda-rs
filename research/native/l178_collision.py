"""D7CF70 original collision caller, explicit support/simplex mock contracts."""
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
import x87_host
from record_l175 import effective_address
from record_main_thread import Oracle, REGS, STACK, STACK_SIZE, SP

ENTRY,LENGTH=0xd7cf70,0x12f4
BASE,SIZE=0xe000000,0x10000
SHAPE,VELOCITY,SIMPLEX,OUT1,OUT2=(BASE+v for v in (0x1000,0x2000,0x3000,0x4000,0x4100))
ARGS={0xd63ff0:1,0xd1b540:3,0xd12e10:1,0xd8e400:5}

class CollisionOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32);self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        pages={a&~4095 for a in ARGS}|{0xd7c000,0xd7d000,0xd7e000,0x18cf000,0x1629000,0x1406000,0x1407000,0x1408000,0x161c000}
        for a in pages:self.uc.mem_map(a,4096)
        for a,n in [(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096)]:self.uc.mem_map(a,n)
        for a in ARGS:self.uc.mem_write(a,b'\xc3')
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH))
        for a,n in [(0x18cfe80,12),(0x1629530,12),(0x1629570,12),(0x1406ce0,4),(0x1407124,4),(0x1408e74,4),(0x161c280,4)]:self.uc.mem_write(a,h.code(a,n))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read);self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_flush(*args))
        self.uc.hook_add(UC_HOOK_CODE,self.code)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_compute(*args))
    def word(self,a,v):self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))
    def get(self,a):return struct.unpack('<I',self.uc.mem_read(a,4))[0]
    def code(self,uc,a,size,data):
        self.visited.add(a)
        if a not in ARGS:
            assert ENTRY<=a<ENTRY+LENGTH,hex(a)
            return
        esp=uc.reg_read(x.UC_X86_REG_ESP);argc=ARGS[a]
        self.remember(esp,4*(argc+1));args=[self.get(esp+4*i) for i in range(argc+1)]
        reads={}
        buffers=[(args[1],12)] if a==0xd63ff0 else ([(args[1],16),(args[2],16),(args[3],16)] if a==0xd1b540 else [])
        for pointer,length in buffers:
            self.remember(pointer,length);reads.update((pointer+i,b) for i,b in enumerate(uc.mem_read(pointer,length)))
        call=dict(target=a,regs=[uc.reg_read(r) for r in REGS],args=args,reads=sorted(reads.items()),
                  memory=[(b,uc.mem_read(b,1)[0]) for b in sorted(self.written)])
        effects=[]
        def effect(addr,v):
            self.remember(addr,4);self.word(addr,v);self.written.update(range(addr,addr+4))
            effects.extend((addr+i,b) for i,b in enumerate(struct.pack('<I',v&0xffffffff)))
        def fword(v):return struct.unpack('<I',struct.pack('<f',v))[0]
        result=0xa17001
        if a==0xd63ff0:
            self.supports+=1
            for i in range(8):effect(SHAPE+16+4*i,0)
            if self.seed%11==0:
                effect(SHAPE+16,fword((0.0 if self.supports%2 else 0.1)))
            elif self.seed%13==0:
                effect(SHAPE+16,fword(2.0 if self.supports%2 else -2.0))
        elif a==0xd1b540:
            self.insertions+=1
            result=0xa17000 if self.insertions>1 or self.seed%17==0 else 0xa17001
            count=3 if self.seed>=544 else (1 if self.seed>=512 else (3 if self.seed%3 else (self.seed//3)%7))
            effect(SIMPLEX,count)
            effect(SIMPLEX+0x170,0x100 if self.seed%7 or count==0 or self.seed>=512 else (self.seed%2))
            for offset in (0x10,0x70,0xd0):
                for i in range(4):
                    for k in range(4):
                        value=self.points[i][k] if k<3 else 0
                        effect(SIMPLEX+offset+i*16+k*4,fword(value))
            if self.seed>=512:
                offset=16 if self.seed<544 else 16+16*((self.seed//8)%3)+4*((self.seed//24)%3)
                effect(SIMPLEX+offset,[0x7f800123,0x7fc12345,1,0x80000000,0x7f800000,0xff800000,0,0x80000001][self.seed%8])
            for k,v in enumerate(self.points[0]):effect(SIMPLEX+0x150+4*k,fword(v))
            effect(SIMPLEX+0x15c,0)
        else:
            for k,v in enumerate(self.points[0]):effect(args[1]+4*k,fword(v))
            effect(args[1]+12,0)
            if a==0xd8e400 and self.seed%2:result=0xa17000
        uc.reg_write(x.UC_X86_REG_EAX,result);uc.reg_write(x.UC_X86_REG_ECX,0xc1a0);uc.reg_write(x.UC_X86_REG_EDX,0xd1a0)
        uc.reg_write(x.UC_X86_REG_ESP,esp+4*(argc+1));uc.reg_write(x.UC_X86_REG_EIP,args[0])
        call.update(effects=effects,result=result,return_esp=esp+4*(argc+1));self.calls.append(call)
    def run(self,seed):
        uc,rng=self.uc,random.Random(seed);self.seed=seed;self.active=False;self.supports=self.insertions=0
        uc.mem_write(BASE,bytes(SIZE));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        self.points=[[rng.choice([-4,-2,-1,-.5,0,.5,1,2,4]) for _ in range(3)] for _ in range(4)]
        # Repeated/collinear inputs cover zero denominators and collapsed regions.
        if seed%19==0:self.points[1]=self.points[0][:]
        if seed%23==0:self.points[2]=[2*v for v in self.points[0]]
        self.word(BASE+0x190,0 if seed%29==0 else (1 if seed%37==0 else 3))
        self.word(BASE+0x194,0x3e800000 if seed%31==0 else 0x322bcc77)
        for i,v in enumerate([0 if seed%5==0 else 1,0,0]):self.word(VELOCITY+4*i,struct.unpack('<I',struct.pack('<f',v))[0])
        sp=SP+4*(seed%4)
        for r in REGS:uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,BASE);uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i,v in enumerate([h.RET,SHAPE,VELOCITY,SIMPLEX,OUT1 if seed%2 else 0,OUT2 if seed%3 else 0]):self.word(sp+4*i,v)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202);uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f);uc.reg_write(x.UC_X86_REG_FPSW,0);uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.remember(sp,24)
        self.host=x87_host.Oracle(effective_address)
        before=self.snapshot(False);self.active=True
        uc.emu_start(ENTRY,h.RET,count=30000);self.host.flush(uc)
        assert self.host.problem is None,self.host.problem
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(entry=ENTRY,seed=seed,before=before,after=self.snapshot(),initial=sorted(self.initial.items()),calls=self.calls,visited=sorted(self.visited),x87_oracle=x87_host.VERSION)

if __name__=='__main__':
    oracle=CollisionOracle();cases=[oracle.run(i) for i in range(640)]
    out=Path('E:/ddda-re/notes/native-L178/00d7cf70.json');out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} original returns; {len(set().union(*(set(c["visited"]) for c in cases)))} instruction addresses; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}',flush=True)
