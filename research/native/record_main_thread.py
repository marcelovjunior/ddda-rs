"""L130 oracles: E0EC50 recursive traversal and 11787C0 geometry dispatch.

Only maps the original function's code page and synthetic memory. Records
all touched initial bytes, every final written byte, and machine/memory
state at each call AND return. Geometry callees have explicit recorded
synthetic ABI effects. Never runs the game. Private output.
Run with the research Python through heavy.py.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
import harness as h
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

ENTRY = 0xE0EC50
BASE, SIZE = 0xE000000, 0x40000
STACK, STACK_SIZE, SP = 0xF000000, 0x10000, 0xF008000
OWNER, BANK, VALUES, INDICES, OUTPUT = (BASE + v for v in (0, 0x1000, 0x5000, 0x9000, 0x20000))
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX','ECX','EDX','EBX','ESP','EBP','ESI','EDI')]


def recipe(seed, depth, width, terminals, alias=False):
    rng = random.Random(seed)
    words, halves = {OWNER+0x28:BANK}, {}
    next_node, next_index, next_value = 0, INDICES, 0

    def node(level):
        nonlocal next_node, next_index, next_value
        index = next_node
        next_node += 1
        children = width if level and next_node < 180 else 0
        count = terminals if index % 3 == 0 else rng.randrange(min(terminals, 4)+1)
        start = next_index
        next_index += 2*(children+count)
        words[BANK+8*index] = (children << 20) | (count << 26) | rng.randrange(1 << 20)
        words[BANK+8*index+4] = start
        for slot in range(children+count):
            value_index = next_value
            next_value += 1
            halves[start+slot*2] = value_index
            if slot < children:
                child = node(level-1)
                value = child | (rng.randrange(1 << 20) << 12 if rng.randrange(2) else 0)
            else:
                value = rng.getrandbits(32) if rng.randrange(3) else 0
            words[VALUES+8*value_index+4] = value
        return index

    root = node(depth)
    assert next_node < 0x800 and next_index < OUTPUT and next_value < 0x800
    packed = root | (0xfffff000 if seed % 2 else 0)
    # Output over the value table is legal: it changes subsequent reads.
    output = VALUES+32 if alias else OUTPUT
    return words, halves, packed, output


class Oracle:
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        self.uc.mem_map(0xE0E000, 0x1000)
        self.uc.mem_write(ENTRY, h.code(ENTRY, 0x306))
        self.uc.mem_map(BASE, SIZE)
        self.uc.mem_map(STACK, STACK_SIZE)
        self.uc.mem_map(h.RET, 0x1000)
        self.active = False
        self.uc.hook_add(UC_HOOK_MEM_READ, self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        self.uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address+i, b)

    def read(self, uc, access, address, size, value, data):
        if self.active: self.remember(address,size)

    def write(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address,size)
            self.written.update(range(address,address+size))

    def snapshot(self, memory=True):
        uc = self.uc
        result = dict(regs=[uc.reg_read(r) for r in REGS], flags=uc.reg_read(x.UC_X86_REG_EFLAGS)&0xCD5,
                      xmm=[[(uc.reg_read(x.UC_X86_REG_XMM0+i) >> (32*j))&0xffffffff for j in range(4)] for i in range(8)],
                      mxcsr=uc.reg_read(x.UC_X86_REG_MXCSR),
                      x87=[uc.reg_read(getattr(x, 'UC_X86_REG_'+name)) for name in ('FPCW','FPSW','FPTAG')]+[uc.reg_read(x.UC_X86_REG_FP0+i) for i in range(8)])
        if memory: result['memory'] = [(a,uc.mem_read(a,1)[0]) for a in sorted(self.written)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert ENTRY <= address <= 0xE0EF53, hex(address)
        self.visited.add(address)
        if self.last is not None: self.edges.add((self.last,address))
        self.last=address
        if address == ENTRY:
            if self.started: self.events.append(dict(kind='call', **self.snapshot()))
            self.started = True
        elif address == 0xE0EDD6:
            self.events.append(dict(kind='return', **self.snapshot()))

    def run(self, seed, depth, width, terminals, alias=False):
        uc = self.uc
        self.active = False
        rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        words, halves, packed, output = recipe(seed,depth,width,terminals,alias)
        for a,v in words.items(): uc.mem_write(a,struct.pack('<I',v))
        for a,v in halves.items(): uc.mem_write(a,struct.pack('<H',v))
        uc.mem_write(SP,struct.pack('<4I',h.RET,packed,VALUES,output))
        regs=[rng.getrandbits(32),OWNER,rng.getrandbits(32),rng.getrandbits(32),SP,
              rng.getrandbits(32),rng.getrandbits(32),rng.getrandbits(32)]
        for r,v in zip(REGS,regs): uc.reg_write(r,v)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0)  # eight nonempty finite sentinels
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1 << 63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xCD5))
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        self.edges, self.last = set(), None
        before=self.snapshot(False)
        self.started=False
        self.active=True
        uc.emu_start(ENTRY,h.RET,count=2000000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET, 'instruction limit'
        self.active=False
        return dict(seed=seed,depth=depth,width=width,terminals=terminals,alias=alias,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited),edges=sorted(self.edges))


class GeometryOracle(Oracle):
    """Execute the dispatcher; callees use recorded, explicit ABI effects."""
    def __init__(self):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for address, size in ((0x1178000,0x1000),(0x1173000,0x3000),
                              (0x162a000,0x1000),(0x18cf000,0x1000),
                              (BASE,SIZE),(STACK,STACK_SIZE),(h.RET,0x1000)):
            self.uc.mem_map(address,size)
        self.uc.mem_write(0x11787c0,h.code(0x11787c0,0x3d4))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop(); return
        if address < 0x1178000:
            assert address in (0x11731b0,0x1173520,0x1173800,0x1173ae0,0x1173dc0,
                               0x11740a0,0x1174380,0x1174730,0x1174aa0,0x1174d80,
                               0x1175060,0x1175340,0x1175620,0x1175900),hex(address)
            self.events.append(dict(kind='call',target=address,**self.snapshot()))
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            return_to=struct.unpack('<I',uc.mem_read(sp,4))[0]
            # Defined synthetic callee: volatile machine state and owned output.
            for reg in (x.UC_X86_REG_EAX,x.UC_X86_REG_ECX,x.UC_X86_REG_EDX):
                uc.reg_write(reg,self.rng.getrandbits(32))
            for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,self.rng.getrandbits(128))
            uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (self.rng.getrandbits(12)&0xcd5))
            changes=[]
            for a in (OUTPUT,sp+24):
                value=self.rng.getrandbits(32)
                self.remember(a,4)
                self.written.update(range(a,a+4))
                uc.mem_write(a,struct.pack('<I',value))
                changes.extend((a+i,b) for i,b in enumerate(struct.pack('<I',value)))
            uc.reg_write(x.UC_X86_REG_ESP,sp+28)
            self.events.append(dict(kind='return',effects=changes,**self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP,return_to)
            return
        assert 0x11787c0 <= address <= 0x1178b29,hex(address)
        self.visited.add(address)
        if self.last is not None: self.edges.add((self.last,address))
        self.last=address

    def run(self, seed, shape, alternate, alignment):
        uc=self.uc
        self.active=False
        self.rng=rng=random.Random(seed)
        uc.mem_write(BASE,rng.randbytes(SIZE))
        uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        # Synthetic masks, defaults and object lanes also exercise arbitrary bit patterns.
        for a,size in ((0x162ac60,16),(0x162adb0,4),(0x18cfe80,16)):
            uc.mem_write(a,rng.randbytes(size))
        uc.mem_write(OWNER+4,struct.pack('<I',shape))
        sp=SP+alignment
        uc.mem_write(sp,struct.pack('<7I',h.RET,OWNER,*[rng.getrandbits(32) for _ in range(3)],alternate,rng.getrandbits(32)))
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,sp)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f)
        uc.reg_write(x.UC_X86_REG_FPSW,0)
        uc.reg_write(x.UC_X86_REG_FPTAG,0)
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1<<63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        self.edges,self.last=set(),None
        before=self.snapshot(False)
        self.active=True
        uc.emu_start(0x11787c0,h.RET,count=1000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(seed=seed,shape=shape,alternate=alternate,alignment=alignment,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited),edges=sorted(self.edges))


class IdentifierOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for address,size in ((0xcd8000,0x1000),(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,0x1000)):
            self.uc.mem_map(address,size)
        self.uc.mem_write(0xcd8760,h.code(0xcd8760,0x72))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self,uc,address,size,data):
        if address==h.RET: uc.emu_stop(); return
        if BASE+0x38000 <= address < BASE+0x38100:
            assert address in (BASE+0x38000,BASE+0x38010,BASE+0x38020,BASE+0x38030)
            self.events.append(dict(kind='call',target=address,**self.snapshot()))
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            return_to=struct.unpack('<I',uc.mem_read(sp,4))[0]
            is_count=address in (BASE+0x38000,BASE+0x38020)
            if is_count:
                value=self.count
                if self.count_calls and self.mode==4: value=0
                if self.count_calls and self.mode==5: value=self.count+3
                self.count_calls+=1
            else:
                index=struct.unpack('<I',uc.mem_read(sp+4,4))[0]
                assert index<40
                value=0 if self.mode==0 else BASE+0xa000+index*0x40
            uc.reg_write(x.UC_X86_REG_EAX,value)
            for reg in (x.UC_X86_REG_ECX,x.UC_X86_REG_EDX): uc.reg_write(reg,self.rng.getrandbits(32))
            for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,self.rng.getrandbits(128))
            uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (self.rng.getrandbits(12)&0xcd5))
            # Changing the vtable tests the original reload before every call.
            self.remember(OWNER,4); self.written.update(range(OWNER,OWNER+4))
            bits=struct.pack('<I',BANK+0x100 if len(self.events)%4==1 else BANK)
            uc.mem_write(OWNER,bits)
            uc.reg_write(x.UC_X86_REG_ESP,sp+(4 if is_count else 8))
            self.events.append(dict(kind='return',effects=[(OWNER+i,b) for i,b in enumerate(bits)],**self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP,return_to)
            return
        assert 0xcd8760 <= address <= 0xcd87d1,hex(address)
        self.visited.add(address)
        if self.last is not None: self.edges.add((self.last,address))
        self.last=address

    def run(self,seed,identifier,count,mode):
        uc=self.uc; self.active=False; self.rng=rng=random.Random(seed)
        self.count,self.mode,self.count_calls=count,mode,0
        uc.mem_write(BASE,rng.randbytes(SIZE)); uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        def word(a,v): uc.mem_write(a,struct.pack('<I',v))
        word(OWNER,BANK)
        for delta in (0,0x100):
            word(BANK+delta+0x40,BASE+0x38000+(0x20 if delta else 0))
            word(BANK+delta+0x3c,BASE+0x38010+(0x20 if delta else 0))
        for i in range(40):
            hi,lo=identifier>>16,identifier&0xffff
            if i != count-1 or mode in (0,1,2,4,5):
                if mode%2: hi=(hi+(-1 if i%2 else 1))&0xffff
                else: lo=(lo+(-1 if i%2 else 1))&0xffff
            uc.mem_write(BASE+0xa000+i*0x40+0x10,struct.pack('<HH',hi,lo))
        word(SP,h.RET)
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,SP); uc.reg_write(x.UC_X86_REG_EAX,identifier)
        uc.reg_write(x.UC_X86_REG_ESI,OWNER)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f); uc.reg_write(x.UC_X86_REG_FPSW,0); uc.reg_write(x.UC_X86_REG_FPTAG,0)
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1<<63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        self.edges,self.last=set(),None; before=self.snapshot(False); self.active=True
        uc.emu_start(0xcd8760,h.RET,count=10000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(seed=seed,identifier=identifier,count=count,mode=mode,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited),edges=sorted(self.edges))


DURATIONS = (0x112bb40,0x112bb60,0x112bb80,0x112bba0,0x112bbc0,0x112bbe0)


class DurationOracle(Oracle):
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for address,size in ((0x112b000,0x1000),(BASE,0x1000),(STACK,STACK_SIZE),(h.RET,0x1000)):
            self.uc.mem_map(address,size)
        self.uc.mem_write(0x112bb40,h.code(0x112bb40,0xc0))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self,uc,address,size,data):
        if address==h.RET: uc.emu_stop(); return
        assert self.entry <= address < self.entry+0x20,hex(address)

    def run(self,seed,entry,value,depth,rounding):
        import x87_host
        uc=self.uc; self.active=False; self.entry=entry; rng=random.Random(seed)
        uc.mem_write(BASE,rng.randbytes(0x1000)); uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        offset={0x112bb40:6,0x112bb60:3,0x112bb80:7,0x112bba0:0,0x112bbc0:4,0x112bbe0:4}[entry]
        uc.mem_write(OWNER+offset,struct.pack('<I',value))
        uc.mem_write(SP,struct.pack('<2I',h.RET,OWNER))
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,SP)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        top=seed%8
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f | (rounding<<10))
        uc.reg_write(x.UC_X86_REG_FPSW,top<<11)
        tags=0xffff
        for i in range(depth): tags &= ~(3 << (((top+i)%8)*2))
        uc.reg_write(x.UC_X86_REG_FPTAG,tags)
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1<<63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202 | (rng.getrandbits(12)&0xcd5))
        self.initial,self.written={},set()
        before=self.snapshot(False); initial_x87=x87_host.unicorn_state(uc); self.active=True
        uc.emu_start(entry,h.RET,count=30)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET
        self.active=False
        after=self.snapshot()
        after['x87_executed']=True
        hardware,_,_=x87_host.execute(initial_x87,0xdb,0,uc.mem_read(SP+4,4))
        x87_host.set_unicorn_state(uc,hardware)
        after['hardware_x87']=self.snapshot(False)['x87']
        if depth < 8:
            assert after['x87']==after['hardware_x87'],('FILD hardware mismatch',entry,seed)
        else:
            # Unicorn overwrites a full stack without raising the hardware's
            # masked stack fault/indefinite result. Retain both observations;
            # the original x86 FILD instruction on the host is authoritative.
            assert after['x87']!=after['hardware_x87']
        # SHR by >1 has undefined AF/OF; compare all architecturally
        # defined arithmetic flags plus DF, never invent undefined bits.
        after['flag_mask']=0x4c5 if entry in (0x112bbc0,0x112bbe0) else 0xcd5
        return dict(entry=entry,seed=seed,value=value,depth=depth,rounding=rounding,
                    before=before,initial=sorted(self.initial.items()),events=[],after=after)


class RecordingOracle(Oracle):
    """Recording-side ABI oracle; memcpy is an explicit synthetic callee.

    The callback copies exactly the requested bytes and varies volatile
    machine state. It is not a replacement claim for the original memcpy.
    """
    def __init__(self):
        self.uc=Uc(UC_ARCH_X86,UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(16*1024*1024)
        for address,size in ((0xfed000,0x2000),(0x12ef000,0x1000),(BASE,SIZE),(STACK,STACK_SIZE),(h.RET,0x1000)):
            self.uc.mem_map(address,size)
        self.uc.mem_write(0xfedb80,h.code(0xfedb80,0x7a))
        self.active=False
        self.uc.hook_add(UC_HOOK_MEM_READ,self.read)
        self.uc.hook_add(UC_HOOK_MEM_WRITE,self.write)
        self.uc.hook_add(UC_HOOK_CODE,self.code)

    def code(self,uc,address,size,data):
        if address==h.RET: uc.emu_stop(); return
        if address==0x12efea0:
            event=dict(kind='call',target=address,**self.snapshot())
            # IMUL defines CF/OF only. Preserve/check DF as well.
            event['flag_mask']=0xc01
            self.events.append(event)
            sp=uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp,16)
            return_to,destination,source,length=struct.unpack('<IIII',uc.mem_read(sp,16))
            assert length<=0x8144
            self.remember(source,length); self.remember(destination,length)
            payload=bytes(uc.mem_read(source,length))
            effects=dict(zip(range(destination,destination+length),payload))
            uc.mem_write(destination,payload)
            if self.mode:
                # The caller must reload depth and the overwritten argument.
                for a,v in ((OWNER+0x30874,0xffffffff),(SP+4,self.rng.getrandbits(32))):
                    self.remember(a,4)
                    bits=struct.pack('<I',v); uc.mem_write(a,bits)
                    effects.update(zip(range(a,a+4),bits))
            self.written.update(effects)
            for reg in REGS[:3]: uc.reg_write(reg,self.rng.getrandbits(32))
            for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,self.rng.getrandbits(128))
            uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|(self.rng.getrandbits(12)&0xcd5))
            uc.reg_write(x.UC_X86_REG_ESP,sp+4)
            self.events.append(dict(kind='return',effects=sorted(effects.items()),**self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP,return_to)
            return
        assert 0xfedb80<=address<=0xfedbf9,hex(address)
        self.visited.add(address)
        if self.last is not None: self.edges.add((self.last,address))
        self.last=address

    def run(self,seed,depth,count,mode):
        uc=self.uc; self.active=False; self.rng=rng=random.Random(seed); self.mode=mode
        uc.mem_write(BASE,rng.randbytes(SIZE)); uc.mem_write(STACK,rng.randbytes(STACK_SIZE))
        for a,v in ((SP,h.RET),(SP+4,rng.getrandbits(32)),(SP+8,rng.getrandbits(32)),
                    (OWNER+0x30874,depth),(OWNER+0x14,count)):
            uc.mem_write(a,struct.pack('<I',v))
        for reg in REGS: uc.reg_write(reg,rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP,SP); uc.reg_write(x.UC_X86_REG_ECX,OWNER)
        for i in range(8): uc.reg_write(x.UC_X86_REG_XMM0+i,rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_FPCW,0x37f); uc.reg_write(x.UC_X86_REG_FPSW,0); uc.reg_write(x.UC_X86_REG_FPTAG,0)
        for i in range(8): uc.reg_write(x.UC_X86_REG_FP0+i,((1<<63)|rng.getrandbits(63),0x3fff))
        uc.reg_write(x.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(x.UC_X86_REG_EFLAGS,0x202|(rng.getrandbits(12)&0xcd5))
        self.initial,self.written,self.events,self.visited={},set(),[],set()
        self.edges,self.last=set(),None; before=self.snapshot(False); self.active=True
        uc.emu_start(0xfedb80,h.RET,count=10000)
        assert uc.reg_read(x.UC_X86_REG_EIP)==h.RET,'instruction limit'
        self.active=False
        return dict(entry=0xfedb80,seed=seed,depth=depth,count=count,mode=mode,
                    before=before,initial=sorted(self.initial.items()),events=self.events,
                    after=self.snapshot(),visited=sorted(self.visited),edges=sorted(self.edges))


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out')
    p.add_argument('--geometry',action='store_true')
    p.add_argument('--identifiers',action='store_true')
    p.add_argument('--durations',action='store_true')
    p.add_argument('--recording',action='store_true')
    a=p.parse_args()
    if a.recording:
        oracle=RecordingOracle()
        cases=[oracle.run(i+1,*recipe) for i,recipe in enumerate(
            ((depth,count,mode) for depth in range(4) for count in (0,1,8,32,256,4096) for mode in (0,1)))]
        out=Path(a.out or 'E:/ddda-re/notes/native-L130/recording.json'); out.parent.mkdir(parents=True,exist_ok=True)
        out.write_text(json.dumps(dict(source='DDDA.exe 2017-12-13 Steam',
                       exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),cases=cases),separators=(',',':')))
        print(f'{len(cases)} recording cases, {sum(len(c["events"]) for c in cases)} boundaries -> {out}')
        return
    if a.durations:
        oracle=DurationOracle()
        rng=random.Random(130)
        values=[0,1,15,16,127,128,255,256,32767,32768,65535,0xffffffff]+[rng.getrandbits(32) for _ in range(32)]
        cases=[oracle.run(i+1,*recipe) for i,recipe in enumerate(
            ((entry,value,depth,rounding) for entry in DURATIONS for value in values
             for depth in (0,3,7,8) for rounding in range(4)))]
        out=Path(a.out or 'E:/ddda-re/notes/native-L130/durations.json'); out.parent.mkdir(parents=True,exist_ok=True)
        out.write_text(json.dumps(dict(source='DDDA.exe 2017-12-13 Steam',
                       exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
                       code={str(entry):h.code(entry,0x20).hex() for entry in DURATIONS},cases=cases),separators=(',',':')))
        # Exact isolated lifter output for missing inventory entries. Never
        # modifies patterns, inventory, generated shards or the canonical DB.
        import lift
        sources=[]
        for entry in DURATIONS:
            sources.append(lift.rust_fn(entry,'animation duration',lift.lift(entry,0x20,ext=True)))
        out.with_suffix('.rs').write_text('\n'.join(sources))
        print(f'{len(cases)} duration cases and six isolated lifted bodies -> {out}')
        return
    if a.identifiers:
        oracle=IdentifierOracle()
        cases=[oracle.run(i+1,*recipe) for i,recipe in enumerate(
            ((identifier,count,mode) for identifier in (0,0xffffffff,0x80000000,0x610001,0x620000,0x620001,0x70ffff,0x7affff,0x7b0000)
             for count in (0,1,8,32) for mode in range(6)))]
        out=Path(a.out or 'E:/ddda-re/notes/native-L130/identifiers.json'); out.parent.mkdir(parents=True,exist_ok=True)
        edges={tuple(e) for c in cases for e in c['edges']}
        branches=[i for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(h.code(0xcd8760,0x72),0xcd8760)
                  if i.mnemonic.startswith('j') and i.mnemonic!='jmp']
        missing=[(hex(i.address),hex(t)) for i in branches for t in (i.address+i.size,int(i.op_str,16)) if (i.address,t) not in edges]
        # EAX == FFFFFFFF cannot survive the preceding negative-input return.
        assert missing==[('0xcd876c','0xcd878a')],missing
        out.write_text(json.dumps(dict(source='DDDA.exe 2017-12-13 Steam',address=0xcd8760,
                       exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
                       uncovered_edges=missing,cases=cases),separators=(',',':')))
        print(f'{len(cases)} identifier cases, {sum(len(c["events"]) for c in cases)} boundaries -> {out}')
        return
    if a.geometry:
        oracle=GeometryOracle()
        cases=[oracle.run(i+1,shape,alt,align) for i,(shape,alt,align) in enumerate(
            ( (shape,alt,align) for shape in (*range(24),0xffffffff,0x80000000)
              for alt in (0,1,0x100,0x80,0xffffffff) for align in range(16) ))]
        out=Path(a.out or 'E:/ddda-re/notes/native-L130/geometry.json')
        out.parent.mkdir(parents=True,exist_ok=True)
        edges={tuple(e) for c in cases for e in c['edges']}
        branches=[i for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(h.code(0x11787c0,0x36c),0x11787c0)
                  if i.mnemonic.startswith('j') and i.mnemonic!='jmp']
        assert all((i.address,target) in edges for i in branches
                   for target in (i.address+i.size,int(i.op_str,16)))
        assert len({e['target'] for c in cases for e in c['events'] if e['kind']=='call'})==14
        out.write_text(json.dumps(dict(source='DDDA.exe 2017-12-13 Steam',address=0x11787c0,
                        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
                        conditional_edges=2*len(branches),cases=cases),separators=(',',':')))
        print(f'{len(cases)} geometry cases, {sum(len(c["events"]) for c in cases)} boundaries -> {out}')
        return
    oracle=Oracle()
    cases=[]
    for depth in range(13):
        for width in (1,2):
            for terminals in (0,1,3):
                seed=len(cases)+1
                cases.append(oracle.run(seed,depth,width,terminals))
    for depth in range(5):
        for terminals in (0,1,63):
            cases.append(oracle.run(len(cases)+1,depth,1,terminals))
    # Deterministic output/value overlap without introducing cycles.
    for seed in range(201,217): cases.append(oracle.run(seed,0,1,63,True))
    for terminals in (0,1,3): cases.append(oracle.run(len(cases)+1,1,63,terminals))
    out=Path(a.out or 'E:/ddda-re/notes/native-L130/dependencies.json'); out.parent.mkdir(parents=True,exist_ok=True)
    edges=set(tuple(e) for c in cases for e in c['edges'])
    branches=[i for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(h.code(ENTRY,0x306),ENTRY)
              if i.mnemonic.startswith('j') and i.mnemonic!='jmp']
    missing=[(hex(i.address),hex(target)) for i in branches
             for target in (i.address+i.size,int(i.op_str,16)) if (i.address,target) not in edges]
    assert not missing, ('uncovered conditional edges',missing)
    result=dict(source='DDDA.exe 2017-12-13 Steam',address=ENTRY,
                conditional_edges=2*len(branches),exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),cases=cases)
    out.write_text(json.dumps(result,separators=(',',':')))
    print(f'{len(cases)} cases, {sum(len(c["events"]) for c in cases)} boundaries, {len(set().union(*(set(c["visited"]) for c in cases)))} instructions -> {out}',flush=True)


if __name__=='__main__': main()
