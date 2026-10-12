"""Finite instruction bounds from known control flow and checked aliases.

This is a deliberately small abstract interpreter, not a guest executor.
Unknown entry registers/flags or non-image inputs never become constants.
It grants a bound only after following a completely known, returning trace.
Stores affect subsequent reads or impose a runtime non-aliasing obligation;
unsupported effects refuse the proof.
"""
from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM
from bisect import bisect_left
import time

MASK = 0xffffffff
REGS = ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp')


class WriteGuard:
    """Enforce every non-aliasing obligation of a guarded trace at runtime."""
    def __init__(self, certificate):
        self.sites = frozenset(certificate['unknown_writes'])
        self.protected = certificate['protected']
        self.calls = certificate.get('mock_calls', {})
        self.visits = {}
        self.mocks = {}

    def call(self, pc):
        if pc in self.calls:
            self.visits[pc] = self.visits.get(pc,0)+1

    def mock(self, pc, pop):
        if pc not in self.calls:
            return True
        if pop != self.calls[pc]:
            return False
        self.mocks[pc] = self.mocks.get(pc,0)+1
        return True

    def calls_match(self):
        # An actual closed/platform/internal callee produces no mock event.
        return self.visits == self.mocks

    def conflicts(self, pc, address, size):
        if pc not in self.sites or size <= 0 or not self.protected:
            return False
        address &= MASK
        end = address+size
        k = bisect_left(self.protected,address)
        if k < len(self.protected) and self.protected[k] < end:
            return True
        return end > MASK+1 and self.protected[0] < end-(MASK+1)


def image_bound(instructions, read_image, stack, ceiling=2_000_000, entry=None,
                deadline=None, max_bytes=256*4096, guarded=False, decode=None,
                closed=frozenset(), models=None, pop_for=None, initial_direction=None):
    """Return a strict count or guarded bound certificate; None if unproven.

    `read_image(address, size)` must return original image bytes only and
    raise KeyError elsewhere. `stack` is the recorder's actual entry ESP.
    The strict mode refuses calls, indirect flow and unknown addresses.
    Guarded mode may ignore unknown-address stores ONLY if the returned
    WriteGuard is enforced on all guest and platform-model writes. Such
    stores may not alias any concrete read, executed code or root return
    byte. Known interior indirect jumps are followed. Direct closed calls
    are traced; root external direct calls use
    the recorder's zero-return mock and static cleanup contract. Unknown
    root indirect calls may propose a mock contract after an argument
    boundary; every actual call visit must produce a matching mock event.
    Unknown conditions and unsupported effects still refuse the proof.
    """
    if not instructions:
        return None
    ins = {i.address: i for i in instructions}
    root_addresses = frozenset(ins)
    code_bytes = {a for i in instructions for a in range(i.address, i.address+i.size)}
    regs = dict.fromkeys(REGS)
    regs['esp'] = stack
    stores = {}
    opaque = {}
    revisions = dict.fromkeys(REGS,0)
    flags = {}
    pc = instructions[0].address if entry is None else entry
    protected, unknown_writes, frames = set(), set(), []
    overhead = 0
    direction = initial_direction if initial_direction in (-1,1) else None
    outgoing = None  # unknown until the first completed root call
    mock_calls = {}
    models = models or {}

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
                    if guarded:
                        return None
                    raise ValueError('input-dependent address')
                value += regs[name]*scale
        return value & MASK

    def symbolic(i, op):
        m = op.mem
        if not guarded or m.segment or i.addr_size != 4 or not m.base or m.index:
            return None
        name = i.reg_name(m.base)
        if name not in regs or regs[name] is not None:
            return None
        return (name,revisions[name]),m.disp & MASK

    def load(a, size, symbol=None):
        if a is None:
            if symbol is None:
                return None
            key,offset = symbol
            values = [opaque.get((key,(offset+k)&MASK)) for k in range(size)]
            return None if None in values else int.from_bytes(bytes(values),'little')
        values = []
        for k in range(size):
            p = (a+k) & MASK
            protected.add(p)
            if len(protected) > max_bytes:
                raise ValueError('proof read limit')
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

    def store(a, size, value, symbol=None):
        if a is None:
            if not guarded:
                raise ValueError('unknown destination')
            unknown_writes.add(i.address)
            if symbol is None:
                opaque.clear()
            else:
                key,offset = symbol
                # Different unknown bases may alias. Offsets from the SAME
                # unchanged base are exact, including 32-bit wrapping.
                for old in list(opaque):
                    if old[0] != key:
                        del opaque[old]
                for k in range(size):
                    opaque[key,(offset+k)&MASK] = None if value is None else (value >> (8*k)) & 255
                if len(opaque) > max_bytes:
                    raise ValueError('proof symbolic memory limit')
            return
        opaque.clear()  # a concrete store might alias any unknown base
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
            return load(address(i, op), op.size, symbolic(i,op))
        if op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            return None if regs[name] is None else (regs[name] >> shift) & ((1 << bits)-1)
        raise ValueError('operand')

    def put(i, op, value):
        if op.type == X86_OP_MEM:
            store(address(i, op), op.size, value, symbolic(i,op))
        elif op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            revisions[name] += 1
            mask = (1 << bits)-1
            regs[name] = (None if value is None or (bits != 32 and regs[name] is None)
                          else ((regs[name] or 0) & ~(mask << shift)) | ((value & mask) << shift))
        else:
            raise ValueError('destination')

    try:
        for count in range(1, ceiling+1):
            if count+overhead > ceiling:
                return None
            if count % 256 == 1 and deadline is not None and time.monotonic() >= deadline:
                return None
            i = ins.get(pc)
            if i is None and guarded and decode is not None:
                i = decode(pc)
                if i is not None:
                    ins[pc] = i
                    code_bytes.update(range(pc,pc+i.size))
            if i is None:
                return None
            mn, op = i.mnemonic, i.operands
            before_esp, was_root = regs['esp'], not frames
            pc += i.size
            if mn in ('ret', 'retn'):
                if frames:
                    ret, saved = frames.pop()
                    if regs['esp'] != saved or load(saved,4) != ret:
                        return None
                    regs['esp'] = (saved+4+(op[0].imm if op else 0)) & MASK
                    pc = ret
                    if not frames:
                        outgoing = 0
                    continue
                # A saved root return address must not have been overwritten.
                if regs['esp'] != stack or any(stack+k in stores for k in range(4)):
                    return None
                if code_bytes.intersection(stores):
                    return None  # includes code discovered in closed callees
                if guarded:
                    return dict(instructions=count+overhead,
                                protected=sorted(protected | code_bytes | set(range(stack,stack+4))),
                                unknown_writes=sorted(unknown_writes), mock_calls=mock_calls)
                return count
            if mn == 'nop':
                continue
            if guarded and mn in ('cld','std'):
                direction = 1 if mn == 'cld' else -1
                continue
            if guarded and mn in ('rep stosb','rep stosw','rep stosd','rep movsb','rep movsw','rep movsd') and i.addr_size == 4:
                n = regs['ecx']
                size = {'b':1,'w':2,'d':4}[mn[-1]]
                copy = mn.startswith('rep movs')
                if n is None or n*size > max_bytes or count+overhead+n > ceiling:
                    return None
                dst = regs['edi']
                if n:
                    if copy and direction is None:
                        return None
                    if dst is None:
                        unknown_writes.add(i.address)
                    elif direction is None:
                        return None
                    if copy:
                        src = regs['esi']
                        for k in range(n):
                            value = load(None if src is None else (src+direction*k*size)&MASK,size)
                            store(None if dst is None else (dst+direction*k*size)&MASK,size,value)
                        regs['esi'] = None if src is None else (src+direction*n*size)&MASK
                        revisions['esi'] += 1
                    elif dst is not None:
                        for k in range(n):
                            store((dst+direction*k*size)&MASK,size,regs['eax'])
                    if dst is not None:
                        regs['edi'] = (dst+direction*n*size)&MASK
                    revisions['edi'] += 1
                regs['ecx'] = 0
                overhead += n  # Unicorn visits each iteration and the completion
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
            elif guarded and mn in ('movzx','movsx'):
                value = get(i,op[1])
                if value is not None and mn == 'movsx' and value & (1 << (op[1].size*8-1)):
                    value -= 1 << (op[1].size*8)
                put(i,op[0],value)
            elif mn == 'lea' and op[0].type == X86_OP_REG and op[0].size == 4:
                put(i, op[0], address(i, op[1]))
            elif guarded and mn == 'not':
                value = get(i,op[0])
                put(i,op[0],None if value is None else ~value)
                # NOT leaves every arithmetic flag unchanged.
            elif mn in ('xor', 'or', 'and', 'add', 'sub', 'inc', 'dec', 'cmp', 'test') or (guarded and mn in ('adc','sbb')):
                a = get(i, op[0])
                b = get(i, op[1]) if len(op) > 1 else 1
                carry = flags.get('cf') if mn in ('adc','sbb') else 0
                if mn in ('xor','sbb') and all(o.type == X86_OP_REG for o in op) and op[0].reg == op[1].reg:
                    a = b = 0
                bits = op[0].size*8
                mask = (1 << bits)-1
                result = None if a is None or b is None or carry is None else {
                    'xor': lambda: a ^ b, 'or': lambda: a | b, 'and': lambda: a & b,
                    'add': lambda: a+b, 'inc': lambda: a+1,
                    'adc': lambda: a+b+carry, 'sbb': lambda: a-b-carry,
                    'sub': lambda: a-b, 'dec': lambda: a-1, 'cmp': lambda: a-b,
                    'test': lambda: a & b}[mn]() & mask
                old_cf = flags.get('cf')
                flags = {} if result is None else dict(zf=result == 0, sf=bool(result & (1 << (bits-1))))
                if result is not None:
                    if mn in ('cmp','sub','sbb'):
                        flags['cf'] = a < b+carry
                    elif mn in ('add','adc'):
                        flags['cf'] = a+b+carry > mask
                    elif mn in ('xor','or','and','test'):
                        flags['cf'] = False
                    elif old_cf is not None:
                        flags['cf'] = old_cf
                if mn not in ('cmp','test'):
                    put(i, op[0], result)
            elif guarded and mn in ('shl','shr','sar'):
                a,n = get(i,op[0]),get(i,op[1])
                bits = op[0].size*8
                if n is not None:
                    n &= 31
                if n == 0:
                    continue  # value and flags remain unchanged
                result = None
                if a is not None and n is not None:
                    signed = a-(1 << bits) if a & (1 << (bits-1)) else a
                    result = ((a << n) if mn == 'shl' else (a >> n) if mn == 'shr' else (signed >> n)) & ((1 << bits)-1)
                put(i,op[0],result)
                flags = {} if result is None else dict(zf=result == 0,sf=bool(result & (1 << (bits-1))))
                if result is not None and n <= bits:
                    flags['cf'] = bool((a >> (bits-n if mn == 'shl' else n-1)) & 1)
            elif guarded and mn == 'jmp' and op[0].type != X86_OP_IMM:
                target = get(i,op[0])
                if target is None or (not frames and target not in root_addresses):
                    return None
                pc = target
            elif mn in ('jmp','je','jne','jb','jae','jbe','ja','js','jns') and op[0].type == X86_OP_IMM:
                if mn == 'jmp': taken = True
                elif mn in ('je','jne'): taken = flags['zf'] == (mn == 'je')
                elif mn in ('js','jns'): taken = flags['sf'] == (mn == 'js')
                elif mn in ('jb','jae'): taken = flags['cf'] == (mn == 'jb')
                else: taken = (flags['cf'] or flags['zf']) == (mn == 'jbe')
                if taken:
                    target = op[0].imm & MASK
                    if guarded and not frames and target not in root_addresses:
                        return None  # a root tail is a different mock boundary
                    pc = target
            elif guarded and mn == 'call':
                opaque.clear()
                direct = op[0].type == X86_OP_IMM
                target = op[0].imm & MASK if direct else None
                if not direct:
                    # A candidate contract, never an assumed actual dispatch:
                    # original execution must mock every visit with this pop.
                    if frames or outgoing is None or not 0 <= outgoing <= 0x400:
                        return None
                    if i.address in mock_calls and mock_calls[i.address] != outgoing:
                        return None
                    mock_calls[i.address] = outgoing
                saved = (regs['esp']-4) & MASK
                store(saved,4,pc)
                if target in models:
                    model = models[target]
                    overhead += 1  # the platform entry hook
                    if model in ('memset','memcpy'):
                        dst,src,n = (load(regs['esp']+k,4) for k in (0,4,8))
                        if n is None or n > max_bytes:
                            return None
                        if dst is None:
                            # The model's mwrite must enforce this obligation too.
                            unknown_writes.add(target)
                        else:
                            values = ([None if src is None else src & 255]*n if model == 'memset'
                                      else [load(None if src is None else (src+k)&MASK,1) for k in range(n)])
                            for k,value in enumerate(values):
                                store((dst+k)&MASK,1,value)
                        regs['eax'] = dst
                    elif model in ('pool40_alloc','mt_alloc'):
                        # Allocation addresses are deliberately not predicted.
                        regs['eax'] = None
                    elif model != 'mt_free':
                        return None
                elif target in closed:
                    regs['esp'] = saved
                    frames.append((pc,saved))
                    pc = target
                elif not frames and (not direct or pop_for is not None):
                    # This policy uses the recorder's zero-return mocks. The
                    # known direct target and cleanup are the same contract.
                    regs['esp'] = (regs['esp']+(pop_for(target) if direct else outgoing)) & MASK
                    regs.update(eax=0,ecx=0,edx=0)
                    overhead += 4  # mock entry plus a possible x87 trampoline
                else:
                    return None
                flags = {}
                if was_root:
                    outgoing = 0
            elif guarded and mn in ('movq','movss','movsd','movaps','movups','movdqa','movdqu',
                                    'pxor','xorps','xorpd','fld','fldz','fld1','fstp','fst','fild','fistp',
                                    'addss','subss','mulss','divss','sqrtss','minss','maxss',
                                    'addps','subps','mulps','divps','sqrtps','minps','maxps',
                                    'addsd','subsd','mulsd','divsd','sqrtsd','minsd','maxsd'):
                # Value-opaque scalar/SIMD data movement: never invent a
                # value or flags. Known destinations become unknown bytes.
                if any(i.reg_name(r) in (*REGS,'eflags') for r in i.regs_access()[1]):
                    return None
                for index,o in enumerate(op):
                    # Capstone labels some SIMD stores (e.g. MOVUPS) as a
                    # read. A memory destination of a move is always written.
                    move_store = index == 0 and mn in ('movq','movss','movsd','movaps','movups','movdqa','movdqu')
                    if o.type == X86_OP_MEM and (o.access & 2 or move_store or mn in ('fstp','fst','fistp')):
                        store(address(i,o),o.size,None,symbolic(i,o))
            else:
                return None
            if regs['esp'] is None:
                return None
            if was_root and mn != 'call' and outgoing is not None:
                delta = before_esp-regs['esp']
                outgoing = max(0,outgoing+delta) if abs(delta) <= 0x10000 else None
    except (KeyError, ValueError, TypeError):
        return None
    return None
