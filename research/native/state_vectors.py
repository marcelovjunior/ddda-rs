"""L117: original nDraw apply calls on constructed guest objects, private output.

Run with the research pyenv, through heavy.py. No game window is opened.
The user's unpacked exe is read at runtime. Device methods are recording
stdcall stubs returning S_OK; the apply functions and their helpers execute
original instructions. Pointer arguments are copied at call time; only the
COM `this` and temporary stack addresses are omitted from call comparison.
No vector, exe byte, or decompiler output belongs in the repository.
"""
import argparse
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
import unicorn.x86_const as xc
import harness as h

BASE, SIZE = h.SCRATCH, 0x20000
CTX, DEVICE, VTABLE = BASE, BASE + 0x1000, BASE + 0x1100
NEW, OLD, OPTIONS, RENDER = BASE+0x2000, BASE+0x3000, BASE+0x4000, BASE+0x5000
STUBS = 0x0D000000
COUNTS = {57: 2, 69: 3, 37: 2, 39: 1, 47: 1, 75: 1, 100: 4, 104: 1,
          87: 1, 92: 1, 107: 1, 65: 2, 94: 3, 96: 3, 98: 3,
          109: 3, 111: 3, 113: 3, 86: 2}
ADDRESSES = {'blend': 0xDA7510, 'depth': 0xDA77D0, 'raster': 0xDA7B30,
             'sampler': 0xDA7250, 'fixed': 0xDAA5F0, 'streams': 0xDAA7C0,
             'targets': 0xDA6FE0, 'shader': 0xDAA8B0}


class Oracle:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(h.TCG_BUFFER)
        for va, data in h.image()[1]:
            start = va & ~4095
            self.uc.mem_map(start, (va+len(data)-start+4095) & ~4095)
            self.uc.mem_write(va, data)
        for addr, size in [(BASE,SIZE),(h.STACK,h.STACK_SIZE),(STUBS,4096),(h.RET,4096)]:
            self.uc.mem_map(addr,size)
        for slot, count in COUNTS.items():
            self.uc.mem_write(STUBS+slot*16, b'\x31\xc0\xc2'+struct.pack('<H',(count+1)*4))
        self.uc.hook_add(UC_HOOK_CODE,self.hook)

    def read(self, addr, n=1):
        return list(struct.unpack('<'+'I'*n,self.uc.mem_read(addr,4*n)))

    def hook(self, uc, address, size, _):
        if address == h.RET:
            uc.emu_stop()
            return
        if STUBS <= address < STUBS+4096 and (address-STUBS)%16 == 0:
            slot = (address-STUBS)//16
            assert slot in COUNTS, hex(address)
            esp = uc.reg_read(xc.UC_X86_REG_ESP)
            values = self.read(esp+4,COUNTS[slot]+1)
            assert values.pop(0) == DEVICE
            data = []
            if slot in (47,75):
                data = self.read(values.pop(0),6 if slot == 47 else 4)
            elif slot in (94,96,98,109,111,113):
                pointer = values.pop(1)
                data = self.read(pointer,values[1]*(1 if slot in (98,113) else 4))
            elif slot == 86:
                pointer, output = values
                values = []
                for i in range(17):
                    elem = self.read(pointer+i*8,2)
                    data.extend(elem)
                    if elem == [255,17]: break
                else: raise AssertionError('unterminated declaration')
                uc.mem_write(output,struct.pack('<I',0x9876))
            self.calls.append({'slot':slot,'args':values,'data':data})

    def run(self, kind, words, *, old=0, stage=0, filtering=0, address=None, args=None, ecx=RENDER, outputs=None):
        uc = self.uc
        uc.mem_write(BASE,bytes(SIZE))
        uc.mem_write(h.STACK,bytes(h.STACK_SIZE))
        for reg in [xc.UC_X86_REG_EAX,xc.UC_X86_REG_EBX,xc.UC_X86_REG_ECX,xc.UC_X86_REG_EDX,
                    xc.UC_X86_REG_ESI,xc.UC_X86_REG_EDI,xc.UC_X86_REG_EBP]:
            uc.reg_write(reg,0)
        uc.reg_write(xc.UC_X86_REG_EFLAGS,2)
        uc.reg_write(xc.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(xc.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(xc.UC_X86_REG_FPTAG,0xffff)
        base = {CTX+4:DEVICE,DEVICE:VTABLE,0x018D1D20:OPTIONS,OPTIONS+0x34:filtering}
        base.update({VTABLE+s*4:STUBS+s*16 for s in COUNTS})
        base.update(words)
        for a,v in base.items(): uc.mem_write(a,struct.pack('<I',v & 0xffffffff))
        if args is None:
            args = [CTX,NEW]
            if kind in ('blend','depth','raster'): args += [old]
            if kind == 'sampler': args = [CTX,stage,NEW,old]
        esp = h.STACK+h.STACK_SIZE-0x1000
        uc.mem_write(esp,struct.pack('<'+'I'*(1+len(args)),h.RET,*args))
        uc.reg_write(xc.UC_X86_REG_ESP,esp)
        uc.reg_write(xc.UC_X86_REG_ECX,ecx)
        self.calls = []
        uc.emu_start(address or ADDRESSES[kind],h.RET,count=100000)
        assert uc.reg_read(xc.UC_X86_REG_EIP) == h.RET, 'instruction bound'
        assert uc.reg_read(xc.UC_X86_REG_ESP) == esp+4+4*len(args), 'ABI cleanup'
        outputs = outputs or [(CTX,0xe28)]
        return {'kind':kind,'words':sorted((a,v & 0xffffffff) for a,v in base.items()),
                'old':old,'stage':stage,'filtering':filtering,'calls':self.calls,
                'after':[(a+i*4,v) for a,n in outputs for i,v in enumerate(self.read(a,n//4))],
                'invalid_viewport':self.read(0x018D04E4,4)}


def cases(oracle, count):
    r = random.Random(117)
    constructors={'blend':(0x117CB70,0x20,8),'depth':(0x117C8E0,0x18,8),
                  'raster':(0x117C6E0,0x20,24),'sampler':(0x117CD70,0x30,12)}
    f32=lambda x:struct.unpack('<I',struct.pack('<f',x))[0]
    for kind,(address,offset,size) in constructors.items():
        for i in range(count):
            if kind=='blend':
                d=[r.randrange(2)<<1 | r.randrange(13)<<9 | r.randrange(13)<<17 | r.randrange(5)<<25,
                   r.randrange(13) | r.randrange(13)<<8 | r.randrange(5)<<16,r.getrandbits(32),r.getrandbits(32)]
            elif kind=='depth':
                d=[r.randrange(4) | r.randrange(8)<<2 | r.randrange(2)<<6 | r.randrange(256)<<7 |
                   r.randrange(256)<<15 | r.randrange(8)<<23 | r.randrange(8)<<27,
                   sum(r.randrange(8)<<(j*4) for j in range(6))]
                if i%3==0:
                    d[1]=(d[1]&255) | ((d[0]>>23)&15)<<8 | ((d[0]>>27)&15)<<12 | (d[1]&255)<<16
            elif kind=='raster':
                d=[r.randrange(2) | r.randrange(3)<<3 | r.randrange(32)<<6,r.getrandbits(32),r.getrandbits(32),r.getrandbits(32)]
            else:
                d=[(i%20) | r.randrange(5)<<8 | r.randrange(5)<<11 | r.randrange(5)<<14 | r.randrange(32)<<17,
                   r.getrandbits(32),f32(r.uniform(-20,20)),r.getrandbits(32),
                   *[f32(r.uniform(-1,2)) for _ in range(4)]]
            words={OLD+j*4:v for j,v in enumerate(d)}
            case=oracle.run('construct_'+kind,words,address=address,args=[OLD],ecx=NEW,outputs=[(NEW+offset,size)])
            case['descriptor']=d
            yield case
    for kind in ('blend','depth','raster','sampler'):
        for i in range(count):
            new = [r.getrandbits(32) for _ in range(3)]
            old = [r.getrandbits(32) for _ in range(3)] if i%3 else list(new)
            if kind == 'sampler' and i%2: new[0] = new[0]&~(15<<13) | 3<<13
            if i < 96: new = [0,0,0];new[i//32]=1<<(i%32);old=[0,0,0]
            words = {NEW+j*4:v for j,v in enumerate(new)}
            words.update({OLD+j*4:v for j,v in enumerate(old)})
            yield oracle.run(kind,words,old=0 if i%4==0 else OLD,stage=r.choice([0,15,257,260]),filtering=i%7)
    for i in range(count):
        words = {}
        for binding,bank in [(NEW,BASE+0x6000),(OLD,BASE+0x7000)]:
            words[binding]=r.getrandbits(32)
            words[binding+4]=r.randrange(512)
            for j,size in enumerate([8,8,24]):
                obj = bank+j*0x100
                words[binding+8+j*4]=obj
                words[obj+12]=r.randrange(4)
                offset = 0x18 if j==1 else 0x20
                for k in range(size//4): words[obj+offset+k*4]=r.getrandbits(32)
        if i%6:
            words[CTX+12]=OLD
            for off,j in [(0x18,0),(0x20,1),(0x1c,2)]:
                words[CTX+off]=words[OLD+8+j*4]
                if i%6==1: words[NEW+8+j*4]=words[OLD+8+j*4]
        if i%6==2: words[CTX+12]=NEW
        yield oracle.run('fixed',words)
    for i in range(count):
        words = {NEW+0x10:0x20181008}
        for j in range(4):
            vb = BASE+0x6000+j*0x100
            words[NEW+j*4]=vb if r.randrange(3) else 0
            words[NEW+0x14+j*4]=r.randrange(128)
            words[vb+0x14]=r.randrange(128,4096)
            words[vb+0x20]=r.randrange(1,20)
            for off in [0x34,0x44,0x54]: words[CTX+off+j*4]=r.randrange(10)
        words[NEW+0x28]=BASE+0x6500 if i%2 else 0
        words[BASE+0x6520]=7
        words[CTX+0x30]=r.randrange(10)
        if i%7==0: words[CTX+0x14]=NEW
        yield oracle.run('streams',words)
    for i in range(count):
        words = {NEW+0x14:(i%2)<<9,NEW+0x58:640 | 480<<16}
        for j in range(5):
            view = BASE+0x6000+j*0x100
            words[NEW+j*4]=view if r.randrange(2) else 0
            words[view+0x14]=j+100
            words[CTX+0x84+j*4]=words[NEW+j*4] if r.randrange(2) else 0
        for j in range(16): words[NEW+0x18+j*4]=r.randrange(-50,750)&0xffffffff
        if i%7==0: words[CTX+8]=NEW
        yield oracle.run('targets',words)
    for i in range(count):
        metadata,descriptors=BASE+0x6000,BASE+0x6100
        sampler,texture,values=BASE+0x7000,BASE+0x8000,BASE+0x9000
        words={NEW:r.randrange(8)<<3 | r.randrange(256)<<11 | (0x180000 if i%2 else 0),
               NEW+8:metadata, metadata+4:descriptors, metadata+0x10:0x100,
               metadata+0x14:0x200,metadata+0x18:0x300,
               NEW+12:sampler,NEW+16:texture,NEW+20:values|(i%3),
               OLD+12:sampler,OLD+16:texture,OLD+20:values|(i%2),
               sampler+12:42,texture+0x20:127 | 63<<16 | (i%2)<<30,
               texture+0x24:31<<16,texture+0x2c:(i%3==0)<<19,texture+0x34:0x777,
               RENDER+0x30:(i%2)<<8}
        for j in range(3):words[sampler+0x30+j*4]=r.getrandbits(32)
        for j in range(100):words[values+j*4]=r.getrandbits(32)
        for pixel in [False,True]:
            stage=r.randrange(16 if pixel else 4)
            entry=descriptors+(24 if pixel else 0)
            # One descriptor for every section, plus a second float conversion.
            ds=[stage | 1<<22,
                (2<<1) | (2<<6) | (7<<16) | (3<<24),
                1 | (2<<1) | (1<<6) | (23<<16) | (2<<24),
                (2<<1) | (3<<6) | (31<<16) | (2<<24),
                (2<<1) | (7<<6) | (2<<16) | (3<<24),
                (1<<1) | ((3 if i%2 else 7)<<8) | (55<<16) | (1<<24)]
            if i%9==0: ds=[]
            for j,d in enumerate(ds):words[entry+j*4]=d
            words[metadata+(12 if pixel else 8)]=(6 | 1<<8 | 2<<12 | 1<<19 | 1<<23 | 1<<28) if ds else 6
            if i%4:
                words[CTX+0x10]=OLD
                words[CTX+(0x28 if pixel else 0x24)]=(0x200 if pixel else 0x100)
                cache=CTX+(0xd8 if pixel else 0x98)+stage*4
                words[cache]=sampler
                words[cache+0x80]=texture if i%4==1 else 0
        if i%5==0:words[CTX+0x24]=99
        if i%7==0:words[CTX+0x10]=NEW
        yield oracle.run('shader',words,filtering=i%7)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',default='E:/ddda-re/notes/native-L117/state-vectors.json')
    parser.add_argument('--count',type=int,default=256)
    args = parser.parse_args()
    oracle = Oracle()
    result = list(cases(oracle,args.count))
    path = Path(args.out)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps({'source':'DDDA.exe 2017-12-13 Steam','cases':result},separators=(',',':')))
    print(f'{len(result)} cases / {sum(len(c["calls"]) for c in result)} calls -> {path}')


if __name__ == '__main__': main()
