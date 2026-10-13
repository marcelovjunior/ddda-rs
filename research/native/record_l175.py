"""L175 isolated original-instruction vectors; outputs only to private research.

No game process. Package lookup executes the real 01008200 body. Virtual
material/growth callees use explicit, recorded ABI effects.
"""
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
import x87_host
from record_main_thread import Oracle, REGS, STACK, STACK_SIZE, SP

BASE, SIZE = 0x0e000000, 0x200000
ENTRIES = {0xea23f0: 0xd34, 0xfed190: 0x4b, 0xe0f3c0: 0x88, 0x1008200: 0x50}
STUB = 0x02000000

def effective_address(uc, instruction, operand):
    mem=operand.mem
    def reg(register):
        return uc.reg_read(getattr(x,'UC_X86_REG_'+instruction.reg_name(register).upper())) if register else 0
    return (reg(mem.base)+reg(mem.index)*mem.scale+mem.disp)&0xffffffff

class MaterialOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for a, n in [(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096),(STUB,4096),
                     (0xea2000,8192),(0xfed000,4096),(0xe0f000,4096),(0x1008000,4096),
                     (0x1406000,4096),(0x18cf000,4096)]: self.uc.mem_map(a,n)
        for a,n in ENTRIES.items(): self.uc.mem_write(a,h.code(a,n))
        self.uc.mem_write(0x1406ce0,h.code(0x1406ce0,4))
        self.uc.mem_write(0x18cfe70,h.code(0x18cfe70,16))
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_flush(*args))
        self.uc.hook_add(UC_HOOK_CODE,self.code)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_compute(*args))

    def word(self,a,v): self.uc.mem_write(a,struct.pack('<I',v & 0xffffffff))
    def get(self,a): return struct.unpack('<I',self.uc.mem_read(a,4))[0]

    def code(self,uc,a,size,data):
        self.visited.add(a)
        if a in (STUB,0xfed890):
            esp = uc.reg_read(x.UC_X86_REG_ESP)
            argc = 2 if a == STUB else 1
            args = [self.get(esp + 4*i) for i in range(argc+1)]
            call = dict(target=a, regs=[uc.reg_read(r) for r in REGS], args=args,
                        memory=[(b,uc.mem_read(b,1)[0]) for b in sorted(self.written)])
            effects = []
            if a == 0xfed890:
                context = uc.reg_read(x.UC_X86_REG_ECX)
                for addr,val in [(context+0x24,BASE+0x180000),(context+0x28,BASE+0x190000)]:
                    self.remember(addr,4); self.word(addr,val)
                    effects.extend((addr+j,b) for j,b in enumerate(struct.pack('<I',val)))
                    self.written.update(range(addr,addr+4))
            result = self.policy if a == STUB else 0xa17e00
            uc.reg_write(x.UC_X86_REG_EAX,result)
            uc.reg_write(x.UC_X86_REG_ECX,0xc1a0)
            uc.reg_write(x.UC_X86_REG_EDX,0xd1a0)
            uc.reg_write(x.UC_X86_REG_ESP,esp+4*(argc+1))
            uc.reg_write(x.UC_X86_REG_EIP,args[0])
            call.update(effects=effects,result=result,return_esp=esp+4*(argc+1))
            self.calls.append(call)
        else:
            assert any(e <= a < e+n for e,n in ENTRIES.items()), hex(a)

    def run(self,entry,seed):
        uc,rng=self.uc,random.Random(seed)
        self.active=False
        uc.mem_write(BASE, bytes(SIZE))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        owner,context,material,bank=BASE,BASE+0xc0000,BASE+0x110000,BASE+0x120000
        key=0x12345123
        self.policy=[0,1,2,0xffffffff][seed%4]
        args=[]
        if entry==0xfed190:
            self.word(material,bank);self.word(bank+0x30,STUB)
            self.word(owner+0x48,rng.getrandbits(32));self.word(owner+0x5c,rng.getrandbits(32))
            args=[material,rng.getrandbits(32)]
        elif entry==0xe0f3c0:
            count=[0,1,3,16,64][seed%5]
            self.word(owner+0xac2a0,count)
            for i in range(count): self.word(owner+0xab2a0+4*i,key if seed%7==0 and i==count-1 else key+i+1)
            for i in range(6):
                package=BASE+0xd0000+i*0x1000
                table=BASE+0x130000+i*0x6000
                node=BASE+0x170000+i*0x1000
                self.word(owner+0xab104+i*4,package if (seed >> i)&1 else 0)
                self.word(package+0x84,table)
                self.word(table+0x1c+(key&0xfff)*4,node)
                self.word(table+0x18,node)
                self.word(node,key if (seed+i)%4==0 else key+1)
                self.word(node+8,1 if seed%3 else 0)
                self.word(node+12,seed%17)
                self.word(node+16,key)
                self.word(node+28,0x80000000 if seed%11==0 else i+3)
            args=[0 if seed%13==0 else key]
        elif entry==0xea23f0:
            context += seed%3
            material += seed%4
            args=[context,material]
            flags=[0,4,0x100,0x104][(seed//4)%4]
            self.word(owner+8,flags);self.word(material,0x104 if seed%5 else 0)
            self.word(context+0x1d4,bank if seed%7 else 0)
            self.word(bank+0x10,4 if seed%3==0 else 0)
            self.word(context+0x164,0x10 if seed%2 else 3)
            self.word(context+0x5c,0x80 if (seed//32)%2 else 0)
            self.word(context+0x20,BASE+0x130000)
            self.word(context+0x18,BASE+0x140000)
            for offset in [0x84c,0x990]:self.word(BASE+0x140000+offset,BASE+0x150000)
            self.word(BASE+0x150018,[0,3,4,17][seed%4])
            self.word(context+0x24,0 if seed%17==0 and seed%3!=0 else (owner+0x1d4 if seed%29==0 else BASE+0x160000))
            self.word(context+0x28,0 if seed%13==0 else BASE+0x170000)
            for off in [0x2a18,0x33d8,0x33d0,0x33c8,0x33c0,0x2a50,0x2e98,0x2ea0,0x2680,0x2688,0x2698,0x26a8,0x26c0,0x26c8,0x26d8,0x26e8]:
                self.word(BASE+0x130000+off,0 if seed%11==0 else rng.getrandbits(32))
            for off in [0x2c10,0x3090,0x2868,0x28a8]: self.word(context+off,0)
            self.word(material+8,(seed%32)<<3 | ((seed%64)<<24))
            self.word(material+20,[0x2f55c03d,0x4325a03e,0,0x12345678][seed%4])
            self.word(context+0x1bc,self.get(material+20) if seed%3==0 else 7)
            self.word(context+0x154,rng.getrandbits(32))
            self.word(bank+0xb8,BASE+0x151000 if seed%3 else 0)
            self.word(BASE+0x151044,BASE+0x152000)
            self.word(BASE+0x152000,seed%2)
            for j in range(4):
                self.word(owner+0x1d0+j*4,self.get(0x18cfe70+j*4) if seed%4==0 else [0x3f123456,0x80000000,0x7fc12345,1,0x7f800123,0xff800000][(seed+j)%6])
        else: raise ValueError(entry)
        for r in REGS: uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,owner);uc.reg_write(x.UC_X86_REG_ESP,SP)
        for i,v in enumerate([h.RET]+args):self.word(SP+i*4,v)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202)
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f|((seed%4)<<10));uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80|((seed%4)<<13)|([0,0x40,0x8000,0x8040][(seed//4)%4]))
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.host=x87_host.Oracle(effective_address)
        before=self.snapshot(False);self.active=True
        uc.emu_start(entry,h.RET,count=100000)
        self.host.flush(uc)
        assert self.host.problem is None,self.host.problem
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(entry=entry,seed=seed,before=before,initial=sorted(self.initial.items()),after=self.snapshot(),calls=self.calls,visited=sorted(self.visited),x87_oracle=x87_host.VERSION)

if __name__=='__main__':
    oracle=MaterialOracle()
    out=Path('E:/ddda-re/notes/native-L175');out.mkdir(exist_ok=True)
    for entry in [0xfed190,0xe0f3c0,0xea23f0]:
        cases=[oracle.run(entry,i) for i in range(1024 if entry==0xea23f0 else 256)]
        data=dict(code_sha256={hex(e):hashlib.sha256(h.code(e,n)).hexdigest() for e,n in ENTRIES.items()},cases=cases)
        path=out/f'{entry:08x}.json';path.write_text(json.dumps(data))
        print(f'{entry:08x}: {len(cases)} original returns; sha256={hashlib.sha256(path.read_bytes()).hexdigest()}',flush=True)
