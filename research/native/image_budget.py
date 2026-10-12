"""Finite instruction bounds from known control flow and checked aliases.

This is a deliberately small abstract interpreter, not a guest executor.
Unknown entry registers/flags never become constants implicitly. Guarded callers may
provide a bounded snapshot of their actual initial argument bytes.
It grants a bound only after following a completely known, returning trace.
Stores affect subsequent reads or impose a runtime non-aliasing obligation;
unsupported effects refuse the proof.
"""
from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM
from bisect import bisect_left
import time

MASK = 0xffffffff
REGS = ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp')


def condition_value(condition, flags):
    """Known x86 condition value; absent flag facts never imply false."""
    try:
        if condition in ('e','ne'):
            return flags['zf'] == (condition == 'e')
        if condition in ('b','ae'):
            return flags['cf'] == (condition == 'b')
        if condition in ('be','a'):
            return (flags['cf'] or flags['zf']) == (condition == 'be')
        if condition in ('s','ns'):
            return flags['sf'] == (condition == 's')
        if condition in ('o','no'):
            return flags['of'] == (condition == 'o')
        if condition in ('l','ge'):
            return (flags['sf'] != flags['of']) == (condition == 'l')
        if condition in ('le','g'):
            return (flags['zf'] or flags['sf'] != flags['of']) == (condition == 'le')
        if condition in ('p','np'):
            return flags['pf'] == (condition == 'p')
    except KeyError:
        pass
    return None


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
                closed=frozenset(), models=None, pop_for=None, initial_direction=None,
                caller_stack=None, entry_pointers=None, initial_writes=None,
                entry_values=None, initial_pages=None, read_initial=None):
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

    `entry_pointers` optionally gives the recorder's actual entry register
    addresses as alias witnesses. Register values remain abstractly unknown.
    MOV/LEA, save/restore and constant offsets preserve pointer identities;
    dereferences through them can read only trace-written bytes. Unknown
    writes must not alias these reads (including saved pointer slots), as
    enforced by the same WriteGuard. Initial object bytes remain unknown,
    except `initial_writes`: exact bytes written by a checked original setup.
    These receive the same overlap and unknown-writer guards as trace stores.
    `entry_values` supplies explicit checked-setup register bindings or an
    opt-in recorder's exact initial GPR values. Each must match its entry
    witness, and ESP cannot be rebound.
    Bounded affine expressions preserve relationships between unknown entry
    values through 32-bit copies, sums/differences, LEA and saved words. Only
    cancellation of every symbolic term creates an integer fact. Arithmetic
    flags remain unknown when the operands were unknown.
    Opt-in `initial_pages` supplies complete captured setup input pages,
    including bytes the setup did not write. Reads receive the same alias
    guards; changed instruction bytes refuse the proof. `read_initial` may
    supply lazily generated input bytes only when it uses the exact same
    immutable input generator as execution. Its caller must cap cached pages.
    """
    # Only the bounded caller-argument window is an input fact. The saved
    # return address and uninitialized callee locals are deliberately absent.
    if caller_stack is not None and (not guarded or type(caller_stack) is not bytes
                                     or len(caller_stack) > 128):
        raise ValueError('caller stack requires guarded bytes, at most 128')
    caller_stack = caller_stack or b''
    # Witness addresses are used ONLY for aliasing and reading bytes this
    # trace has already written. They never supply unknown register values,
    # initial object contents or branch/count facts.
    if entry_pointers is not None and (not guarded or set(entry_pointers) != set(REGS)
            or entry_pointers['esp'] != stack
            or any(type(v) is not int or not 0 <= v <= MASK for v in entry_pointers.values())):
        raise ValueError('entry pointers require guarded 32-bit register witnesses')
    pointers = dict(entry_pointers or {})
    pointer_slots = {}
    # Linear expressions modulo 2**32 preserve relationships, not concrete
    # input values. Only complete cancellation may produce a known integer.
    affine_regs = ({r: ({r: 1}, 0) for r in REGS} if entry_pointers is not None else {})
    affine_slots = {}
    if initial_writes is not None and (entry_pointers is None or len(initial_writes) > max_bytes
            or any(type(a) is not int or not 0 <= a <= MASK or type(v) is not int or not 0 <= v <= 255
                   for a,v in initial_writes.items())):
        raise ValueError('setup writes require guarded pointer witnesses and bounded bytes')
    if initial_pages is not None and (entry_pointers is None
            or len(initial_pages)*4096 > max_bytes
            or any(type(p) is not int or not 0 <= p <= 0xfffff000 or p % 4096
                   or type(data) is not bytes or len(data) != 4096
                   for p,data in initial_pages.items())):
        raise ValueError('setup pages require guarded pointer witnesses and bounded pages')
    if read_initial is not None and (entry_pointers is None or not callable(read_initial)):
        raise ValueError('initial input reader requires guarded pointer witnesses')
    initial_pages = initial_pages or {}
    prior = dict(initial_writes or {})
    if not instructions:
        return None
    ins = {i.address: i for i in instructions}
    root_addresses = frozenset(ins)
    code_bytes = {a for i in instructions for a in range(i.address, i.address+i.size)}
    partials = {}
    zero_vectors = set()
    class Registers(dict):
        # Every implicit full-register assignment clears narrower facts too.
        def __setitem__(self, name, value):
            partials.pop(name,None)
            super().__setitem__(name,value)
        def update(self,*args,**kwargs):
            for name,value in dict(*args,**kwargs).items():
                self[name] = value
    regs = Registers(dict.fromkeys(REGS))
    regs['esp'] = stack
    if entry_values is not None:
        if (entry_pointers is None or any(r not in REGS or r == 'esp'
                or type(v) is not int or not 0 <= v <= MASK
                or v != entry_pointers[r] for r,v in entry_values.items())):
            raise ValueError('entry values require matching explicit setup-register bindings')
        regs.update(entry_values)
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

    def pointer_address(i, op):
        if entry_pointers is None or op.mem.segment or i.addr_size != 4:
            return None
        value = op.mem.disp
        for r, scale in ((op.mem.base, 1), (op.mem.index, op.mem.scale)):
            if r:
                name = i.reg_name(r)
                v = regs.get(name)
                if v is None:
                    v = pointers.get(name)
                if v is None:
                    return None
                value += v*scale
        return value & MASK

    def stored_load(a, size):
        # Trace stores, checked writes, or opt-in captured pages supply facts.
        if a is None or any((a+k)&MASK not in stores and (a+k)&MASK not in prior
                            and read_initial is None and (((a+k)&MASK & ~4095) not in initial_pages
                                 or stack <= (a+k)&MASK < stack+4) for k in range(size)):
            return None
        return load(a, size)

    def load_pointer(a):
        value = pointer_slots.get(a)
        if value is not None:
            protected.update((a+k)&MASK for k in range(4))
            if len(protected) > max_bytes:
                raise ValueError('proof read limit')
        return value

    def pointer(i, op):
        if entry_pointers is None or op.size != 4:
            return None
        value = get(i, op)
        if value is not None:
            return value
        if op.type == X86_OP_REG:
            return pointers.get(i.reg_name(op.reg))
        if op.type == X86_OP_MEM:
            a = address(i, op)
            return load_pointer(pointer_address(i,op) if a is None else a)
        return None

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
            if p in stores:
                v = stores[p]
            elif p in prior:
                v = prior[p]
            elif p & ~4095 in initial_pages and not stack <= p < stack+4:
                v = initial_pages[p & ~4095][p & 4095]
            elif 0 <= p-(stack+4) < len(caller_stack):
                v = caller_stack[p-(stack+4)]
            else:
                try:
                    raw = (read_initial(p, 1) if read_initial is not None and not stack <= p < stack+4
                           else read_image(p, 1))
                    v = raw[0] if len(raw) == 1 else None
                except KeyError:
                    v = None
            values.append(v)
        if None in values:
            return None  # no value was inferred from this load
        protected.update((a+k)&MASK for k in range(size))
        if len(protected) > max_bytes:
            raise ValueError('proof read limit')
        return int.from_bytes(bytes(values), 'little')

    def store(a, size, value, symbol=None, pointer_value=None, affine_value=None):
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
        for old in list(pointer_slots):
            if any((a+k-old)&MASK < 4 for k in range(size)):
                del pointer_slots[old]
        for old in list(affine_slots):
            if any((a+k-old)&MASK < 4 for k in range(size)):
                del affine_slots[old]
        if size == 4 and value is None and affine_value is not None:
            affine_slots[a] = affine_value
        if entry_pointers is not None and size == 4 and value is None and pointer_value is not None:
            pointer_slots[a] = pointer_value
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
            a = address(i, op)
            if a is None and entry_pointers is not None:
                return stored_load(pointer_address(i,op),op.size)
            return load(a, op.size, symbolic(i,op))
        if op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            if regs[name] is not None:
                return (regs[name] >> shift) & ((1 << bits)-1)
            known,value = partials.get(name,(0,0))
            wanted = ((1 << bits)-1) << shift
            return (value >> shift) & ((1 << bits)-1) if known & wanted == wanted else None
        raise ValueError('operand')

    def affine_add(left, right, scale=1):
        if left is None or right is None:
            return None
        terms = dict(left[0])
        for origin, coefficient in right[0].items():
            terms[origin] = (terms.get(origin,0)+scale*coefficient)&MASK
        return {k:v for k,v in terms.items() if v}, (left[1]+scale*right[1])&MASK

    def affine_load(a):
        value = affine_slots.get(a)
        if value is not None:
            protected.update((a+k)&MASK for k in range(4))
            if len(protected) > max_bytes:
                raise ValueError('proof read limit')
        return value

    def affine_get(i, op):
        if entry_pointers is None or op.size != 4:
            return None
        value = get(i,op)
        if value is not None:
            return {}, value
        if op.type == X86_OP_REG:
            return affine_regs.get(i.reg_name(op.reg))
        if op.type == X86_OP_MEM:
            a = address(i,op)
            return affine_load(pointer_address(i,op) if a is None else a)
        return None

    def affine_address(i, op):
        if entry_pointers is None or op.mem.segment or i.addr_size != 4:
            return None
        result = ({}, op.mem.disp & MASK)
        for reg, scale in ((op.mem.base,1),(op.mem.index,op.mem.scale)):
            if reg:
                name = i.reg_name(reg)
                value = regs.get(name)
                source = ({},value) if value is not None else affine_regs.get(name)
                result = affine_add(result,source,scale)
        return result

    def put(i, op, value, pointer_value=None, affine_value=None):
        if value is None and affine_value is not None and not affine_value[0]:
            value = affine_value[1]
        if op.type == X86_OP_MEM:
            a = address(i,op)
            store(pointer_address(i,op) if a is None else a, op.size, value,
                  symbolic(i,op),pointer_value,affine_value)
        elif op.type == X86_OP_REG:
            name, shift, bits = register(i, op)
            revisions[name] += 1
            pointers[name] = pointer_value if bits == 32 else None
            affine_regs[name] = affine_value if bits == 32 else None
            mask = (1 << bits)-1
            known,old = (MASK,regs[name]) if regs[name] is not None else partials.get(name,(0,0))
            lane = mask << shift
            known = (known & ~lane) | (lane if value is not None else 0)
            combined = (old & ~lane) | (((value & mask) << shift) if value is not None else 0)
            regs[name] = combined & MASK if known == MASK else None
            if known and known != MASK:
                partials[name] = known,combined & MASK
        else:
            raise ValueError('destination')

    def finish(extra=0):
        if regs['esp'] != stack or any(stack+k in stores for k in range(4)):
            return None
        if code_bytes.intersection(stores) or count+overhead+extra > ceiling:
            return None
        if guarded:
            return dict(instructions=count+overhead+extra,
                        protected=sorted(protected | code_bytes | set(range(stack,stack+4))),
                        unknown_writes=sorted(unknown_writes),mock_calls=mock_calls)
        return count+extra

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
            if any(a in prior for a in range(i.address,i.address+i.size)):
                return None  # setup may not change the instructions being proved
            if any((a & ~4095) in initial_pages and initial_pages[a & ~4095][a & 4095] != b
                   for a,b in zip(range(i.address,i.address+i.size),i.bytes)):
                return None  # initial pages must retain the decoded instructions
            mn, op = i.mnemonic, i.operands
            before_esp, was_root = regs['esp'], not frames
            pc += i.size
            if mn in ('ret', 'retn'):
                if frames:
                    ret, saved = frames.pop()
                    # A stack allocation helper may adjust ESP. The
                    # actual RET operand must still be the saved call PC.
                    if regs['esp'] is None or load(regs['esp'],4) != ret:
                        return None
                    regs['esp'] = (regs['esp']+4+(op[0].imm if op else 0)) & MASK
                    pc = ret
                    if not frames:
                        outgoing = 0
                    continue
                # A saved root return address must not have been overwritten.
                return finish()
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
                        pointers['esi'] = None
                        affine_regs['esi'] = None
                        revisions['esi'] += 1
                    elif dst is not None:
                        for k in range(n):
                            store((dst+direction*k*size)&MASK,size,regs['eax'])
                    if dst is not None:
                        regs['edi'] = (dst+direction*n*size)&MASK
                    revisions['edi'] += 1
                    pointers['edi'] = None
                    affine_regs['edi'] = None
                regs['ecx'] = 0
                affine_regs['ecx'] = None
                overhead += n  # Unicorn visits each iteration and the completion
                continue
            if mn == 'push' and op[0].size == 4:
                value = get(i, op[0])
                pointer_value = pointer(i,op[0])
                affine_value = affine_get(i,op[0])
                regs['esp'] = (regs['esp']-4) & MASK
                store(regs['esp'], 4, value, pointer_value=pointer_value, affine_value=affine_value)
            elif mn == 'pop' and op[0].type == X86_OP_REG and op[0].size == 4 and i.reg_name(op[0].reg) != 'esp':
                value = load(regs['esp'], 4)
                pointer_value = load_pointer(regs['esp'])
                affine_value = affine_load(regs['esp'])
                regs['esp'] = (regs['esp']+4) & MASK
                put(i, op[0], value, pointer_value, affine_value)
            elif mn == 'mov':
                put(i, op[0], get(i, op[1]), pointer(i,op[1]), affine_get(i,op[1]))
            elif guarded and mn == 'xchg' and all(o.type == X86_OP_REG and o.size == 4 for o in op):
                a,b = (get(i,o) for o in op)
                pa,pb = (pointer(i,o) for o in op)
                aa,ab = (affine_get(i,o) for o in op)
                put(i,op[0],b,pb,ab)
                put(i,op[1],a,pa,aa)
            elif guarded and mn in ('movzx','movsx'):
                value = get(i,op[1])
                if value is not None and mn == 'movsx' and value & (1 << (op[1].size*8-1)):
                    value -= 1 << (op[1].size*8)
                put(i,op[0],value)
            elif mn == 'lea' and op[0].type == X86_OP_REG and op[0].size == 4:
                put(i, op[0], address(i, op[1]),pointer_address(i,op[1]),affine_address(i,op[1]))
            elif guarded and mn == 'not':
                value = get(i,op[0])
                put(i,op[0],None if value is None else ~value)
                # NOT leaves every arithmetic flag unchanged.
            elif guarded and mn == 'neg' and op[0].size in (1,2,4):
                value = get(i,op[0])
                bits = op[0].size*8
                result = None if value is None else -value & ((1 << bits)-1)
                flags = {} if result is None else dict(zf=result == 0,
                    sf=bool(result & (1 << (bits-1))),cf=value != 0,
                    of=value == (1 << (bits-1)))
                put(i,op[0],result)
            elif guarded and mn.startswith('set') and len(op) == 1 and op[0].size == 1:
                value = condition_value(mn[3:],flags)
                put(i,op[0],None if value is None else int(value))
                # SETcc materializes a condition without changing its flags.
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
                if result is not None:
                    if mn in ('xor','or','and','test'):
                        flags['of'] = False
                    else:
                        signed_a = a-(1 << bits) if a & (1 << (bits-1)) else a
                        signed_b = b-(1 << bits) if b & (1 << (bits-1)) else b
                        total = (signed_a-signed_b-carry if mn in ('cmp','sub','sbb','dec')
                                 else signed_a+signed_b+carry)
                        flags['of'] = not -(1 << (bits-1)) <= total < (1 << (bits-1))
                if mn not in ('cmp','test'):
                    affine_value = None
                    if bits == 32 and mn in ('add','sub','inc','dec'):
                        right = affine_get(i,op[1]) if len(op)>1 else ({},1)
                        affine_value = affine_add(affine_get(i,op[0]),right,
                                                  1 if mn in ('add','inc') else -1)
                    base_pointer = pointer(i,op[0])
                    adjusted = None
                    if base_pointer is not None and b is not None and mn in ('add','sub','inc','dec'):
                        adjusted = (base_pointer+(b if mn in ('add','inc') else -b)) & MASK
                    put(i, op[0], result, adjusted, affine_value)
            elif guarded and mn in ('div','idiv') and op[0].size == 4:
                low,high,divisor = regs['eax'],regs['edx'],get(i,op[0])
                if low is None or high is None or divisor in (None,0):
                    return None
                dividend = (high << 32) | low
                if mn == 'idiv':
                    if high & 0x80000000: dividend -= 1 << 64
                    if divisor & 0x80000000: divisor -= 1 << 32
                    quotient = abs(dividend)//abs(divisor)
                    if (dividend < 0) != (divisor < 0): quotient = -quotient
                    if not -(1 << 31) <= quotient < (1 << 31): return None
                else:
                    quotient = dividend//divisor
                    if quotient > MASK: return None
                remainder = dividend-quotient*divisor
                for name,value in (('eax',quotient),('edx',remainder)):
                    regs[name] = value & MASK
                    revisions[name] += 1
                    pointers[name] = affine_regs[name] = None
                flags = {}  # Neither division form defines arithmetic flags.
            elif guarded and mn in ('mul','imul') and len(op) == 1 and op[0].size == 4:
                a,b = regs['eax'],get(i,op[0])
                product,overflow = None,None
                if a is not None and b is not None:
                    if mn == 'imul':
                        a = a-(1 << 32) if a & 0x80000000 else a
                        b = b-(1 << 32) if b & 0x80000000 else b
                    product = a*b
                    overflow = (not -(1 << 31) <= product < (1 << 31) if mn == 'imul'
                                else product > MASK)
                for name,shift in (('eax',0),('edx',32)):
                    regs[name] = None if product is None else (product >> shift) & MASK
                    revisions[name] += 1
                    pointers[name] = affine_regs[name] = None
                flags = {} if overflow is None else dict(cf=overflow,of=overflow)
            elif guarded and mn == 'imul' and len(op) in (2,3) and op[0].size == 4:
                a,b = (get(i,op[0]),get(i,op[1])) if len(op)==2 else (get(i,op[1]),get(i,op[2]))
                result, overflow = None, None
                if a is not None and b is not None:
                    signed_a = a-(1 << 32) if a & 0x80000000 else a
                    signed_b = b-(1 << 32) if b & 0x80000000 else b
                    product = signed_a*signed_b
                    result = product & MASK
                    overflow = not -(1 << 31) <= product < (1 << 31)
                put(i,op[0],result)
                # IMUL defines only CF/OF; a known product does not define ZF/SF.
                flags = {} if overflow is None else dict(cf=overflow,of=overflow)
            elif guarded and mn in ('rol','ror') and op[0].size == 4:
                a,n = get(i,op[0]),get(i,op[1])
                n = None if n is None else n & 31
                if n == 0:
                    continue
                result = None
                flags.pop('cf',None);flags.pop('of',None)
                if a is not None and n is not None:
                    result = (((a << n) | (a >> (32-n))) if mn == 'rol'
                              else ((a >> n) | (a << (32-n)))) & MASK
                    flags['cf'] = bool(result & (1 if mn == 'rol' else 0x80000000))
                    if n == 1:
                        flags['of'] = (bool(result & 0x80000000) != flags['cf'] if mn == 'rol'
                                       else bool(result & 0x80000000) != bool(result & 0x40000000))
                put(i,op[0],result)  # ZF/SF remain unchanged by a rotate.
            elif guarded and mn == 'cdq':
                value = regs['eax']
                regs['edx'] = None if value is None else MASK if value & 0x80000000 else 0
                revisions['edx'] += 1
                pointers['edx'] = affine_regs['edx'] = None
                # Sign extension preserves all flags.
            elif guarded and mn in ('shld','shrd') and op[0].size == op[1].size == 4:
                a,b,n = get(i,op[0]),get(i,op[1]),get(i,op[2])
                n = None if n is None else n & 31
                if n == 0:
                    continue
                result = None
                if a is not None and b is not None and n is not None:
                    result = (((a >> n) | (b << (32-n))) if mn == 'shrd'
                              else ((a << n) | (b >> (32-n)))) & MASK
                put(i,op[0],result)
                flags = {} if result is None else dict(zf=result == 0,sf=bool(result & 0x80000000),
                    cf=bool((a >> (n-1 if mn == 'shrd' else 32-n)) & 1))
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
            elif mn in ('jmp','je','jne','jb','jae','jbe','ja','js','jns','jl','jge','jle','jg') and op[0].type == X86_OP_IMM:
                if mn == 'jmp': taken = True
                elif mn in ('je','jne'): taken = flags['zf'] == (mn == 'je')
                elif mn in ('js','jns'): taken = flags['sf'] == (mn == 'js')
                elif mn in ('jl','jge'): taken = (flags['sf'] != flags['of']) == (mn == 'jl')
                elif mn in ('jle','jg'): taken = (flags['zf'] or flags['sf'] != flags['of']) == (mn == 'jle')
                elif mn in ('jb','jae'): taken = flags['cf'] == (mn == 'jb')
                else: taken = (flags['cf'] or flags['zf']) == (mn == 'jbe')
                if taken:
                    target = op[0].imm & MASK
                    if guarded and not frames and target not in root_addresses:
                        if mn != 'jmp' or target in closed or target in models or pop_for is None:
                            return None
                        pop = pop_for(target)
                        if type(pop) is not int or not 0 <= pop <= 0x400 or pop % 4:
                            return None
                        # A root tail uses its existing return address. Execution
                        # must perform this exact external mock and stack cleanup.
                        mock_calls[i.address] = pop
                        return finish(4)
                    pc = target
            elif guarded and mn == 'call':
                zero_vectors.clear()  # do not assume vector values survive a call
                direct = op[0].type == X86_OP_IMM
                target = op[0].imm & MASK if direct else get(i,op[0])
                opaque.clear()
                if not direct and target not in closed and target not in models:
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
                        dst_pointer = load_pointer(regs['esp'])
                        src_pointer = load_pointer(regs['esp']+4)
                        if n is None or n > max_bytes:
                            return None
                        location = dst if dst is not None else dst_pointer
                        if location is None:
                            # The model's mwrite must enforce this obligation too.
                            unknown_writes.add(target)
                        else:
                            values = ([None if src is None else src & 255]*n if model == 'memset'
                                      else [(stored_load(None if src_pointer is None else (src_pointer+k)&MASK,1)
                                             if src is None else load((src+k)&MASK,1)) for k in range(n)])
                            for k,value in enumerate(values):
                                store((location+k)&MASK,1,value)
                        regs['eax'] = dst
                        pointers['eax'] = dst_pointer
                        affine_regs['eax'] = None
                    elif model in ('pool40_alloc','mt_alloc'):
                        # Allocation addresses are deliberately not predicted.
                        regs['eax'] = None
                        pointers['eax'] = None
                        affine_regs['eax'] = None
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
                    affine_regs.update(eax=None,ecx=None,edx=None)
                    pointers.update(eax=None,ecx=None,edx=None)
                    overhead += 4  # mock entry plus a possible x87 trampoline
                else:
                    return None
                flags = {}
                if was_root:
                    outgoing = 0
            elif guarded and mn in ('movq','movss','movsd','movaps','movups','movdqa','movdqu',
                                    'pxor','xorps','xorpd','shufps','shufpd','fxch',
                                    'fld','fldz','fld1','fstp','fst','fild','fistp',
                                    'addss','subss','mulss','divss','sqrtss','minss','maxss',
                                    'addps','subps','mulps','divps','sqrtps','minps','maxps',
                                    'addsd','subsd','mulsd','divsd','sqrtsd','minsd','maxsd'):
                # Only exact bitwise-zero vectors supply known store bytes.
                # All other scalar/SIMD results remain opaque; no FP arithmetic
                # or flags are inferred here.
                # Register shuffles and x87 exchanges preserve integer control
                # flow; their numeric outputs remain unknown.
                zero_result = False
                if len(op) == 2 and all(o.type == X86_OP_REG for o in op):
                    dst,src = (i.reg_name(o.reg) for o in op)
                    if dst.startswith('xmm') and src.startswith('xmm'):
                        zero_result = (mn in ('pxor','xorps','xorpd') and
                                       (dst == src or dst in zero_vectors and src in zero_vectors))
                        zero_result |= mn in ('movq','movaps','movups','movdqa','movdqu') and src in zero_vectors
                written_vectors = {i.reg_name(r) for r in i.regs_access()[1] if i.reg_name(r).startswith('xmm')}
                if op and op[0].type == X86_OP_REG and i.reg_name(op[0].reg).startswith('xmm'):
                    written_vectors.add(i.reg_name(op[0].reg))
                if any(i.reg_name(r) in (*REGS,'eflags') for r in i.regs_access()[1]):
                    return None
                for index,o in enumerate(op):
                    # Capstone labels some SIMD stores (e.g. MOVUPS) as a
                    # read. A memory destination of a move is always written.
                    move_store = index == 0 and mn in ('movq','movss','movsd','movaps','movups','movdqa','movdqu')
                    if o.type == X86_OP_MEM and (o.access & 2 or move_store or mn in ('fstp','fst','fistp')):
                        zero_store = (move_store and len(op) > 1 and op[1].type == X86_OP_REG
                                      and i.reg_name(op[1].reg) in zero_vectors)
                        put(i,o,0 if zero_store else None)
                zero_vectors.difference_update(written_vectors)
                if zero_result:
                    zero_vectors.add(i.reg_name(op[0].reg))
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
