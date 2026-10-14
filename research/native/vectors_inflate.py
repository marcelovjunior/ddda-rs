"""L109 original zlib fast-loop oracle. No game process or generated shards.

Runs 0x00cf2cb0 from the user's analysis exe in Unicorn, using synthetic
streams/tables/windows. Writes private binary vectors, including all GPRs,
scratch and stack bytes. No mocked callees (the function is a leaf).
Run through heavy.py with the research Python; output defaults to
E:/ddda-re/notes/native-L109/vectors/inflate_fast.bin.
"""
import os
import random
import struct
import zlib
from pathlib import Path
import vectors_core as core


original_uc = core.Uc
final_flags = 0
def oracle(*args, **kwargs):
    """Extend the existing recorder with the leaf's final arithmetic flags."""
    uc = original_uc(*args, **kwargs)
    start = uc.emu_start
    def run(*args, **kwargs):
        global final_flags
        start(*args, **kwargs)
        final_flags = uc.reg_read(core.xc.UC_X86_REG_EFLAGS)
    uc.emu_start = run
    return uc


core.Uc = oracle


def packed(operation, bits, value):
    return operation | bits << 8 | value << 16


def reverse(value, count):
    return int(f"{value:0{count}b}"[::-1], 2)


def fixed_tables():
    lengths = [8] * 144 + [9] * 112 + [7] * 24 + [8] * 8
    counts = [lengths.count(i) for i in range(10)]
    codes, next_code = 0, {}
    for bits in range(1, 10):
        codes = (codes + counts[bits - 1]) << 1
        next_code[bits] = codes
    # RFC 1951 length/distance base values, generated from their bit widths.
    length_extra = [0]*8 + [1]*4 + [2]*4 + [3]*4 + [4]*4 + [5]*4 + [0]
    length_base, value = [], 3
    for extra in length_extra[:-1]:
        length_base.append(value)
        value += 1 << extra
    length_base.append(258)
    literal = [0]*512
    for symbol, bits in enumerate(lengths):
        code = reverse(next_code[bits], bits)
        next_code[bits] += 1
        entry = (packed(0, bits, symbol) if symbol < 256 else
                 packed(96, bits, 0) if symbol == 256 else
                 packed(16 + length_extra[symbol-257], bits, length_base[symbol-257]) if symbol <= 285 else
                 packed(64, bits, 0))
        for index in range(code, 512, 1 << bits):
            literal[index] = entry
    distance, value = [0]*32, 1
    for symbol in range(32):
        extra = max(0, symbol//2-1)
        distance[reverse(symbol, 5)] = packed(16+extra, 5, value) if symbol < 30 else packed(64, 5, 0)
        value += 1 << extra
    return literal, distance


def case(seed, literal, distance, data, *, hold=0, bits=0, out=512,
         history=0, window_size=256, window_have=256, window_write=0,
         literal_bits=9, distance_bits=5):
    rnd = random.Random(seed)
    sc = core.Scratch(rnd)
    stream = sc.alloc(56)
    state = sc.alloc(0x100)
    source = sc.alloc(len(data)+8)
    output = sc.alloc(1024+history)
    window = sc.alloc(window_size)
    literals = sc.alloc(4*len(literal))
    distances = sc.alloc(4*len(distance))
    sc.b[source-core.SCRATCH:source-core.SCRATCH+len(data)] = data
    for i, value in enumerate(literal): sc.u32(literals+4*i, value)
    for i, value in enumerate(distance): sc.u32(distances+4*i, value)
    for offset, value in {0: source, 4: len(data), 12: output+history, 16: out, 28: state}.items():
        sc.u32(stream+offset, value)
    for offset, value in {0: 20, 0x20: window_size, 0x24: window_have,
                          0x28: window_write, 0x2c: window, 0x30: hold,
                          0x34: bits, 0x44: literals, 0x48: distances,
                          0x4c: literal_bits, 0x50: distance_bits}.items():
        sc.u32(state+offset, value)
    regs = {name: rnd.getrandbits(32) for name in core.GPR}
    regs['esp'] = core.ESP
    xmm = [[rnd.getrandbits(32) for _ in range(4)] for _ in range(8)]
    result = core.run(0x00cf2cb0, regs, xmm, sc, [stream, out+history], [])
    assert not result['calls']
    result['flags'] = final_flags
    return result


def generate():
    literals, distances = fixed_tables()
    cases = []
    for seed in range(80):
        rnd = random.Random(seed)
        payload = (bytes(range(64)) if seed % 3 == 0 else b'abracadabra' if seed % 3 == 1 else rnd.randbytes(90)) * 8
        compressor = zlib.compressobj(6, zlib.DEFLATED, -15, 8, zlib.Z_FIXED)
        data = compressor.compress(payload)+compressor.flush()
        assert (data[0] >> 1) & 3 == 1
        cases.append(case(seed, literals, distances, data[1:]+bytes(8), hold=data[0]>>3,
                          bits=5, out=[258, 259, 511, 1024][seed % 4], window_have=0))
    # Every window-copy arm, triple/remainder path, extra bits, and short
    # input/output exit. Table entries describe valid fast-loop contracts;
    # table construction is deliberately outside the function under test.
    for write in [0, 1, 8, 128, 255]:
        for history in [0, 7, 32]:
            for length in [3, 4, 5, 8, 17, 258]:
                for distance in [1, 8, 64, 256, 300]:
                    cases.append(case(len(cases), [packed(16, 1, length)]*2,
                        [packed(16, 1, distance)]*2, bytes(6),
                        history=history, out=258, window_write=write,
                        literal_bits=1, distance_bits=1))
    for extra in range(1, 14):
        cases.append(case(len(cases), [packed(16+min(extra,5),1,3)]*2,
            [packed(16+extra,1,1)]*2, bytes(10), literal_bits=1, distance_bits=1))
    for literal in [packed(0,1,77), packed(96,1,0), packed(64,1,0), packed(16,1,3)]:
        for distance in [packed(64,1,0), packed(16,1,1)]:
            for nested in [False, True]:
                lit = [packed(1,1,2)]*2+[literal]*2 if nested else [literal]*2
                dist = [packed(1,1,2)]*2+[distance]*2 if nested else [distance]*2
                cases.append(case(len(cases), lit, dist, bytes(6), literal_bits=1, distance_bits=1))
    return cases


def dynamic_cases():
    """Capture real fast-loop calls while the original outer inflate builds
    dynamic tables and expands complete synthetic raw-DEFLATE streams.

    Initial state is raw TYPEDO (12), no history, 15 window bits, as read by
    0x00cf02e0. The output can hold the whole stream, so no window allocator
    is needed. Outer inflate and table-building instructions run unmocked.
    The final outer output must equal the synthetic plaintext as well.
    """
    results = []
    for seed in range(24):
        rnd = random.Random(10000+seed)
        # Unequal symbol frequencies force a dynamic tree, unlike tiny or
        # uniformly random streams which often select fixed/stored blocks.
        payload = bytes(rnd.choices(range(48), weights=range(48,0,-1), k=8192))
        compressor = zlib.compressobj(6, zlib.DEFLATED, -15)
        data = compressor.compress(payload)+compressor.flush()
        assert (data[0] >> 1) & 3 == 2, 'expected dynamic Huffman block'
        uc = original_uc(core.UC_ARCH_X86, core.UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(core.h.TCG_BUFFER)
        for va, b in core.h.image()[1]:
            start = va & ~0xfff
            uc.mem_map(start, (va+len(b)-start+0xfff)&~0xfff)
            uc.mem_write(va,b)
        uc.mem_map(core.h.SCRATCH, core.h.SCRATCH_SIZE)
        uc.mem_map(core.h.STACK, core.h.STACK_SIZE)
        uc.mem_map(core.h.RET & ~0xfff, 0x1000)
        base = core.h.SCRATCH
        stream, state, source, output = base+0x100, base+0x1000, base+0x3000, base+0x8000
        uc.mem_write(base, rnd.randbytes(0x10000))
        uc.mem_write(stream, bytes(56))
        uc.mem_write(state, bytes(0x1c00))
        def put(at, value): uc.mem_write(at,struct.pack('<I',value))
        for offset,value in {0:source,4:len(data),12:output,16:len(payload)+1024,28:state}.items():put(stream+offset,value)
        for offset,value in {0:12,0x1c:15}.items():put(state+offset,value)
        uc.mem_write(source,data)
        uc.mem_write(core.ESP-0x1000,rnd.randbytes(0x1100))
        uc.mem_write(core.ESP,struct.pack('<III',core.h.RET,stream,4))
        for reg in core.UC_GPR:uc.reg_write(reg,rnd.getrandbits(32))
        uc.reg_write(core.xc.UC_X86_REG_ESP,core.ESP)
        uc.reg_write(core.xc.UC_X86_REG_MXCSR,0x1f80)
        uc.reg_write(core.xc.UC_X86_REG_FPCW,0x37f)
        pending = []
        def machine(windows):
            return ({n:uc.reg_read(r) for n,r in zip(core.GPR,core.UC_GPR)},
                    [[(uc.reg_read(r)>>(32*i))&0xffffffff for i in range(4)] for r in core.UC_XMM],
                    [[a,bytes(uc.mem_read(a,n)).hex()] for a,n in windows])
        # Use a return-address guard; there are no callees inside fast.
        def record_hook(uc, address, size, user):
            if pending and address == pending[0][0]:
                _,windows,c=pending.pop()
                regs,xmm,memory=machine(windows)
                c.update(regs_out=regs,xmm_out=xmm,mem_out=memory,flags=uc.reg_read(core.xc.UC_X86_REG_EFLAGS))
                results.append(c)
            if address == 0x00cf2cb0:
                esp=uc.reg_read(core.xc.UC_X86_REG_ESP)
                back=struct.unpack('<I',uc.mem_read(esp,4))[0]
                windows=[(base,0x10000),(esp-0x200,0x240)]
                regs,xmm,memory=machine(windows)
                pending.append((back,windows,{'addr':address,'regs_in':regs,'xmm_in':xmm,'mem_in':memory,'calls':[]}))
        before=len(results)
        uc.hook_add(core.UC_HOOK_CODE,record_hook)
        uc.emu_start(0x00cf02e0,core.h.RET,count=2_000_000)
        assert uc.reg_read(core.xc.UC_X86_REG_EIP)==core.h.RET and not pending
        assert uc.reg_read(core.xc.UC_X86_REG_EAX)==1, 'outer inflate did not reach stream end'
        assert bytes(uc.mem_read(output,len(payload)))==payload
        assert len(results)>before
    return results


def main():
    cases = generate() + dynamic_cases()
    path = Path(os.environ.get('L109_VECTORS', 'E:/ddda-re/notes/native-L109/vectors/inflate_fast.bin'))
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('wb') as f:
        f.write(b'L109IF02'+struct.pack('<I', len(cases)))
        for c in cases:
            for side in ['in', 'out']:
                f.write(struct.pack('<8I', *(c['regs_'+side][name] for name in core.GPR)))
                f.write(struct.pack('<32I', *(v for xmm in c['xmm_'+side] for v in xmm)))
                windows = c['mem_'+side]
                f.write(struct.pack('<I', len(windows)))
                for address, text in windows:
                    data = bytes.fromhex(text)
                    f.write(struct.pack('<II', address, len(data)))
                    f.write(data)
            f.write(struct.pack('<I', c['flags']))
    print(f'{len(cases)} original cases -> {path}', flush=True)


if __name__ == '__main__': main()
