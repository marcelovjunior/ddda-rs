"""L22 recorder events for instructions that report the machine.

harness.run_sparse installs `Machine` next to the x87 oracle:

- CPUID is answered with the host CPU's own result (a native stub) and
  recorded as an event; the replay's platform::Machine returns it.
- RDTSC runs in Unicorn; EDX:EAX is read at the next instruction (or the
  end of the run) and recorded as an event.
- A read of a segment register makes the case record Unicorn's six
  selectors; the replay loads them into Cpu::seg.
- POPFD: the game runs at CPL3, Unicorn at CPL0. A value that would set TF
  or change IF, IOPL, VIF or VIP behaves differently, so it rejects.

The initial EFLAGS image is 0x202 (IF set, as in a Windows thread) so a
PUSHFD stores what the lifted pushfd_image computes.
"""
import ctypes

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import UC_HOOK_INSN
from unicorn import x86_const as xc

import x87_host

SEGMENTS = ("es", "cs", "ss", "ds", "fs", "gs")
_SELECTOR = {n: getattr(xc, "UC_X86_REG_" + n.upper()) for n in SEGMENTS}
_CPUID = [None]


def host_cpuid(leaf, subleaf):
    """CPUID on the host (x86-64 stub; rbx is saved)."""
    if _CPUID[0] is None:
        # rcx = buffer {leaf, subleaf, out[4]}
        code = bytes.fromhex("53"            # push rbx
                             "4989C8"        # mov r8, rcx
                             "418B00"        # mov eax, [r8]
                             "418B4804"      # mov ecx, [r8+4]
                             "0FA2"          # cpuid
                             "41894008"      # mov [r8+8], eax
                             "4189580C"      # mov [r8+12], ebx
                             "41894810"      # mov [r8+16], ecx
                             "41895014"      # mov [r8+20], edx
                             "5B"            # pop rbx
                             "C3")
        _CPUID[0] = code
    buf = (leaf & 0xFFFFFFFF).to_bytes(4, "little") + (subleaf & 0xFFFFFFFF).to_bytes(4, "little") + bytes(16)
    out = x87_host.native(_CPUID[0], buf)
    return [int.from_bytes(out[8 + 4 * k:12 + 4 * k], "little") for k in range(4)]


def host_brand():
    """The host processor's CPUID brand string (leaves 0x80000002..4)."""
    words = [w for leaf in (0x80000002, 0x80000003, 0x80000004) for w in host_cpuid(leaf, 0)]
    return b"".join(w.to_bytes(4, "little") for w in words).rstrip(b"\0").decode("ascii", "replace")


class Machine:
    def __init__(self, uc):
        self.md = Cs(CS_ARCH_X86, CS_MODE_32)
        self.md.detail = True
        self.decoded = {}
        self.events = []
        self.rdtsc_pending = False
        self.segments_read = False
        self.problem = None
        uc.reg_write(xc.UC_X86_REG_EFLAGS, 0x202)
        uc.hook_add(UC_HOOK_INSN, self.on_cpuid, None, 1, 0, xc.UC_X86_INS_CPUID)

    def on_cpuid(self, uc, _):
        leaf, subleaf = uc.reg_read(xc.UC_X86_REG_EAX), uc.reg_read(xc.UC_X86_REG_ECX)
        out = host_cpuid(leaf, subleaf)
        for reg, value in zip((xc.UC_X86_REG_EAX, xc.UC_X86_REG_EBX, xc.UC_X86_REG_ECX, xc.UC_X86_REG_EDX), out):
            uc.reg_write(reg, value)
        self.events.append(["cpuid", leaf, subleaf, *out])
        return True  # skip Unicorn's own CPUID

    def flush(self, uc):
        if self.rdtsc_pending:
            self.rdtsc_pending = False
            self.events.append(["rdtsc", uc.reg_read(xc.UC_X86_REG_EAX), uc.reg_read(xc.UC_X86_REG_EDX)])

    def on_code(self, uc, addr, size, _):
        """Registered before the recorder's other code hooks."""
        self.flush(uc)
        if addr not in self.decoded:
            try:
                i = next(self.md.disasm(bytes(uc.mem_read(addr, max(1, min(size, 15)))), addr))
            except StopIteration:
                i = None
            kind = None
            if i is not None:
                if i.mnemonic in ("rdtsc", "popfd"):
                    kind = i.mnemonic
                elif any(o.type == 1 and i.reg_name(o.reg) in SEGMENTS for o in i.operands):
                    kind = "segment"
            self.decoded[addr] = kind
        kind = self.decoded[addr]
        if kind is None or uc.reg_read(xc.UC_X86_REG_EIP) != addr:
            return
        if kind == "rdtsc":
            self.rdtsc_pending = True
        elif kind == "segment":
            self.segments_read = True
        elif kind == "popfd" and not self.problem:
            value = int.from_bytes(uc.mem_read(uc.reg_read(xc.UC_X86_REG_ESP), 4), "little")
            current = uc.reg_read(xc.UC_X86_REG_EFLAGS)
            if value & 0x100 or (value ^ current) & (0x200 | 0x3000 | 0x80000 | 0x100000):
                self.problem = f"popfd at {addr:#010x} sets TF or changes IF/IOPL/VIF/VIP (CPL3 differs)"
                uc.emu_stop()

    def result(self, uc):
        out = {}
        if self.events:
            out["machine"] = self.events
        if self.segments_read:
            out["segments"] = {n: uc.reg_read(r) for n, r in _SELECTOR.items()}
        return out
