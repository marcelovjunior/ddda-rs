"""L163: original CFEEC0 instructions with explicit synthetic refill replies.

No game process or shards. The refill is a boundary stub, not an oracle for
CFE9D0 itself. Private output contains machine state, never commit it.
"""
import argparse
import hashlib
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn import x86_const as xc
import harness

REGS = [getattr(xc, 'UC_X86_REG_' + name) for name in
        ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI', 'EFLAGS')]


def case(cursor, end, count, mode):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(4 * 1024 * 1024)
    for address in (0x1000, 0x8000, 0x9000, 0xcfe000):
        uc.mem_map(address, 4096)
    uc.mem_write(0xcfeec0, harness.code(0xcfeec0, 0x30))
    def put(address, value):
        uc.mem_write(address, struct.pack('<I', value))
    initial = [0x1234, 0x1000, 0x5678, 0x9876, 0x9000, 0x7654, 0x3210, 0xabc, 0x202]
    for reg, value in zip(REGS, initial):
        uc.reg_write(reg, value)
    put(0x100c, cursor)
    put(0x1010, end)
    put(0x9000, 0x9100)
    put(0x9004, count)
    calls = []
    def hook(uc, address, size, _):
        if address != 0xcfe9d0:
            return
        calls.append([uc.reg_read(r) for r in REGS])
        success = mode == 0 or mode == 2 and len(calls) % 2 == 0 or mode == 3
        uc.reg_write(REGS[0], 0x12340000 | int(success))
        uc.reg_write(REGS[2], len(calls))
        if success:
            put(0x100c, 0xffffffff if mode == 3 else 0)
            put(0x1010, 11)
        sp = uc.reg_read(REGS[4])
        assert struct.unpack('<I', uc.mem_read(sp, 4))[0] == 0xcfeedf
        uc.reg_write(REGS[4], sp + 4)
        uc.reg_write(xc.UC_X86_REG_EIP, 0xcfeedf)
    uc.hook_add(UC_HOOK_CODE, hook)
    uc.emu_start(0xcfeec0, 0x9100, count=100000)
    assert uc.reg_read(xc.UC_X86_REG_EIP) == 0x9100
    values = [cursor, end, count, mode] + [uc.reg_read(r) for r in REGS]
    values += list(struct.unpack('<II', uc.mem_read(0x100c, 8)))
    values += list(struct.unpack('<5I', uc.mem_read(0x8ff4, 20)))
    values += [len(calls)] + [v for call in calls for v in call]
    return struct.pack('<' + 'I' * len(values), *values)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--output', type=Path, default=Path('E:/ddda-re/notes/native-L163/skip.bin'))
    args = ap.parse_args()
    cases = [case(cursor, end, count, mode)
             for cursor, end in [(0, 0), (0, 1), (3, 9), (10, 9), (0xfffffffa, 0xffffffff), (0xffffffff, 0xffffffff)]
             for count in [0, 1, 2, 5, 6, 7, 16, 257] for mode in range(4)]
    data = b'L163SK01' + struct.pack('<I', len(cases)) + b''.join(cases)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
    print(f'{len(cases)} cases; sha256={hashlib.sha256(data).hexdigest()}; {args.output}')


if __name__ == '__main__':
    main()
