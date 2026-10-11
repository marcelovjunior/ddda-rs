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

    def decode(self, uc, addr, size):
        if addr not in self.decoded:
            try:
                i = next(self.md.disasm(bytes(uc.mem_read(addr, max(1, min(size, 15)))), addr))
            except StopIteration:
                i = None
            self.decoded[addr] = i if i is not None and (encoding(i) is not None
                                                         or i.mnemonic in APPROXIMATIONS) else None
        return self.decoded[addr]

    def on_compute(self, uc, addr, size, _):
        from unicorn.x86_const import UC_X86_REG_EIP
        if self.problem or uc.reg_read(UC_X86_REG_EIP) != addr:
            return
        i = self.decode(uc, addr, size)
        if i is None:
            return
        if i.mnemonic in APPROXIMATIONS:
            self.approximation(uc, i)
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

    def approximation(self, uc, i):
        """RCPPS/RSQRTPS/RCPSS/RSQRTSS: the host's result for the same
        operands (Unicorn's differs: probe_approx.py)."""
        from unicorn import x86_const as xc
        reg = lambda o: getattr(xc, "UC_X86_REG_" + i.reg_name(o.reg).upper())
        dst, src = i.operands
        dest = uc.reg_read(reg(dst)).to_bytes(16, "little")
        if src.type == X86_OP_MEM:
            source = bytes(uc.mem_read(self.address_of(uc, i, src), src.size)).ljust(16, b"\0")
        else:
            source = uc.reg_read(reg(src)).to_bytes(16, "little")
        code = bytes.fromhex("0f1001" "0f104910" + APPROXIMATIONS[i.mnemonic] + "0f1101" "c3")
        out = native(code, dest + source)[:16]
        self.used = self.cpu_dependent = True
        self.pending = ("xmm", reg(dst), int.from_bytes(out, "little"), i.address)

    def flush(self, uc):
        if self.pending is None:
            return
        if self.pending[0] == "xmm":
            _, register, value, _ = self.pending
            self.pending = None
            uc.reg_write(register, value)
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
