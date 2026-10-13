"""CPU-only original inflate versus original inflate + actual Rust Adler port.

No game process, lifted shards, mocked decoder or copied game assets. Reads
installed compressed ARC entries and the analysis exe. Build the private
cdylib prepared by scripts/bench_l140.py first (--release --lib).
Checks every inflate return: GPRs/defined flags, full scratch/state/window/
output and stack bytes. Also checks final output against Python zlib.
"""
import argparse
import ctypes as c
import hashlib
import struct
import zlib
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
import unicorn.x86_const as xc
import harness as h

REGS = [getattr(xc, 'UC_X86_REG_' + name) for name in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
STREAM, STATE, WINDOW, SOURCE, OUTPUT = [h.SCRATCH + x for x in (0, 0x1000, 0x4000, 0x10000, 0x50000)]
ESP = h.STACK + h.STACK_SIZE - 0x1000


def archives(root):
    found = 0
    for path in sorted(root.rglob('*.arc')):
        with path.open('rb') as f:
            head = f.read(8)
            if len(head) != 8 or head[:4] != b'ARC\0':
                continue
            records = f.read(struct.unpack_from('<H', head, 6)[0] * 80)
            for index in range(len(records) // 80):
                packed, size, location = struct.unpack_from('<III', records, index * 80 + 68)
                size &= (1 << 29) - 1
                if not 5552 <= size <= 131072:
                    continue
                f.seek(location)
                stream = f.read(packed)
                assert len(zlib.decompress(stream)) == size
                yield f'{path.name}:{index}', stream, 1
                found += 1
                if found == 8:
                    return


def execute(data, wrap, chunks, bridge=None):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(32 << 20)
    for address, content in h.image()[1]:
        start = address & ~4095
        uc.mem_map(start, (address + len(content) - start + 4095) & ~4095)
        uc.mem_write(address, content)
    uc.mem_map(h.SCRATCH, h.SCRATCH_SIZE)
    uc.mem_map(h.STACK, h.STACK_SIZE)
    uc.mem_map(h.RET & ~4095, 4096)
    uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1f80)
    uc.reg_write(xc.UC_X86_REG_EFLAGS, 0x202)
    word = lambda address: struct.unpack('<I', uc.mem_read(address, 4))[0]
    def set_word(address, value):
        uc.mem_write(address, struct.pack('<I', value & 0xffffffff))
    def call(address, args):
        uc.reg_write(xc.UC_X86_REG_ESP, ESP)
        uc.mem_write(ESP, struct.pack('<' + 'I' * (len(args) + 1), h.RET, *args))
        uc.emu_start(address, h.RET, count=20_000_000, timeout=20_000_000)
        assert uc.reg_read(xc.UC_X86_REG_EIP) == h.RET, 'original did not return within budget'
        assert uc.reg_read(xc.UC_X86_REG_ESP) == ESP + 4, 'cdecl stack mismatch'
        return uc.reg_read(xc.UC_X86_REG_EAX)
    native_calls = [0]
    if bridge:
        def hook(uc, address, size, _):
            stack_address = uc.reg_read(xc.UC_X86_REG_ESP)
            source = word(stack_address + 8)
            length = word(stack_address + 12)
            data = bytes(uc.mem_read(source, length)) if source and length else b''
            buffer = c.create_string_buffer(data)
            registers = (c.c_uint32 * 8)(*(uc.reg_read(r) for r in REGS))
            stack = (c.c_ubyte * 64).from_buffer_copy(uc.mem_read(stack_address - 32, 64))
            back = word(stack_address)
            flags = bridge(buffer, len(data), registers, stack)
            uc.mem_write(stack_address - 32, bytes(stack))
            for r, value in zip(REGS, registers):
                uc.reg_write(r, value)
            old_flags = uc.reg_read(xc.UC_X86_REG_EFLAGS)
            uc.reg_write(xc.UC_X86_REG_EFLAGS, (old_flags & ~0x8d5) | (flags & 0x8d5))
            uc.reg_write(xc.UC_X86_REG_EIP, back)
            native_calls[0] += 1
        uc.hook_add(UC_HOOK_CODE, hook, begin=0x00cf34d0, end=0x00cf34d0)
    set_word(STREAM + 28, STATE)
    set_word(STATE + 8, wrap)
    set_word(STATE + 28, 15)
    set_word(STATE + 44, WINDOW)  # caller-provided existing allocation
    assert call(0x00cf00a0, [STREAM]) == 0  # original inflateReset
    uc.mem_write(SOURCE, data)
    set_word(STREAM, SOURCE)
    set_word(STREAM + 12, OUTPUT)
    consumed_limit = 0
    results = []
    for iteration in range(10000):
        if word(STREAM + 4) == 0 and consumed_limit < len(data):
            count = min(chunks[0], len(data) - consumed_limit)
            set_word(STREAM + 4, count)
            consumed_limit += count
        set_word(STREAM + 16, chunks[1])
        code = call(0x00cf02e0, [STREAM, 0])
        image = bytes(uc.mem_read(h.SCRATCH, h.SCRATCH_SIZE))
        stack = bytes(uc.mem_read(ESP - 512, 576))
        result = (tuple(uc.reg_read(r) for r in REGS),
                  tuple(uc.reg_read(getattr(xc, f'UC_X86_REG_XMM{i}')) for i in range(8)),
                  uc.reg_read(xc.UC_X86_REG_EFLAGS) & 0x8c5,
                  hashlib.sha256(image + stack).digest())
        results.append(result)
        if code in (1, 0xfffffffd, 0xfffffffe, 2):
            break
        if code == 0xfffffffb and consumed_limit == len(data) and word(STREAM + 4) == 0:
            break  # exhausted truncated stream, explicit Z_BUF_ERROR
        assert code in (0, 0xfffffffb), f'unexpected inflate result {code:#x}'
    else:
        raise AssertionError('stream exceeded bounded call count')
    output = bytes(uc.mem_read(OUTPUT, word(STREAM + 20)))
    return results, output, code, native_calls[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dll', type=Path, default=Path('C:/ddda-targets/L140/release/l140_adler_bench.dll'))
    parser.add_argument('--install', type=Path, default=Path('E:/SteamLibrary/steamapps/common/DDDA/nativePC'))
    args = parser.parse_args()
    dll = c.CDLL(str(args.dll))
    bridge = dll.l140_adler
    bridge.argtypes = [c.c_void_p, c.c_size_t, c.POINTER(c.c_uint32), c.POINTER(c.c_ubyte)]
    bridge.restype = c.c_uint32
    cases = [(name, data, wrap, (8192, 16384)) for name, data, wrap in archives(args.install)]
    payload = b'original streaming boundary check' * 16
    for chunks in [(1, 1), (5, 257), (6, 258), (4096, 4096)]:
        cases.append((f'boundary-{chunks}', zlib.compress(payload), 1, chunks))
    valid = zlib.compress(payload)
    cases.extend([(name, data, 1, (7, 259)) for name, data in (
        ('bad-checksum', valid[:-1] + bytes([valid[-1] ^ 1])), ('truncated', valid[:-3]), ('bad-header', b'xx' + valid[2:]))])
    cases.append(('raw', zlib.compress(payload)[2:-4], 0, (11, 127)))
    assert len(cases) == 16, 'expected eight installed archives plus eight synthetic cases'
    for name, data, wrap, chunks in cases:
        baseline = execute(data, wrap, chunks)
        native = execute(data, wrap, chunks, bridge)
        assert baseline[:3] == native[:3], f'{name}: original/native state, registers, output or status differ'
        if native[2] == 1:
            assert native[1] == zlib.decompress(data, -15 if wrap == 0 else 15)
        print(f'{name} calls={len(native[0])} native_adler={native[3]} return={native[2]:#x} output={len(native[1])} sha256={hashlib.sha256(native[1]).hexdigest()} PASS', flush=True)
    print(f'{len(cases)} full-stream original/native cases PASS')


if __name__ == '__main__':
    main()
