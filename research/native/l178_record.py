"""L178 isolated original traversal oracle; private output, no game process."""
import hashlib
import json
import random
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
from record_main_thread import Oracle, REGS, STACK, STACK_SIZE, SP

ENTRY, LENGTH, CALLBACK, FALLBACK = 0x10a35d0, 0x629, 0x2000000, 0x10a2460
BASE, SIZE = 0xe000000, 0x40000
TREE, QUERY, WORK = BASE+0x1000, BASE+0x20000, BASE+0x24000

class BoundsOracle(Oracle):
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for a,n in [(0x10a2000,8192),(0x18d5000,8192),(CALLBACK,4096),(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096)]:
            self.uc.mem_map(a,n)
        self.uc.mem_write(ENTRY,h.code(ENTRY,LENGTH))
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def word(self,a,v): self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))
    def get(self,a): return struct.unpack('<I',self.uc.mem_read(a,4))[0]

    def code(self,uc,a,size,data):
        self.visited.add(a)
        if a not in (CALLBACK,FALLBACK):
            assert ENTRY <= a < ENTRY+LENGTH,hex(a)
            return
        esp=uc.reg_read(x.UC_X86_REG_ESP)
        argc=3 if a==CALLBACK else 5
        self.remember(esp,4*(argc+1))
        args=[self.get(esp+4*i) for i in range(argc+1)]
        call=dict(target=a,regs=[uc.reg_read(r) for r in REGS],args=args,
                  memory=[(b,uc.mem_read(b,1)[0]) for b in sorted(self.written)])
        effects=[]
        # Mutate future bounds and a heap word to detect stale caching.
        if a==CALLBACK and self.seed%7==0:
            for addr,value in [(QUERY+16,0x40000000 if len(self.calls)%2 else 0xbf800000),
                               (BASE+0x30000,len(self.calls)+1)]:
                self.remember(addr,4);self.word(addr,value)
                effects.extend((addr+i,b) for i,b in enumerate(struct.pack('<I',value)))
                self.written.update(range(addr,addr+4))
        result=(0x12345678 if a==FALLBACK else (1 << (args[2]%31)))
        uc.reg_write(x.UC_X86_REG_EAX,result)
        uc.reg_write(x.UC_X86_REG_ECX,0xc1a0);uc.reg_write(x.UC_X86_REG_EDX,0xd1a0)
        uc.reg_write(x.UC_X86_REG_ESP,esp+4*(argc+1));uc.reg_write(x.UC_X86_REG_EIP,args[0])
        call.update(effects=effects,result=result,return_esp=esp+4*(argc+1))
        self.calls.append(call)

    def run(self,seed):
        uc,rng=self.uc,random.Random(seed)
        self.seed=seed;self.active=False
        uc.mem_write(BASE,rng.randbytes(SIZE));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        uc.mem_write(0x18d5000,bytes(8192))
        self.word(BASE+0x58,TREE)
        next_node=0
        def node(depth):
            nonlocal next_node
            index=next_node;next_node+=1
            flags=0
            for lane in range(4):
                inner=depth>0 and (seed%8==0 or rng.randrange(3)==0)
                active=seed%5!=0 or rng.randrange(2)
                child=node(depth-1) if inner else rng.randrange(65536)
                flags |= ((1 << lane) if inner else ((16 << lane) if active else 0)) << (lane*8)
                uc.mem_write(TREE+index*0x70+4+lane*2,struct.pack('<H',child))
                for axis in range(3):
                    lower,upper=0xbf800000,0x3f800000
                    if seed%9==1: lower,upper=rng.choice([(0x7fc12345,0x7f800000),(1,0x80000001),(0x40000000,0x40400000),(0xbf800000,0x3f800000)])
                    self.word(TREE+index*0x70+16+axis*16+lane*4,lower)
                    self.word(TREE+index*0x70+64+axis*16+lane*4,upper)
            self.word(TREE+index*0x70,flags)
            return index
        node(seed%4)
        for lane in range(4):
            for axis in range(3):
                self.word(QUERY+16+axis*16+lane*4,0xbf800000)
                self.word(QUERY+64+axis*16+lane*4,0x3f800000)
        lane=[0,1,7,24,25,0xffffffff][seed%6];depth=(seed//6)%2
        if lane<25:
            self.word(0x18d5ee8+lane*4,depth)
            self.word(0x18d5e20+(depth*25+lane)*4,WORK)
        sp=SP+(seed%4)*4
        for r in REGS: uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX,BASE);uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i,v in enumerate([h.RET,QUERY,BASE+0x31000,CALLBACK,BASE+0x32000,lane]):self.word(sp+4*i,v)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202);uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f);uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        self.initial,self.written,self.visited,self.calls={},set(),set(),[]
        self.remember(sp,24)  # Complete caller-owned ABI argument area.
        before=self.snapshot(False);self.active=True
        uc.emu_start(ENTRY,h.RET,count=500000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(entry=ENTRY,seed=seed,before=before,after=self.snapshot(),initial=sorted(self.initial.items()),calls=self.calls,visited=sorted(self.visited))

if __name__=='__main__':
    oracle=BoundsOracle()
    cases=[oracle.run(seed) for seed in range(192)]
    out=Path('E:/ddda-re/notes/native-L178/010a35d0.json');out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} original returns; {len(set().union(*(set(c["visited"]) for c in cases)))} instruction addresses; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}',flush=True)
