"""Opt-in mock cleanup hints from supplied, complete callee bodies.

The body is an input recipe, not a new execution boundary. Every edge must
remain inside it; table jumps need finite register bounds, including joins.
Unknown flow retains the recorder's ordinary mock fallback. Return, memory,
and stack checks are unchanged; the chosen pop is recorded in the call event.
"""
from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_GRP_RET, CS_GRP_IRET, CS_GRP_INT


class BodyCleanup:
    def __init__(self, layout, read):
        if not isinstance(layout, dict) or set(layout) != {'bodies'}:
            raise ValueError('cleanup layout requires bodies')
        bodies = layout['bodies']
        if not isinstance(bodies, dict) or not 1 <= len(bodies) <= 64:
            raise ValueError('cleanup layout requires 1..64 bodies')
        self.bodies = {}
        for address, body in bodies.items():
            try:
                entry = int(address, 0)
            except (TypeError, ValueError):
                raise ValueError('callee address must be an integer string') from None
            if (not 0 <= entry <= 0xffffffff or entry in self.bodies
                    or not isinstance(body, list) or not 1 <= len(body) <= 4096
                    or any(type(pc) is not int or not 0 <= pc <= 0xffffffff for pc in body)
                    or len(set(body)) != len(body) or entry not in body):
                raise ValueError('invalid callee body')
            self.bodies[entry] = tuple(body)
        self.read = read
        self.cache = {}

    def pop(self, target):
        if target not in self.bodies:
            return None
        if target not in self.cache:
            self.cache[target] = uniform_cleanup(target, self.bodies[target], self.read)
        return self.cache[target]


def uniform_cleanup(entry, body, read):
    """Return the common RET immediate, or None for incomplete/unknown flow.

    Calls retain the existing mock-ABI assumption that they return. This is
    a cleanup-input hint, not a proof of all effects or of a function's ABI.
    """
    import lift
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    try:
        found = {pc: next(md.disasm(read(pc, 15), pc, count=1), None) for pc in body}
    except (KeyError, ValueError):
        return None
    if entry not in found or any(i is None for i in found.values()):
        return None
    pending, switches, pops = {}, {}, set()
    for pc, ins in found.items():
        if ins.mnemonic in ('ret', 'retn', 'repz ret'):
            pops.add(ins.operands[0].imm if ins.operands else 0)
            continue
        if (ins.mnemonic in ('int3', 'ud2', 'hlt', 'loop', 'loope', 'loopne')
                or any(ins.group(group) for group in (CS_GRP_RET, CS_GRP_IRET, CS_GRP_INT))):
            return None
        if ins.mnemonic.startswith('j'):
            op = ins.operands[0]
            if op.type == 2:
                if op.imm not in found:
                    return None
            elif (ins.mnemonic == 'jmp' and ins.addr_size == 4 and op.type == 3 and op.size == 4
                    and not op.mem.base and not op.mem.segment and op.mem.index
                    and op.mem.scale == 4):
                pending[pc] = ins
            else:
                return None
            if ins.mnemonic == 'jmp':
                continue
        if pc+ins.size not in found:
            return None

    def targets_at(pc):
        memory = found[pc].operands[0].mem
        values = lift.register_bound(found, pc, memory.index, entry=entry,
                                     switches=switches, read=read)
        if values is None or not values or len(values) > 4096 or max(values) > 4095:
            return None
        targets = set()
        for value in values:
            raw = read((memory.disp+4*value) & 0xffffffff, 4)
            if len(raw) != 4:
                return None
            targets.add(int.from_bytes(raw, 'little'))
        return targets if targets <= found.keys() else None

    try:
        while pending:
            progress = False
            for pc in list(pending):
                targets = targets_at(pc)
                if targets is not None:
                    switches[pc] = targets
                    del pending[pc]
                    progress = True
            if not progress:
                return None
        # Later discovered edges must not invalidate an earlier bound.
        if any(targets_at(pc) != targets for pc, targets in switches.items()):
            return None
    except (KeyError, ValueError, lift.Unsupported):
        return None
    return next(iter(pops)) if len(pops) == 1 else None
