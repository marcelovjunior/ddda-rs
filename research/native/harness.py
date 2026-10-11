"""Differential harness: run an ORIGINAL DDDA.exe function in Unicorn.

Maps every section of the unpacked analysis exe at its image address,
calls one function with the given ECX (thiscall), stack arguments and
scratch memory, and returns EAX/EDX, ST0 (via the x87 stack, if asked) and
the final scratch bytes. Original instructions only: no patching, stubs or
callbacks. Any call to an address outside `allow` stops the run as an
error, so a test can never silently skip a callee.

Known emulator limit: Unicorn does not quiet a signalling NaN on x87 fld
(real hardware does), so x87 paths are tested with quiet NaNs only and
signalling NaNs against the CPU itself (ddnative x87 hardware test).

Reads the user's own exe at run time; nothing from it is committed.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe (capstone, unicorn).
"""
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UcError
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
                               UC_X86_REG_EIP, UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI,
                               UC_X86_REG_EBP)

EXE = "E:/ddda-re/bin/DDDA.exe.unpacked.exe"
STACK, STACK_SIZE = 0x0F000000, 0x00100000
SCRATCH, SCRATCH_SIZE = 0x0E000000, 0x00100000
RET = 0x0DEAD000

_image = None


def image():
    """(base, [(va, bytes)]) of the PE sections."""
    global _image
    if _image is None:
        data = open(EXE, "rb").read()
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        nsec = struct.unpack_from("<H", data, pe + 6)[0]
        opt = pe + 24
        base = struct.unpack_from("<I", data, opt + 28)[0]
        sec = opt + struct.unpack_from("<H", data, pe + 20)[0]
        secs = []
        for i in range(nsec):
            o = sec + i * 40
            vsize, va, rsize, rptr = struct.unpack_from("<IIII", data, o + 8)
            secs.append((base + va, data[rptr:rptr + rsize].ljust(max(vsize, rsize), b"\0")))
        _image = (base, secs)
    return _image


def code(address, length):
    base, secs = image()
    for va, b in secs:
        if va <= address < va + len(b):
            return b[address - va:address - va + length]
    raise KeyError(hex(address))


def executable(address, length=1, _ranges=[]):
    """PE executable sections, rather than a guessed .text address limit."""
    if not _ranges:
        with open(EXE, "rb") as f:
            data = f.read()
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        opt = pe + 24
        base = struct.unpack_from("<I", data, opt + 28)[0]
        sec = opt + struct.unpack_from("<H", data, pe + 20)[0]
        for k in range(struct.unpack_from("<H", data, pe + 6)[0]):
            o = sec + k * 40
            vsize, va, rsize = struct.unpack_from("<III", data, o + 8)
            if struct.unpack_from("<I", data, o + 36)[0] & 0x20000000:
                _ranges.append((base + va, base + va + max(vsize, rsize)))
    return any(a <= address and address + length <= b for a, b in _ranges)


def run(address, ecx=0, args=(), scratch=b"", regs=None, allow=(), max_insn=100000, poke=None):
    """Call `address`. Returns dict(eax, edx, scratch, insns)."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(TCG_BUFFER)  # default 1 GB per instance: workers reached 4 GB
    base, secs = image()
    for va, b in secs:
        start = va & ~0xFFF
        size = ((va + len(b) - start) + 0xFFF) & ~0xFFF
        uc.mem_map(start, size)
        uc.mem_write(va, b)
    uc.mem_map(STACK, STACK_SIZE)
    uc.mem_map(SCRATCH, SCRATCH_SIZE)
    uc.mem_map(RET & ~0xFFF, 0x1000)
    uc.mem_write(SCRATCH, scratch)
    for addr, value in (poke or {}).items():  # test fixture: a global pointer
        uc.mem_write(addr, struct.pack("<I", value & 0xFFFFFFFF))
    esp = STACK + STACK_SIZE - 0x1000
    frame = struct.pack("<I", RET) + b"".join(struct.pack("<I", a & 0xFFFFFFFF) for a in args)
    uc.mem_write(esp, frame)
    uc.reg_write(UC_X86_REG_ESP, esp)
    uc.reg_write(UC_X86_REG_ECX, ecx & 0xFFFFFFFF)
    for r, v in (regs or {}).items():
        uc.reg_write(r, v & 0xFFFFFFFF)
    count = [0]
    bad = []

    def hook(uc_, addr, size, _):
        count[0] += 1
        if count[0] > max_insn:
            bad.append(f"over {max_insn} instructions")
            uc_.emu_stop()
        insn = uc_.mem_read(addr, size if 0 < size <= 15 else 5)  # the hook size can be a huge sentinel
        if insn[0] == 0xE8:  # direct call: must be explicitly allowed
            target = (addr + size + struct.unpack("<i", bytes(insn[1:5]))[0]) & 0xFFFFFFFF
            if target not in allow:
                bad.append(f"call to {target:#010x} at {addr:#010x} not allowed")
                uc_.emu_stop()

    uc.hook_add(UC_HOOK_CODE, hook)
    try:
        uc.emu_start(address, RET)
    except UcError as e:
        bad.append(f"{e} at {uc.reg_read(UC_X86_REG_EIP):#010x}")
    if bad:
        raise RuntimeError("; ".join(bad))
    return {
        "eax": uc.reg_read(UC_X86_REG_EAX), "edx": uc.reg_read(UC_X86_REG_EDX),
        "esp_delta": uc.reg_read(UC_X86_REG_ESP) - esp - 4,
        "saved": [uc.reg_read(r) for r in (UC_X86_REG_EBX, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP)],
        "scratch": bytes(uc.mem_read(SCRATCH, max(len(scratch), 0x100))),
        "insns": count[0],
    }


def disasm(address, length=0x200, stop_at_ret=True):
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    out = []
    for i in md.disasm(code(address, length), address):
        out.append(f"{i.address:08X}  {i.mnemonic} {i.op_str}")
        if stop_at_ret and i.mnemonic.startswith("ret"):
            break
    return "\n".join(out)


# --- sparse runs: any function, automatic inputs --------------------------
from unicorn import UC_HOOK_MEM_UNMAPPED, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM0, UC_X86_REG_FPSW, UC_X86_REG_EFLAGS  # noqa: E402
import unicorn.x86_const as _xc  # noqa: E402
XMM_REGS = {f"xmm{k}": getattr(_xc, f"UC_X86_REG_XMM{k}") for k in range(8)}

GPR = [("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX), ("ebx", UC_X86_REG_EBX),
       ("esp", UC_X86_REG_ESP), ("ebp", UC_X86_REG_EBP), ("esi", UC_X86_REG_ESI), ("edi", UC_X86_REG_EDI)]
FLOAT_LOADS = {"movss", "comiss", "ucomiss", "addss", "subss", "mulss", "divss", "minss", "maxss", "sqrtss",
               "fld", "fadd", "fsub", "fsubr", "fmul", "fdiv", "fdivr", "fcom", "fcomp",
               "cvtss2sd", "cvttss2si", "cvtss2si"}


# Platform services lifted code may call: run_sparse runs these models
# (identical to crates/ddnative/src/platform.rs) instead of the original.
PLATFORM = {0x00CF73F0: "pool40_alloc", 0x00DE0B80: "mt_alloc", 0x00DE0B90: "mt_free",
            0x012EFE20: "memset", 0x012EFEA0: "memcpy"}
HEAP_BASE = 0x0C000000  # mem::HEAP_BASE: Sparse's bump allocator
MAX_PAGES = 256  # run_sparse rejects runs that touch more distinct pages
MODEL_BYTES = 1 << 20  # run_sparse rejects runs whose memset/memcpy calls touch more in total
TCG_BUFFER = 32 << 20  # Unicorn's translated-code buffer per instance (our functions are small)
X87_TRAMP, X87_SLOT = 0x0DEB0000, 0x0DEB1000  # mocked float returns (run_sparse x87_push)


SSE_BINARY = {"addss", "subss", "mulss", "divss", "minss", "maxss", "addps", "subps", "mulps", "divps",
              "minps", "maxps", "addpd", "subpd", "mulpd", "divpd", "minpd", "maxpd",
              "addsd", "subsd", "mulsd", "divsd", "minsd", "maxsd"}


class Rejected(Exception):
    """The case is not a valid vector (NaN float input, runaway loop, ...)."""


INPUT_STRATEGIES = ('small-v1', 'objects-v1', 'zero-objects-v1')


class SparseInputs:
    """Synthetic fixture distributions, not inferred game object layouts.

    Page contents are fixed before the first guest access. Never rewrite a
    value in response to a branch/load: overlapping reads and guest writes
    must describe one consistent initial memory image.
    """
    OBJECT_BASE = 0x20000000
    OBJECT_PAGES = 32

    def __init__(self, name):
        if name not in INPUT_STRATEGIES:
            raise ValueError(f'unknown input strategy: {name}')
        self.name = name

    def pointer(self, rnd):
        return self.OBJECT_BASE + rnd.randrange(self.OBJECT_PAGES)*0x1000 + 0x100

    def word(self, rnd):
        if self.name == 'zero-objects-v1':
            return 0
        if self.name == 'objects-v1' and rnd.randrange(8) == 0:
            return self.pointer(rnd)
        return rnd.choice((0,0,0,1,1,2,3,4,7,15,31))

    def page(self, seed, page):
        import random
        if self.name == 'zero-objects-v1':
            return bytes(0x1000)
        rnd = random.Random((seed << 32) ^ page)
        return b''.join(struct.pack('<I',self.word(rnd)) for _ in range(0x400))

    def register(self, rnd, name):
        if self.name == 'zero-objects-v1' or (self.name == 'objects-v1' and name == 'ecx'):
            return self.pointer(rnd)
        # They may be interpreted as pointers or counts: alignment avoids
        # combining two adjacent small words into a huge unaligned load.
        return rnd.randrange(8)*4

    def returns(self, rnd):
        zero = self.name == 'zero-objects-v1'
        return dict(eax=self.pointer(rnd) if self.name != 'small-v1' else rnd.randrange(8)*4,
                    ecx=0 if zero else rnd.randrange(8)*4, edx=0 if zero else rnd.randrange(8)*4,
                    xmm0=[self.float_bits(rnd) for _ in range(4)])

    def float_bits(self, rnd):
        value = 0.0 if self.name == 'zero-objects-v1' else rnd.choice((-1.0,0.0,1.0))
        return struct.unpack('<I',struct.pack('<f',value))[0]


def static_pop(target, _cache={}):
    """Bytes a function pops with its `ret N` (first ret in a linear sweep;
    0 when none is found or the target is outside the image)."""
    if target not in _cache:
        from capstone import Cs, CS_ARCH_X86, CS_MODE_32
        pop = 0
        try:
            for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(code(target, 0x1000), target):
                if i.mnemonic in ("ret", "retn"):
                    pop = int(i.op_str, 0) if i.op_str else 0
                    break
        except KeyError:
            pass
        _cache[target] = pop
    return _cache[target]


def run_sparse(address, seed, max_insn=20000, end=None, lifted=frozenset(), zero_inputs=False, size=None,
               x87_push=frozenset(), body=None, noreturn_sites=frozenset(), noreturn_targets=frozenset(),
               max_seconds=None, input_strategy=None):
    """Run `address` with every general register random (ESP points into a
    random stack page holding the return address), every untouched page
    mapped on first access and filled with seeded random bytes. Returns
    {"regs_in", "pages_in", "regs_out", "pages_out", "xmm0_out", "st0_out"};
    pages are {page_address: hex}. Raises Rejected for unusable cases.

    With `end` (legacy extent, also enables external-call recording), calls and jumps from the
    function's own code to anything outside it that is neither in `lifted`
    nor a PLATFORM model are mocked: the callee does not run; the event is
    recorded in "calls" (target, all eight registers at entry) with what the
    mock returns (random eax/ecx/edx, random finite xmm0, the callee's
    stack pop). ddnative's tests replay these through x86::call_ext.

    `body` gives exact instruction starts, including disjoint/shared blocks.
    It replaces the size/end range for ownership and own-return checks.
    Nested calls into shared body code are tracked by return PC and ESP,
    so their returns are not mistaken for the root function's return.
    Known noreturn paths reject instead of manufacturing a returning mock.
    Without a body, legacy `size` (implied by `end`) lets the run check that
    the function's own `ret` goes back to the caller: random inputs can
    overwrite the return address (a store through a random pointer), and the
    original then runs on into whatever is there. Such a case is rejected.

    `x87_push` holds return addresses of mocked call sites whose callee is
    taken to return a float in st0 (lift.returns_float): there the mock
    pushes a random finite float onto the x87 stack (through a trampoline,
    `fld dword [X87_SLOT]; jmp [X87_SLOT + 4]`) and records it as
    "st_push"; the replay pushes the same bits.

    `zero_inputs` is an opt-in boundary fixture: non-image memory, initial
    general registers except ESP, and mocked return values are zero. Image
    bytes, stack/return setup, instruction execution and recording are unchanged.
    This exercises switch index zero and empty counts that full-width random
    inputs almost never select; it is a test input, not a game-state model.

    `input_strategy` selects an opt-in, versioned SparseInputs distribution
    for non-image pages, registers and mocked returns. It is mutually
    exclusive with zero_inputs and recorded in successful vectors. Original
    image bytes, executed instructions and rejection checks never change.
    `max_seconds` bounds a trial at instruction boundaries (no resume).
    """
    import random
    import time
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    rnd = random.Random(seed)
    if input_strategy is not None and zero_inputs:
        raise ValueError('input_strategy and zero_inputs are mutually exclusive')
    inputs = SparseInputs(input_strategy) if input_strategy is not None else None
    deadline = time.monotonic()+max_seconds if max_seconds is not None else None
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(TCG_BUFFER)  # default 1 GB per instance: workers reached 4 GB
    # Unicorn's reset tag word reads 0x5555 (eight zero values), not an
    # empty x87 stack. Match the fixture's actual empty-stack contract.
    uc.reg_write(_xc.UC_X86_REG_FPTAG, 0xffff)
    uc.reg_write(_xc.UC_X86_REG_FPCW, 0x037f)
    uc.reg_write(_xc.UC_X86_REG_MXCSR, 0x1f80)
    base, secs = image()
    pages_in = {}

    def fill(page):
        """Image pages hold the exe; any other page seeded random bytes."""
        for va, b in secs:
            if va & ~0xFFF <= page < va + len(b):
                lo = max(page, va)
                hi = min(page + 0x1000, va + len(b))
                buf = bytearray(0x1000)
                buf[lo - page:hi - page] = b[lo - va:hi - va]
                return bytes(buf)
        if zero_inputs:
            return bytes(0x1000)
        if inputs:
            return inputs.page(seed, page)
        prnd = random.Random((seed << 32) ^ page)
        return prnd.randbytes(0x1000)

    def on_unmapped(uc_, access, addr, size, value, _):
        if len(pages_in) > MAX_PAGES:
            # a loop over a random count of random pointers: thousands of
            # random pages, minutes per run, and no useful vector
            if not bad:
                bad.append(f"touches over {MAX_PAGES} pages")
            uc_.emu_stop()
            return False
        page = addr & ~0xFFF
        for p in {page, (addr + size - 1) & ~0xFFF}:
            try:
                uc_.mem_map(p, 0x1000)
            except UcError:
                pass
            if p not in pages_in:
                content = fill(p)
                uc_.mem_write(p, content)
                pages_in[p] = content
        return True

    reads, written = {}, set()

    def note(uc_, access, addr, size, value, _):
        for p in {addr & ~0xFFF, (addr + size - 1) & ~0xFFF}:
            if p not in pages_in:
                pages_in[p] = bytes(uc_.mem_read(p, 0x1000))
        if access == 16:  # UC_MEM_READ: record bytes read before any write
            cur = bytes(uc_.mem_read(addr, size))
            for k in range(size):
                if addr + k not in written and addr + k not in reads:
                    reads[addr + k] = cur[k]
        else:
            written.update(range(addr, addr + size))

    decoded = {}
    instruction_sizes = {}

    def ins_len(pc):
        # the size Unicorn's code hook reports, clamped: it is sometimes a
        # huge sentinel (one read of ~4 GB took a worker to 3.9 GB)
        n = instruction_sizes.get(pc, 0)
        return n if 0 < n <= 15 else min(15, 0x1000 - (pc & 0xFFF))  # stay on the mapped page

    def check_float(uc_, access, addr, size, value, _):
        eip = uc_.reg_read(UC_X86_REG_EIP)
        if eip not in decoded:
            ins = list(md.disasm(bytes(uc_.mem_read(eip, ins_len(eip))), eip, 1))
            if not ins:  # data access from undecodable code: report, don't crash
                bad.append(f"undecodable code at {eip:#010x} reading {addr:#010x}")
                uc_.emu_stop()
                return
            decoded[eip] = ins[0].mnemonic
        if decoded[eip] == "fldcw" and size == 2:
            cw = int.from_bytes(uc_.mem_read(addr, 2), "little")
            if cw & 0x3f != 0x3f:
                bad.append("unmasked x87 exceptions unsupported")
                uc_.emu_stop()
                return
        if decoded[eip] in FLOAT_LOADS and size in (4, 8, 10):
            raw = bytes(uc_.mem_read(addr, size))
            bits = int.from_bytes(raw, "little")
            nan = (((bits >> 23) & 0xFF) == 0xFF if size == 4 else
                   ((bits >> 52) & 0x7FF) == 0x7FF if size == 8 else
                   ((bits >> 64) & 0x7FFF) == 0x7FFF)
            if nan:
                bad.append("NaN/Inf float input")
                uc_.emu_stop()

    count, bad = [0], []

    # SSE ops with two NaN operands: the CPU returns the first operand's NaN,
    # Unicorn (QEMU) applies the x87 rule (larger payload). Such a case
    # cannot be judged by the emulator: reject it.
    nmd = Cs(CS_ARCH_X86, CS_MODE_32)
    nmd.detail = True
    sse_dec = {}

    def two_nans(uc_, addr):
        if addr not in sse_dec:
            try:
                i = next(nmd.disasm(bytes(uc_.mem_read(addr, ins_len(addr))), addr))
                sse_dec[addr] = i if i.mnemonic in SSE_BINARY else None
            except (UcError, StopIteration):
                sse_dec[addr] = None
        i = sse_dec[addr]
        if i is None:
            return False
        width = 64 if i.mnemonic.endswith(("pd", "sd")) else 32
        lanes = 128 // width if i.mnemonic.endswith(("ps", "pd")) else 1
        a = uc_.reg_read(XMM_REGS[i.reg_name(i.operands[0].reg)])
        o = i.operands[1]
        if o.type == 1:  # register
            b = uc_.reg_read(XMM_REGS[i.reg_name(o.reg)])
        else:
            mm = o.mem
            ea = mm.disp + (uc_.reg_read(dict(GPR)[i.reg_name(mm.base)]) if mm.base else 0) +                 (uc_.reg_read(dict(GPR)[i.reg_name(mm.index)]) * mm.scale if mm.index else 0)
            try:
                b = int.from_bytes(bytes(uc_.mem_read(ea & 0xFFFFFFFF, width // 8 * lanes)), "little")
            except UcError:
                return False
        exponent, fraction = (0x7ff0000000000000, 0xfffffffffffff) if width == 64 else (0x7f800000, 0x7fffff)
        nan = lambda v: v & exponent == exponent and v & fraction != 0
        return any(nan(a >> width*k) and nan(b >> width*k) for k in range(lanes))

    # Map every page an instruction will touch *before* it runs. Unicorn's
    # UNMAPPED hook is not reliable (it can map the page and still raise),
    # and resuming after such a fault loses QEMU's lazily computed flags.
    mem_ops = {}
    integer_ops = {}
    x87_ops = {}
    rets = {}
    gpr = dict(GPR)

    def premap(uc_, addr):
        if addr not in mem_ops:
            try:
                i = next(nmd.disasm(bytes(uc_.mem_read(addr, ins_len(addr))), addr))
                integer_ops[addr] = i
                x87_ops[addr] = i if i.mnemonic.startswith("f") else None
                mem_ops[addr] = [(i.reg_name(o.mem.base) if o.mem.base else None,
                                  i.reg_name(o.mem.index) if o.mem.index else None,
                                  o.mem.scale, o.mem.disp, max(o.size, 1))
                                 for o in i.operands if o.type == 3 and (not o.mem.segment or i.reg_name(o.mem.segment) in ("es", "ds"))]                     if not (i.mnemonic in ("lea", "nop") or i.mnemonic.startswith("prefetch")) else []
            except (UcError, StopIteration):
                mem_ops[addr] = []
                x87_ops[addr] = None
        esp = uc_.reg_read(UC_X86_REG_ESP)
        need = {(esp - 0x40) & ~0xFFF, esp & ~0xFFF}  # implicit stack accesses
        for base, index, scale, disp, sz in mem_ops[addr]:
            a = disp
            if base:
                a += uc_.reg_read(gpr[base]) if base in gpr else 0
            if index:
                a += (uc_.reg_read(gpr[index]) if index in gpr else 0) * scale
            a &= 0xFFFFFFFF
            need.add(a & ~0xFFF)
            need.add(((a + sz - 1) & 0xFFFFFFFF) & ~0xFFF)
        i = integer_ops.get(addr)
        if i is not None and i.mnemonic in ("bt", "bts", "btr", "btc") and i.operands[0].type == 3 and i.operands[1].type == 1:
            operand, index = i.operands
            bits = operand.size * 8
            offset = uc_.reg_read(getattr(_xc, "UC_X86_REG_" + i.reg_name(index.reg).upper()))
            offset &= (1 << bits) - 1
            if offset & (1 << (bits - 1)):
                offset -= 1 << bits
            a = (effective_address(uc_, i, operand) + (offset // bits) * operand.size) & 0xffffffff
            need.update((a & ~0xfff, ((a + operand.size - 1) & 0xffffffff) & ~0xfff))
        for p in need:
            if p not in pages_in and p != RET & ~0xFFF:
                on_unmapped(uc_, None, p, 1, 0, None)

    body_end = end if end is not None else (address + size if size else None)
    body = frozenset(body) if body is not None else None
    is_own = lambda pc: pc in body if body is not None else body_end is not None and address <= pc < body_end
    frames = []  # (return PC, caller ESP), distinguishing calls into shared code
    root_frame = [True]
    flow = {}
    own_ret = [False]  # the previous instruction was one of the function's own `ret`s
    valid_flags = [0x8d5]

    def check_integer_flags(uc_, i):
        # Undefined flags differ between real CPUs and QEMU. Reject observed
        # reads instead of assigning them invented semantics in a fixture.
        import capstone.x86_const as xc
        if i.mnemonic in ("fcomi", "fcompi", "fucomi", "fucompi"):
            valid_flags[0] = 0x8d5
            return None
        if i.mnemonic.startswith("f"):
            return None  # Capstone's detail union contains x87 status flags here
        mapping = (("CF",1),("PF",4),("AF",0x10),("ZF",0x40),("SF",0x80),("OF",0x800))
        reads = sum(bit for name,bit in mapping if i.eflags & getattr(xc,"X86_EFLAGS_TEST_"+name))
        if i.mnemonic in ("adc","sbb","inc","dec"):
            reads |= 1
        if i.mnemonic == "lahf":
            reads |= 0xd5  # Capstone describes these as PRIOR, not TEST
        if reads & ~valid_flags[0]:
            return "read of undefined integer flags"
        mn = i.mnemonic
        defined = sum(bit for name,bit in mapping if i.eflags & (
            getattr(xc,"X86_EFLAGS_MODIFY_"+name)|getattr(xc,"X86_EFLAGS_SET_"+name)|getattr(xc,"X86_EFLAGS_RESET_"+name)))
        if mn in ("div","idiv"):
            valid_flags[0] = 0
        elif mn in ("mul", "imul"):
            valid_flags[0] = 0x801
        elif mn in ("bsf","bsr"):
            op=i.operands[1]
            v=uc_.reg_read(getattr(_xc,"UC_X86_REG_"+i.reg_name(op.reg).upper())) if op.type==1 else int.from_bytes(uc_.mem_read(effective_address(uc_,i,op),op.size),"little")
            if not v:
                return "undefined bit-scan destination"
            valid_flags[0] = 0x40
        elif mn in ("bt","bts","btr","btc"):
            valid_flags[0] = (valid_flags[0]&0x40)|1
        elif mn in ("rol","ror","shld","shrd"):
            op=i.operands[-1]
            n=(op.imm if op.type==2 else uc_.reg_read(_xc.UC_X86_REG_CL))&31
            if n:
                if mn in ("shld","shrd"):
                    if n>i.operands[0].size*8:
                        return "undefined double-shift count"
                    valid_flags[0]=0xc5|(0x800 if n==1 else 0)
                else:
                    valid_flags[0]=(valid_flags[0]&~0x801)|1|(0x800 if n==1 else 0)
        elif mn in ("and","or","xor","test"):
            valid_flags[0]=0x8d5  # existing logical helper's AF convention
        else:
            valid_flags[0] |= defined
        return None

    def effective_address(uc_,i,op):
        v=op.mem.disp
        for reg,scale in ((op.mem.base,1),(op.mem.index,op.mem.scale)):
            if reg:
                v+=uc_.reg_read(getattr(_xc,"UC_X86_REG_"+i.reg_name(reg).upper()))*scale
        return v&0xffffffff

    def on_frame(uc_, addr, size, _):
        # Must precede on_ext: the first root instruction after a closed
        # callee returns can itself be another call that needs recording.
        esp_now = uc_.reg_read(UC_X86_REG_ESP)
        while frames and addr == frames[-1][0] and esp_now >= frames[-1][1]:
            frames.pop()
        root_frame[0] = not frames
        if addr in noreturn_targets:
            bad.append(f"noreturn target at {addr:#010x}")
            uc_.emu_stop()

    def on_code(uc_, addr, size, _):
        # Unicorn has already decoded this instruction. Reading an arbitrary
        # 15-byte window can cross an unmapped next page even when the whole
        # instruction is mapped, breaking both premap and float checks.
        instruction_sizes[addr] = size
        esp_now = uc_.reg_read(UC_X86_REG_ESP)
        if own_ret[0] and addr != RET:
            bad.append(f"returned to {addr:#010x} instead of the caller (return address overwritten)")
            uc_.emu_stop()
            return
        own_ret[0] = False
        if root_frame[0] and is_own(addr):
            if addr not in rets:
                try:
                    rets[addr] = next(nmd.disasm(code(addr, 15), addr)).mnemonic in ("ret", "retn")
                except (KeyError, StopIteration):
                    rets[addr] = False
            own_ret[0] = rets[addr]
            if addr in noreturn_sites:
                bad.append(f"noreturn call at {addr:#010x}")
                uc_.emu_stop()
                return
        # The ext recorder already rejects stray returns in on_ext. Closed
        # runs must not accidentally execute seeded heap/stack bytes either.
        # The only non-image code we intentionally execute is the st0 capture.
        if (end is None and addr != 0x0DEAF000
                and not any(va <= addr < va + len(data) for va, data in secs)):
            bad.append(f"execution outside the image at {addr:#010x}")
            uc_.emu_stop()
            return
        premap(uc_, addr)
        if addr in integer_ops and addr not in PLATFORM and uc_.reg_read(UC_X86_REG_EIP) == addr:
            reason = check_integer_flags(uc_, integer_ops[addr])
            if reason:
                bad.append(f"{reason} at {addr:#010x}")
                uc_.emu_stop()
                return
        if body is not None and addr not in PLATFORM and addr != RET:
            if addr not in flow:
                try:
                    flow[addr] = next(nmd.disasm(bytes(uc_.mem_read(addr, ins_len(addr))), addr)).mnemonic
                except (UcError, StopIteration):
                    flow[addr] = ""
            if flow[addr] == "call" and uc_.reg_read(UC_X86_REG_EIP) == addr:
                frames.append((addr + size, esp_now))
        i = x87_ops.get(addr)
        if i is not None and uc_.reg_read(UC_X86_REG_EIP) == addr:
            # QEMU does not reliably set SF on overflow/underflow. Reject
            # these inputs before execution instead of recording overwritten
            # stack registers as successful arithmetic.
            top = (uc_.reg_read(UC_X86_REG_FPSW) >> 11) & 7
            tags = uc_.reg_read(_xc.UC_X86_REG_FPTAG)
            empty = lambda k: (tags >> (2 * ((top + k) & 7))) & 3 == 3
            pushes = i.mnemonic in {"fld", "fild", "fld1", "fldz", "fldpi", "fldl2e", "fldl2t", "fldlg2", "fldln2"}
            slots = [int(i.reg_name(o.reg)[3:-1]) for o in i.operands
                     if o.type == 1 and i.reg_name(o.reg).startswith("st(")]
            control = i.mnemonic in {"fnstcw", "fldcw", "fnstsw", "fstsw", "fnclex", "fninit"}
            if not pushes and not control:
                slots.append(0)
            if i.mnemonic in {"fcompp", "fucompp"}:
                slots.append(1)
            if any(empty(k) for k in slots) or (pushes and not empty(-1)):
                bad.append(f"x87 stack fault before {i.mnemonic} at {addr:#010x}")
                uc_.emu_stop()
                return
        if two_nans(uc_, addr):
            bad.append(f"two NaN operands at {addr:#010x}")
            uc_.emu_stop()
            return
        count[0] += 1
        if deadline is not None and count[0] % 256 == 1 and time.monotonic() >= deadline:
            bad.append("function time budget")
            uc_.emu_stop()
        if count[0] > max_insn:
            bad.append("runaway")
            uc_.emu_stop()

    heap = [HEAP_BASE]
    model_bytes = [0]  # bytes memset/memcpy touched in this run

    def ensure(a, n):
        for p in range(a & ~0xFFF, ((a + n - 1) & ~0xFFF) + 1, 0x1000):
            if p not in pages_in and p != RET & ~0xFFF:
                on_unmapped(uc, None, p, 1, 0, None)

    def mread(a, n):
        ensure(a, n)
        cur = bytes(uc.mem_read(a, n))
        for k in range(n):
            if a + k not in written and a + k not in reads:
                reads[a + k] = cur[k]
        return cur

    def mwrite(a, data):
        ensure(a, len(data))
        uc.mem_write(a, data)
        written.update(range(a, a + len(data)))

    def alloc(size):
        a = heap[0]
        heap[0] = ((a + max(size, 1) + 15) & ~15) & 0xFFFFFFFF
        return a

    def on_platform(uc_, addr, size, _):
        """platform.rs, line for line: arguments, effects, then `ret`."""
        name = PLATFORM[addr]
        esp = uc_.reg_read(UC_X86_REG_ESP)
        eax = uc_.reg_read(UC_X86_REG_EAX)
        arg = lambda k: struct.unpack("<I", mread(esp + 4 + 4 * k, 4))[0]  # noqa: E731
        if name == "pool40_alloc":
            eax = alloc(0x28)
        elif name == "mt_alloc":
            eax = alloc(uc_.reg_read(UC_X86_REG_ECX))
        elif name in ("memset", "memcpy"):
            dst, x, n = arg(0), arg(1), arg(2)
            model_bytes[0] += n
            if n > 0x10000 or model_bytes[0] > MODEL_BYTES:
                # one huge copy, or a loop of them: every byte becomes a
                # Python dict/set entry (a worker once reached 3.7 GB)
                bad.append(f"{name} of {n:#x} bytes ({model_bytes[0]:#x} in this run)")
                uc_.emu_stop()
                return
            mwrite(dst, bytes([x & 0xFF]) * n if name == "memset" else mread(x, n))
            eax = dst
        ret = struct.unpack("<I", mread(esp, 4))[0]
        uc_.reg_write(UC_X86_REG_EAX, eax)
        uc_.reg_write(UC_X86_REG_ESP, esp + 4)
        uc_.reg_write(UC_X86_REG_EIP, ret)

    for a in PLATFORM:
        uc.hook_add(UC_HOOK_CODE, on_platform, None, a, a)

    calls = []
    branch = [None]          # the last instruction of the function itself, if it can leave it
    pushed = [0, False, 0]   # bytes pushed since the last call (after the register-saving
                             # prologue), prologue over, the count at the last call
    dmd = Cs(CS_ARCH_X86, CS_MODE_32)
    dmd.detail = True
    own = {}

    def on_ext(uc_, pc, size, _):
        # a call to the function's own entry (recursion) leaves it like any
        # other call: the lifted code makes it through call_ext, so mock it
        recursion = pc == address and branch[0] is not None and branch[0].mnemonic == "call"
        if root_frame[0] and is_own(pc) and not recursion:
            if pc not in own:
                own[pc] = next(dmd.disasm(code(pc, 15), pc))  # the function's own code: from the image
            i = own[pc]
            branch[0] = i if i.mnemonic == "call" or i.mnemonic.startswith("j") else None
            if i.mnemonic == "call":  # its arguments: what was pushed since the previous call
                pushed[2], pushed[0] = pushed[0], 0
            prologue = (i.mnemonic == "push" and i.operands[0].type == 1) or                 (i.mnemonic == "mov" and i.op_str == "ebp, esp") or                 (i.mnemonic in ("sub", "and") and i.op_str.startswith("esp, "))
            if not pushed[1] and not prologue:
                pushed[1] = True
            if pushed[1]:  # argument space: pushes and `sub esp, N` (float args stored with movss)
                if i.mnemonic == "push":
                    pushed[0] += 4
                elif i.mnemonic == "sub" and i.op_str.startswith("esp, ") and i.operands[1].type == 2:
                    pushed[0] += i.operands[1].imm
                elif i.mnemonic == "add" and i.op_str.startswith("esp, ") and i.operands[1].type == 2:
                    pushed[0] = max(0, pushed[0] - i.operands[1].imm)
            return
        i, branch[0] = branch[0], None
        if i is None and not (0x00400000 <= pc < 0x02000000) and pc != 0x0DEAF000                 and not X87_TRAMP <= pc < X87_TRAMP + 12:
            # running code outside the exe: a mocked callee's stack pop was
            # wrong and a `ret` went astray. Not a valid case.
            bad.append(f"execution outside the image at {pc:#010x}")
            uc_.emu_stop()
            return
        if i is None or pc in lifted or pc in PLATFORM:
            return
        esp = uc_.reg_read(UC_X86_REG_ESP)
        ev = {"target": pc, "regs": {n: uc_.reg_read(r) for n, r in GPR}}
        direct = i.operands[0].type == 2  # X86_OP_IMM
        pop = static_pop(pc) if direct or i.mnemonic != "call" else pushed[2]
        ret = struct.unpack("<I", mread(esp, 4))[0]
        out = {"eax": rnd.getrandbits(32), "ecx": rnd.getrandbits(32), "edx": rnd.getrandbits(32),
               "xmm0": [struct.unpack("<I", struct.pack("<f", rnd.uniform(-1000, 1000)))[0] for _ in range(4)],
               "pop": pop}
        if zero_inputs:
            out.update(eax=0, ecx=0, edx=0, xmm0=[0] * 4)
        if inputs:
            out.update(inputs.returns(rnd))
        for n, r in (("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX)):
            uc_.reg_write(r, out[n])
        uc_.reg_write(UC_X86_REG_XMM0, sum(v << (32 * k) for k, v in enumerate(out["xmm0"])))
        uc_.reg_write(UC_X86_REG_ESP, (esp + 4 + pop) & 0xFFFFFFFF)
        if ret in x87_push:  # the callee returns a float: push it on the way back
            v = struct.unpack("<I", struct.pack("<f", rnd.uniform(-1000, 1000)))[0]
            if inputs:
                v = inputs.float_bits(rnd)
            out["st_push"] = v
            uc_.mem_write(X87_SLOT, struct.pack("<II", v, ret))
            ret = X87_TRAMP
        uc_.reg_write(UC_X86_REG_EIP, ret)
        calls.append({**ev, "out": out})

    if body is not None:
        uc.hook_add(UC_HOOK_CODE, on_frame)
    if end is not None:
        uc.hook_add(UC_HOOK_CODE, on_ext)
    if x87_push:
        # fixed code on its own page (Unicorn caches translated code per
        # page): fld dword [X87_SLOT]; jmp dword [X87_SLOT + 4]
        uc.mem_map(X87_TRAMP, 0x1000)
        uc.mem_map(X87_SLOT, 0x1000)
        uc.mem_write(X87_TRAMP, bytes([0xD9, 0x05]) + struct.pack("<I", X87_SLOT)
                     + bytes([0xFF, 0x25]) + struct.pack("<I", X87_SLOT + 4))
        pages_in[X87_TRAMP] = bytes(0x1000)  # premap: already mapped
        pages_in[X87_SLOT] = bytes(0x1000)
    uc.hook_add(UC_HOOK_MEM_UNMAPPED, on_unmapped)
    uc.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, note)
    uc.hook_add(UC_HOOK_MEM_READ, check_float)
    uc.hook_add(UC_HOOK_CODE, on_code)
    uc.mem_map(RET & ~0xFFF, 0x1000)
    regs_in = {}
    for name, r in GPR:
        v = rnd.getrandbits(32)
        if zero_inputs:
            v = 0
        if inputs:
            v = inputs.register(rnd, name)
        if name == "esp":
            v = (0x0F000000 + rnd.randrange(0x100) * 0x1000 + 0x800) & ~3
        regs_in[name] = v
        uc.reg_write(r, v)
    # the return address at [esp]; the rest of the stack page is random
    on_unmapped(uc, None, regs_in["esp"], 4, 0, None)
    stack_page = regs_in["esp"] & ~0xFFF
    page = bytearray(pages_in[stack_page])
    struct.pack_into("<I", page, regs_in["esp"] - stack_page, RET)
    pages_in[stack_page] = bytes(page)
    uc.mem_write(stack_page, bytes(page))
    try:
        uc.emu_start(address, RET)
    except UcError as e:
        # pages are mapped before each instruction (premap), so a fault here
        # is real; resuming would lose QEMU's lazy flags: never resume
        bad.append(f"{e} at {uc.reg_read(UC_X86_REG_EIP):#010x}")
    if not bad and not (4 <= ((uc.reg_read(UC_X86_REG_ESP) - regs_in["esp"]) & 0xFFFFFFFF) <= 0x404):
        bad.append("returned with a misaligned stack")
    if not bad and uc.reg_read(UC_X86_REG_FPSW) & 0x40:
        bad.append("x87 stack fault")  # the Rust model has no empty/full-stack values
    if bad:
        raise Rejected("; ".join(bad))
    regs_out = {name: uc.reg_read(r) for name, r in GPR}
    xmm0 = uc.reg_read(UC_X86_REG_XMM0) & 0xFFFFFFFF
    # x87 values left on the stack: depth from TOP; read st0 as a single
    # with an `fstp dword [cap]` run from the return page
    top = (uc.reg_read(UC_X86_REG_FPSW) >> 11) & 7
    tags = uc.reg_read(_xc.UC_X86_REG_FPTAG)
    depth = sum((tags >> (2 * k)) & 3 != 3 for k in range(8))
    st0_ext = list(uc.reg_read(_xc.UC_X86_REG_FP0 + top)) if depth else None
    x87_control = uc.reg_read(_xc.UC_X86_REG_FPCW)
    st0 = None
    if depth:
        cap, tramp = 0x0DEAE000, 0x0DEAF000
        uc.mem_map(cap, 0x1000)
        uc.mem_map(tramp, 0x1000)  # a fresh page: Unicorn caches code it has already translated
        uc.mem_write(tramp, bytes([0xD9, 0x1D]) + struct.pack("<I", cap) + bytes([0x90]))
        own_ret[0] = False  # the capture runs after the function's own ret
        uc.emu_start(tramp, tramp + 6)
        if bad:
            raise Rejected("st0 capture: " + "; ".join(bad))
        st0 = struct.unpack("<I", bytes(uc.mem_read(cap, 4)))[0]
        written.difference_update(range(cap, cap + 4))
    # the return address read by `ret` is part of the inputs
    def runs(d):
        out, cur = [], None
        for a in sorted(d):
            if cur and a == cur[0] + len(cur[1]):
                cur[1].append(d[a])
            else:
                cur = [a, [d[a]]]
                out.append(cur)
        return [[a, bytes(b).hex()] for a, b in out]
    final = {a: uc.mem_read(a, 1)[0] for a in written}
    return {"regs_in": regs_in, "reads": runs(reads), "regs_out": regs_out, "writes": runs(final),
            "xmm0_out": xmm0, "st0_out": st0, "x87_depth": depth,
            "st0_ext": st0_ext, "x87_control": x87_control,
            **({"calls": calls} if end is not None else {}), **({"zero_inputs": True} if zero_inputs else {}),
            **({"input_strategy": input_strategy} if inputs else {})}
