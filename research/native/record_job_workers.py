"""Isolated worker-function original-instruction oracles; artifacts stay private.

Joint evaluator callees are explicit synthetic ABI boundaries, with complete
states recorded on entry/return. The dispatcher under test executes original
instructions. No game window or generated shards.
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

JOINTS, LENGTH = 0x1054700, 0xFE
TARGETS = {0x1054690:0,0x104ef10:4,0x1051b60:4,0x1052330:4,0x10533a0:4,h.RET+0x100:8}
MODEL, INDICES, CALLBACK, VTABLE, RECORDS = BASE, BASE+0x1000, BASE+0x1500, BASE+0x1600, BASE+0x2000


class JointOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16<<20)
        for a,n in [(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096),
                    (0x104e000,4096),(0x1051000,0x4000)]:self.uc.mem_map(a,n)
        self.uc.mem_write(JOINTS,h.code(JOINTS,LENGTH))
        for target in TARGETS:self.uc.mem_write(target,b'\xc3')
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def put(self,a,v):self.uc.mem_write(a,struct.pack('<I',v&0xffffffff))

    def code(self,uc,address,size,_):
        if address==h.RET:uc.emu_stop();return
        if address in TARGETS:
            event=dict(target=address,before=self.snapshot())
            ordinal=len(self.events)+1
            for reg,v in [(x.UC_X86_REG_EAX,0xa5000000+ordinal),(x.UC_X86_REG_ECX,0xc6000000+ordinal),
                          (x.UC_X86_REG_EDX,0xd7000000+ordinal)]:uc.reg_write(reg,v)
            uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|((ordinal*17)&0x8d5))
            # Explicit memory effect: changes flags of a later joint, exposing
            # ports that cache the bank's flags across callback boundaries.
            at=RECORDS+((ordinal+1)%8)*0xa0+0x90
            self.remember(at,1);self.written.add(at)
            uc.mem_write(at,bytes([0x20 if ordinal%2 else 0]))
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            ret=struct.unpack('<I',uc.mem_read(sp,4))[0]
            uc.reg_write(x.UC_X86_REG_ESP,sp+4+TARGETS[address])
            uc.reg_write(x.UC_X86_REG_EIP,ret)
            event['after']=self.snapshot();self.events.append(event)
            return
        assert JOINTS<=address<JOINTS+LENGTH,hex(address)
        self.visited.add(address)

    def run(self,seed,count,model_flags,mode,pending,pattern):
        self.active=False;uc=self.uc;rng=random.Random(seed)
        uc.mem_write(BASE,rng.randbytes(SIZE));uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        self.put(MODEL+0x364,RECORDS);self.put(MODEL+0x378,model_flags)
        uc.mem_write(MODEL+0xe19,bytes([pending]))
        self.put(CALLBACK,VTABLE);self.put(VTABLE+20,h.RET+0x100)
        for i in range(256):
            uc.mem_write(INDICES+i,bytes([(i*13)%256]))
            self.put(RECORDS+i*0xa0+4,CALLBACK if i%3 else 0)
            uc.mem_write(RECORDS+i*0xa0+0x90,bytes([0x20 if pattern==1 or pattern==2 and i%2 else 0]))
            uc.mem_write(RECORDS+i*0xa0+0x96,bytes([i%2 if pattern==2 else 1]))
        self.put(SP,h.RET);self.put(SP+4,INDICES);self.put(SP+8,count);self.put(SP+12,mode)
        for r in REGS:uc.reg_write(r,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,SP);uc.reg_write(x.UC_X86_REG_ECX,MODEL)
        for i in range(8):uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|rng.getrandbits(8))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80|(seed%4)<<13)
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f);uc.reg_write(x.UC_X86_REG_FPSW,0);uc.reg_write(x.UC_X86_REG_FPTAG,0xffff)
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        before=self.snapshot(False);self.active=True
        uc.emu_start(JOINTS,h.RET,count=1000000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        return dict(seed=seed,count=count,model_flags=model_flags,mode=mode,pending=pending,pattern=pattern,
                    before=before,initial=sorted(self.initial.items()),events=self.events,after=self.snapshot(),visited=sorted(self.visited))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L165/job_joints.json'))
    args=p.parse_args();oracle=JointOracle();cases=[]
    for count in [0,1,2,8,64]:
        for flags in [0,0x40,0x4040,0x100040,0x4000000,0x8000000,0x01000000]:
            for mode in [0,1,2,3]:
                for pending in [0,1]:
                    for pattern in [0,1,2]:cases.append(oracle.run(len(cases)+1,count,flags,mode,pending,pattern))
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(dict(entry=JOINTS,code_sha256=hashlib.sha256(h.code(JOINTS,LENGTH)).hexdigest(),cases=cases)),encoding='utf-8')
    print(f'{len(cases)} returns / {sum(len(v["events"])for v in cases)} callback boundaries; sha256={hashlib.sha256(args.out.read_bytes()).hexdigest()}',flush=True)

if __name__=='__main__':main()
