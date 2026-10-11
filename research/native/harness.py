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


def run(address, ecx=0, args=(), scratch=b"", regs=None, allow=(), max_insn=100000, poke=None):
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


# --- sparse runs: any function, automatic inputs --------------------------
from unicorn import UC_HOOK_MEM_UNMAPPED, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_XMM0, UC_X86_REG_FPSW  # noqa: E402

GPR = [("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX), ("ebx", UC_X86_REG_EBX),
       ("esp", UC_X86_REG_ESP), ("ebp", UC_X86_REG_EBP), ("esi", UC_X86_REG_ESI), ("edi", UC_X86_REG_EDI)]
FLOAT_LOADS = {"movss", "comiss", "ucomiss", "addss", "subss", "mulss", "divss", "minss", "maxss", "sqrtss",
               "fld", "cvtss2sd", "cvttss2si", "cvtss2si"}


# Platform services lifted code may call: run_sparse runs these models
# (identical to crates/ddnative/src/platform.rs) instead of the original.
PLATFORM = {0x00CF73F0: "pool40_alloc", 0x00DE0B80: "mt_alloc", 0x00DE0B90: "mt_free",
            0x012EFE20: "memset", 0x012EFEA0: "memcpy"}
HEAP_BASE = 0x0C000000  # mem::HEAP_BASE: Sparse's bump allocator


class Rejected(Exception):
    """The case is not a valid vector (NaN float input, runaway loop, ...)."""


def run_sparse(address, seed, max_insn=20000):
    """Run `address` with every general register random (ESP points into a
    random stack page holding the return address), every untouched page
    mapped on first access and filled with seeded random bytes. Returns
    {"regs_in", "pages_in", "regs_out", "pages_out", "xmm0_out", "st0_out"};
    pages are {page_address: hex}. Raises Rejected for unusable cases."""
    import random
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    rnd = random.Random(seed)
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
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
        prnd = random.Random((seed << 32) ^ page)
        return prnd.randbytes(0x1000)

    def on_unmapped(uc_, access, addr, size, value, _):
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

    def check_float(uc_, access, addr, size, value, _):
        eip = uc_.reg_read(UC_X86_REG_EIP)
        if eip not in decoded:
            decoded[eip] = next(md.disasm(bytes(uc_.mem_read(eip, 15)), eip)).mnemonic
        if decoded[eip] in FLOAT_LOADS and size in (4, 8):
            raw = bytes(uc_.mem_read(addr, size))
            bits = int.from_bytes(raw, "little")
            nan = ((bits >> 23) & 0xFF) == 0xFF if size == 4 else ((bits >> 52) & 0x7FF) == 0x7FF
            if nan:
                bad.append("NaN/Inf float input")
                uc_.emu_stop()

    count, bad = [0], []

    def on_code(uc_, addr, size, _):
        count[0] += 1
        if count[0] > max_insn:
            bad.append("runaway")
            uc_.emu_stop()

    heap = [HEAP_BASE]

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
            if n > 0x10000:
                bad.append(f"{name} of {n:#x} bytes")
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
    uc.hook_add(UC_HOOK_MEM_UNMAPPED, on_unmapped)
    uc.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, note)
    uc.hook_add(UC_HOOK_MEM_READ, check_float)
    uc.hook_add(UC_HOOK_CODE, on_code)
    uc.mem_map(RET & ~0xFFF, 0x1000)
    regs_in = {}
    for name, r in GPR:
        v = rnd.getrandbits(32)
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
    # Unicorn's SSE memory helpers can raise UNMAPPED for a page the hook has
    # just mapped (or never call the hook): map what the faulting instruction
    # touches and resume from it, once per instruction (the machine state is
    # intact; the instruction has not retired).
    md.detail = True
    regs = dict(GPR)
    pc, retried = address, set()
    while True:
        try:
            uc.emu_start(pc, RET)
            break
        except UcError as e:
            pc = uc.reg_read(UC_X86_REG_EIP)
            if "UNMAPPED" not in str(e) or bad or pc in retried:
                bad.append(f"{e} at {pc:#010x}")
                break
            retried.add(pc)
            i = next(md.disasm(bytes(uc.mem_read(pc, 15)), pc))
            for op in i.operands:
                if op.type == 3:  # X86_OP_MEM
                    mm = op.mem
                    a = mm.disp
                    if mm.base:
                        a += uc.reg_read(regs[i.reg_name(mm.base)])
                    if mm.index:
                        a += uc.reg_read(regs[i.reg_name(mm.index)]) * mm.scale
                    a &= 0xFFFFFFFF
                    for p in {a & ~0xFFF, (a + op.size - 1) & ~0xFFF}:
                        if p not in pages_in:
                            on_unmapped(uc, None, p, 1, 0, None)
    if bad:
        raise Rejected("; ".join(bad))
    regs_out = {name: uc.reg_read(r) for name, r in GPR}
    xmm0 = uc.reg_read(UC_X86_REG_XMM0) & 0xFFFFFFFF
    # x87 values left on the stack: depth from TOP; read st0 as a single
    # with an `fstp dword [cap]` run from the return page
    top = (uc.reg_read(UC_X86_REG_FPSW) >> 11) & 7
    depth = (8 - top) % 8
    st0 = None
    if depth:
        cap, tramp = 0x0DEAE000, 0x0DEAF000
        uc.mem_map(cap, 0x1000)
        uc.mem_map(tramp, 0x1000)  # a fresh page: Unicorn caches code it has already translated
        uc.mem_write(tramp, bytes([0xD9, 0x1D]) + struct.pack("<I", cap) + bytes([0x90]))
        uc.emu_start(tramp, tramp + 6)
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
            "xmm0_out": xmm0, "st0_out": st0, "x87_depth": depth}
