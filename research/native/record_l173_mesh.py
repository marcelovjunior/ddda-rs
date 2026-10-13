"""Head #14007: EA5320 ABI oracle, synthetic callees with recorded effects.

No game. Writes native-L171/draw_mesh.json, a private original-instruction
corpus. Call signatures from original sites/RET; no callee implementation
claim. Volatile returns are randomized except view/filter/grow contracts.
"""
import hashlib
import json
import random
import struct
from pathlib import Path
import unicorn.x86_const as x
import record_l173 as draw
from record_main_thread import REGS,BASE,STACK,STACK_SIZE,SP
import harness as h

ENTRY,LENGTH=0xea5320,0x747
MODEL,CTX,MESH,PRIMS,MATERIALS,POSITION,VIEW,LODS,QUALITY,LISTENER,VTABLE,VERTICES,STREAM,BOUNDS,SLOTS,ARENA=(
    BASE+n for n in (0,0x10000,0x20000,0x21000,0x22000,0x23000,0x24000,0x25000,0x26000,0x27000,0x28000,0x29000,0x2a000,0x2b000,0x30000,0x40000))
CALLBACK=0x1200000
SIGNATURES={0xfed940:(0,0),0xea42e0:(3,12),0xff14d0:(0,0),0xfed190:(2,8),
            0xea23f0:(2,8),0xfeef20:(0,0),0xfed890:(1,4),0xff1920:(0,0),CALLBACK:(4,16)}

class MeshOracle(draw.DrawOracle):
    def __init__(self):
        draw.SIZES[ENTRY]=LENGTH
        draw.CALLS.clear();draw.CALLS.update(SIGNATURES)
        super().__init__(ENTRY)

    def code(self,u,address,size,data):
        if address not in SIGNATURES:
            assert ENTRY<=address<ENTRY+LENGTH,hex(address)
            self.visited.add(address)
            return
        argc,pop=SIGNATURES[address];sp=u.reg_read(x.UC_X86_REG_ESP)
        self.remember(sp,4+4*argc)
        args=[self.readword(sp+4+4*k) for k in range(argc)]
        if address==CALLBACK:self.remember(args[3],16)
        state=self.snapshot()
        state['memory']=[(a,u.mem_read(a,1)[0]) for a in sorted(self.initial)]
        self.events.append(dict(kind='call',target=address,args=args,**state))
        effects=[]
        def effect(a,value):
            self.remember(a,4);self.word(a,value)
            self.written.update(range(a,a+4))
            effects.extend((a+k,b) for k,b in enumerate(struct.pack('<I',value&0xffffffff)))
        result=self.rng.getrandbits(32)
        if address==0xfed940:result=VIEW
        elif address==0xea42e0:
            index=(args[1]-PRIMS)//48
            result=0xffffffff if self.reject and index%2 else 0
        elif address==0xfed890:
            effect(CTX+0x24,ARENA+0x10000)
            effect(CTX+0x28,ARENA+0x12000)
            result=ARENA+0x10000
        elif address==0xfed190:
            # Explicit fake material contract; changes data consumed by the
            # caller's command append, testing its reads across the call.
            for offset in (0x194,0x198,0x19c,0x1a0):effect(CTX+offset,self.rng.getrandbits(32))
        for r in REGS[:3]:u.reg_write(r,self.rng.getrandbits(32))
        u.reg_write(x.UC_X86_REG_EAX,result)
        u.reg_write(x.UC_X86_REG_ESP,sp+4+pop)
        self.events.append(dict(kind='return',effects=effects,**self.snapshot()))
        u.reg_write(x.UC_X86_REG_EIP,self.readword(sp))

    def run(self,seed,lod,quality,sort,depth,bounds,flags,count,filter_kind):
        u=self.uc;self.active=False;self.rng=rng=random.Random(seed)
        self.reject=filter_kind==2
        u.mem_write(BASE,rng.randbytes(0x90000));u.mem_write(STACK,rng.randbytes(STACK_SIZE))
        w=self.word
        def f(a,v):u.mem_write(a,struct.pack('<f',v))
        w(0x18d2ce8,seed);w(0x18d1d20,QUALITY);w(QUALITY+0x38,quality)
        w(0x18d2c80,LISTENER);w(LISTENER,VTABLE);w(VTABLE+0x34,CALLBACK)
        for off,value in {0x14c:VERTICES if seed%2 else 0,0x24:ARENA,0x28:ARENA if flags&0x100 else ARENA+4096,
                          0x44:SLOTS,0x48:0,0x5c:flags,0x6c:seed%7,0x74:0x7ffe,
                          0x164:sort,0x168:0x00010000,0x178:17,0x124:0,0x138:0,0x134:0}.items():w(CTX+off,value)
        for k,v in enumerate((0.25,-0.5,0.75)):f(CTX+0x184+4*k,v)
        if flags&0x10000:w(CTX+0x168,0x00200000)
        w(MESH+0x70,PRIMS);w(MESH+0x74,count);w(MESH+0x9c,VERTICES);w(MESH+0xa0,STREAM)
        for row in range(4):
            for col in range(4):f(MODEL+0xb0+16*row+4*col,(1.0+row*0.25) if row==col else (0.125 if col==3 else 0.0))
        w(MODEL+0xf0,LODS);w(LODS+0xe0,10);w(LODS+0xe4,100)
        w(MODEL+0x100,0x13572468);w(MODEL+0x104,1 if bounds else 0)
        w(MODEL+0x108,((lod&15)<<9)|(3<<25));w(MODEL+0x10c,seed%16)
        w(MODEL+0x110,0xffffffff);w(MODEL+0x154,seed%19-9);w(MODEL+0x15c,1 if seed%7==0 else 0)
        f(MODEL+0x16c,0.25)
        for row in range(4):
            for col in range(4):f(VIEW+16*row+4*col,1.0 if row==col else 0.0)
        f(VIEW+0x38,-200.25)
        for k,v in enumerate((1.25,2.5,float((seed%5)*50)+0.75)):f(POSITION+4*k,v)
        for k in range(8):w(MATERIALS+4*k,BASE+0x2c000+64*k)
        for k in range(count):
            prim=PRIMS+48*k;bound=BOUNDS+32*k
            # Different masks make LOD selection observable in the calls and
            # output count, rather than merely visiting its branches.
            lod_bits=(7,1,2,4,3,7)[k%6]
            w(prim,1);w(prim+4,k|((k//2)<<12)|(lod_bits<<24))
            w(prim+8,(1|(4 if depth else 0))|(k<<8)|(3<<16));w(prim+16,0x5a5a0000+k)
            w(prim+24,0x101+k);w(prim+28,0x202+k);w(prim+32,0x303+k)
            w(prim+44,bound)
            for j,v in enumerate((1.25+k,2.5-k,3.75,0.875)):f(bound+16+4*j,v)
        # Each last primitive of the rejection scenarios fails a different
        # visibility gate, independently of the synthetic pass filter.
        if count and filter_kind==2:
            rejection=seed%4;prim=PRIMS+48*(count-1)
            if rejection==0:w(MODEL+0x110,0xffffffff^(1<<(count-1)))
            elif rejection==1:w(prim+8,0)
            elif rejection==2:w(prim+4,self.readword(prim+4)&0xffffff)
            else:w(prim,0)
        w(SP,h.RET)
        for k,v in enumerate((CTX,MESH,MATERIALS,POSITION,0x3f if filter_kind==0 else 2,rng.getrandbits(32))):w(SP+4+4*k,v)
        for r in REGS:u.reg_write(r,rng.getrandbits(32))
        u.reg_write(x.UC_X86_REG_ECX,MODEL);u.reg_write(x.UC_X86_REG_ESP,SP)
        u.reg_write(x.UC_X86_REG_EFLAGS,0x202);u.reg_write(x.UC_X86_REG_FPCW,0x37f)
        u.reg_write(x.UC_X86_REG_FPSW,0);u.reg_write(x.UC_X86_REG_FPTAG,0xffff);u.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        # The entire incoming ABI frame is known, even on early-return paths
        # where the original does not load all six arguments. Semantic code
        # may read these valid arguments eagerly without inventing contents.
        self.remember(SP,28)
        before=self.snapshot(False)
        self.active=True;u.emu_start(ENTRY,h.RET,count=100000)
        assert u.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(seed=seed,lod=lod,quality=quality,sort=sort,depth=depth,bounds=bounds,flags=flags,count=count,filter=filter_kind,
                    before=before,initial=sorted(self.initial.items()),events=self.events,after=self.snapshot(),visited=sorted(self.visited))

def main():
    oracle=MeshOracle();cases=[]
    for lod in (-1,1,2,4):
        for quality in (1,2,3):
            for sort,depth in ((0x10,False),(0x10,True),(0xe,False),(0xe,True),(3,False)):
                for flags in (0,0x10,0x40,0x100,0x150):
                    seed=len(cases)+1
                    cases.append(oracle.run(seed,lod,quality,sort,depth,seed%2,flags,6,seed%3))
    cases.append(oracle.run(len(cases)+1,1,1,3,False,False,0,0,0))
    cases.append(oracle.run(len(cases)+1,1,1,3,False,False,0x10000,6,0))
    out=Path('E:/ddda-re/notes/native-L171/draw_mesh.json');out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text(json.dumps(dict(entry=ENTRY,code_sha256=hashlib.sha256(h.code(ENTRY,LENGTH)).hexdigest(),cases=cases)))
    print(f'{len(cases)} original returns; {out}; sha256={hashlib.sha256(out.read_bytes()).hexdigest()}',flush=True)

if __name__=='__main__':main()
