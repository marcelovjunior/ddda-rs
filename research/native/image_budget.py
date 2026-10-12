"""Finite instruction bounds from image-only control flow.

This is a deliberately small abstract interpreter, not a guest executor.
Unknown entry registers/flags or non-image inputs never become constants.
It grants a bound only after following a completely known, returning trace.
All stores affect subsequent reads; unsupported effects refuse the proof.
"""
from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM
import time

MASK = 0xffffffff
REGS = ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp')


def image_bound(instructions, read_image, stack, ceiling=2_000_000, entry=None,
                deadline=None, max_bytes=256*4096):
    """Return an exact root instruction count, or None (not proven).

    `read_image(address, size)` must return original image bytes only and
    raise KeyError elsewhere. `stack` is the recorder's actual entry ESP.
    Calls, indirect flow, unknown addresses/conditions, code writes and
    REP instructions are outside this proof. The ordinary recorder still
    executes and checks every instruction, page access and own return.
    """
    if not instructions:
        return None
    ins = {i.address: i for i in instructions}
    code_bytes = {a for i in instructions for a in range(i.address, i.address+i.size)}
    regs = dict.fromkeys(REGS)
    regs['esp'] = stack
    stores = {}
    flags = {}
    pc = instructions[0].address if entry is None else entry

    def register(i, op):
        name = i.reg_name(op.reg)
        if name in REGS:
            return name, 0, 32
        for full in REGS:
            if name == full[1:]:
                return full, 0, 16
        for full in REGS[:4]:
            if name in (full[1]+'l', full[1]+'h'):
                return full, 8 if name.endswith('h') else 0, 8
        raise ValueError('unsupported register')

    def address(i, op):
        m = op.mem
        if m.segment or i.addr_size != 4:
            raise ValueError('segmented or narrow address')
        value = m.disp
        for r, scale in ((m.base, 1), (m.index, m.scale)):
            if r:
                name = i.reg_name(r)
                if name not in regs or regs[name] is None:
                    raise ValueError('input-dependent address')
                value += regs[name]*scale
        return value & MASK

    def load(a, size):
        values = []
        for k in range(size):
            p = (a+k) & MASK
            if p in stores:
                v = stores[p]
            else:
                try:
                    raw = read_image(p, 1)
                    v = raw[0] if len(raw) == 1 else None
                except KeyError:
                    v = None
            values.append(v)
        return None if None in values else int.from_bytes(bytes(values), 'little')

    def store(a, size, value):
        for k in range(size):
            p = (a+k) & MASK
            if p in code_bytes:
                raise ValueError('self modifying code')
            stores[p] = None if value is None else (value >> (8*k)) & 255
        if len(stores) > max_bytes:
            raise ValueError('proof memory limit')

    def get(i, op):
        if op.type == X86_OP_IMM:
            return op.imm & ((1 << (op.size*8))-1)
        if op.type == X86_OP_MEM:
            return load(address(i, op), op.size)
        if op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            return None if regs[name] is None else (regs[name] >> shift) & ((1 << bits)-1)
        raise ValueError('operand')

    def put(i, op, value):
        if op.type == X86_OP_MEM:
            store(address(i, op), op.size, value)
        elif op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            mask = (1 << bits)-1
            regs[name] = (None if value is None or (bits != 32 and regs[name] is None)
                          else ((regs[name] or 0) & ~(mask << shift)) | ((value & mask) << shift))
        else:
            raise ValueError('destination')

    try:
        for count in range(1, ceiling+1):
            if count % 256 == 1 and deadline is not None and time.monotonic() >= deadline:
                return None
            i = ins.get(pc)
            if i is None:
                return None
            mn, op = i.mnemonic, i.operands
            pc += i.size
            if mn in ('ret', 'retn'):
                # A saved root return address must not have been overwritten.
                return count if regs['esp'] == stack and not any(stack+k in stores for k in range(4)) else None
            if mn == 'nop':
                continue
            if mn == 'push' and op[0].size == 4:
                value = get(i, op[0])
                regs['esp'] = (regs['esp']-4) & MASK
                store(regs['esp'], 4, value)
            elif mn == 'pop' and op[0].type == X86_OP_REG and op[0].size == 4 and i.reg_name(op[0].reg) != 'esp':
                value = load(regs['esp'], 4)
                regs['esp'] = (regs['esp']+4) & MASK
                put(i, op[0], value)
            elif mn == 'mov':
                put(i, op[0], get(i, op[1]))
            elif mn == 'lea' and op[0].type == X86_OP_REG and op[0].size == 4:
                put(i, op[0], address(i, op[1]))
            elif mn in ('xor', 'add', 'sub', 'inc', 'dec', 'cmp', 'test'):
                a = get(i, op[0])
                b = get(i, op[1]) if len(op) > 1 else 1
                if mn == 'xor' and all(o.type == X86_OP_REG for o in op) and op[0].reg == op[1].reg:
                    a = b = 0
                bits = op[0].size*8
                mask = (1 << bits)-1
                result = None if a is None or b is None else {
                    'xor': lambda: a ^ b, 'add': lambda: a+b, 'inc': lambda: a+1,
                    'sub': lambda: a-b, 'dec': lambda: a-1, 'cmp': lambda: a-b,
                    'test': lambda: a & b}[mn]() & mask
                old_cf = flags.get('cf')
                flags = {} if result is None else dict(zf=result == 0, sf=bool(result & (1 << (bits-1))))
                if result is not None:
                    if mn in ('cmp','sub'):
                        flags['cf'] = a < b
                    elif mn == 'add':
                        flags['cf'] = a+b > mask
                    elif mn in ('xor','test'):
                        flags['cf'] = False
                    elif old_cf is not None:
                        flags['cf'] = old_cf
                if mn not in ('cmp','test'):
                    put(i, op[0], result)
            elif mn in ('jmp','je','jne','jb','jae','jbe','ja','js','jns') and op[0].type == X86_OP_IMM:
                if mn == 'jmp': taken = True
                elif mn in ('je','jne'): taken = flags['zf'] == (mn == 'je')
                elif mn in ('js','jns'): taken = flags['sf'] == (mn == 'js')
                elif mn in ('jb','jae'): taken = flags['cf'] == (mn == 'jb')
                else: taken = (flags['cf'] or flags['zf']) == (mn == 'jbe')
                if taken:
                    pc = op[0].imm & MASK
            else:
                return None
            if regs['esp'] is None:
                return None
    except (KeyError, ValueError, TypeError):
        return None
    return None
