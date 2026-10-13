"""C24640 original caller oracle. Explicit mock contracts; no game process."""
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
from record_main_thread import Oracle, REGS, STACK, STACK_SIZE, SP

ENTRY,LENGTH=0xc24640,0x582
BASE,SIZE=0xe000000,0x100000
CTX,MESH,MATERIALS,PRIMS,STATE,REV,ACTIVE,BOUNDS,LISTENER=(BASE+v for v in (0x10000,0x50000,0x51000,0x52000,0x54000,0x58000,0x59000,0x5a000,0x5b000))
CALLBACK=0x2000000
ARGS={0x1004ab0:0,0xfed940:0,0x113c160:1,0x113aea0:2,0xff14d0:0,0xfed190:2,
      0x113c1b0:2,0xea23f0:2,CALLBACK:4,0x113aee0:3,0xfeef20:0,0xc26310:2,0xff1920:0,0xc242c0:1}

class InstanceOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32);self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        pages={a&~4095 for a in ARGS}|{ENTRY&~4095,0x18d2000,CALLBACK}
        for a in pages:self.uc.mem_map(a,4096)
        for a,n in [(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096)]:self.uc.mem_map(a,n)
        for a in ARGS:self.uc.mem_write(a,b'\xc3')  # Bound stub decoding before the code hook.
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH));self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read);self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write);self.uc.hook_add(UC_HOOK_CODE,self.code)
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
        buffers=[]
        if a==CALLBACK:buffers=[(args[4],16)]
        elif a==0x113c1b0:buffers=[(args[2],44)]
        elif a==0x113aee0:buffers=[(args[3],44)]
        for pointer,length in buffers:
            self.remember(pointer,length)
            reads.update((pointer+i,b) for i,b in enumerate(uc.mem_read(pointer,length)))
        call=dict(target=a,regs=[uc.reg_read(r) for r in REGS],args=args,reads=sorted(reads.items()),
                  memory=[(b,uc.mem_read(b,1)[0]) for b in sorted(self.written)])
        effects=[]
        def effect(addr,v):
            self.remember(addr,4);self.word(addr,v);self.written.update(range(addr,addr+4))
            effects.extend((addr+i,b) for i,b in enumerate(struct.pack('<I',v&0xffffffff)))
        if a==0x113c160:
            self.selection=args[1]
            for i in range(11):effect(self.selection+4*i,0)
            for off,v in [(8,REV),(12,9),(16,7),(32,REV+0x100 if self.seed%2 else 0),(36,9),(40,7)]:effect(self.selection+off,v)
        elif a==0x113aea0:
            index=args[2]
            if self.seed%4==0:effect(REV+16,9+index%2)
            elif self.seed%4==1:effect(self.selection+16,index%2+7)
            elif self.seed%4==2:effect(REV+0x110,9+index%2)
        elif a==0x113c1b0:
            for off,obj in [(8,REV),(32,REV+0x100)]:
                if self.get(self.selection+off):
                    effect(self.selection+off+4,self.get(obj+16))
                    effect(self.selection+off+8,self.uc.mem_read(obj+0x32,1)[0])
        elif a==0xc26310:
            effect(CTX+0x4000,self.get(CTX+0x4000)+1)
        result=(0 if self.seed%17==0 else ACTIVE) if a==0x1004ab0 else 0xa175
        uc.reg_write(x.UC_X86_REG_EAX,result);uc.reg_write(x.UC_X86_REG_ECX,0xc1a0);uc.reg_write(x.UC_X86_REG_EDX,0xd1a0)
        uc.reg_write(x.UC_X86_REG_ESP,esp+4*(argc+1));uc.reg_write(x.UC_X86_REG_EIP,args[0])
        call.update(effects=effects,result=result,return_esp=esp+4*(argc+1));self.calls.append(call)
    def run(self,seed):
        uc,rng=self.uc,random.Random(seed);self.seed=seed;self.active=False
        uc.mem_write(BASE,bytes(SIZE));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        for addr,val in [(ACTIVE+0x60,0 if seed%19==0 else 1),(CTX+0x16a,0x20 if seed%23==0 else 0x813),
                         (MESH+0x74,[0,1,2,5,8][seed%5]),(MESH+0x70,PRIMS),(MESH+0x9c,STATE+0x4000),
                         (MESH+0xa0,0 if seed%7==0 else STATE+0x4100),(CTX+0x14c,STATE+0x4000 if seed%3==0 else 0),
                         (CTX+0x20,STATE),(CTX+0x5c,0x80 if seed%2 else 0),(CTX+0x6c,0),
                         (STATE+0x2eb0,0xa000),(STATE+0x2ea8,0xb000),(CTX+0x30a0,0xa000 if seed%3==0 else 0xc000),
                         (CTX+0x30a4,rng.getrandbits(32)),(CTX+0x154,rng.getrandbits(32)),
                         (REV+16,9),(REV+0x110,9),(REV+0x32,7),(REV+0x132,7),
                         (BASE+0x104,seed%2),(BASE+0x15c,1 if seed%11==0 else 0),(BASE+0x108,rng.getrandbits(32)),
                         (BASE+0x100,rng.getrandbits(32)),(BASE+0x110,0xffffffff if seed%13 else 0x55),
                         (0x18d2ce8,rng.getrandbits(32)),(0x18d2c80,LISTENER),(LISTENER,LISTENER+0x100),(LISTENER+0x134,CALLBACK)]:self.word(addr,val)
        for i in range(8):
            for off,val in [(0,0x813 if i%4!=seed%4 else 0),(4,(i%3 if seed%3 else 0)<<12|i),
                            (8,(1 if seed%9 else 0)|((i%4)<<16)),(16,0x100+i%2),(0x2c,BOUNDS)]:self.word(PRIMS+i*48+off,val)
            self.word(MATERIALS+4*i,STATE+0x4200+4*i)
        for i in range(16):self.word(BASE+0xb0+4*i,struct.unpack('<I',struct.pack('<f',rng.uniform(-4,4)))[0])
        for i in range(4):self.word(BOUNDS+16+4*i,struct.unpack('<I',struct.pack('<f',rng.uniform(-4,4)))[0])
        if seed>=192:
            special=[0x7fc12345,0x7f800123,0x7f800000,0xff800000,0,0x80000000,1,0x80000001]
            self.word(BASE+0xb0+4*((seed//8)%16),special[seed%8])
        sp=SP+(seed%4)*4
        for r in REGS:uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,BASE);uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i,v in enumerate([h.RET,CTX,MESH,MATERIALS,0,0,0]):self.word(sp+i*4,v)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202);uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f);uc.reg_write(x.UC_X86_REG_FPSW,0);uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.remember(sp,28)
        before=self.snapshot(False);self.active=True
        try:
            uc.emu_start(ENTRY,h.RET,count=50000)
        except Exception:
            print('failed seed',seed,'EIP',hex(uc.reg_read(x.UC_X86_REG_EIP)),'ESP',hex(uc.reg_read(x.UC_X86_REG_ESP)),
                  'calls',[(hex(c['target']),c['args']) for c in self.calls],flush=True)
            raise
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(entry=ENTRY,seed=seed,before=before,after=self.snapshot(),initial=sorted(self.initial.items()),calls=self.calls,visited=sorted(self.visited))

if __name__=='__main__':
    oracle=InstanceOracle();cases=[oracle.run(i) for i in range(256)]
    out=Path('E:/ddda-re/notes/native-L178/00c24640.json');out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} original returns; {len(set().union(*(set(c["visited"]) for c in cases)))} instruction addresses; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}',flush=True)
