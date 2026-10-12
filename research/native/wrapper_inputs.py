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


def fixed_frame_allocations(entry, body, read):
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
        elif (ins.mnemonic not in ('push','mov','lea','test','cmp')
              or (ins.mnemonic != 'push' and any(ins.reg_name(r)=='esp' for r in ins.regs_access()[1]))
              or any(o.type == X86_OP_MEM and o.access & 2 for o in ins.operands)):
            return set()
        pc += ins.size
    return set()
