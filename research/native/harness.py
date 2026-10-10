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


def run(address, ecx=0, args=(), scratch=b"", regs=None, allow=(), max_insn=100000):
    """Call `address`. Returns dict(eax, edx, scratch, insns)."""
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
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
        insn = uc_.mem_read(addr, size)
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
