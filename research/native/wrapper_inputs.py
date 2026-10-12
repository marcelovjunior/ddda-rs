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
