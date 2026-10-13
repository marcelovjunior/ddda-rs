"""L174 original-instruction material oracles (no game window or shards).

Run through heavy.py with research Python. Inputs, exe bytes and vectors
stay private. Calls outside the function under test have explicit synthetic
ABI effects, recorded alongside all live object bytes and stack arguments.
The callee results deliberately clobber volatile registers. This verifies
the caller and its seams, not the implementation of its guest callees.
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

MATERIAL, DRAW, BANK = BASE, BASE + 0x1000, BASE + 0x35000
OWNER, RESOURCE, INDIRECT, BINDING, TEXTURE = (BASE + k for k in (0x39000, 0x39200, 0x39400, 0x39600, 0x39800))
FACTOR, RECORDS, VTABLE, MANAGER = (BASE + k for k in (0x39a00, 0x39b00, 0x39c00, 0x39d00))
GET_COLOR, SET_MANAGER = 0x0d000000, 0x0d000100
LENGTHS = {0x10655a0: 0x302, 0x1066c60: 0x58f}
CALLEES = {GET_COLOR:1, SET_MANAGER:2, 0xff1090:2, 0x1062f20:1,
           0x10655a0:2, 0x1066380:2, 0x1065e40:2, 0x10658b0:3, 0x1065b00:3}


class MaterialOracle(Oracle):
    def __init__(self, entry):
        self.entry, self.length = entry, LENGTHS[entry]
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        ranges = [(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,4096),(GET_COLOR,4096),
                  (0x1062000,4096),(0x1065000,0x3000),(0xff1000,4096),
                  (0x1406000,4096),(0x1408000,4096),(0x161c000,4096),
                  (0x1561000,4096),(0x1621000,4096),(0x18d2000,4096)]
        for a,size in ranges: self.uc.mem_map(a,size)
        # Valid decode at interception boundaries; hooks replace execution
        # with the explicit effects below before this RET can execute.
        for a in CALLEES: self.uc.mem_write(a,b'\xc3')
        self.uc.mem_write(entry,h.code(entry,self.length))
        for a in (0x1406ce0,0x1408e58,0x161c298,0x1561738,0x1621120,0x162113c):
            self.uc.mem_write(a,h.code(a,4))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def word(self,a): return struct.unpack('<I',self.uc.mem_read(a,4))[0]
    def put(self,a,v): self.uc.mem_write(a,struct.pack('<I',v & 0xffffffff))
    def f32(self,a,v): self.uc.mem_write(a,struct.pack('<f',v))

    def code(self,uc,address,size,data):
        if address==h.RET:
            uc.emu_stop(); return
        if self.entry<=address<self.entry+self.length:
            self.visited.add(address)
            return
        assert address in CALLEES, hex(address)
        sp=uc.reg_read(x.UC_X86_REG_ESP)
        argc=CALLEES[address]
        args=[self.word(sp+4+4*i) for i in range(argc)]
        # Capture the full initialized objects and all writes so far, apart
        # from dead caller frame bytes. This catches omitted caller writes
        # even when a synthetic callee would not otherwise read them.
        observed=set(self.object_bytes)|{a for a in self.written if not STACK<=a<STACK+STACK_SIZE}
        observed.update(range(sp,sp+4*(argc+1)))
        self.remember(sp,4*(argc+1))
        inputs=[(a,uc.mem_read(a,1)[0]) for a in sorted(observed)]
        effects=[]
        def effect(a,v):
            self.remember(a,4)
            raw=struct.pack('<I',v & 0xffffffff)
            self.written.update(range(a,a+4))
            uc.mem_write(a,raw)
            effects.extend((a+i,b) for i,b in enumerate(raw))
        eax=self.rng.getrandbits(32)
        if address==GET_COLOR:
            for i,value in enumerate((0.25,0.5,0.75,self.alpha)):
                effect(args[0]+4*i,struct.unpack('<I',struct.pack('<f',value))[0])
            eax=args[0]
        elif address in (0x1066380,0x1065e40,0x10658b0,0x1065b00):
            eax=(eax & ~255)|self.return_byte
        elif address==0x10655a0:
            # An observable effect used after this seam: original caller
            # must reload context state, not cache pre-call values.
            effect(DRAW+0x110,self.word(0x1621120) if self.callee_equal else self.rng.getrandbits(32))
            effect(DRAW+0x118,self.word(DRAW+0xd80) if self.callee_equal else self.rng.getrandbits(32))
        event=dict(target=address,this=uc.reg_read(x.UC_X86_REG_ECX),esp=sp,
                   inputs=inputs,effects=effects,eax=eax,returned_esp=sp+4*(argc+1))
        self.events.append(event)
        resume=self.word(sp)
        uc.reg_write(x.UC_X86_REG_EAX,eax)
        uc.reg_write(x.UC_X86_REG_ECX,0xcccccccc)
        uc.reg_write(x.UC_X86_REG_EDX,0xdddddddd)
        uc.reg_write(x.UC_X86_REG_ESP,event['returned_esp'])
        uc.reg_write(x.UC_X86_REG_EIP,resume)

    def run(self,seed,config):
        uc=self.uc
        self.active=False
        self.rng=rng=random.Random(seed)
        self.callee_equal=config.get('callee_equal',False)
        for a in (0x1621120,0x162113c): self.uc.mem_write(a,h.code(a,4))
        uc.mem_write(BASE,bytes(SIZE))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        self.object_bytes=set()
        def put(a,v):
            self.put(a,v); self.object_bytes.update(range(a,a+4))
        def f32(a,v): put(a,struct.unpack('<I',struct.pack('<f',v))[0])
        # Explicit initialized fields plus the read/write hooks provide a
        # sparse initial image; untouched padding need not fill the file.
        flags=config.get('flags',0)
        color=config.get('color',(0.0,0.5,1.0,2.0))
        mode=config.get('pass',0)
        bits=config.get('bits',rng.getrandbits(26)) | ((mode & 31)<<26)
        put(MATERIAL,VTABLE); put(VTABLE+0x24,GET_COLOR); put(VTABLE+0x38,SET_MANAGER)
        put(MANAGER,VTABLE); put(0x18d2c80,MANAGER); put(0x18d2ce8,rng.getrandbits(32))
        put(MATERIAL+0x10,rng.getrandbits(32)); put(MATERIAL+0x14,bits)
        count=config.get('count',seed%9)
        put(MATERIAL+0x18,count | config.get('high',0))
        put(MATERIAL+0x24,rng.getrandbits(32)); put(MATERIAL+0x28,rng.getrandbits(32))
        put(MATERIAL+0x2c,seed%17); put(MATERIAL+0x30,RECORDS)
        for i,v in enumerate(color): f32(MATERIAL+0x40+4*i,v)
        for i in range(count):
            put(RECORDS+i*12,((i+30)<<20)|rng.randrange(1<<20))
            put(RECORDS+i*12+4,rng.getrandbits(32));put(RECORDS+i*12+8,rng.getrandbits(32))
        put(DRAW+0x20,BANK); put(DRAW+0x5c,0x80 if config.get('save',False) else 0)
        put(DRAW+0x6c,config.get('cursor',0))
        for off in (0x110,0x114,0x118,0x11c,0x120,0x154,0x1cc,0x1d0,0x1c30,0x1c34,0x1d90,0x1d94,0x3730,0x3734):
            put(DRAW+off,rng.getrandbits(32) if config.get('dirty',True) else 0)
        for i in range(3): put(DRAW+0x30c7c+i*12,rng.getrandbits(32))
        for i in range(48): put(DRAW+0x1f8+8*i,rng.getrandbits(32))
        put(DRAW+0x168,flags<<16)
        put(DRAW+0x1d4,OWNER if config.get('owner',0)>0 else 0)
        put(OWNER+0xb8,RESOURCE if config.get('owner',0)>1 else 0)
        put(RESOURCE+0x44,INDIRECT);put(INDIRECT,BINDING if config.get('owner',0)>2 else 0)
        put(BINDING+0x10,TEXTURE if config.get('texture',True) else 0)
        put(TEXTURE+12,rng.getrandbits(32))
        put(BANK+0x3538,rng.getrandbits(32));put(BANK+0x3540,rng.getrandbits(32))
        put(DRAW+0x1330,FACTOR|7)
        f32(FACTOR+8,config.get('factor',0.0));f32(DRAW+0x17c,config.get('opacity',0.5))
        put(DRAW+0xd40,rng.getrandbits(32));put(DRAW+0xd80,rng.getrandbits(32))
        if config.get('equal',False):
            put(DRAW+0x1cc,self.word(MATERIAL+0x10));put(DRAW+0x1d0,0x10fdca55)
            put(DRAW+0x168,(flags<<16)|((config.get('variant',0)&255)<<8))
            put(DRAW+0x11c,self.word(MATERIAL+0x28));put(DRAW+0x114,bits&255)
            put(DRAW+0x120,self.word(MATERIAL+0x24));put(DRAW+0x1c30,0)
            put(DRAW+0x3730,self.word(BANK+(0x3540 if mode in (11,12) and config.get('owner',0)>2 else 0x3538)))
            put(DRAW+0x1d90,TEXTURE if mode in (11,12) and config.get('owner',0)>2 and config.get('texture',True) else 0)
        if config.get('color_equal',False):
            put(DRAW+0x118,self.word(DRAW+0x1f8+(seed%17)*8))
            put(DRAW+0x110,0xff_ff_7f_00) # for (0,.5,1,2), CVTT byte pack
        if config.get('flags_equal',False):
            put(DRAW+0x154,((bits>>8)&1)<<20 | ((bits>>13)&1)<<19 |
                ((bits>>9)&15)<<3 | ((bits>>14)&255)<<11 | 3)
        if config.get('fallback_equal',False):
            put(DRAW+0xd80,self.word(DRAW+0xd40))
            put(0x1621120,self.word(0x162113c))
        self.alpha=config.get('alpha',1.0)
        self.return_byte=config.get('return_byte',1)
        sp=SP+config.get('align',0)
        requested=config.get('requested',mode)
        self.put(sp,h.RET);self.put(sp+4,DRAW)
        self.put(sp+8,config.get('variant',0) if self.entry==0x10655a0 else requested)
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,sp);uc.reg_write(x.UC_X86_REG_ECX,MATERIAL)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202)
        uc.reg_write(x.UC_X86_REG_MXCSR,config.get('mxcsr',0x1f80))
        self.initial,self.written,self.visited,self.events={},set(),set(),[]
        for a in self.object_bytes: self.remember(a,1)
        before=self.snapshot(False)
        self.active=True
        try:
            uc.emu_start(self.entry,h.RET,count=100000)
        except Exception:
            print(f'failure seed={seed} entry={self.entry:x} eip={uc.reg_read(x.UC_X86_REG_EIP):x} esp={uc.reg_read(x.UC_X86_REG_ESP):x} events={[hex(e["target"]) for e in self.events]}',flush=True)
            raise
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(seed=seed,config=config,before=before,initial=sorted(self.initial.items()),
                    after=self.snapshot(),events=self.events,visited=sorted(self.visited))


def configs(entry):
    if entry==0x10655a0:
        for color in ((0,0.5,1,2),(-1,-0.1,0.003,1.00001),(float('nan'),float('inf'),-float('inf'),-0.0),(1e30,-1e30,0.99999,0.25),(2,2,2,2)):
            for count in (0,1,8):
                for save in (False,True):
                    for equal in (False,True):
                        for variant in (0,255,0x12345678):
                            yield dict(color=color,count=count,save=save,equal=equal,variant=variant)
        for bits in (0,0xffffffff,0x12345678):
            for equal in (False,True):
                yield dict(bits=bits,color_equal=True,flags_equal=True,equal=equal,dirty=False)
        for rounding in range(4):
            for denormals in (0,0x8040):
                for color in ((0.1,0.3,0.7,0.99999),(1e-40,-1e-40,-0.1,float('nan'))):
                    yield dict(color=color,mxcsr=0x1f80|(rounding<<13)|denormals)
        for count in (128,511): yield dict(count=count,save=True)
    else:
        for flags in (0,1,2,0x800,0x10,0x20,0x1000,0x200,8,4,0x100,0x104,0x208,0xffff):
            for mode in (0,10,11,12,13,16,23,31,0xffffffff):
                for save in (False,True):
                    yield dict(flags=flags,pass_=mode,requested=mode,save=save,
                               owner=4,high=0x80000000,return_byte=0 if save else 0x80)
        for mode in (11,12,16):
            for factor in (0.0,-0.0,1.0,float('nan')):
                for opacity in (0.5,1.0,2.0,float('nan')):
                    for flags in (1,4):
                        yield dict(flags=flags,pass_=mode,requested=mode,owner=4,factor=factor,opacity=opacity)
        for owner in range(5):
            for mode in (0,11,12):
                for equal in (False,True):
                    for texture in (False,True):
                        yield dict(flags=1,pass_=mode,requested=24,owner=owner,equal=equal,texture=texture)
        for alpha in (-1,0,0.00099,0.001,0.00101,float('nan')):
            for align in range(0,16,4): yield dict(alpha=alpha,align=align)
        for mode in (11,12):
            for equal in (False,True):
                yield dict(flags=1,pass_=mode,bits=0x80000000,high=0x80000000,
                           callee_equal=True,fallback_equal=equal,owner=4)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',type=Path,default=Path('E:/ddda-re/notes/native-L174'))
    args=p.parse_args();args.out.mkdir(parents=True,exist_ok=True)
    for entry in LENGTHS:
        oracle=MaterialOracle(entry);cases=[]
        for i,config in enumerate(configs(entry)):
            if 'pass_' in config: config['pass']=config.pop('pass_')
            cases.append(oracle.run(i+1,config))
        # JSON config uses symbolic strings for non-finite values; actual
        # oracle state is always integer bit patterns.
        for case in cases: case.pop('config')
        path=args.out/f'{entry:08x}.json'
        path.write_text(json.dumps(dict(entry=entry,code_sha256=hashlib.sha256(h.code(entry,LENGTHS[entry])).hexdigest(),cases=cases)))
        visited=set().union(*(set(c['visited']) for c in cases))
        print(f'{entry:08x}: {len(cases)} returns; {len(visited)} instructions; {path}; sha256={hashlib.sha256(path.read_bytes()).hexdigest()}',flush=True)


if __name__=='__main__': main()
