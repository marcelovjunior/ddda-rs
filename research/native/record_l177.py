"""Isolated L177 original instructions; synthetic call seams, private output.

Never starts a game process. x87 uses the existing host-v1 oracle; allocations,
shader selection and memcpy have explicit recorded effects and call frames.
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
from record_l175 import effective_address

BASE,SIZE=0x0e000000,0x200000
ENTRY,LENGTH=0xfee320,0xbf7
ENTRIES={ENTRY:LENGTH,0xffc350:0x695,0x1145bf0:0x25}
CALLEES={0xda7d70:1,0xe132b0:7,0x12efea0:3,0x1066c60:2,0x1144fe0:2,
         0xea5320:6,0xfed940:0,0xff14d0:0,0xfed190:2,0xea23f0:2,
         0xfeef20:0,0xfed890:1,0xff1920:0}

class CommitOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for a,n in [(0,4096),(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096),(0xfee000,4096),
                    (0xda7000,4096),(0xe13000,4096),(0x12ef000,4096),(0x18d0000,12288),
                    (0x161c000,4096),(0x1409000,4096),(0x1447000,8192),
                    (0xffc000,4096),(0x1145000,4096),(0x1144000,4096),(0x1066000,4096),
                    (0xea5000,4096),(0xea2000,4096),(0xfed000,4096),(0xff1000,4096)]:self.uc.mem_map(a,n)
        for a,n in ENTRIES.items():self.uc.mem_write(a,h.code(a,n))
        # Bound Unicorn's translation block at each mocked entry. An empty
        # page tail can otherwise decode into the next unmapped code page
        # before the code hook gets a chance to perform its explicit return.
        for address in CALLEES:self.uc.mem_write(address,b'\xc3')
        for a,n in [(0x161c220,4),(0x161c29c,4),(0x14091ac,4),(0x1447b5c,1024)]:self.uc.mem_write(a,h.code(a,n))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_flush(*args))
        self.uc.hook_add(UC_HOOK_CODE,self.code)
        self.uc.hook_add(UC_HOOK_CODE,lambda *args:self.host.on_compute(*args))

    def word(self,a,v):self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))
    def get(self,a):return struct.unpack('<I',self.uc.mem_read(a,4))[0]

    def code(self,uc,a,size,data):
        self.visited.add(a)
        if a not in CALLEES:
            assert self.entry<=a<self.entry+ENTRIES[self.entry],hex(a)
            return
        argc=CALLEES[a]
        esp=uc.reg_read(x.UC_X86_REG_ESP)
        args=[self.get(esp+4*i) for i in range(argc+1)]
        self.remember(esp,4*(argc+1))
        call=dict(target=a,regs=[uc.reg_read(r) for r in REGS],args=args,
                  memory=[(b,uc.mem_read(b,1)[0]) for b in sorted(self.written)])
        effects=[]
        if a==0xda7d70:
            result=BASE+0x180000+len(self.calls)*0x2000
            if self.seed%37==0:result=0
        elif a==0xe132b0:result=BASE+0x90000 if self.seed%11 else 0
        elif a==0x12efea0:
            dest,source,length=args[1:]
            self.remember(source,length);self.remember(dest,length)
            copied=bytes(uc.mem_read(source,length));uc.mem_write(dest,copied)
            effects=list(enumerate(copied,dest));self.written.update(range(dest,dest+length))
            result=dest
        else:
            result=(0xa17e0000+self.seed*16+len(self.calls))&0xffffffff
            if a==0x1066c60:result=BASE+0x130000 if self.seed%3 else 0
            if a==0xfed940:result=BASE+0x140000
            if a==0xfed890:
                context=uc.reg_read(x.UC_X86_REG_ECX)
                for address,value in [(context+0x24,BASE+0x180000),(context+0x28,BASE+0x190000)]:
                    self.remember(address,4);self.word(address,value)
                    effects.extend(enumerate(struct.pack('<I',value),address));self.written.update(range(address,address+4))
        cleanup=4 if a==0x12efea0 else 4*(argc+1)
        uc.reg_write(x.UC_X86_REG_EAX,result)
        uc.reg_write(x.UC_X86_REG_ECX,0xc1a0);uc.reg_write(x.UC_X86_REG_EDX,0xd1a0)
        uc.reg_write(x.UC_X86_REG_ESP,esp+cleanup);uc.reg_write(x.UC_X86_REG_EIP,args[0])
        call.update(effects=effects,result=result,return_esp=esp+cleanup)
        self.calls.append(call)

    def run(self,seed):
        self.seed=seed
        self.entry=ENTRY
        uc,rng=self.uc,random.Random(seed)
        self.active=False
        uc.mem_write(BASE,bytes(SIZE));uc.mem_write(0,bytes(4096));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        context=BASE+(seed%3)
        model,bank,descs,desc=BASE+0x40000,BASE+0x50000,BASE+0x60000,BASE+0x61000
        resource,registry,dependency,keys=BASE+0x70000,BASE+0x80000,BASE+0x90000,BASE+0x91000
        buffer=BASE+0x100000
        self.word(0x18d08f4,BASE+0xa0000);self.word(0x18d12cc,registry)
        self.word(context+0x20,bank);self.word(context+24,descs);self.word(descs+0x568,desc)
        self.word(desc+24,[5,8,12,1025][(seed//17)%4]);self.word(desc+36,BASE+0xb0000)
        uc.mem_write(BASE+0xb0000,rng.randbytes(8192))
        self.word(context+0x24,buffer);self.word(context+0x28,buffer+rng.choice([0,16,32,48,80,96,128,0x10000]))
        for i in range(4096):
            self.word(context+0x1f8+i*8,resource+(i%16)*256)
            self.word(bank+i*8,rng.getrandbits(32))
        for i in range(16):
            self.word(resource+i*256+16,rng.choice([0,1,8,9,0x38,0x39]))
            self.word(resource+i*256+32,0x20 if rng.randrange(2) else 0)
            self.word(resource+i*256+36,i%2)
            self.word(registry+0x3c+i*8,resource+i*256)
        for i in range(4):self.word(context+0x1bc+i*4,(seed+i)%16)
        for i in range(24):self.word(context+0xb4+i*4,rng.getrandbits(32))
        self.word(context+0xb4,resource if seed%5 else 0)
        self.word(context+0xc8,8 if seed%4==0 else 0)
        self.word(context+0x110,rng.getrandbits(32));self.word(context+0x114,((seed%2)<<8)|(0x1000 if seed%3==0 else 0))
        for i in range(3):self.word(context+0x118+i*4,resource+i*256)
        self.word(model+0x1c+(seed%4)*40+24,0 if seed%5==0 else ((seed%16)<<20)|((seed+1)%16)|(((seed+2)%16)<<10))
        self.word(context+0x150,seed%2)
        for field,offset in [(0x194,0xc0000),(0x19c,0xc0100),(0x1a0,0xc0200)]:self.word(context+field,BASE+offset)
        self.word(BASE+0xc0008,resource);self.word(BASE+0xc0010,resource+256)
        self.word(BASE+0xc0200,resource if seed%5 else 0)
        for field,bankoff in [(0x20c0,0x1ec8),(0x2248,0x2050)]:
            self.word(context+field,self.get(bank+bankoff) if rng.randrange(2) else 0)
        self.word(context+0x5c,0x80 if rng.randrange(2) else 0)
        self.word(context+0x154,((seed%256)<<11)|(((seed//4)%256)<<3)|(rng.randrange(2)<<19)|(rng.randrange(2)<<20)|rng.randrange(8))
        self.word(context+0x158,rng.getrandbits(32));self.word(context+0x15c,dependency if seed%7 else 0)
        self.word(dependency+32,[0,1,2,7,16][seed%5]);self.word(dependency+28,keys)
        for i in range(16):self.word(keys+i*4,i)
        self.word(context+0xcc8,BASE+0xd0000|(seed%16))
        for off in [0,12]:self.word(BASE+0xd0000+off,[0,0x3f800000,0x7fc12345,0x7f800123,1,0x80000000][seed%6])
        self.word(context+0x17c,[0x3f123456,0x7f800123,1,0x80000000][seed%4])
        self.word(context+0x198,BASE+0xc0300 if seed%3 else 0)
        self.word(BASE+0xc0308,dependency if seed%2 else 0)
        if seed>=768 and seed%37!=0:
            self.execute(ENTRY,seed,context,[model,seed%4])
            self.word(context+0x154,self.get(context+0x154)|1)
            # Stable re-commit exercises identical packets and exact constant
            # comparisons; corrupt selected prior bytes to reach both compare
            # loops' unequal exits without changing pointer validity.
            packet=self.get(context+0x198)
            if packet and seed%4==0:
                size=self.get(dependency+32)*4+12
                address=packet+(size-1 if seed%8==0 else 4)
                self.uc.mem_write(address,bytes([self.uc.mem_read(address,1)[0]^1]))
        return self.execute(ENTRY,seed,context,[model,seed%4])

    def execute(self,entry,seed,owner,args):
        self.entry,self.seed=entry,seed
        uc,rng=self.uc,random.Random(seed)
        for r in REGS:uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,owner);uc.reg_write(x.UC_X86_REG_ESP,SP)
        for i,v in enumerate([h.RET]+args):self.word(SP+i*4,v)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202)
        uc.reg_write(x.UC_X86_REG_FPCW,0x7f|([0,0x200,0x300][(seed//4)%3])|((seed%4)<<10))
        uc.reg_write(x.UC_X86_REG_FPSW,0);uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80|((seed%4)<<13)|([0,0x40,0x8000,0x8040][(seed//4)%4]))
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.remember(SP,4*(len(args)+1))
        self.host=x87_host.Oracle(effective_address)
        before=self.snapshot(False);self.active=True
        try:uc.emu_start(entry,h.RET,count=100000)
        except Exception:
            print('fault EIP/ESP',hex(uc.reg_read(x.UC_X86_REG_EIP)),hex(uc.reg_read(x.UC_X86_REG_ESP)),
                  'calls',[(hex(c['target']),list(map(hex,c['args']))) for c in self.calls],flush=True)
            raise
        self.host.flush(uc)
        assert self.host.problem is None,self.host.problem
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(entry=entry,seed=seed,before=before,initial=sorted(self.initial.items()),after=self.snapshot(),calls=self.calls,visited=sorted(self.visited),x87_oracle=x87_host.VERSION)

    def mesh(self,seed):
        self.active=False
        uc,rng=self.uc,random.Random(seed)
        uc.mem_write(BASE,bytes(SIZE));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        model,context,mesh,materials,position=BASE,BASE+0x40000,BASE+0x80000,BASE+0x90000,BASE+0xa0000
        primitives,resource,ranges,binding,joints=BASE+0xb0000,BASE+0xc0000,BASE+0xd0000,BASE+0xe0000,BASE+0xf0000
        self.word(model+0x360,0 if seed%17==0 else 1);self.word(model+0x364,joints)
        self.word(model+0xf0,resource);self.word(resource+0xe0,10);self.word(resource+0xe4,50)
        self.word(model+0x108,rng.choice([1,2,4,7,15])<<9)
        self.word(model+0x10c,seed%256);self.word(model+0x154,rng.choice([0,13,0xffffffff,0x80000000,0x7fffffff]))
        def floating():return rng.choice([0,0x3f800000,0x41200000,0xc1200000,0x7fc12345,1,0x7f800000,0x80000000])
        self.word(model+0x16c,floating());self.word(model+0xd84,floating())
        self.word(0x18d1d20,resource);self.word(resource+56,seed%4);self.word(0x18d2ce8,rng.getrandbits(32))
        self.word(0x161c280,0x80000000)
        passmask=rng.choice([1,2,3,0x20,0x23]);self.word(context+0x16a,passmask)
        self.word(context+0x164,rng.choice([0,3,14,16]));self.word(context+0x178,rng.getrandbits(32))
        self.word(context+0x5c,rng.choice([0,0x80,0x50,0x150,0x100,0x1d0]))
        self.word(context+0x44,BASE+0x150000);self.word(context+0x24,BASE+0x160000)
        self.word(context+0x28,BASE+0x160000+(0 if seed%5==0 else 4096))
        self.word(context+0x74,rng.choice([0,0x7fff,0xffffffff]))
        self.word(mesh+0x70,primitives);self.word(mesh+0x74,rng.choice([0,1,2,4,8]))
        self.word(mesh+0x9c,resource+0x100);self.word(mesh+0xa0,resource+0x200 if seed%5 else 0)
        self.word(mesh+0x220,ranges if seed%2 else 0);self.word(mesh+0x22c,resource+0x300 if seed%7 else 0)
        for i in range(3):
            self.word(position+i*4,floating());self.word(context+0x184+i*4,floating())
        for i in range(16):self.word(BASE+0x140000+i*4,floating())
        for i in range(8):
            visibility=rng.choice([0,1,31,32,63,127]);lod=rng.choice([1,2,4,7,0xff]);material=rng.randrange(4)
            address=model+0x110+(visibility>>5)*4
            if rng.randrange(5):self.word(address,self.get(address)|(1<<(visibility&31)))
            self.word(materials+i*4,BASE+0x130000+i*64)
            self.word(primitives+i*48,rng.choice([0,1,2,3,0x20,0xffff]))
            self.word(primitives+i*48+4,visibility|(material<<12)|(lod<<24))
            self.word(primitives+i*48+8,rng.choice([0,1,5])|rng.getrandbits(24)<<8)
            self.word(primitives+i*48+16,rng.getrandbits(32))
            for off in [24,28,32]:self.word(primitives+i*48+off,rng.getrandbits(32))
            self.word(primitives+i*48+44,binding+i*32);self.word(binding+i*32,i)
            for j in range(3):self.word(binding+i*32+16+j*4,floating())
            for j in range(16):self.word(joints+i*160+16+j*4,floating())
            self.word(ranges+i*4,rng.getrandbits(32))
        if seed>=384:
            # Finite, visible repeats cover equal stream bindings, high LOD
            # and the far-key upper clamp (random NaNs usually take cold paths).
            self.word(mesh+0x74,2);self.word(context+0x16a,1)
            self.word(model+0x108,15<<9);self.word(resource+56,0)
            self.word(model+0x16c,0);self.word(model+0x154,0)
            self.word(context+0x164,3)
            self.word(position,struct.unpack('<I',struct.pack('<f',1e8 if seed%4==0 else 100.0))[0])
            for j in [1,2]:self.word(position+j*4,0)
            for j in range(3):self.word(context+0x184+j*4,0)
            for off in [24,40,56]:self.word(BASE+0x140000+off,0)
            self.word(BASE+0x140008,0xbf800000)
            self.word(model+0x110,0xffffffff)
            for i in range(2):
                self.word(primitives+i*48,1);self.word(primitives+i*48+4,1|(0xff<<24))
                self.word(primitives+i*48+8,1|(4<<16));self.word(primitives+i*48+16,123)
                self.word(ranges+i*4,234)
            self.word(context+0x138,123);self.word(context+0x124,self.get(mesh+0xa0) if seed%3 else 0x1234)
            self.word(context+0x134,4 if seed%5 else 3)
            self.word(context+0x144,234);self.word(context+0x130,self.get(mesh+0x22c) if seed%3 else 0x1234)
            self.uc.mem_write(context+0x137,bytes([4 if seed%5 else 3]))
        return self.execute(0xffc350,seed,model,[context,mesh,materials,position,rng.getrandbits(32),rng.getrandbits(32)])

    def material(self,seed):
        self.active=False
        self.uc.mem_write(BASE,bytes(SIZE));self.uc.mem_write(STACK,random.Random(seed).randbytes(STACK_SIZE))
        return self.execute(0x1145bf0,seed,BASE,[BASE+0x40000,BASE+0x80000])

if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser();parser.add_argument('--cases',type=int,default=1024)
    parser.add_argument('--entry',type=lambda v:int(v,16),default=ENTRY);args=parser.parse_args()
    oracle=CommitOracle();cases=[]
    for seed in range(args.cases):
        try:cases.append({ENTRY:oracle.run,0xffc350:oracle.mesh,0x1145bf0:oracle.material}[args.entry](seed))
        except Exception:print('failed seed',seed,flush=True);raise
    out=Path('E:/ddda-re/notes/native-L177');out.mkdir(exist_ok=True)
    path=out/f'{args.entry:08x}.json';path.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(args.entry,ENTRIES[args.entry])).hexdigest(),cases=cases)))
    print(len(cases),'original returns;',len(set.union(*(set(c['visited']) for c in cases))),'visited;',hashlib.sha256(path.read_bytes()).hexdigest(),flush=True)
