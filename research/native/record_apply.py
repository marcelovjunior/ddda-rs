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
        before, xmm = self.registers(), self.xmm()
        self.active = True
        uc.emu_start(sv.ADDRESSES[recipe['kind']], h.RET, count=100000)
        self.active = False
        assert uc.reg_read(xc.UC_X86_REG_EIP) == h.RET, 'instruction bound'
        assert uc.reg_read(xc.UC_X86_REG_ESP) == esp+12, 'stack cleanup'
        return dict(kind=recipe['kind'], regs=before, xmm=xmm,
                    initial=sorted(self.initial.items()), calls=self.calls,
                    writes=[(a,uc.mem_read(a,1)[0]) for a in sorted(self.written)],
                    after=self.registers(), after_xmm=self.xmm(),
                    x87=[uc.reg_read(r) for r in (xc.UC_X86_REG_FPCW,xc.UC_X86_REG_FPSW,xc.UC_X86_REG_FPTAG)],
                    visited=sorted(self.visited))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', default='E:/ddda-re/notes/native-L125/apply.json')
    p.add_argument('--count', type=int, default=128)
    p.add_argument('--kinds', default='streams,targets,fixed,shader')
    a = p.parse_args()
    oracle = Oracle()
    kinds = a.kinds.split(',')
    cases = [oracle.record(c,i) for i,c in enumerate(sv.cases(Recipes(),a.count)) if c['kind'] in kinds]
    path = Path(a.out)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(dict(exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
                                   cases=cases),separators=(',',':')))
    print(f'{len(cases)} ABI cases / {sum(len(c["calls"]) for c in cases)} calls -> {path}')


if __name__ == '__main__':
    main()
