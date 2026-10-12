"""Recognize frameless virtual tail wrappers for opt-in mock ABI inputs.

This establishes only that the wrapper leaves the argument stack untouched.
The caller's existing indirect-call argument heuristic supplies the mock pop;
it does not claim to recover the unavailable virtual callee's ABI.
"""
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_MEM, X86_OP_REG


def virtual_tail_wrapper(target, read):
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    volatile = {'eax', 'ecx', 'edx'}
    loaded = set()
    pc = target
    for _ in range(8):
        try:
            ins = next(md.disasm(read(pc, 15), pc, count=1), None)
        except (KeyError, ValueError):
            return False
        if ins is None:
            return False
        operands = ins.operands
        if ins.mnemonic == 'jmp':
            return (len(operands) == 1 and operands[0].type == X86_OP_REG
                    and ins.reg_name(operands[0].reg) in loaded)
        if (ins.mnemonic != 'mov' or len(operands) != 2
                or operands[0].type != X86_OP_REG or operands[0].size != 4
                or ins.reg_name(operands[0].reg) not in volatile
                or operands[1].type != X86_OP_MEM or operands[1].size != 4):
            return False
        memory = operands[1].mem
        if (memory.segment or memory.index or not memory.base or ins.addr_size != 4
                or ins.reg_name(memory.base) not in volatile):
            return False
        loaded.add(ins.reg_name(operands[0].reg))
        pc += ins.size
    return False


def fixed_frame_allocations(entry, body, read, *, arithmetic_prefix=False):
    """Opt-in mock-input heuristic for a fixed frame with matching epilogues.

    Unlike the legacy straight-line heuristic this permits writes to locals.
    Every decoded return must immediately follow the same ADD ESP,N; only
    one entry-prefix SUB ESP,N is considered. Actual return/stack guards
    still validate every recording.
    """
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    decoded = {}
    for pc in body or ():
        ins = next(md.disasm(read(pc,15),pc,count=1),None)
        if ins is None:
            return set()
        decoded[pc] = ins
    previous = {i.address+i.size:i for i in decoded.values()}
    cleanup = set()
    for ins in decoded.values():
        if ins.mnemonic not in ('ret','retn','repz ret'):
            continue
        prior = previous.get(ins.address)
        if (prior is None or prior.mnemonic != 'add' or len(prior.operands) != 2
                or prior.op_str.split(',')[0] != 'esp' or prior.operands[1].type != 2):
            return set()
        cleanup.add(prior.operands[1].imm)
    if len(cleanup) != 1:
        return set()
    allowed = {'push', 'mov', 'lea', 'test', 'cmp'}
    if arithmetic_prefix:
        # Explicit register computations may precede or follow the allocation.
        # Stack writes and implicit stack operations remain excluded below.
        allowed |= {'add', 'sub', 'and', 'or', 'xor', 'shl', 'shr', 'sar',
                    'movss', 'movsd', 'movaps', 'movups', 'movd', 'movq',
                    'addss', 'subss', 'mulss', 'divss', 'xorps', 'pxor',
                    'movzx', 'movsx', 'cvtsi2ss', 'cvtsi2sd'}
    pc = entry
    found = set()
    for _ in range(32):
        ins = decoded.get(pc)
        if ins is None:
            return set()
        if ins.mnemonic == 'call' or ins.mnemonic.startswith('j'):
            return found
        if ins.mnemonic == 'sub' and ins.op_str.split(',')[0] == 'esp':
            if found or ins.operands[1].type != 2 or ins.operands[1].imm not in cleanup:
                return set()
            found.add(pc)
        elif (ins.mnemonic not in allowed
              or (ins.mnemonic != 'push' and any(ins.reg_name(r) in (('esp', 'sp') if arithmetic_prefix else ('esp',)) for r in ins.regs_access()[1]))
              or any(o.type == X86_OP_MEM and o.access & 2 for o in ins.operands)):
            return set()
        pc += ins.size
    return set()


def aligned_frame_saves(entry, body, read):
    """Mock-input hint for saving the frame pointer after stack alignment.

    The first EBP push saves the caller's register; the second preserves
    the new frame pointer while EBP is used as a local register. Require
    matching reverse-order pops at every return, then MOV ESP,EBP; POP EBP.
    This selects mock arguments only; recording still checks the real return.
    """
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    decoded = {}
    for pc in body or ():
        ins = next(md.disasm(read(pc, 15), pc, count=1), None)
        if ins is None:
            return set()
        decoded[pc] = ins
    pc = entry
    for mnemonic, operands in (('push', 'ebp'), ('mov', 'ebp, esp')):
        ins = decoded.get(pc)
        if ins is None or (ins.mnemonic, ins.op_str) != (mnemonic, operands):
            return set()
        pc += ins.size
    ins = decoded.get(pc)
    if (ins is None or ins.mnemonic != 'and' or len(ins.operands) != 2
            or ins.op_str.split(',')[0] != 'esp' or ins.operands[1].type != 2
            or (ins.operands[1].imm & 0xffffffff) not in (0xfffffff8, 0xfffffff0, 0xffffffe0)):
        return set()
    pc += ins.size
    saves, allocation = [], False
    for _ in range(32):
        ins = decoded.get(pc)
        if ins is None:
            return set()
        if ins.mnemonic == 'call' or ins.mnemonic.startswith('j'):
            break
        if ins.mnemonic == 'push':
            name = ins.op_str
            if name not in ('ebx', 'ebp', 'esi', 'edi') or name in [r for _, r in saves]:
                break
            saves.append((pc, name))
        elif ins.mnemonic == 'sub' and ins.op_str.split(',')[0] == 'esp':
            if allocation or saves or ins.operands[1].type != 2 or not 0 < ins.operands[1].imm <= 4096:
                return set()
            allocation = True
        elif any(ins.reg_name(r) in ('esp', 'sp', 'ebp', 'bp') for r in ins.regs_access()[1]):
            break
        pc += ins.size
    else:
        return set()
    if 'ebp' not in [r for _, r in saves]:
        return set()
    previous = {i.address+i.size: i for i in decoded.values()}
    returns = [i for i in decoded.values() if i.mnemonic in ('ret', 'retn', 'repz ret')]
    if not returns:
        return set()
    for ins in returns:
        expected = [('pop', 'ebp'), ('mov', 'esp, ebp')]
        expected += [('pop', name) for _, name in saves]
        for pair in expected:
            ins = previous.get(ins.address)
            if ins is None or (ins.mnemonic, ins.op_str) != pair:
                return set()
    return {pc for pc, name in saves if name == 'ebp'}
