"""Host x87 execution for the recorder and probes (L22).

The lifted Rust runs each original x87 opcode on the host CPU over a saved
FSAVE image (crates/ddnative-base/src/x87.rs, X87::exec). This module does
the same from Python: one small x86-64 stub per (opcode, ModRM) pair,

    fnsave [r8]; frstor [rcx]; mov rax, rdx; <opcode> <modrm>;
    pushfq; pop rdx; fnsave [rcx]; frstor [r8]; mov rax, rdx; ret

with rcx = the guest's 108-byte FSAVE image (32-bit protected-mode format,
Intel SDM vol. 1 8.1.10), rdx = a 16-byte operand buffer (memory forms use
[rax]) and r8 = a save area for the host's own x87 state. The host state
is restored, SSE/MXCSR untouched. Windows x64 only (VirtualAlloc).

`unicorn_state`/`set_unicorn_state` convert between Unicorn's x87
registers and that image, so the recorder can make every x87 instruction
follow the hardware (harness.run_sparse, `x87_oracle`).
"""
import ctypes
import struct

from capstone.x86 import X86_OP_MEM

_MEM = ctypes.c_char * 108
_STUBS = {}
_PAGE = [None, 0]  # current executable page, bytes used
_HOST = (ctypes.c_char * 108)()
_DATA = (ctypes.c_char * 16)()


def _kernel():
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.VirtualAlloc.restype = ctypes.c_void_p
    k.VirtualAlloc.argtypes = (ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint32, ctypes.c_uint32)
    return k


def _stub(opcode, modrm):
    key = (opcode, modrm)
    if key not in _STUBS:
        code = bytes([0x41, 0xDD, 0x30,        # fnsave [r8]
                      0xDD, 0x21,              # frstor [rcx]
                      0x48, 0x89, 0xD0,        # mov rax, rdx
                      opcode, modrm,
                      0x9C, 0x5A,              # pushfq; pop rdx
                      0xDD, 0x31,              # fnsave [rcx]
                      0x41, 0xDD, 0x20,        # frstor [r8]
                      0x48, 0x89, 0xD0,        # mov rax, rdx
                      0xC3])
        if _PAGE[0] is None or _PAGE[1] + 32 > 0x10000:
            _PAGE[0] = _kernel().VirtualAlloc(None, 0x10000, 0x3000, 0x40)  # MEM_COMMIT|RESERVE, RWX
            _PAGE[1] = 0
            if not _PAGE[0]:
                raise OSError("VirtualAlloc failed")
        at = _PAGE[0] + _PAGE[1]
        ctypes.memmove(at, code, len(code))
        _PAGE[1] += 32
        _STUBS[key] = ctypes.CFUNCTYPE(ctypes.c_uint64, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)(at)
    return _STUBS[key]


def execute(state, opcode, modrm, data=b""):
    """Run one x87 opcode on the host. `state` is a 108-byte FSAVE image,
    `modrm` the raw ModRM for register forms (>= 0xC0) or the reg field
    (memory forms address [rax] = `data`). Returns (state, data, rflags).
    """
    if not 0xD8 <= opcode <= 0xDF:
        raise ValueError(f"not an x87 opcode {opcode:#x}")
    if modrm < 0xC0 and modrm & 0xC7:
        raise ValueError("memory forms address [rax]: pass only the reg field")
    guest = _MEM.from_buffer_copy(bytes(state))
    ctypes.memset(_DATA, 0, 16)
    ctypes.memmove(_DATA, bytes(data), len(data))
    flags = _stub(opcode, modrm)(ctypes.addressof(guest), ctypes.addressof(_DATA), ctypes.addressof(_HOST))
    return bytes(guest), bytes(_DATA)[:10], flags


def native(code, buffer):
    """Execute `code` (x86-64, must end with ret and preserve nonvolatile
    registers) with rcx = a writable copy of `buffer`; return that buffer.
    For probes of whole instruction sequences without FNSAVE/FRSTOR."""
    if _PAGE[0] is None or _PAGE[1] + len(code) + 16 > 0x10000:
        _PAGE[0] = _kernel().VirtualAlloc(None, 0x10000, 0x3000, 0x40)
        _PAGE[1] = 0
    at = _PAGE[0] + _PAGE[1]
    ctypes.memmove(at, bytes(code), len(code))
    _PAGE[1] += (len(code) + 15) & ~15
    buf = (ctypes.c_char * len(buffer)).from_buffer_copy(bytes(buffer))
    ctypes.CFUNCTYPE(None, ctypes.c_void_p)(at)(ctypes.addressof(buf))
    return bytes(buf)


# ---- encodings (the same set ddnative-base's X87::exec accepts)

def supported(opcode, modrm):
    """Mirror of X87::exec's encoding check (crates/ddnative-base/src/x87.rs)."""
    if modrm < 0xC0:
        return {0xD8: modrm <= 0x38 and modrm % 8 == 0, 0xDA: modrm <= 0x38 and modrm % 8 == 0,
                0xDC: modrm <= 0x38 and modrm % 8 == 0, 0xDE: modrm <= 0x38 and modrm % 8 == 0,
                0xD9: modrm in (0, 0x10, 0x18, 0x28, 0x38), 0xDB: modrm in (0, 0x10, 0x18, 0x28, 0x38),
                0xDD: modrm in (0, 0x10, 0x18, 0x38), 0xDF: modrm in (0, 0x10, 0x18, 0x28, 0x38)}.get(opcode, False)
    return {0xD8: True, 0xDC: 0xC0 <= modrm <= 0xCF or modrm >= 0xE0,
            0xD9: 0xC0 <= modrm <= 0xCF or modrm in (0xE0, 0xE1, 0xE5, 0xE8, 0xEA, 0xEB, 0xED, 0xEE, 0xF0, 0xF1,
                                                     0xF2, 0xF3, 0xF5, 0xF8, 0xFA, 0xFC, 0xFD, 0xFE, 0xFF),
            0xDA: modrm == 0xE9, 0xDB: modrm == 0xE2 or 0xE8 <= modrm <= 0xF7, 0xDF: 0xE8 <= modrm <= 0xF7,
            0xDD: 0xD0 <= modrm <= 0xEF, 0xDE: 0xC0 <= modrm <= 0xCF or modrm == 0xD9 or modrm >= 0xE0}.get(opcode, False)


def encoding(i):
    """(opcode, modrm, memory operand or None) of a Capstone x87 instruction,
    as the lifter emits it (segment prefixes are applied to the address)."""
    raw = bytes(i.bytes)
    while raw and raw[0] in (0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65):
        raw = raw[1:]
    if raw[:1] == b"\x9b":  # fstsw/fstcw: the wait prefix has no effect with masked exceptions
        raw = raw[1:]
    if len(raw) < 2 or not 0xD8 <= raw[0] <= 0xDF:
        return None
    mem = next((o for o in i.operands if o.type == X86_OP_MEM), None)
    if mem is not None:
        return raw[0], raw[1] & 0x38, mem
    return raw[0], raw[1], None


# ---- Unicorn <-> FSAVE image

def _tag(mantissa, exponent):
    """Full x87 tag of a non-empty register (SDM vol. 1 8.1.7)."""
    e = exponent & 0x7FFF
    if e == 0x7FFF or (e and not mantissa >> 63) or (e == 0 and mantissa):
        return 2  # special: NaN/infinity, unnormal, denormal
    return 1 if e == 0 and mantissa == 0 else 0


def unicorn_state(uc):
    from unicorn import x86_const as xc
    cw = uc.reg_read(xc.UC_X86_REG_FPCW)
    sw = uc.reg_read(xc.UC_X86_REG_FPSW)
    tags = uc.reg_read(xc.UC_X86_REG_FPTAG)
    top = (sw >> 11) & 7
    regs = [uc.reg_read(xc.UC_X86_REG_FP0 + k) for k in range(8)]  # physical registers
    full = 0
    for k in range(8):
        tag = 3 if (tags >> 2 * k) & 3 == 3 else _tag(*regs[k])
        full |= tag << 2 * k
    state = bytearray(108)
    struct.pack_into("<HxxHxxHxx", state, 0, cw, sw, full)
    for k in range(8):
        mantissa, exponent = regs[(top + k) & 7]
        struct.pack_into("<QH", state, 28 + 10 * k, mantissa, exponent)
    return bytes(state)


def set_unicorn_state(uc, state):
    from unicorn import x86_const as xc
    cw, sw, tags = struct.unpack_from("<HxxHxxHxx", state, 0)
    top = (sw >> 11) & 7
    uc.reg_write(xc.UC_X86_REG_FPCW, cw)
    uc.reg_write(xc.UC_X86_REG_FPSW, sw)
    uc.reg_write(xc.UC_X86_REG_FPTAG, tags)
    for k in range(8):
        uc.reg_write(xc.UC_X86_REG_FP0 + ((top + k) & 7), struct.unpack_from("<QH", state, 28 + 10 * k))


def initial_state(control=0x037F):
    state = bytearray(108)
    struct.pack_into("<HxxHxxHxx", state, 0, control, 0, 0xFFFF)
    return bytes(state)


# ---- recorder oracle

# Results that differ between processor models (Intel SDM: approximations
# with relative error bounds; transcendental accuracy). Cases that ran one
# record the host CPU (`cpu_dependent`); the replay compares them only there.
CPU_DEPENDENT = {"fsin", "fcos", "fsincos", "fptan", "fpatan", "fyl2x", "fyl2xp1", "f2xm1"}
APPROXIMATIONS = {"rcpps": "0f53c1", "rsqrtps": "0f52c1", "rcpss": "f30f53c1", "rsqrtss": "f30f52c1"}

# SSE instructions that can set MXCSR exception flags (Intel SDM vol. 1,
# 11.5): floating arithmetic, conversions and comparisons. Moves, logic,
# shuffles and integer operations do not.
_SSE_ARITH = {"add", "sub", "mul", "div", "min", "max", "sqrt"}
SSE_COMPARE_FLAGS = {"comiss", "ucomiss", "comisd", "ucomisd"}


def sse_flagging(i):
    """Whether Capstone instruction `i` is one of those SSE instructions.
    CMPSD names both the SSE2 compare and the string compare: only the
    form with an xmm operand counts."""
    mn = i.mnemonic
    xmm = any(o.type == 1 and i.reg_name(o.reg).startswith("xmm") for o in i.operands)
    if mn in SSE_COMPARE_FLAGS or (mn.startswith("cvt") and "pi" not in mn):
        return True
    if mn[-2:] in ("ss", "ps", "sd", "pd") and mn[:-2] in _SSE_ARITH:
        return True
    return mn.startswith("cmp") and mn[-2:] in ("ss", "ps", "sd", "pd") and xmm


_LEGACY_PREFIXES = {0x66, 0xF2, 0xF3, 0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65, 0x67, 0xF0}
_SSE_STUBS = {}


def _sse_stub(i):
    """x86-64 code running instruction `i` re-encoded: mandatory prefixes
    kept, a memory operand replaced by [r11] (REX.B, mod 00, rm 011), the
    register operands unchanged (xmm0-7, eax-edi need no REX)."""
    raw = bytes(i.bytes)
    k = 0
    while raw[k] in _LEGACY_PREFIXES:
        k += 1
    keep = bytes(b for b in raw[:k] if b in (0x66, 0xF2, 0xF3))
    m = i.modrm_offset
    if not m or m <= k:
        raise ValueError(f"no ModRM in {i.mnemonic} {i.op_str}")
    opcode, modrm = raw[k:m], raw[m]
    imm = raw[i.imm_offset:i.imm_offset + i.imm_size] if i.imm_size else b""
    if modrm >> 6 == 3:
        insn = keep + opcode + bytes([modrm]) + imm
    else:
        insn = keep + bytes([0x41]) + opcode + bytes([(modrm & 0x38) | 0x03]) + imm
    if insn not in _SSE_STUBS:
        reg = lambda op, k: bytes([0x41, op, 0x82 | (k << 3)]) + (0x80 + 4 * k).to_bytes(4, "little")
        code = bytearray(bytes.fromhex("53565755" "4989ca" "4d8d9ac0000000"
                                       "410fae9a00010000" "410fae9204010000"))
        for x in range(8):
            code += bytes([0x41, 0x0F, 0x10, 0x42 | (x << 3), 16 * x])
        for g in (0, 1, 2, 3, 5, 6, 7):
            code += reg(0x8B, g)
        code += insn
        code += bytes.fromhex("9c" "418f82a0000000")
        for g in (0, 1, 2, 3, 5, 6, 7):
            code += reg(0x89, g)
        for x in range(8):
            code += bytes([0x41, 0x0F, 0x11, 0x42 | (x << 3), 16 * x])
        code += bytes.fromhex("410fae9a04010000" "410fae9200010000" "5d5f5e5b" "c3")
        _SSE_STUBS[insn] = bytes(code)
    return _SSE_STUBS[insn]


def sse_native(i, xmm, gpr, data, mxcsr):
    """Run SSE instruction `i` on the host with xmm0-7, eax..edi (esp is
    never an operand here), the memory operand bytes and the guest MXCSR.
    Returns (xmm list, gpr list, rflags, mxcsr)."""
    buf = bytearray(0x110)
    for k, v in enumerate(xmm):
        buf[16 * k:16 * k + 16] = v.to_bytes(16, "little")
    for k, v in enumerate(gpr):
        if k != 4:
            buf[0x80 + 4 * k:0x84 + 4 * k] = (v & 0xFFFFFFFF).to_bytes(4, "little")
    buf[0xC0:0xC0 + len(data)] = data
    buf[0x104:0x108] = mxcsr.to_bytes(4, "little")
    out = native(_sse_stub(i), buf)
    return ([int.from_bytes(out[16 * k:16 * k + 16], "little") for k in range(8)],
            [gpr[k] if k == 4 else int.from_bytes(out[0x80 + 4 * k:0x84 + 4 * k], "little") for k in range(8)],
            int.from_bytes(out[0xA0:0xA8], "little"), int.from_bytes(out[0x104:0x108], "little"))


def _regs():
    from unicorn import x86_const as xc
    return ([getattr(xc, f"UC_X86_REG_XMM{k}") for k in range(8)],
            [getattr(xc, "UC_X86_REG_" + n) for n in ("EAX", "ECX", "EDX", "EBX", "ESP", "EBP", "ESI", "EDI")])


_XMMS, _GPRS = _regs()

STORES = {"fst", "fstp", "fist", "fistp", "fisttp", "fnstcw", "fnstsw", "fstsw", "fstcw", "fbstp"}
COMPARE_FLAGS = {"fcomi", "fcompi", "fucomi", "fucompi", "fcomip", "fucomip"}
VERSION = "host-v1"


class Oracle:
    """Make every x87 instruction in a Unicorn run follow the host CPU.

    `on_compute` (registered after the recorder's own code hooks, so pages
    are premapped and rejections already made) computes the host result of
    the instruction about to run, from Unicorn's current x87 state and the
    operand bytes. Unicorn then executes the instruction itself (memory
    hooks still record its reads and writes); `on_flush` (registered before
    every other code hook) replaces the x87 state and any stored operand
    with the host's at the start of the next instruction, and `flush` does
    the same after emu_start returns. EFLAGS from FCOMI-family compares are
    not rewritten inside a translation block (QEMU computes flags lazily);
    a difference rejects the case instead (`problem`).
    """

    def __init__(self, address_of):
        from capstone import Cs, CS_ARCH_X86, CS_MODE_32
        self.address_of = address_of
        self.md = Cs(CS_ARCH_X86, CS_MODE_32)
        self.md.detail = True
        self.decoded = {}
        self.pending = None
        self.problem = None
        self.used = False
        self.cpu_dependent = False
        self.mxcsr_touched = False  # an SSE oracle step or LDMXCSR ran: record the final MXCSR

    def decode(self, uc, addr, size):
        if addr not in self.decoded:
            try:
                i = next(self.md.disasm(bytes(uc.mem_read(addr, max(1, min(size, 15)))), addr))
            except StopIteration:
                i = None
            self.decoded[addr] = i if i is not None and (encoding(i) is not None or i.mnemonic in APPROXIMATIONS
                                                         or sse_flagging(i) or i.mnemonic == "ldmxcsr") else None
        return self.decoded[addr]

    def on_compute(self, uc, addr, size, _):
        from unicorn.x86_const import UC_X86_REG_EIP
        if self.problem or uc.reg_read(UC_X86_REG_EIP) != addr:
            return
        i = self.decode(uc, addr, size)
        if i is None:
            return
        if i.mnemonic in APPROXIMATIONS or sse_flagging(i):
            self.sse(uc, i)
            return
        if i.mnemonic == "ldmxcsr":
            self.mxcsr_touched = True
            value = int.from_bytes(uc.mem_read(self.address_of(uc, i, i.operands[0]), 4), "little")
            if value & 0x1F80 != 0x1F80 or value >> 16:
                self.problem = f"ldmxcsr {value:#x} at {addr:#010x}: unmasked SSE exceptions or reserved bits"
                uc.emu_stop()
            return
        self.cpu_dependent |= i.mnemonic in CPU_DEPENDENT
        enc = encoding(i)
        if enc is None:
            return
        opcode, modrm, op = enc
        if not supported(opcode, modrm):
            if (opcode, modrm) != (0xDF, 0xE0):  # fnstsw ax reads the corrected status
                self.problem = f"x87 instruction outside the host oracle at {addr:#010x}: {i.mnemonic}"
                uc.emu_stop()
            return
        data, target = b"", None
        if op is not None:
            a = self.address_of(uc, i, op)
            if i.mnemonic in STORES:
                target = (a, op.size)
            else:
                data = bytes(uc.mem_read(a, op.size))
        state, out, flags = execute(unicorn_state(uc), opcode, modrm, data)
        self.used = True
        self.pending = (state, target and (target[0], out[:target[1]]),
                        flags & 0x45 if i.mnemonic in COMPARE_FLAGS else None, addr)

    def sse(self, uc, i):
        """A flag-setting SSE instruction (or an approximation): run the
        original instruction natively under the guest MXCSR (sse_native)
        and take the host's destination, GPR result and MXCSR. Unicorn 2.1
        sets no MXCSR exception flags (probe_x87.py, MXCSR case)."""
        from unicorn import x86_const as xc
        mxcsr = uc.reg_read(xc.UC_X86_REG_MXCSR)
        if mxcsr & 0x1F80 != 0x1F80 or mxcsr >> 16:
            self.problem = f"MXCSR {mxcsr:#x} at {i.address:#010x}: unmasked SSE exceptions or reserved bits"
            uc.emu_stop()
            return
        xmm = [uc.reg_read(getattr(xc, f"UC_X86_REG_XMM{k}")) for k in range(8)]
        gpr = [uc.reg_read(r) for r in _GPRS]
        mem = next((o for o in i.operands if o.type == X86_OP_MEM), None)
        data = bytes(uc.mem_read(self.address_of(uc, i, mem), mem.size)) if mem is not None else b""
        out_xmm, out_gpr, flags, out_mxcsr = sse_native(i, xmm, gpr, data, mxcsr)
        self.used = self.mxcsr_touched = True
        self.cpu_dependent |= i.mnemonic in APPROXIMATIONS
        # Every register the host ran with, unconditionally: Unicorn's own
        # result may differ even where the host's equals the old value (a
        # two-NaN ADDSS keeps the first operand; Unicorn picks the larger).
        self.pending = ("sse", list(zip(_XMMS, out_xmm))
                        + [(r, v) for k, (r, v) in enumerate(zip(_GPRS, out_gpr)) if k != 4],
                        out_mxcsr, flags & 0x45 if i.mnemonic in SSE_COMPARE_FLAGS else None, i.address)

    def flush(self, uc):
        if self.pending is None:
            return
        if self.pending[0] == "sse":
            from unicorn.x86_const import UC_X86_REG_EFLAGS, UC_X86_REG_MXCSR
            _, writes, mxcsr, flags, addr = self.pending
            self.pending = None
            for register, value in writes:
                uc.reg_write(register, value)
            uc.reg_write(UC_X86_REG_MXCSR, mxcsr)
            if flags is not None and uc.reg_read(UC_X86_REG_EFLAGS) & 0x45 != flags and not self.problem:
                self.problem = f"SSE compare flags differ from the host at {addr:#010x}"
                uc.emu_stop()
            return
        from unicorn.x86_const import UC_X86_REG_EFLAGS
        state, store, flags, addr = self.pending
        self.pending = None
        set_unicorn_state(uc, state)
        if store is not None:
            uc.mem_write(store[0], store[1])
        if flags is not None and uc.reg_read(UC_X86_REG_EFLAGS) & 0x45 != flags and not self.problem:
            self.problem = f"x87 compare flags differ from the host at {addr:#010x}"
            uc.emu_stop()

    def on_flush(self, uc, addr, size, _):
        self.flush(uc)
