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


def fixed_frame_allocations(entry, body, read, *, arithmetic_prefix=False, tail_exits=False):
    """Opt-in mock-input heuristic for a fixed frame with matching epilogues.

    Unlike the legacy straight-line heuristic this permits writes to locals.
    Every decoded return must immediately follow the same ADD ESP,N; only
    one entry-prefix SUB ESP,N is considered. Actual return/stack guards
    still validate every recording. tail_exits additionally checks external
    unconditional jumps and refuses conditional transfers outside the body.
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
        is_exit = ins.mnemonic in ('ret','retn','repz ret')
        if tail_exits and ins.mnemonic.startswith('j'):
            target = ins.operands[0]
            outside = target.type != 2 or (target.imm & 0xffffffff) not in decoded
            if outside and ins.mnemonic != 'jmp':
                return set()
            is_exit |= outside
        if not is_exit:
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


def ebp_frame_allocation(entry, body, read):
    """Input hint for locals released by MOV ESP,EBP after stack alignment.

    Require a stable EBP, one allocation before any pushes/calls/branches,
    and the same reverse-order register restores at every own return.
    As with fixed frames, actual recording guards still check the return.
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
    for pair in (('push', 'ebp'), ('mov', 'ebp, esp')):
        ins = decoded.get(pc)
        if ins is None or (ins.mnemonic, ins.op_str) != pair:
            return set()
        pc += ins.size
    frame_set = ins.address
    ins = decoded.get(pc)
    if (ins is None or ins.mnemonic != 'and' or len(ins.operands) != 2
            or ins.op_str.split(',')[0] != 'esp' or ins.operands[1].type != 2
            or (ins.operands[1].imm & 0xffffffff) not in (0xfffffff8, 0xfffffff0, 0xffffffe0)):
        return set()
    pc += ins.size
    allocation, saves = None, []
    for _ in range(32):
        ins = decoded.get(pc)
        if ins is None:
            return set()
        if ins.mnemonic == 'call' or ins.mnemonic.startswith('j'):
            break
        if ins.mnemonic == 'sub' and ins.op_str.split(',')[0] == 'esp':
            if (allocation is not None or saves or ins.operands[1].type != 2
                    or not 0 < ins.operands[1].imm <= 4096):
                return set()
            allocation = pc
        elif ins.mnemonic == 'push':
            if allocation is None:
                return set()
            if ins.op_str not in ('ebx', 'esi', 'edi') or ins.op_str in saves:
                break
            saves.append(ins.op_str)
        elif any(ins.reg_name(r) in ('esp', 'sp', 'ebp', 'bp') for r in ins.regs_access()[1]):
            return set()
        pc += ins.size
    else:
        return set()
    if allocation is None:
        return set()
    previous = {i.address+i.size: i for i in decoded.values()}
    returns = [i for i in decoded.values() if i.mnemonic in ('ret', 'retn', 'repz ret')]
    if not returns:
        return set()
    restores = set()
    for ins in returns:
        expected = [('pop', 'ebp'), ('mov', 'esp, ebp')]
        expected += [('pop', name) for name in saves]
        for index, pair in enumerate(expected):
            ins = previous.get(ins.address)
            # A compiler may schedule ordinary operations between saved
            # register pops. They must neither change ESP/EBP nor branch.
            for _ in range(8):
                if ins is None:
                    return set()
                if (ins.mnemonic, ins.op_str) == pair:
                    break
                if (index < 2 or ins.mnemonic not in (
                        'mov', 'movss', 'movsd', 'movaps', 'movups', 'movzx', 'movsx',
                        'lea', 'add', 'sub', 'xor', 'or', 'and', 'test', 'cmp',
                        'addss', 'subss', 'mulss', 'divss', 'xorps', 'pxor')
                        or any(ins.reg_name(r) in ('esp', 'sp', 'ebp', 'bp')
                               for r in ins.regs_access()[1])):
                    return set()
                ins = previous.get(ins.address)
            else:
                return set()
            if pair == ('pop', 'ebp'):
                restores.add(ins.address)
    for ins in decoded.values():
        if (ins.address != frame_set and ins.address not in restores
                and any(ins.reg_name(r) in ('ebp', 'bp') for r in ins.regs_access()[1])):
            return set()
    return {allocation}


def initial_outgoing_allocation(entry, body, read):
    """Hint for an entry allocation consumed by the final indirect call.

    Every acyclic path must reach an indirect CALL immediately followed by
    RET. Before that call, only 32-bit pushes may change ESP after the entry
    allocation. There is no local cleanup: the mock must consume that area
    to preserve the caller's return address. Unknown paths are refused.
    """
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    if not body or len(body) > 256:
        return set()
    decoded = {}
    for pc in body:
        ins = next(md.disasm(read(pc, 15), pc, count=1), None)
        if ins is None:
            return set()
        decoded[pc] = ins
    first = decoded.get(entry)
    if (first is None or first.mnemonic != 'sub' or len(first.operands) != 2
            or first.op_str.split(',')[0] != 'esp' or first.operands[1].type != 2
            or not 0 < first.operands[1].imm <= 4096):
        return set()
    graph, todo = {}, [entry+first.size]
    while todo:
        pc = todo.pop()
        if pc in graph:
            continue
        ins = decoded.get(pc)
        if ins is None:
            return set()
        after = pc+ins.size
        successors = [after]
        if ins.mnemonic == 'call':
            ret = decoded.get(after)
            if (len(ins.operands) != 1 or ins.operands[0].type not in (X86_OP_REG, X86_OP_MEM)
                    or ret is None or ret.mnemonic not in ('ret', 'retn', 'repz ret')):
                return set()
            successors = []
        elif ins.mnemonic.startswith('j'):
            if len(ins.operands) != 1 or ins.operands[0].type != 2:
                return set()
            successors = [ins.operands[0].imm & 0xffffffff]
            if ins.mnemonic != 'jmp':
                successors.append(after)
        elif ins.mnemonic == 'push':
            if len(ins.operands) != 1 or ins.operands[0].size != 4:
                return set()
        elif (ins.mnemonic in ('ret', 'retn', 'repz ret', 'retf', 'int', 'int3', 'ud2', 'hlt')
                or ins.mnemonic.startswith('loop')
                or any(ins.reg_name(r) in ('esp', 'sp') for r in ins.regs_access()[1])):
            return set()
        graph[pc] = successors
        todo.extend(successors)
    # Remove terminal paths first; any nodes left form or lead to a cycle.
    remaining = dict(graph)
    while remaining:
        done = {pc for pc, edges in remaining.items() if all(e not in remaining for e in edges)}
        if not done:
            return set()
        for pc in done:
            del remaining[pc]
    return {entry}


def null_leaf_cleanup(entry, read, registers, memory):
    """Mock cleanup for a checked zero-memory branch returning EAX=0.

    The entry must compare one dword to zero, branch on equality, then the
    chosen null path may only clear EAX and return. The other path is not
    interpreted, and never supplies a cleanup hint for nonzero input.
    """
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    def instruction(pc):
        return next(md.disasm(read(pc, 15), pc, count=1), None)
    cmp = instruction(entry)
    if (cmp is None or cmp.mnemonic != 'cmp' or len(cmp.operands) != 2
            or cmp.operands[0].type != X86_OP_MEM or cmp.operands[0].size != 4
            or cmp.operands[1].type != 2 or cmp.operands[1].imm != 0
            or cmp.addr_size != 4):
        return None
    mem = cmp.operands[0].mem
    register = cmp.reg_name(mem.base)
    if mem.segment or mem.index or register not in registers:
        return None
    branch = instruction(entry+cmp.size)
    if (branch is None or branch.mnemonic not in ('je', 'jne')
            or 0x66 in branch.prefix or 0x67 in branch.prefix
            or len(branch.operands) != 1 or branch.operands[0].type != 2):
        return None
    pc = branch.operands[0].imm & 0xffffffff if branch.mnemonic == 'je' else branch.address+branch.size
    clear = instruction(pc)
    if clear is None or (clear.mnemonic, clear.op_str) not in (('xor', 'eax, eax'), ('mov', 'eax, 0')):
        return None
    ret = instruction(pc+clear.size)
    if ret is None or ret.mnemonic != 'ret' or ret.bytes[0] not in (0xc2, 0xc3):
        return None
    pop = ret.operands[0].imm if ret.operands else 0
    if not 0 <= pop <= 4096 or pop % 4:
        return None
    value = memory((registers[register]+mem.disp) & 0xffffffff, 4)
    return pop if len(value) == 4 and int.from_bytes(value, 'little') == 0 else None


def exception_frame_pushes(entry, body, read):
    """Input hint for an inline exception frame with matching unlink exits.

    Five entry pushes hold the state, handler, previous FS link, one local
    slot and a frame value. Require the link store and every return's exact
    inverse stack/link sequence before excluding those pushes from arguments.
    """
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    decoded = {}
    for pc in body or ():
        ins = next(md.disasm(read(pc, 15), pc, count=1), None)
        if ins is None:
            return set()
        decoded[pc] = ins
    prefix, pc = [], entry
    for _ in range(10):
        ins = decoded.get(pc)
        if ins is None:
            return set()
        prefix.append(ins)
        pc += ins.size
    def reg(ins, op):
        return ins.reg_name(op.reg) if op.type == X86_OP_REG and op.size == 4 else None
    def fs_link(ins, op):
        return (op.type == X86_OP_MEM and op.size == 4 and ins.addr_size == 4
                and ins.reg_name(op.mem.segment) == 'fs'
                and not op.mem.base and not op.mem.index and op.mem.disp == 0)
    def stack_slot(ins, op, offset):
        return (op.type == X86_OP_MEM and op.size == 4 and ins.addr_size == 4
                and not op.mem.segment and ins.reg_name(op.mem.base) == 'esp'
                and not op.mem.index and op.mem.disp == offset)
    a,b,c,d,e,f,g,k,l,m = prefix
    if (a.mnemonic != 'push' or a.operands[0].type != 2
            or a.operands[0].size != 4 or a.operands[0].imm & 0xffffffff != 0xffffffff
            or b.mnemonic != 'push' or b.operands[0].type != 2 or b.operands[0].size != 4
            or c.mnemonic != 'mov' or len(c.operands) != 2
            or not fs_link(c,c.operands[1])):
        return set()
    link_reg = reg(c,c.operands[0])
    if (link_reg not in ('eax','ecx','edx') or d.mnemonic != 'push'
            or reg(d,d.operands[0]) != link_reg or e.mnemonic != 'push'
            or reg(e,e.operands[0]) not in ('eax','ecx','edx')
            or f.mnemonic != 'mov' or len(f.operands) != 2
            or f.operands[1].type != X86_OP_MEM or f.operands[1].size != 4
            or f.operands[1].mem.segment or f.operands[1].mem.base or f.operands[1].mem.index):
        return set()
    value_reg = reg(f,f.operands[0])
    if (value_reg not in ('eax','ecx','edx')
            or (g.mnemonic,g.op_str) != ('xor', value_reg+', esp')
            or k.mnemonic != 'push' or reg(k,k.operands[0]) != value_reg
            or l.mnemonic != 'lea' or len(l.operands) != 2
            or reg(l,l.operands[0]) != value_reg or not stack_slot(l,l.operands[1],8)
            or m.mnemonic != 'mov' or len(m.operands) != 2
            or not fs_link(m,m.operands[0]) or reg(m,m.operands[1]) != value_reg):
        return set()
    previous = {i.address+i.size: i for i in decoded.values()}
    returns = [i for i in decoded.values() if i.mnemonic in ('ret','retn','repz ret')]
    if not returns:
        return set()
    for ret in returns:
        suffix, ins = [], ret
        for _ in range(4):
            ins = previous.get(ins.address)
            if ins is None:
                return set()
            suffix.append(ins)
        add,pop,unlink,load = suffix
        if ((add.mnemonic,add.op_str) != ('add','esp, 0x10')
                or pop.mnemonic != 'pop' or reg(pop,pop.operands[0]) not in ('eax','ecx','edx')
                or unlink.mnemonic != 'mov' or len(unlink.operands) != 2
                or not fs_link(unlink,unlink.operands[0])
                or load.mnemonic != 'mov' or len(load.operands) != 2
                or not stack_slot(load,load.operands[1],8)):
            return set()
        restore_reg = reg(load,load.operands[0])
        if restore_reg not in ('eax','ecx','edx') or reg(unlink,unlink.operands[1]) != restore_reg:
            return set()
    return {i.address for i in (a,b,d,e,k)}
