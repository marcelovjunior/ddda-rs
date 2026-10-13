"""Compare CFEEC0 with the production Rust skip through ORIGINAL refill/zlib.

CPU-only, no game process. Installed archives are read-only. Both sides run
CFE9D0 -> DB92C0 -> CF02E0 and its original helpers. Only Win32 critical-section
calls/Sleep are explicit deterministic boundary stubs on a private workspace.
All stream, workspace, output and stack bytes and defined flags are compared.
"""
import argparse
import ctypes as c
import hashlib
import struct
import zlib
from pathlib import Path
from unicorn import UC_HOOK_CODE
import unicorn.x86_const as xc
from check_l145_streams import Machine, Window, CALLEE, STREAM, STATE, WINDOW, SOURCE, OUTPUT, ESP, REGS
from check_l140_streams import archives
import harness as h

READER, DECOMP, WORKSPACE, ENTRY = h.SCRATCH + 0x800, h.SCRATCH + 0x900, h.SCRATCH + 0xa00, h.SCRATCH + 0xb00


class SkipMachine(Machine):
    def __init__(self, data, size, capacity, wrapped=False, delayed=False):
        super().__init__(data, max(size, capacity), 1)
        ring_start=max(0,len(data)-7) if wrapped else 0
        if wrapped:
            # Same complete compressed member, rotated inside a published ring.
            # Original DB92C0 must split input across the ring's physical end.
            ring=bytearray(len(data))
            for i,b in enumerate(data):ring[(ring_start+i)%len(ring)]=b
            self.uc.mem_write(SOURCE,bytes(ring))
        self.uc.mem_write(DECOMP + 0x14, bytes(self.uc.mem_read(STREAM, 56)))
        for a, v in {
            DECOMP: 0x142a550, DECOMP+4: WORKSPACE, DECOMP+8: ENTRY,
            DECOMP+12: 0, DECOMP+16: size,
            WORKSPACE+0x20: SOURCE, WORKSPACE+0x24: len(data),
            WORKSPACE+0x2c: ring_start+len(data)+1,
            ENTRY+4: ring_start, ENTRY+0xc: len(data), ENTRY+0x10: 0,
            READER: 0x1407150, READER+4: DECOMP, READER+8: OUTPUT,
            READER+0xc: 0, READER+0x10: 0, READER+0x14: capacity,
        }.items(): self.set_word(a, v)
        self.boundaries = []
        self.services = []
        self.refills = 0
        self.publish_end=ring_start+len(data)+1
        self.delayed=delayed
        if delayed:self.set_word(WORKSPACE+0x2c,0)
        # Dedicated targets in the already mapped return-sentinel page.
        self.services_at = {h.RET+0x100: 'enter', h.RET+0x110: 'leave', h.RET+0x120: 'sleep'}
        for iat, target in [(0x139d160,h.RET+0x100),(0x139d164,h.RET+0x110),(0x139d15c,h.RET+0x120)]:
            self.set_word(iat,target)
        for address in [0xcfe9d0,*self.services_at]:
            self.uc.hook_add(UC_HOOK_CODE,self.observe,begin=address,end=address)

    def observe(self, uc, address, size, _):
        if address == 0xcfe9d0:
            self.refills += 1
            regs = list(self.registers()); regs[8] &= 0x8d5
            self.boundaries.append((regs, hashlib.sha256(bytes(uc.mem_read(h.SCRATCH,0x10000))).hexdigest()))
        if address in self.services_at:
            sp = uc.reg_read(xc.UC_X86_REG_ESP)
            self.services.append((self.services_at[address], self.word(sp+4)))
            if self.services_at[address]=='sleep':
                assert self.delayed,'unexpected producer wait in fully published fixture'
                # Explicit synthetic producer publication at the first Sleep.
                self.set_word(WORKSPACE+0x2c,self.publish_end)
            uc.reg_write(xc.UC_X86_REG_EAX,1)
            uc.reg_write(xc.UC_X86_REG_ESP,sp+8)
            uc.reg_write(xc.UC_X86_REG_EIP,self.word(sp))

    def skip(self, count, bridge=None):
        self.uc.reg_write(xc.UC_X86_REG_ECX,READER)
        self.uc.reg_write(xc.UC_X86_REG_ESP,ESP)
        self.uc.mem_write(ESP,struct.pack('<II',h.RET,count))
        if bridge:
            r=self.registers()
            self.valid_flags=bridge(self.native_windows,len(self.windows),r,self.callback)&0x8d5
            if self.error: raise self.error
            self.restore(r)
        else:
            self.run_original(0xcfeec0,h.RET)
        assert self.uc.reg_read(xc.UC_X86_REG_ESP)==ESP+8

    def read(self, count, output):
        self.uc.reg_write(xc.UC_X86_REG_ECX,READER)
        self.uc.reg_write(xc.UC_X86_REG_ESP,ESP)
        self.uc.mem_write(ESP,struct.pack('<III',h.RET,output,count))
        self.run_original(0xcfec00,h.RET)
        assert self.uc.reg_read(xc.UC_X86_REG_ESP)==ESP+12
        return self.uc.reg_read(xc.UC_X86_REG_EAX)


def check(name, data, capacity, bridge, wrapped=False, delayed=False):
    payload=zlib.decompress(data)
    baseline,native=[SkipMachine(data,len(payload),capacity,wrapped,delayed) for _ in range(2)]
    # Both buffered spans and repeated real refills; final EOF failures too.
    schedule=[]
    remaining=len(payload)
    for wanted in [0,1,capacity-1,1,capacity+1,len(payload)//3,len(payload)]:
        count=min(wanted,remaining);schedule.append(count);remaining-=count
    schedule.append(17)
    for index,count in enumerate(schedule):
        baseline.skip(count)
        native.skip(count,bridge)
        if baseline.snapshot(native.valid_flags)!=native.snapshot(native.valid_flags):
            for k,(a,b) in enumerate(zip(baseline.buffers[:4],native.buffers[:4])):
                diffs=[i for i in range(len(a)) if a[i]!=b[i]]
                if diffs: print('memory-diff',k,[(hex(baseline.windows[k].address+i),a[i],b[i])for i in diffs[:16]],flush=True)
            print('registers',list(baseline.registers()),list(native.registers()),flush=True)
            raise AssertionError(f'{name} capacity={capacity} step={index} count={count}')
        assert baseline.boundaries==native.boundaries, (name,'refill boundaries')
        assert baseline.services==native.services, (name,'service effects')
    assert native.word(DECOMP+12)==len(payload)
    assert native.word(DECOMP+0x14+20)==len(payload)
    assert sum(name=='sleep' for name,_ in native.services)==int(delayed)
    print(f'{name} capacity={capacity} wrapped={wrapped} delayed={delayed} bytes={len(payload)} steps={len(schedule)} refills={native.refills} services={len(native.services)} PASS',flush=True)
    return len(schedule)


def textures(root):
    """Read eight real mip-bearing textures, at most one per shape/format."""
    seen=set()
    kind=(~zlib.crc32(b'rTexture'))&0x7fffffff
    for path in sorted(root.rglob('*.arc')):
        with path.open('rb') as f:
            head=f.read(8)
            if len(head)!=8 or head[:4]!=b'ARC\0':continue
            records=f.read(struct.unpack_from('<H',head,6)[0]*80)
            for index in range(len(records)//80):
                entry_kind,packed,size,offset=struct.unpack_from('<IIII',records,index*80+64)
                size&=(1<<29)-1
                if entry_kind!=kind or not 4096<=size<=262144:continue
                f.seek(offset);data=f.read(packed);payload=zlib.decompress(data)
                magic,version,dims,shape=struct.unpack_from('<4I',payload)
                assert magic==0x584554
                key=(version>>28,(shape>>8)&255)
                mips=dims&63;faces=shape&255
                if key in seen or mips<2:continue
                table=16+(108 if key[0]==6 else 0)
                offsets=struct.unpack_from('<'+'I'*(mips*faces),payload,table)
                seen.add(key)
                yield f'{path.name}:{index}',data,offsets
                if len(seen)==8:return


def check_texture(name,data,offsets,capacity,bridge):
    payload=zlib.decompress(data)
    baseline,native=[SkipMachine(data,len(payload),capacity) for _ in range(2)]
    position=0;steps=0
    for mip in range(1,len(offsets),2):
        offset=offsets[mip]
        assert position<=offset<len(payload)
        next_offset=offsets[mip+1] if mip+1<len(offsets) else len(payload)
        count=min(17,next_offset-offset)
        baseline.skip(offset-position);native.skip(offset-position,bridge)
        assert baseline.snapshot(native.valid_flags)==native.snapshot(native.valid_flags),(name,'texture skip')
        for machine in [baseline,native]:
            output=OUTPUT+capacity+256
            assert machine.read(count,output)==count
            assert bytes(machine.uc.mem_read(output,count))==payload[offset:offset+count],(name,'selected mip bytes')
        assert baseline.snapshot()==native.snapshot(),(name,'post-skip original read')
        assert baseline.boundaries==native.boundaries
        assert baseline.services==native.services
        position=offset+count;steps+=1
    print(f'texture {name} capacity={capacity} bytes={len(payload)} selected_mips={steps} refills={native.refills} PASS',flush=True)
    return steps


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dll',type=Path,default=Path('C:/ddda-targets/L165/release/l165_skip_chain.dll'))
    p.add_argument('--install',type=Path,default=Path('E:/SteamLibrary/steamapps/common/DDDA/nativePC'))
    args=p.parse_args()
    dll=c.CDLL(str(args.dll));bridge=dll.l165_skip
    bridge.argtypes=[c.POINTER(Window),c.c_size_t,c.POINTER(c.c_uint32),CALLEE];bridge.restype=c.c_uint32
    print('bridge_sha256='+hashlib.sha256(args.dll.read_bytes()).hexdigest(),flush=True)
    total=0
    for name,data,wrap in archives(args.install):
        assert wrap==1
        for capacity in [257,4096,32768]: total+=check(name,data,capacity,bridge)
        total+=check(name,data,4096,bridge,wrapped=True,delayed=True)
    selected=0
    for name,data,offsets in textures(args.install):
        for capacity in [257,4096,32768]:selected+=check_texture(name,data,offsets,capacity,bridge)
    assert selected>0
    print(f'{total} composed skip returns + {selected} actual texture skip/read pairs PASS',flush=True)

if __name__=='__main__':main()
