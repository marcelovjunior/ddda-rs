"""Private original-exe oracle for the complete FE9580 geometry builder.

Run through heavy.py. Synthetic allocator/layout/vertex-buffer boundaries
are recorded with full registers, flags, XMM and stack. The original
function performs every subdivision, projection, normal and tangent step.
No game process or lifted shards are used; output must stay outside repo.
"""
import argparse
import ctypes
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as xc
import harness as h
import vectors_core as core
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_MEM

ENTRY = 0xFE9580
HEAP, HEAP_SIZE = 0x0C000000, 0x200000
ALLOC, FREE = core.STUBS, core.STUBS + 16
STACK_BASE, STACK_SIZE = core.ESP - 0x400, 0x480
BOUNDARIES = {ALLOC, FREE, 0x12EFE20, 0x12EF5D7, 0x130875A,
              0x1128EA0, 0x1129130, 0x1128EE0, 0x1129A60, 0x1128F40}


class HardwareSse:
    """Run the original scalar SSE instruction on Win64's physical CPU.

    Unicorn's mixed SNaN/QNaN payload preference and sticky MXCSR differ
    from x86 hardware. Keep integer/control execution in Unicorn, but run
    each original arithmetic/compare opcode with its complete input XMM
    state and MXCSR on hardware. The private opcode is read from the exe;
    a memory source is redirected to a temporary buffer. No game is run.
    """
    operations = {'addss', 'subss', 'mulss', 'divss', 'sqrtss',
                  'sqrtsd', 'cvtsd2ss', 'comiss', 'ucomiss'}

    def __init__(self):
        assert ctypes.sizeof(ctypes.c_void_p) == 8, 'Win64 research Python required'
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        self.kernel.VirtualAlloc.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong, ctypes.c_ulong]
        self.kernel.VirtualAlloc.restype = ctypes.c_void_p
        self.kernel.VirtualProtect.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong)]
        self.kernel.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_ulong]
        self.cache = {}

    def close(self):
        for address, _ in self.cache.values():
            self.kernel.VirtualFree(address, 0, 0x8000)
        self.cache.clear()

    def execute(self, instruction, uc, read):
        raw = bytes(instruction.bytes)
        source = instruction.operands[1]
        operand = (ctypes.c_ubyte * 16)()
        if source.type == X86_OP_MEM:
            address = source.mem.disp
            for register, scale in ((source.mem.base, 1), (source.mem.index, source.mem.scale)):
                if register:
                    name = instruction.reg_name(register).upper()
                    address += uc.reg_read(getattr(xc, 'UC_X86_REG_' + name)) * scale
            address &= 0xffffffff
            assert not source.mem.segment
            size = 8 if instruction.mnemonic in ('sqrtsd', 'cvtsd2ss') else 4
            payload = bytes(uc.mem_read(address, size))
            read(None, 0, address, size, 0, None)
            ctypes.memmove(operand, payload, size)
            opcode = raw.index(b'\x0f')
            # Keep mandatory prefixes/opcode/destination; use [rdx] source.
            raw = raw[:opcode + 2] + bytes([(raw[opcode + 2] & 0x38) | 2])
        if raw not in self.cache:
            # Win64 ABI: RCX=guest state, RDX=memory source. Preserve the
            # host's nonvolatile XMM6/7 and MXCSR around the original op.
            code = bytearray.fromhex('4883ec38 f30f7f3424 f30f7f7c2410 0fae5c2420')
            code += bytes.fromhex('0fae9180000000')  # ldmxcsr [rcx+128]
            for register in range(8):
                code += bytes([0xf3, 0x0f, 0x6f, 0x41 | (register << 3), register * 16])
            code += raw
            code += bytes.fromhex('9c415844898184000000 0fae9980000000')
            for register in range(8):
                code += bytes([0xf3, 0x0f, 0x7f, 0x41 | (register << 3), register * 16])
            code += bytes.fromhex('0fae542420 f30f6f3424 f30f6f7c2410 4883c438 c3')
            address = self.kernel.VirtualAlloc(None, len(code), 0x3000, 4)
            if not address:
                raise ctypes.WinError(ctypes.get_last_error())
            ctypes.memmove(address, bytes(code), len(code))
            previous = ctypes.c_ulong()
            if not self.kernel.VirtualProtect(address, len(code), 0x20, ctypes.byref(previous)):
                raise ctypes.WinError(ctypes.get_last_error())
            function = ctypes.WINFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p)(address)
            self.cache[raw] = (address, function)
        state = (ctypes.c_uint32 * 34)()
        for index, register in enumerate(core.UC_XMM):
            value = uc.reg_read(register)
            for lane in range(4):
                state[index * 4 + lane] = (value >> (lane * 32)) & 0xffffffff
        state[32] = uc.reg_read(xc.UC_X86_REG_MXCSR)
        assert state[32] & 0x1f80 == 0x1f80, 'oracle fixtures require masked SSE exceptions'
        self.cache[raw][1](state, operand)
        for index, register in enumerate(core.UC_XMM):
            uc.reg_write(register, sum(state[index * 4 + lane] << (32 * lane) for lane in range(4)))
        uc.reg_write(xc.UC_X86_REG_MXCSR, state[32])
        if instruction.mnemonic in ('comiss', 'ucomiss'):
            old = uc.reg_read(xc.UC_X86_REG_EFLAGS)
            uc.reg_write(xc.UC_X86_REG_EFLAGS, (old & ~0x8d5) | (state[33] & 0x8d5))
        uc.reg_write(xc.UC_X86_REG_EIP, instruction.address + instruction.size)


def record(seed, subdivisions, semantics, *, units=None, fail_vertex_object=False):
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
    if units is not None:
        for address, value in zip((0x1406ce0, 0x161c20c), units):
            uc.mem_write(address, struct.pack('<I', value))
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
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    decoder.detail = True
    scalar_instructions = {i.address: i for i in decoder.disasm(h.code(ENTRY, 0xFEA4EB - ENTRY), ENTRY)
                           if i.mnemonic in HardwareSse.operations}
    hardware = HardwareSse()

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

    def write_memory(_, access, address, size, value, data):
        if HEAP <= address < HEAP + HEAP_SIZE and address + size > heap_top:
            raise ValueError('Original writes beyond allocated geometry fixture')

    def instruction(_, address, size, data):
        nonlocal output
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in BOUNDARIES:
            if not ENTRY <= address < 0xFEA4EB:
                raise ValueError(f'Unmodeled boundary {address:08x}')
            if address in scalar_instructions:
                hardware.execute(scalar_instructions[address], uc, read_memory)
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
            result = 0 if fail_vertex_object else allocate(arg(0))
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
    uc.hook_add(UC_HOOK_MEM_WRITE, write_memory)
    uc.hook_add(UC_HOOK_CODE, instruction)
    try:
        uc.emu_start(ENTRY, h.RET, count=20_000_000)
    finally:
        hardware.close()
    if uc.reg_read(xc.UC_X86_REG_EIP) != h.RET:
        raise ValueError('Original geometry did not return within instruction budget')
    # Coalesce only data reads; never export code sections to vectors.
    static_windows = []
    for address, byte in sorted(statics.items()):
        if static_windows and address == static_windows[-1][0] + len(static_windows[-1][1]):
            static_windows[-1][1].append(byte)
        else:
            static_windows.append((address, bytearray([byte])))
    heap_window = min(HEAP_SIZE, heap_top - HEAP + 4096)
    return dict(seed=seed, subdivisions=subdivisions, semantics=semantics,
                units=units, fail_vertex_object=fail_vertex_object,
                before=initial, after=machine(), stack_base=STACK_BASE,
                memory_before=[(core.SCRATCH, initial_scratch), (HEAP, initial_heap[:heap_window * 2])] +
                              [(a, bytes(b).hex()) for a, b in static_windows],
                memory_after=[(core.SCRATCH, bytes(uc.mem_read(core.SCRATCH, len(scratch.b))).hex()),
                              (HEAP, bytes(uc.mem_read(HEAP, heap_window)).hex())], calls=calls)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('E:/ddda-re/notes/native-L109c/vectors/geometry.json'))
    args = parser.parse_args()
    fixtures = [(0, []), (0, ['Position', 'Normal', 'Tangent', 'Color', 'TexCoord', 'Tex1', 'Other']),
                (1, ['Position', 'Normal', 'Tangent', 'COLOR', 'texcoord', 'tex9', 'Other'])]
    cases = []
    fixtures = [(depth, semantics, {}) for depth, semantics in fixtures]
    attributes = fixtures[1][1]
    fixtures += [(0, [], dict(fail_vertex_object=True))]
    # Fault-free masked SSE edge inputs. Constants remain private test data;
    # the game still reads unmodified values from the player's executable.
    fixtures += [(0, attributes, dict(units=units)) for units in (
        (0, 0x80000000), (0x7fc12345, 0xbf800000),
        (0x7f812345, 0xffc56789), (0x7f800000, 0xff800000),
        (1, 0x80000001), (0x3f800000, 0x3f800000))]
    fixtures += [(1, attributes, dict(units=(0x7fc12345, 0xffc56789))), (2, [], {})]
    for seed, (depth, semantics, options) in enumerate(fixtures):
        cases.append(record(seed, depth, semantics, **options))
        print(f'case {seed}: subdivisions={depth}, calls={len(cases[-1]["calls"])}', flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(cases))
    write_binary(args.output.with_suffix('.bin'), cases)
    print(f'{len(cases)} original geometry vectors -> {args.output}', flush=True)


def write_binary(path, cases):
    with path.open('wb') as stream:
        def word(value):
            stream.write(struct.pack('<I', value))

        def blob(value):
            raw = bytes.fromhex(value)
            word(len(raw))
            stream.write(raw)

        def machine(value):
            for register in core.GPR:
                word(value['regs'][register])
            for lanes in value['xmm']:
                for lane in lanes:
                    word(lane)
            word(value['flags'])
            word(value['mxcsr'])
            blob(value['stack'])

        def windows(value):
            word(len(value))
            for address, data in value:
                word(address)
                blob(data)

        stream.write(b'L109GE01')
        word(len(cases))
        for case in cases:
            word(case['seed'])
            word(case['stack_base'])
            machine(case['before'])
            machine(case['after'])
            windows(case['memory_before'])
            windows(case['memory_after'])
            word(len(case['calls']))
            for call in case['calls']:
                word(call['target'])
                machine(call['before'])
                machine(call['after'])
                windows(call['writes'])


if __name__ == '__main__':
    main()
