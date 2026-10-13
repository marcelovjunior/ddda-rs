"""L125 whole-ABI nDraw vectors; original instructions, synthetic call boundaries.

Run through heavy.py with the research Python. Output is private. Reuses L117
object recipes, but records all GPR/XMM state at calls, all read initial bytes
and final written bytes, and final x87 state. Helpers (including the packed
applies) are explicit mocked boundaries, just like the lifted differential.
This does not execute the game or open a window.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path

from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as xc
import state_vectors as sv
import harness as h

REGS = [getattr(xc, 'UC_X86_REG_' + n) for n in
        ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
HELPERS = {0xda7510: 3, 0xda77d0: 3, 0xda7b30: 3,
           0xda7250: 4, 0xda68a0: 0}


class Recipes:
    def run(self, kind, words, **kw):
        return dict(kind=kind, words=words, **kw)


class Oracle(sv.Oracle):
    def __init__(self):
        self.active = False
        super().__init__()
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read_hook)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write_hook)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address+i, b)

    def read_hook(self, uc, access, address, size, value, _):
        if self.active:
            self.remember(address, size)

    def write_hook(self, uc, access, address, size, value, _):
        if self.active:
            self.remember(address, size)
            self.written.update(range(address, address+size))

    def registers(self):
        return [self.uc.reg_read(r) for r in REGS]

    def xmm(self):
        return [[(self.uc.reg_read(xc.UC_X86_REG_XMM0+i) >> (j*32)) & 0xffffffff
                 for j in range(4)] for i in range(8)]

    def hook(self, uc, address, size, _):
        if not self.active:
            return
        self.visited.add(address)
        if self.last_address is not None: self.edges.add((self.last_address,address))
        self.last_address=address
        # Record the original x87 sequence for an independent host-instruction
        # oracle: Unicorn omits precision status for inexact reciprocal stores.
        if address in (0xdaae55,0xdaaea8,0xdab325,0xdab378):
            offset=0x30 if address>=0xdab000 else 0x2c
            self.x87_ops.append(['load',self.read(uc.reg_read(xc.UC_X86_REG_ESP)+offset)[0]])
        elif address in (0xdaaeb1,0xdab381): self.x87_ops.append(['one',0])
        elif address in (0xdaaeb6,0xdab386): self.x87_ops.append(['divide',0])
        elif address in (0xdaae5c,0xdaaebb,0xdab32c,0xdab38b): self.x87_ops.append(['store',0])
        if address == h.RET:
            uc.emu_stop()
            return
        slot = (address-sv.STUBS)//16
        com = sv.STUBS <= address < sv.STUBS+4096 and slot in sv.COUNTS
        if address not in HELPERS and not com:
            return
        n = sv.COUNTS[slot]+1 if com else HELPERS[address]
        esp = uc.reg_read(xc.UC_X86_REG_ESP)
        self.remember(esp, 4*(n+1))
        args = self.read(esp+4, n)
        self.calls.append(dict(target=address, return_to=self.read(esp)[0],
                               regs=self.registers(), xmm=self.xmm(), args=args,
                               x87_op_count=len(self.x87_ops),
                               memory=[(a,uc.mem_read(a,1)[0]) for a in sorted(self.written)]))
        # Deterministic volatile clobbers, all nonvolatile state preserved.
        index = len(self.calls)
        uc.reg_write(xc.UC_X86_REG_EAX, 0 if com else 0x51500000 | index)
        uc.reg_write(xc.UC_X86_REG_ECX, 0xc1c10000 ^ index)
        uc.reg_write(xc.UC_X86_REG_EDX, 0xd2d20000 ^ index)
        uc.reg_write(xc.UC_X86_REG_ESP, esp+4+n*4)
        uc.reg_write(xc.UC_X86_REG_EIP, self.read(esp)[0])

    def record(self, recipe, seed):
        uc = self.uc
        self.active = False
        uc.mem_write(sv.BASE, bytes(sv.SIZE))
        uc.mem_write(h.STACK, bytes(h.STACK_SIZE))
        words = {sv.CTX+4:sv.DEVICE, sv.DEVICE:sv.VTABLE,
                 0x018d1d20:sv.OPTIONS, sv.OPTIONS+0x34:recipe.get('filtering', 0)}
        words.update({sv.VTABLE+s*4:sv.STUBS+s*16 for s in sv.COUNTS})
        words.update(recipe['words'])
        for a, v in words.items():
            uc.mem_write(a, struct.pack('<I', v & 0xffffffff))
        for i, reg in enumerate(REGS):
            uc.reg_write(reg, (0x12345678*(i+1) + seed) & 0xffffffff)
        for i in range(8):
            uc.reg_write(xc.UC_X86_REG_XMM0+i, (0x3f800000+i) * ((1 << 96)+(1 << 64)+(1 << 32)+1))
        for reg, value in [(xc.UC_X86_REG_EFLAGS,2), (xc.UC_X86_REG_MXCSR,0x1f80),
                           (xc.UC_X86_REG_FPCW,0x37f), (xc.UC_X86_REG_FPSW,0),
                           (xc.UC_X86_REG_FPTAG,0xffff)]:
            uc.reg_write(reg,value)
        esp = h.STACK+h.STACK_SIZE-0x1000
        uc.mem_write(esp,struct.pack('<III',h.RET,sv.CTX,sv.NEW))
        uc.reg_write(xc.UC_X86_REG_ESP,esp)
        uc.reg_write(xc.UC_X86_REG_ECX,sv.RENDER)
        self.initial, self.written, self.calls, self.visited = {}, set(), [], set()
        self.x87_ops=[]
        self.edges=set()
        self.last_address=None
        before, xmm = self.registers(), self.xmm()
        self.active = True
        try:
            uc.emu_start(sv.ADDRESSES[recipe['kind']], h.RET, count=100000)
        except Exception as error:
            raise RuntimeError(f"case {seed} {recipe['kind']}, eip={uc.reg_read(xc.UC_X86_REG_EIP):08x}, regs={self.registers()}") from error
        self.active = False
        assert uc.reg_read(xc.UC_X86_REG_EIP) == h.RET, 'instruction bound'
        assert uc.reg_read(xc.UC_X86_REG_ESP) == esp+12, 'stack cleanup'
        return dict(kind=recipe['kind'], regs=before, xmm=xmm,
                    initial=sorted(self.initial.items()), calls=self.calls,
                    writes=[(a,uc.mem_read(a,1)[0]) for a in sorted(self.written)],
                    after=self.registers(), after_xmm=self.xmm(),
                    x87=[uc.reg_read(r) for r in (xc.UC_X86_REG_FPCW,xc.UC_X86_REG_FPSW,xc.UC_X86_REG_FPTAG)],
                    x87_ops=self.x87_ops,
                    visited=sorted(self.visited),edges=sorted(self.edges))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', default='E:/ddda-re/notes/native-L125/apply.jsonl')
    p.add_argument('--count', type=int, default=256)
    p.add_argument('--kinds', default='streams,targets,fixed,shader')
    a = p.parse_args()
    oracle = Oracle()
    kinds = a.kinds.split(',')
    recipes=[c for c in sv.cases(Recipes(),a.count) if c['kind'] in kinds]
    for kind in ('targets','fixed','streams'):
        base=next((c for c in recipes if c['kind']==kind),None)
        if base is None: continue
        for seed in range(96):
            words=dict(base['words'])
            if kind=='targets':
                words[sv.CTX+8]=sv.OLD
                words[sv.NEW+0x14]=0
                for j in range(5):
                    view=sv.BASE+0x6000+j*0x100
                    words[sv.NEW+j*4]=view if seed&1 else 0
                    words[sv.CTX+0x84+j*4]=words[sv.NEW+j*4] if seed%3 else view
                for j,v in enumerate([10,20,100,200]):
                    words[sv.NEW+0x18+j*4]=v
                    words[sv.NEW+0x38+j*4]=v
                    words[sv.CTX+0x64+j*4]=v
                    words[sv.CTX+0x74+j*4]=v
                words[sv.CTX+(0x64 if seed%2 else 0x74)+(seed//2%4)*4]+=1
            elif kind=='fixed':
                words[sv.CTX+12]=0 if seed%5==0 else sv.OLD
                for j,off in enumerate([0x18,0x20,0x1c]):
                    words[sv.CTX+off]=[0,words[sv.NEW+8+j*4],words[sv.OLD+8+j*4]][(seed//(3**j))%3]
                if seed%2:words[sv.OLD]=words[sv.NEW]
                if seed%3:words[sv.OLD+4]=words[sv.NEW+4]
            else:
                words[sv.CTX+0x14]=sv.OLD
                words[sv.CTX+0x30]=0
                for j in range(4):
                    vb=sv.BASE+0x6000+j*0x100
                    words[sv.NEW+j*4]=vb if seed&1 else 0
                    words[sv.CTX+0x34+j*4]=words[vb+0x20] if seed&1 else 0
                    words[sv.CTX+0x44+j*4]=[8,16,24,32][j] if seed&1 else 0
                    words[sv.CTX+0x54+j*4]=words[sv.NEW+0x14+j*4] if seed&1 else 0
            recipes.append(dict(kind=kind,words=words))
    shader=next((c for c in recipes if c['kind']=='shader' and c['words'].get(sv.CTX+0x10)!=sv.NEW),None)
    if shader is not None:
        for seed in range(a.count):
            words=dict(shader['words'])
            metadata,descriptors=sv.BASE+0x6000,sv.BASE+0x6100
            words[sv.CTX+0x10]=sv.OLD if seed%3 else 0
            words[sv.CTX+0x24]=0x100 if seed%4 else 99
            words[sv.CTX+0x28]=0x200 if seed%5 else 99
            if words[sv.CTX+0x10]==0:
                words[sv.CTX+0x24]=0
                words[sv.CTX+0x28]=0
            words[sv.NEW+20]=(sv.BASE+0x9000)|(seed%4)
            words[sv.OLD+20]=words[sv.NEW+20] if seed%2 else sv.BASE+0x9000
            words[sv.NEW]=(seed%8)<<3 | (seed%256)<<11 | (0x180000 if seed%2 else 0)
            if seed%13==0: words[sv.NEW]|=0x100000;words[sv.NEW]&=~0x80000
            if seed%7==0:
                flags=words[sv.NEW]
                words[sv.CTX]=(0x2000000 if flags&0x180000==0x180000 else 0)|((flags&0x7f8)<<6)|((flags>>10)&0x1fe)
                words[sv.CTX+0x2c]=0x300
            words[sv.RENDER+0x30]=(seed%3)<<8
            words[sv.BASE+0x8020]=[0,1,2,127,1023][seed%5] | (63<<16) | ((seed%2)<<30)
            if seed%11==0:words[sv.OLD+16]=sv.BASE+0x8200
            cursor=descriptors
            for pixel in (False,True):
                # Independently empty/nonempty sections; multiple entries,
                # repeated slots, caps and derived scratch reuse.
                counts=[(seed//(3**j))%5 for j in range(5)]
                if pixel: counts=[(n+seed%3)%5 for n in reversed(counts)]
                entries=[]
                for j in range(counts[0]): entries.append((j%4) | 1<<22)
                for section,count in enumerate(counts[1:]):
                    for j in range(count):
                        if section==3:
                            kind=[3,7,1][(seed+j)%3]
                            entries.append(2 | kind<<8 | (50+j)<<16 | (seed%3)<<24)
                        else:
                            n=[0,1,15,16,17,31][(seed+j)%6]
                            entries.append(4 | (j*4)<<6 | (j+3)<<16 | n<<24 |
                                           (int((seed+j)%2==0) if section==0 else 0))
                summary=len(entries) | counts[0]<<8 | counts[1]<<12 | counts[2]<<19 | counts[3]<<23 | counts[4]<<28
                words[metadata+(12 if pixel else 8)]=summary
                for j,d in enumerate(entries): words[cursor+j*4]=d
                cursor+=len(entries)*4
            recipes.append(dict(kind='shader',words=words,filtering=seed%7))
    path = Path(a.out)
    path.parent.mkdir(parents=True,exist_ok=True)
    calls=0
    with path.open('w') as f:
        f.write(json.dumps(dict(exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest()))+'\n')
        for i,c in enumerate(recipes):
            case=oracle.record(c,i)
            calls+=len(case['calls'])
            f.write(json.dumps(case,separators=(',',':'))+'\n')
    print(f'{len(recipes)} ABI cases / {calls} calls -> {path}')


if __name__ == '__main__':
    main()
