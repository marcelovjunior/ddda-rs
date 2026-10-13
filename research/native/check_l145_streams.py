"""Original instructions vs whole native inflate; CPU only, install read-only.

Unicorn and Rust share explicitly mapped byte windows. Unowned callees run
their original instructions, including allocators when provided by a case.
Each return compares all GPR/XMM, defined flags, stream/state/window, output
and stack bytes. No real save is read. Random schedules have fixed seeds.
"""
import argparse
import ctypes as c
import hashlib
import random
import struct
import zlib
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
import unicorn.x86_const as xc
import harness as h
from check_l140_streams import archives

REGS = [getattr(xc, 'UC_X86_REG_' + name) for name in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
STREAM, STATE, WINDOW = h.SCRATCH, h.SCRATCH + 0x1000, h.SCRATCH + 0x4000
SOURCE, OUTPUT = 0x20000000, 0x30000000
ESP = h.STACK + h.STACK_SIZE - 0x1000
CALLEE = c.CFUNCTYPE(None, c.c_uint32, c.POINTER(c.c_uint32))


def arc_classes(root):
    """One smallest nonempty compressed member per type across ALL indexes."""
    chosen = {}
    archive_count = entry_count = 0
    for path in sorted(root.rglob('*.arc')):
        with path.open('rb') as f:
            head=f.read(8)
            if len(head)!=8 or head[:4]!=b'ARC\0':
                continue
            archive_count+=1
            count=struct.unpack_from('<H',head,6)[0]
            records=f.read(count*80)
            assert len(records)==count*80
            for index in range(count):
                kind,packed,size,offset=struct.unpack_from('<IIII',records,index*80+64)
                size&=(1<<29)-1
                entry_count+=1
                if packed and (kind not in chosen or size<chosen[kind][0]):
                    chosen[kind]=(size,path,index,packed,offset)
    print(f'ARC census archives={archive_count} entries={entry_count} classes={len(chosen)}',flush=True)
    for kind,(size,path,index,packed,offset) in sorted(chosen.items()):
        with path.open('rb') as f:
            f.seek(offset)
            data=f.read(packed)
        assert len(zlib.decompress(data))==size
        yield f'arc-class-{kind:08x}-{path.name}:{index}',data


def extended_cases(root):
    for name,data in arc_classes(root):
        yield name,data,1,(4096,8192),145,0
    rng=random.Random(145)
    payloads=[b'',b'x',b'abc'*12000,rng.randbytes(70000),bytes(range(256))*300]
    for index,payload in enumerate(payloads):
        for level in (0,1,6,9):
            for seed in (145,607):
                yield f'random-{index}-level{level}',zlib.compress(payload,level),1,(257,521),seed,0
    xml=b'<?xml version="1.0" encoding="UTF-8"?><save>'+b'<u32 name="synthetic" value="145"/>'*4096+b'</save>'
    for flush in (0,1,2,3,4,5):
        yield 'synthetic-save-xml',zlib.compress(xml),1,(1024,8192),145,flush
    payload=b'header and trailer checks'*127
    raw=zlib.compress(payload)[2:-4]
    header=b'\x1f\x8b\x08\x1e'+struct.pack('<I',145)+b'\x00\xff'+struct.pack('<H',7)+b'example'+b'save.xml\0'+b'synthetic comment\0'
    header+=struct.pack('<H',zlib.crc32(header)&0xffff)
    gzip=header+raw+struct.pack('<II',zlib.crc32(payload),len(payload))
    for wrap in (2,3):
        plain=zlib.compress(payload,wbits=31)
        yield 'gzip-plain',plain,wrap,(7,259),145,0
        # The original rejects this Python-valid FHCRC stream even when all
        # input is offered at once. Preserve that behavior, do not repair it.
        yield 'gzip-original-header-crc-rejection',gzip,wrap,(7,259),145,0
        no_crc=header[:3]+b'\x1c'+header[4:-2]+raw+struct.pack('<II',zlib.crc32(payload),len(payload))
        yield 'gzip-extra-name-comment',no_crc,wrap,(7,259),145,0
        yield 'gzip-bad-crc',plain[:-8]+bytes([plain[-8]^1])+plain[-7:],wrap,(7,259),607,0
        yield 'gzip-bad-length',plain[:-1]+bytes([plain[-1]^1]),wrap,(7,259),145,0
    compressor=zlib.compressobj(zdict=b'example preset dictionary')
    yield 'preset-dictionary-needed',compressor.compress(payload)+compressor.flush(),1,(3,71),145,0


class Window(c.Structure):
    _fields_ = [('address', c.c_uint32), ('length', c.c_uint32), ('bytes', c.c_void_p)]


class Machine:
    def __init__(self, data, output_size, wrap):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.ctl_set_tcg_buffer_size(32 << 20)
        self.buffers = []
        self.windows = []
        self.error = None
        self.valid_flags = 0x8d5
        def mapping(address, size, content=b''):
            size = (size + 4095) & ~4095
            buffer = c.create_string_buffer(size)
            if content:
                c.memmove(buffer, content, len(content))
            self.buffers.append(buffer)
            self.windows.append(Window(address, size, c.addressof(buffer)))
            self.uc.mem_map_ptr(address, size, 7, c.addressof(buffer))
        # Writable regions first, for the native adapter's lookup.
        mapping(h.SCRATCH, 0x10000)
        mapping(h.STACK, h.STACK_SIZE)
        mapping(SOURCE, max(4096, len(data)), data)
        mapping(OUTPUT, max(4096, output_size + 65536))
        for address, content in h.image()[1]:
            start = address & ~4095
            mapping(start, address + len(content) - start, bytes(address-start) + content)
        mapping(h.RET & ~4095, 4096)
        self.native_windows = (Window * len(self.windows))(*self.windows)
        self.uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1f80)
        self.uc.reg_write(xc.UC_X86_REG_EFLAGS, 0x202)
        self.set_word(STREAM + 28, STATE)
        self.set_word(STATE + 8, wrap)
        self.set_word(STATE + 28, 15)
        self.set_word(STATE + 44, WINDOW)
        assert self.call(0x00cf00a0, [STREAM]) == 0
        self.set_word(STREAM, SOURCE)
        self.set_word(STREAM + 12, OUTPUT)
        self.callback = CALLEE(self.callee)

    def word(self, a):
        return struct.unpack('<I', self.uc.mem_read(a, 4))[0]

    def set_word(self, a, v):
        self.uc.mem_write(a, struct.pack('<I', v & 0xffffffff))

    def registers(self):
        values = [self.uc.reg_read(r) for r in REGS]
        values.append(self.uc.reg_read(xc.UC_X86_REG_EFLAGS))
        for i in range(8):
            x = self.uc.reg_read(getattr(xc, f'UC_X86_REG_XMM{i}'))
            values.extend((x >> (32*j)) & 0xffffffff for j in range(4))
        return (c.c_uint32 * 41)(*values)

    def restore(self, r):
        for register, value in zip(REGS, r[:8]):
            self.uc.reg_write(register, value)
        self.uc.reg_write(xc.UC_X86_REG_EFLAGS, r[8] | 0x202)
        for i in range(8):
            self.uc.reg_write(getattr(xc, f'UC_X86_REG_XMM{i}'), sum(r[9+i*4+j] << (j*32) for j in range(4)))

    def run_original(self, address, stop):
        # Instruction bound is deterministic. A per-call wall timer creates a
        # Windows timer thread at each of thousands of tiny helper boundaries.
        self.uc.emu_start(address, stop, count=100_000_000)
        assert self.uc.reg_read(xc.UC_X86_REG_EIP) == stop, f'original {address:#x} exceeded budget'

    def callee(self, address, r):
        try:
            self.restore(r)
            self.run_original(address, self.word(r[4]))
            for i,value in enumerate(self.registers()):
                r[i] = value
        except BaseException as error:
            self.error = error

    def call(self, address, args, bridge=None, implementation=1):
        self.uc.reg_write(xc.UC_X86_REG_ESP, ESP)
        self.uc.mem_write(ESP, struct.pack('<' + 'I'*(len(args)+1), h.RET, *args))
        if bridge:
            r = self.registers()
            self.valid_flags = bridge(self.native_windows, len(self.windows), r, self.callback, implementation) & 0x8d5
            if self.error:
                raise self.error
            self.restore(r)
        else:
            self.run_original(address, h.RET)
        assert self.uc.reg_read(xc.UC_X86_REG_ESP) == ESP + 4
        return self.uc.reg_read(xc.UC_X86_REG_EAX)

    def snapshot(self, valid_flags=0x8d5):
        registers = list(self.registers())
        registers[8] &= valid_flags
        digest = hashlib.sha256()
        for buffer in self.buffers[:4]:
            digest.update(buffer)
        return registers, digest.digest()


def check(name, data, wrap, chunks, bridge, seed=0, flush=0, implementation=1):
    try:
        expected = zlib.decompress(data, {0:-15, 1:15, 2:31, 3:47}[wrap])
    except zlib.error:
        expected = None
    output_size = len(expected) if expected is not None else 1 << 20
    baseline, native = [Machine(data, output_size, wrap) for _ in range(2)]
    rng = random.Random(seed)
    limit = 0
    for iteration in range(50000):
        input_chunk, output_chunk = chunks
        if seed:
            input_chunk = rng.randint(1, input_chunk)
            output_chunk = rng.randint(1, output_chunk)
        if baseline.word(STREAM + 4) == 0 and limit < len(data):
            count = min(input_chunk, len(data)-limit)
            for machine in (baseline, native):
                machine.set_word(STREAM + 4, count)
            limit += count
        for machine in (baseline, native):
            machine.set_word(STREAM + 16, output_chunk)
        code = baseline.call(0x00cf02e0, [STREAM, flush])
        native_code = native.call(0x00cf02e0, [STREAM, flush], bridge, implementation)
        if baseline.snapshot(native.valid_flags) != native.snapshot(native.valid_flags):
            print('registers', list(baseline.registers()), list(native.registers()), flush=True)
            for k,(a,b) in enumerate(zip(baseline.buffers[:4],native.buffers[:4])):
                changes=[i for i in range(len(a)) if a[i] != b[i]]
                if changes:
                    print('window',k,'differences',[(hex(baseline.windows[k].address+i),a[i],b[i])for i in changes[:20]],flush=True)
            raise AssertionError(f'{name}: return {iteration} differs ({code:#x}/{native_code:#x})')
        if code in (1, 2, 0xfffffffd, 0xfffffffe, 0xfffffffc):
            break
        if code == 0xfffffffb and limit == len(data) and baseline.word(STREAM+4) == 0:
            break
        assert code in (0, 0xfffffffb), f'unexpected return {code:#x}'
    else:
        raise AssertionError(f'{name}: too many returns')
    output = bytes(native.uc.mem_read(OUTPUT, native.word(STREAM+20)))
    if code == 1:
        assert output == expected, name
    if expected is not None:
        if name=='gzip-original-header-crc-rejection':
            assert code==0xfffffffd
        else:
            assert code==1, f'{name}: original rejected a valid stream ({code:#x})'
    print(f'{name} seed={seed} flush={flush} calls={iteration+1} return={code:#x} output={len(output)} sha256={hashlib.sha256(output).hexdigest()} PASS', flush=True)
    return iteration+1


def record_guards(path):
    """Small callee-free original vectors for cargo's light regression suite."""
    cases=[('null-stream',None),('null-state',(STREAM+28,0)),
           ('null-output',(STREAM+12,0)),('null-input',(STREAM,0))]
    cases += [(f'mode-{mode}',(STATE,mode)) for mode in (0,11,12,26,27,28,29,0xffffffff)]
    records=[]
    def snapshot(machine):
        r=machine.registers()
        data=struct.pack('<41I',*r)
        windows=[(h.SCRATCH,bytes(machine.uc.mem_read(h.SCRATCH,0x10000))),
                 (ESP-512,bytes(machine.uc.mem_read(ESP-512,576))),
                 (0xcf1598,bytes(machine.uc.mem_read(0xcf1598,29*4)))]
        data+=struct.pack('<I',len(windows))
        for address,content in windows:
            data+=struct.pack('<II',address,len(content))+content
        return data
    for name,change in cases:
        machine=Machine(b'',0,1)
        # Distinct nonvolatile, volatile and XMM values expose ABI damage.
        for i,register in enumerate(REGS):
            machine.uc.reg_write(register,0x12340000+i*0x102)
        for i in range(8):
            machine.uc.reg_write(getattr(xc,f'UC_X86_REG_XMM{i}'),(145+i)*0x100000001)
        machine.uc.reg_write(xc.UC_X86_REG_ESP,ESP)
        machine.uc.mem_write(ESP,struct.pack('<III',h.RET,0 if change is None else STREAM,0))
        machine.set_word(STREAM+16,0)
        if change:
            machine.set_word(*change)
        if name=='null-input':
            machine.set_word(STREAM+4,1)
        initial=snapshot(machine)
        machine.run_original(0x00cf02e0,h.RET)
        records.append(initial+snapshot(machine))
    content=b'L145IF01'+struct.pack('<I',len(records))+b''.join(records)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(content)
    print(f'guards={len(records)} bytes={len(content)} sha256={hashlib.sha256(content).hexdigest()}',flush=True)


def record_tables(path):
    """Original cf30c0 leaf including malformed trees and table exhaustion."""
    fixed=[8]*144+[9]*112+[7]*24+[8]*8
    trees=[[],[0]*32,[1],[1,1],[1,1,1],[2,2],[2]*4,
           [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,15],
           fixed,[5]*32,[11]*2048]
    records=[]
    for kind in (0,1,2):
        for index,lengths in enumerate(trees):
            if (kind == 0 and len(lengths) > 19) or (kind == 2 and len(lengths) > 32): continue
            for root in (1,9,15):
                machine=Machine(b'',0x20000,1)
                # Guard/padding bytes and all caller-saved state are nonzero.
                machine.uc.mem_write(h.SCRATCH,random.Random(156).randbytes(0x10000))
                machine.uc.mem_write(ESP-512,random.Random(157).randbytes(576))
                lens,work,table=h.SCRATCH+0x100,h.SCRATCH+0x2000,OUTPUT
                machine.uc.mem_write(lens,struct.pack('<'+'H'*len(lengths),*lengths))
                machine.set_word(h.SCRATCH,table)
                machine.set_word(h.SCRATCH+4,root)
                for i,register in enumerate(REGS): machine.uc.reg_write(register,0x12340000+i*0x102)
                machine.uc.reg_write(xc.UC_X86_REG_ESP,ESP)
                machine.uc.mem_write(ESP,struct.pack('<7I',h.RET,kind,lens,len(lengths),h.SCRATCH,h.SCRATCH+4,work))
                def snapshot():
                    windows=[(h.SCRATCH,bytes(machine.uc.mem_read(h.SCRATCH,0x10000))),
                             (ESP-512,bytes(machine.uc.mem_read(ESP-512,576))),
                             (0x1403f00,bytes(machine.uc.mem_read(0x1403f00,0x1000))),
                             (OUTPUT,bytes(machine.uc.mem_read(OUTPUT,0x20000)))]
                    result=struct.pack('<41II',*machine.registers(),len(windows))
                    for address,data in windows: result+=struct.pack('<II',address,len(data))+data
                    return result
                initial=snapshot()
                try: machine.run_original(0xcf30c0,h.RET)
                except Exception as error: raise RuntimeError(f"table kind={kind} tree={index} root={root}") from error
                records.append(initial+snapshot())
    content=b'L156TB01'+struct.pack('<I',len(records))+b''.join(records)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(content)
    print(f'tables={len(records)} bytes={len(content)} sha256={hashlib.sha256(content).hexdigest()}',flush=True)


def bench_image(path,root):
    def windows(pairs):
        content=struct.pack('<I',len(pairs))
        for address,data in pairs:
            content+=struct.pack('<II',address,len(data))+data
        return content
    payloads=sorted([data for _,data,_ in archives(root)],key=lambda data:len(zlib.decompress(data)),reverse=True)[:2]
    payloads += [zlib.compress(b'original streaming boundary check'*4096)]
    records=[]
    for data in payloads:
        size=len(zlib.decompress(data))
        machine=Machine(data,size,1)
        initial=[(w.address,bytes(b))for w,b in zip(machine.windows[:4],machine.buffers[:4])]
        records.append(windows(initial)+struct.pack('<II',len(data),size))
    common=[(w.address,bytes(b))for w,b in zip(machine.windows[4:],machine.buffers[4:])]
    content=windows(common)+struct.pack('<I',len(records))+b''.join(records)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(content)
    print(f'benchmark image cases={len(records)} sha256={hashlib.sha256(content).hexdigest()}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dll', type=Path, default=Path('C:/ddda-targets/L145/release/l145_inflate_native.dll'))
    parser.add_argument('--install', type=Path, default=Path('E:/SteamLibrary/steamapps/common/DDDA/nativePC'))
    parser.add_argument('--original-lift', action='store_true')
    parser.add_argument('--fast', action='store_true', help='actual native fast leaf through page-backed guest memory')
    parser.add_argument('--table', action='store_true', help='native exact Huffman builder')
    parser.add_argument('--wide', action='store_true', help='L156 FAST=2 page-backed decoder')
    parser.add_argument('--extended', action='store_true')
    parser.add_argument('--record-tables', type=Path)
    parser.add_argument('--record-guards', type=Path)
    parser.add_argument('--bench-image', type=Path)
    args = parser.parse_args()
    if args.record_tables:
        record_tables(args.record_tables)
        return
    if args.record_guards:
        record_guards(args.record_guards)
        return
    if args.bench_image:
        bench_image(args.bench_image,args.install)
        return
    dll = c.CDLL(str(args.dll))
    bridge = dll.l145_inflate
    bridge.argtypes = [c.POINTER(Window), c.c_size_t, c.POINTER(c.c_uint32), CALLEE, c.c_uint32]
    bridge.restype = c.c_uint32
    implementation=(0 if args.original_lift else 1)+(10 if args.wide else 2 if args.fast else 0)+(16 if args.table else 0)
    print(f'oracle implementation={implementation} fast_pages={args.fast or args.wide} table_port={args.table} original_lift={args.original_lift} dll_sha256={hashlib.sha256(args.dll.read_bytes()).hexdigest()}',flush=True)
    cases = [(name,data,wrap,(8192,16384),0,0) for name,data,wrap in archives(args.install)]
    payload = b'original streaming boundary check' * 16
    for chunks in [(1,1),(5,257),(6,258),(4096,4096)]:
        cases.append((f'boundary-{chunks}',zlib.compress(payload),1,chunks,0,0))
    valid=zlib.compress(payload)
    cases.extend((name,data,1,(7,259),0,0)for name,data in [
        ('bad-checksum',valid[:-1]+bytes([valid[-1]^1])),('truncated',valid[:-3]),('bad-header',b'xx'+valid[2:])])
    cases.append(('raw',valid[2:-4],0,(11,127),0,0))
    if args.extended:
        cases.extend(extended_cases(args.install))
    total=0
    for name,data,wrap,chunks,seed,flush in cases:
        total+=check(name,data,wrap,chunks,bridge,seed,flush,implementation)
    print(f'{len(cases)} streams / {total} returns PASS')


if __name__ == '__main__':
    main()
