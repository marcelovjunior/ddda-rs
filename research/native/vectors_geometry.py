"""Private original-exe oracle for the complete FE9580 geometry builder.

Run through heavy.py. Synthetic allocator/layout/vertex-buffer boundaries
are recorded with full registers, flags, XMM and stack. The original
function performs every subdivision, projection, normal and tangent step.
No game process or lifted shards are used; output must stay outside repo.
"""
import argparse
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ
import unicorn.x86_const as xc
import harness as h
import vectors_core as core

ENTRY = 0xFE9580
HEAP, HEAP_SIZE = 0x0C000000, 0x200000
ALLOC, FREE = core.STUBS, core.STUBS + 16
STACK_BASE, STACK_SIZE = core.ESP - 0x400, 0x480
BOUNDARIES = {ALLOC, FREE, 0x12EFE20, 0x12EF5D7, 0x130875A,
              0x1128EA0, 0x1129130, 0x1128EE0, 0x1129A60, 0x1128F40}


def record(seed, subdivisions, semantics):
    rng = random.Random(seed)
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(h.TCG_BUFFER)
    _, sections = h.image()
    for address, data in sections:
        start = address & ~4095
        uc.mem_map(start, (address + len(data) - start + 4095) & ~4095)
        uc.mem_write(address, data)
    for address, size in ((h.STACK, h.STACK_SIZE), (core.SCRATCH, h.SCRATCH_SIZE),
                          (HEAP, HEAP_SIZE), (core.STUBS, 4096), (h.RET & ~4095, 4096)):
        uc.mem_map(address, size)
    scratch = core.Scratch(rng)
    allocator = scratch.vtable_object({8: ALLOC, 9: FREE})
    layout = scratch.alloc(0x24)
    attributes = scratch.alloc(max(1, len(semantics)) * 8)
    for index, name in enumerate(semantics):
        scratch.u32(attributes + index * 8, scratch.cstr(name.encode('ascii')))
        scratch.u32(attributes + index * 8 + 4, index * 16)
    stride = max(1, len(semantics)) * 16
    scratch.u32(layout + 0x18, attributes)
    scratch.u32(layout + 0x1c, len(semantics))
    scratch.u32(layout + 0x20, stride)
    uc.mem_write(core.SCRATCH, bytes(scratch.b))
    uc.mem_write(STACK_BASE, rng.randbytes(STACK_SIZE))
    uc.mem_write(HEAP, rng.randbytes(HEAP_SIZE))
    uc.mem_write(core.ESP, struct.pack('<3I', h.RET, subdivisions, layout))
    uc.mem_write(0x187663c, struct.pack('<I', allocator))
    for reg in core.UC_GPR:
        uc.reg_write(reg, rng.getrandbits(32))
    uc.reg_write(xc.UC_X86_REG_ESP, core.ESP)
    for reg in core.UC_XMM:
        uc.reg_write(reg, rng.getrandbits(128))
    uc.reg_write(xc.UC_X86_REG_EFLAGS, 0x202)
    uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1f80)
    uc.reg_write(xc.UC_X86_REG_FPCW, 0x37f)
    uc.reg_write(xc.UC_X86_REG_FPTAG, 0xffff)

    def u32(address):
        return struct.unpack('<I', uc.mem_read(address, 4))[0]

    def machine():
        return dict(regs={name: uc.reg_read(reg) for name, reg in zip(core.GPR, core.UC_GPR)},
                    xmm=[[(uc.reg_read(reg) >> (32 * lane)) & 0xffffffff for lane in range(4)]
                         for reg in core.UC_XMM],
                    flags=uc.reg_read(xc.UC_X86_REG_EFLAGS), mxcsr=uc.reg_read(xc.UC_X86_REG_MXCSR),
                    stack=bytes(uc.mem_read(STACK_BASE, STACK_SIZE)).hex())

    def string(address, limit=None):
        result = bytearray()
        for index in range(1024 if limit is None else limit):
            byte = uc.mem_read(address + index, 1)[0]
            if not byte:
                return bytes(result).lower()
            result.append(byte)
        if limit is None:
            raise ValueError('Unterminated boundary string')
        return bytes(result).lower()

    heap_top, output = HEAP, 0
    calls, statics = [], {}
    initial = machine()
    initial_scratch = bytes(scratch.b).hex()
    initial_heap = bytes(uc.mem_read(HEAP, HEAP_SIZE)).hex()

    def allocate(size):
        nonlocal heap_top
        address = heap_top
        heap_top = (address + max(size, 1) + 15) & ~15
        if heap_top > HEAP + HEAP_SIZE:
            raise ValueError('Geometry fixture heap exhausted')
        return address

    def read_memory(_, access, address, size, value, data):
        if 0x400000 <= address < 0x2000000:
            for offset, byte in enumerate(uc.mem_read(address, size)):
                statics.setdefault(address + offset, byte)

    def instruction(_, address, size, data):
        nonlocal output
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in BOUNDARIES:
            if not ENTRY <= address < 0xFEA4EB:
                raise ValueError(f'Unmodeled boundary {address:08x}')
            return
        before = machine()
        esp = before['regs']['esp']
        arg = lambda index: u32(esp + 4 + index * 4)
        result, pop, writes = before['regs']['eax'], 0, []
        if address == ALLOC:
            result, pop = allocate(arg(0)), 8
        elif address == FREE:
            pop = 4
        elif address == 0x12EFE20:
            result = arg(0)
            writes.append((result, bytes([arg(1) & 255]) * arg(2)))
        elif address in (0x12EF5D7, 0x130875A):
            limit = arg(2) if address == 0x130875A else None
            left, right = string(arg(0), limit), string(arg(1), limit)
            result = 0 if left == right else 1 if left > right else 0xffffffff
        elif address == 0x1128EA0:
            result = allocate(arg(0))
        elif address == 0x1129130:
            output = allocate(arg(0))
            result, pop = before['regs']['ecx'], 12
        elif address == 0x1128EE0:
            result, pop = output, 4
        elif address == 0x1129A60:
            writes.append((arg(0) + u32(arg(1) + 4), bytes(uc.mem_read(arg(2), 16))))
        elif address == 0x1128F40:
            pass
        for destination, payload in writes:
            uc.mem_write(destination, payload)
        uc.reg_write(xc.UC_X86_REG_EAX, result)
        uc.reg_write(xc.UC_X86_REG_EIP, u32(esp))
        uc.reg_write(xc.UC_X86_REG_ESP, esp + 4 + pop)
        calls.append(dict(target=address, before=before, after=machine(),
                          writes=[(a, b.hex()) for a, b in writes]))

    uc.hook_add(UC_HOOK_MEM_READ, read_memory)
    uc.hook_add(UC_HOOK_CODE, instruction)
    uc.emu_start(ENTRY, h.RET, count=20_000_000)
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET:
        raise ValueError('Original geometry did not return within instruction budget')
    # Coalesce only data reads; never export code sections to vectors.
    static_windows = []
    for address, byte in sorted(statics.items()):
        if static_windows and address == static_windows[-1][0] + len(static_windows[-1][1]):
            static_windows[-1][1].append(byte)
        else:
            static_windows.append((address, bytearray([byte])))
    return dict(seed=seed, subdivisions=subdivisions, semantics=semantics,
                before=initial, after=machine(), stack_base=STACK_BASE,
                memory_before=[(core.SCRATCH, initial_scratch), (HEAP, initial_heap)] +
                              [(a, bytes(b).hex()) for a, b in static_windows],
                memory_after=[(core.SCRATCH, bytes(uc.mem_read(core.SCRATCH, len(scratch.b))).hex()),
                              (HEAP, bytes(uc.mem_read(HEAP, HEAP_SIZE)).hex())], calls=calls)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('E:/ddda-re/notes/native-L109c/vectors/geometry.json'))
    args = parser.parse_args()
    fixtures = [(0, []), (0, ['Position', 'Normal', 'Tangent', 'Color', 'TexCoord', 'Tex1', 'Other']),
                (1, ['Position', 'Normal', 'Tangent', 'COLOR', 'texcoord', 'tex9', 'Other'])]
    cases = []
    for seed, (depth, semantics) in enumerate(fixtures):
        cases.append(record(seed, depth, semantics))
        print(f'case {seed}: subdivisions={depth}, calls={len(cases[-1]["calls"])}', flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(cases))
    print(f'{len(cases)} original geometry vectors -> {args.output}', flush=True)


if __name__ == '__main__':
    main()
