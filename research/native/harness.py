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
CONST_MODEL_BYTES = 4 << 20  # per-run budget for memset/memcpy sizes pushed as immediates
TCG_BUFFER = 32 << 20  # Unicorn's translated-code buffer per instance (our functions are small)
X87_TRAMP, X87_SLOT = 0x0DEB0000, 0x0DEB1000  # mocked float returns (run_sparse x87_push)
TEB_BASE, GDT_BASE = 0x7FFDE000, 0x7FFDC000


def configure_fs(uc):
    """32-bit Unicorn ignores FS_BASE; load an ordinary writable GDT segment.

    The TEB remains demand mapped and recorded. The separate GDT page is
    emulator configuration, not guest fixture data. Limit is 4 GiB.
    """
    base, limit = TEB_BASE, 0xfffff
    descriptor = ((limit & 0xffff) | ((base & 0xffff) << 16)
                  | (((base >> 16) & 0xff) << 32) | (0x93 << 40)
                  | ((limit >> 16) << 48) | (0xc << 52) | ((base >> 24) << 56))
    uc.mem_map(GDT_BASE, 0x1000)
    # Loading a segment makes Unicorn consult SS's cached B bit for PUSH.
    # Its reset SS cache is 16-bit: explicitly keep the flat 32-bit stack.
    uc.mem_write(GDT_BASE + 8, struct.pack('<QQ', 0x00CF93000000FFFF, descriptor))
    uc.reg_write(_xc.UC_X86_REG_GDTR, (0, GDT_BASE, 0xfff, 0))
    uc.reg_write(_xc.UC_X86_REG_SS, 8)
    uc.reg_write(_xc.UC_X86_REG_FS, 16)


SSE_BINARY = {"addss", "subss", "mulss", "divss", "minss", "maxss", "addps", "subps", "mulps", "divps",
              "minps", "maxps", "addpd", "subpd", "mulpd", "divpd", "minpd", "maxpd",
              "addsd", "subsd", "mulsd", "divsd", "minsd", "maxsd"}


class Rejected(Exception):
    """The case is not a valid vector (NaN float input, runaway loop, ...)."""


PAGE_STRATEGIES = ('small-v1', 'objects-v1', 'zero-objects-v1')
# Zero-return strategies: every mocked callee returns 0. Pages and registers
# come from the named page strategy (None: the default random fill).
MOCKZERO = {'mockzero-v1': None, 'small-mockzero-v1': 'small-v1', 'objects-mockzero-v1': 'objects-v1'}
RETURN_STRATEGIES = tuple(MOCKZERO)
INPUT_STRATEGIES = (*PAGE_STRATEGIES, 'image-v1', 'zero-call-v1', *RETURN_STRATEGIES)


# Callee cleanup (bytes) of imported functions, from their documented x86
# signatures on Microsoft Learn (and the Steamworks headers): WINAPI, STDAPI,
# WSAAPI and SEC_ENTRY are __stdcall, so the callee pops 4 bytes per
# parameter and 8 per 64-bit one; the steam_api C exports are __cdecl (0).
# Only the mock's stack cleanup comes from here; effects and results stay
# mocked and recorded. Names are DLL!export (lowercase DLL) or DLL!ordinal:N.
IMPORT_CLEANUP = {
    # KERNEL32
    'kernel32.dll!EncodePointer': 4, 'kernel32.dll!DecodePointer': 4,          # (PVOID)
    'kernel32.dll!EnterCriticalSection': 4, 'kernel32.dll!LeaveCriticalSection': 4,  # (LPCRITICAL_SECTION)
    'kernel32.dll!InterlockedIncrement': 4, 'kernel32.dll!InterlockedDecrement': 4,  # (LONG volatile *)
    'kernel32.dll!Sleep': 4,                                                     # (DWORD)
    'kernel32.dll!GetLastError': 0,                                              # (void)
    'kernel32.dll!LoadLibraryW': 4,                                              # (LPCWSTR)
    'kernel32.dll!GetProcAddress': 8,                                            # (HMODULE, LPCSTR)
    'kernel32.dll!QueryPerformanceCounter': 4, 'kernel32.dll!QueryPerformanceFrequency': 4,  # (LARGE_INTEGER *)
    'kernel32.dll!MultiByteToWideChar': 24,                                      # (UINT, DWORD, LPCCH, int, LPWSTR, int)
    'kernel32.dll!VerSetConditionMask': 16,                                      # (ULONGLONG, DWORD, BYTE)
    'kernel32.dll!VerifyVersionInfoA': 16,                                       # (LPOSVERSIONINFOEXA, DWORD, DWORDLONG)
    'kernel32.dll!RtlUnwind': 16,                                                # (PVOID, PVOID, PEXCEPTION_RECORD, PVOID)
    # USER32
    'user32.dll!ScreenToClient': 8,                                              # (HWND, LPPOINT)
    'user32.dll!GetCursorPos': 4, 'user32.dll!ClipCursor': 4,                    # (LPPOINT) / (const RECT *)
    'user32.dll!ShowCursor': 4,                                                  # (BOOL)
    'user32.dll!SystemParametersInfoA': 16,                                      # (UINT, UINT, PVOID, UINT)
    # IMM32
    'imm32.dll!ImmGetContext': 4, 'imm32.dll!ImmGetDefaultIMEWnd': 4,            # (HWND)
    'imm32.dll!ImmReleaseContext': 8,                                            # (HWND, HIMC)
    'imm32.dll!ImmGetOpenStatus': 4,                                             # (HIMC)
    'imm32.dll!ImmSetCompositionWindow': 8, 'imm32.dll!ImmSetCandidateWindow': 8,  # (HIMC, LP...FORM)
    'imm32.dll!ImmGetCompositionStringW': 16,                                    # (HIMC, DWORD, LPVOID, DWORD)
    'imm32.dll!ImmGetCandidateListW': 16,                                        # (HIMC, DWORD, LPCANDIDATELIST, DWORD)
    'imm32.dll!ImmGetCandidateListCountA': 8,                                    # (HIMC, LPDWORD)
    # Secur32 (SEC_ENTRY)
    'secur32.dll!FreeContextBuffer': 4, 'secur32.dll!DeleteSecurityContext': 4,  # (PVOID) / (PCtxtHandle)
    'secur32.dll!InitializeSecurityContextA': 48,                                # 12 parameters
    # AVIFIL32 / MSVFW32 (STDAPI / VFWAPI)
    'avifil32.dll!AVIFileInit': 0,                                               # (void)
    'avifil32.dll!AVIFileOpenA': 16,                                             # (PAVIFILE *, LPCSTR, UINT, LPCLSID)
    'avifil32.dll!AVIFileCreateStreamA': 12,                                     # (PAVIFILE, PAVISTREAM *, AVISTREAMINFOA *)
    'avifil32.dll!AVIMakeCompressedStream': 16,                                  # (PAVISTREAM *, PAVISTREAM, AVICOMPRESSOPTIONS *, CLSID *)
    'avifil32.dll!AVIStreamSetFormat': 16,                                       # (PAVISTREAM, LONG, LPVOID, LONG)
    'msvfw32.dll!ICCompressorChoose': 24,                                        # (HWND, UINT, LPVOID, LPVOID, PCOMPVARS, LPSTR)
    # DINPUT8
    'dinput8.dll!DirectInput8Create': 20,                                        # (HINSTANCE, DWORD, REFIID, LPVOID *, LPUNKNOWN)
    # WS2_32 by ordinal (WSAAPI): 10 ioctlsocket, 18 select, 151 __WSAFDIsSet
    'ws2_32.dll!ordinal:10': 12, 'ws2_32.dll!ordinal:18': 20, 'ws2_32.dll!ordinal:151': 8,
    # steam_api (S_API ... __cdecl): the caller cleans up
    'steam_api.dll!SteamAPI_RegisterCallback': 0, 'steam_api.dll!SteamAPI_UnregisterCallback': 0,
}


def import_pops(_cache={}):
    """Documented x86 cleanup per IAT slot (IMPORT_CLEANUP); resolve slots
    and their initial targets from the PE. No import behavior is emulated."""
    if EXE not in _cache:
        import sys
        from pathlib import Path
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'runtime'))
        from measure import PE
        pe = PE(EXE)
        slots = {}
        for a, n in pe.imports().items():
            dll, _, name = n.partition('!')
            key = dll.lower() + '!' + name
            if key in IMPORT_CLEANUP:
                slots[a] = IMPORT_CLEANUP[key]
        _cache[EXE] = slots, {pe.word(a): v for a, v in slots.items()}
    return _cache[EXE]


class SparseInputs:
    """Synthetic fixture distributions, not inferred game object layouts.

    Page contents are fixed before the first guest access. Never rewrite a
    value in response to a branch/load: overlapping reads and guest writes
    must describe one consistent initial memory image.
    """
    OBJECT_BASE = 0x20000000
    OBJECT_PAGES = 32

    def __init__(self, name):
        if name not in PAGE_STRATEGIES:
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


STATIC_POP_LIMIT = 20000  # lift.decode_body's instruction limit


def static_pop(target, _cache={}, limit=None):
    """Bounded reachable ret cleanup, following direct tails, never neighbours.

    Proven bounded switches are followed; unknown indirect or inconsistent
    exits retain the zero-pop fallback.
    The cache includes the reader so synthetic images cannot contaminate it.
    """
    limit = STATIC_POP_LIMIT if limit is None else limit
    key = (target, code, image, limit)
    if key not in _cache:
        from capstone import Cs, CS_ARCH_X86, CS_MODE_32
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.detail = True
        todo, seen, pops, unknown = [target], set(), set(), False
        owners = {}
        while todo and not unknown:
            pc = todo.pop()
            history = []
            while pc not in seen:
                if len(seen) >= limit:
                    unknown = True
                    break
                seen.add(pc)
                try:
                    i = next(md.disasm(code(pc, 15), pc, count=1))
                except (KeyError, StopIteration):
                    unknown = True
                    break
                if any(a in owners and owners[a] != pc for a in range(pc, pc+i.size)):
                    unknown = True
                    break
                owners.update((a,pc) for a in range(pc,pc+i.size))
                history.append(i)
                history = history[-4:]
                if i.mnemonic in ('ret', 'retn', 'repz ret'):
                    pops.add(i.operands[0].imm if i.operands else 0)
                    break
                if i.mnemonic in ('int3', 'ud2', 'hlt'):
                    unknown = True
                    break
                if i.mnemonic.startswith('j'):
                    if i.operands[0].type != 2:
                        op = i.operands[0]
                        if (i.mnemonic == 'jmp' and op.type == 3 and not op.mem.base
                                and not op.mem.index and not op.mem.segment
                                and (op.mem.disp & 0xffffffff) in import_pops()[0]):
                            pops.add(import_pops()[0][op.mem.disp & 0xffffffff])
                            break
                        # Reuse the lifter's bound/remap validation. The false
                        # unbounded accept predicate forbids table-only guesses.
                        import lift
                        try:
                            todo.extend(lift.switch_targets(history,len(history)-1,target,None,
                                code,accept=lambda _:False,discover=True,bounded_accept=executable))
                        except lift.Unsupported:
                            unknown = True
                        break
                    todo.append(i.operands[0].imm & 0xffffffff)
                    if i.mnemonic == 'jmp':
                        break
                pc += i.size
        _cache[key] = pops.pop() if not unknown and len(pops) == 1 else 0
    return _cache[key]


def saved_push_sites(address, body):
    """Recognize interleaved callee saves, including after calls.

    This refines the mock ABI heuristic, not a recovered callee contract.
    Require an unmodified nonvolatile register and a matching own pop;
    Calls preserve nonvolatile registers under this mock ABI. Stop at returns.
    Never infer from random memory.
    """
    if body is None:
        return set()
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    restored = set()
    for pc in body:
        byte = code(pc, 1)
        if byte and 0x58 <= byte[0] <= 0x5f:
            restored.add(byte[0] - 0x58)
    eligible = {name for k, name in enumerate(('eax','ecx','edx','ebx','esp','ebp','esi','edi'))
                if k in restored and name in ('ebx','ebp','esi','edi')}
    saved, invalid, seen = set(), set(), set()
    todo = [(address, frozenset(eligible))]
    while todo:
        pc, available = todo.pop()
        if pc not in body or (pc, available) in seen:
            continue
        seen.add((pc, available))
        eligible = set(available)
        i = next(md.disasm(code(pc, 15), pc, count=1), None)
        if i is None or i.mnemonic in ('ret','retn','repz ret'):
            continue
        if i.mnemonic == 'push' and i.operands[0].type == 1:
            name = i.reg_name(i.operands[0].reg)
            if name in eligible:
                saved.add(pc)
                eligible.remove(name)
            else:
                invalid.add(pc)
        identity = (i.mnemonic == 'mov' and len(i.operands) == 2
                    and all(o.type == 1 for o in i.operands)
                    and i.operands[0].reg == i.operands[1].reg)
        for r in (() if identity else i.regs_access()[1]):
            name = i.reg_name(r)
            if name in ('bl','bh','bx'): name = 'ebx'
            if name in ('bp','si','di'): name = 'e'+name
            eligible.discard(name)
        if i.mnemonic.startswith('j'):
            if i.operands[0].type == 2:
                todo.append((i.operands[0].imm & 0xffffffff, frozenset(eligible)))
            if i.mnemonic == 'jmp':
                continue
        todo.append((pc+i.size, frozenset(eligible)))
    saved -= invalid
    return saved


def local_alloc_sites(address, body):
    """Conservative mock-ABI hint for interleaved entry-frame allocations.

    A matching add-ESP immediately before an own return must exist. Require
    the allocation in the straight-line entry prefix and no stores or further
    ESP adjustment between the allocation and the first call on every path
    (L28: paths through branches are followed, bounded; saved-register pops
    and the matching cleanup right before a return end a path harmlessly):
    initialized outgoing argument space must not be mistaken for local
    storage. This is not an ABI proof.
    """
    if body is None:
        return set()
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    decoded = {}
    def instruction(pc):
        if pc not in body:
            return None
        if pc not in decoded:
            decoded[pc] = next(md.disasm(code(pc, 15), pc, count=1), None)
        return decoded[pc]
    cleanup = set()
    for pc in body:
        # ADD ESP,imm has one of these two encodings in this 32-bit subset.
        if code(pc, 2) not in (b'\x83\xc4', b'\x81\xc4'):
            continue
        i = instruction(pc)
        after = instruction(pc + i.size) if i else None
        if after and after.mnemonic in ('ret', 'retn', 'repz ret'):
            cleanup.add(i.operands[1].imm)
    def store_or_adjust(i):
        adjustment = i.mnemonic != 'push' and any(i.reg_name(r) == 'esp' for r in i.regs_access()[1])
        return adjustment or any(o.type == 3 and o.access & 2 for o in i.operands)

    def paths_clean(start, size):
        # every path from the branch at `start` reaches a call or a return
        # with no store and no ESP adjustment other than pops/final cleanup
        todo, seen = [start], set()
        while todo:
            pc = todo.pop()
            if pc in seen:
                continue
            seen.add(pc)
            i = instruction(pc)
            if i is None or len(seen) > 512:
                return False
            mn = i.mnemonic
            if mn == 'call' or mn in ('ret', 'retn', 'repz ret', 'int3', 'ud2'):
                continue
            if mn.startswith('j'):
                if i.operands[0].type != 2:
                    return False
                todo.append(i.operands[0].imm & 0xffffffff)
                if mn != 'jmp':
                    todo.append(pc + i.size)
                continue
            after = instruction(pc + i.size)
            final = (mn == 'add' and len(i.operands) == 2 and i.operands[0].type == 1
                     and i.reg_name(i.operands[0].reg) == 'esp' and i.operands[1].type == 2)
            if final and i.operands[1].imm == size and after is not None and after.mnemonic in ('ret', 'retn', 'repz ret'):
                continue
            if mn != 'pop' and store_or_adjust(i):
                return False
            todo.append(pc + i.size)
        return True

    result, pc, pending = set(), address, None
    while pc in body:
        i = instruction(pc)
        if i is None:
            break
        mn = i.mnemonic
        if mn == 'call':
            if pending is not None:
                result.add(pending)
            break
        if mn.startswith('j') and pending is not None:
            if paths_clean(pc, instruction(pending).operands[1].imm):
                result.add(pending)
            break
        if mn.startswith('j') or mn in ('ret','retn','repz ret','int3','ud2'):
            break
        adjustment = mn != 'push' and any(i.reg_name(r) == 'esp' for r in i.regs_access()[1])
        if pending is not None:
            if adjustment or any(o.type == 3 and o.access & 2 for o in i.operands):
                pending = None
                break
        if (mn == 'sub' and i.op_str.startswith('esp, ') and i.operands[1].type == 2
                and i.operands[1].imm > 0 and i.operands[1].imm in cleanup):
            pending = pc
        pc += i.size
    return result


def frame_exit(address, body, limit=4096):
    """Exit ESP of a call-free caller-frame helper, derived from its code.

    Ordinary functions return with ESP = entry ESP + 4 + N. Frame helpers
    (`__SEH_prolog4`, `__SEH_epilog4`, `__EH_prolog3`) instead move the
    caller's frame: prolog4 returns with entry ESP - 24 - entry [esp+8],
    epilog4 with entry EBP + 4. This walks every path of the body with
    symbolic values `(base, constant, terms)`: base 'esp'/'ebp' (the entry
    register) or None, terms `((sign, k), ...)` adding or subtracting the
    entry stack dword at entry ESP + k (k >= 0, read before any write).
    Returns that value for every `ret` (all must agree), or None: any call,
    indirect or outward jump, trap, unmodelled ESP write, aliasing store
    before a stack load, or disagreement. None keeps the ordinary contract.
    """
    result = _frame_walk(address, body, limit)
    return None if result == 'dynamic' else result


def moves_frame(address, body, limit=4096):
    """A call-free helper whose exit a constant-cleanup mock cannot copy:
    a caller-frame exit (frame_exit), or ESP written from a value the walk
    cannot resolve (`__alloca_probe` 0x012F2280: `xchg esp, eax`, exit =
    entry ESP + 4 - entry EAX). Alignment (`and esp`) and `leave` do not
    count."""
    result = _frame_walk(address, body, limit)
    return result == 'dynamic' or caller_frame(result)


def _frame_walk(address, body, limit, _cache={}):
    if body is None:
        return None
    key = (address, frozenset(body), code)
    if key not in _cache:
        _cache.clear()  # one function per recorder run; bound the memory
        _cache[key] = _frame_exit(address, key[1], limit)
    return _cache[key]


def _frame_exit(address, body, limit):
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    names = ('eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi')

    def add(a, b, sign=1):
        if a is None or b is None or (a[0] is not None and b[0] is not None):
            return None
        if b[0] is not None and sign < 0:
            return None
        terms = tuple(sorted(a[2] + tuple((sign * t, k) for t, k in b[2])))
        return (a[0] or b[0], (a[1] + sign * b[1]) & 0xffffffff, terms)

    def address_of(i, o, regs):
        if o.mem.segment:
            return 'other' if i.reg_name(o.mem.segment) == 'fs' else None
        if o.mem.index or not o.mem.base:
            return None
        base = regs.get(i.reg_name(o.mem.base))
        value = add(base, (None, o.mem.disp & 0xffffffff, ()))
        return value

    def offset(a):
        k = a[1]
        return k - (1 << 32) if k & 0x80000000 else k

    def stack(a):
        return a == 'other' or (a is not None and a[0] == 'esp' and not a[2])

    def load(a, size, stores, clobbered):
        if not stack(a) or a == 'other':
            return None
        k = offset(a)
        for start, value in stores:
            if start < k + size and k < start + 4:
                return value if (start, size) == (k, 4) else None
        if clobbered or size != 4 or k < 0:
            return None
        return (None, 0, ((1, k),))

    def store(a, size, value, stores, clobbered):
        if a == 'other':
            return stores, clobbered
        if not stack(a):
            return stores, True
        k = offset(a)
        kept = tuple((s, v) for s, v in stores if not (s < k + size and k < s + 4))
        return kept + (((k, value if size == 4 else None),)), clobbered

    def operand(i, o, regs, stores, clobbered):
        if o.type == 1:
            name = i.reg_name(o.reg)
            return regs.get(name) if name in names else None
        if o.type == 2:
            return (None, o.imm & 0xffffffff, ())
        if o.type == 3:
            return load(address_of(i, o, regs), o.size, stores, clobbered)
        return None

    start = {n: None for n in names}
    start.update(esp=('esp', 0, ()), ebp=('ebp', 0, ()))
    todo = [(address, tuple(sorted(start.items())), (), False)]
    seen, exits, decoded = set(), set(), {}
    while todo:
        pc, regs, stores, clobbered = todo.pop()
        key = (pc, regs, stores, clobbered)
        if key in seen:
            continue
        seen.add(key)
        if len(seen) > limit or pc not in body:
            return None
        regs = dict(regs)
        if pc not in decoded:
            decoded[pc] = next(md.disasm(code(pc, 15), pc, count=1), None)
        i = decoded[pc]
        if i is None:
            return None
        mn, ops = i.mnemonic, i.operands
        if mn in ('ret', 'retn'):
            exits.add(add(regs['esp'], (None, 4 + (ops[0].imm if ops else 0), ())))
            continue
        if mn.startswith('j'):
            if ops[0].type != 2:
                return None
            target = ops[0].imm & 0xffffffff
            if target not in body:
                return None
            todo.append((target, tuple(sorted(regs.items())), stores, clobbered))
            if mn != 'jmp':
                todo.append((pc + i.size, tuple(sorted(regs.items())), stores, clobbered))
            continue
        if mn in ('call', 'int3', 'ud2', 'hlt', 'leave', 'pushal', 'popal', 'pushfd', 'popfd'):
            return None
        esp = regs['esp']
        if mn == 'push':
            value = operand(i, ops[0], regs, stores, clobbered)
            regs['esp'] = add(esp, (None, 4, ()), -1)
            if regs['esp'] is None:
                return None
            stores, clobbered = store(regs['esp'], 4, value, stores, clobbered)
        elif mn == 'pop' and ops[0].type == 1 and i.reg_name(ops[0].reg) in names and i.reg_name(ops[0].reg) != 'esp':
            value = load(esp, 4, stores, clobbered)
            regs['esp'] = add(esp, (None, 4, ()))
            regs[i.reg_name(ops[0].reg)] = value
        elif mn in ('mov', 'lea', 'add', 'sub') and ops[0].type == 1 and i.reg_name(ops[0].reg) in names:
            name = i.reg_name(ops[0].reg)
            if mn == 'lea':
                o = ops[1]
                value = (None if o.mem.index or o.mem.segment or not o.mem.base else
                         add(regs.get(i.reg_name(o.mem.base)), (None, o.mem.disp & 0xffffffff, ())))
            else:
                value = operand(i, ops[1], regs, stores, clobbered)
                if mn != 'mov':
                    value = add(regs[name], value, 1 if mn == 'add' else -1)
            regs[name] = value
        else:
            written = {i.reg_name(r) for r in i.regs_access()[1]}
            if 'esp' in written or 'sp' in written:
                return 'dynamic' if mn in ('xchg', 'mov', 'add', 'sub', 'lea') or mn.startswith('cmov') else None
            for name in names:
                if name in written or name[1:] in written or (name[1] + 'l') in written or (name[1] + 'h') in written:
                    regs[name] = None
            for o in ops:
                if o.type == 3 and o.access & 2:
                    value = operand(i, ops[1], regs, stores, clobbered) if mn == 'mov' and len(ops) == 2 else None
                    stores, clobbered = store(address_of(i, o, regs), o.size, value, stores, clobbered)
        if regs['esp'] is None:
            # mov/lea/add/sub into ESP from an unresolved value
            return 'dynamic' if mn in ('mov', 'lea', 'add', 'sub') and ops and ops[0].type == 1 else None
        todo.append((pc + i.size, tuple(sorted(regs.items())), stores, clobbered))
    return exits.pop() if len(exits) == 1 else None


def caller_frame(contract):
    """True for a frame-relative exit (entry EBP, or entry stack slots).

    A plain `entry ESP + constant` exit is the ordinary form and keeps the
    4..0x404 range check even when it falls outside (misdecoded data such
    as 0x004E4843 ends in `ret 0x57e`): the contract never widens it.
    """
    return contract is not None and (contract[0] != 'esp' or bool(contract[2]))


def frame_text(contract):
    base, constant, terms = contract
    signed = constant - (1 << 32) if constant & 0x80000000 else constant
    text = base + (f' - {-signed:#x}' if signed < 0 else f' + {signed:#x}' if signed else '')
    return text + ''.join(f" {'+' if t > 0 else '-'} [esp+{k:#x}]" for t, k in terms)


def run_sparse(address, seed, max_insn=20000, end=None, lifted=frozenset(), zero_inputs=False, size=None,
               x87_push=frozenset(), body=None, noreturn_sites=frozenset(), noreturn_targets=frozenset(),
               max_seconds=None, input_strategy=None, original_entry=False, x87_entry=0, frame_contracts=None,
               terminal=False, visited=None):
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

    `terminal` (terminal-v1) accepts a root call at a noreturn site into a
    mocked noreturn entry as the end of the case: the call event is recorded
    with "terminal": true and no "out", the run stops at the callee's entry
    (the return address pushed) and that state is the case's result. The
    replay stops at the same call_ext and compares there. Without it such
    runs are rejected, as before.

    `frame_contracts` maps closed callees with a caller-frame exit
    (frame_exit, e.g. a promoted `__SEH_prolog4`) to that contract. Their
    nested frame ends at the return PC with exactly the derived ESP rather
    than an ESP at or above the call's (the prolog returns below it). The
    value 'return-pc' (a call-free ESP-moving helper such as
    `__alloca_probe`) ends it at the first arrival at the return PC.

    `mockzero-v1` keeps the default random pages and registers but makes
    every mocked callee return zero (eax, ecx, edx, xmm0 and any st0 push;
    the inferred stack pop is unchanged). Loops that wait until a call
    returns 0 and then test a flag in memory (worker threads around
    WaitForSingleObject) need both: zero returns to leave the wait, random
    memory to find the flag set. `small-mockzero-v1` and
    `objects-mockzero-v1` do the same over small-v1/objects-v1 pages and
    registers, for such loops that also walk a count read from memory; each
    is identical to its page strategy up to the first mocked call. These are
    test inputs, not callee models.

    `visited`, a set, receives the address of every executed instruction
    (coverage for case selection, L45); it changes nothing in the run.
    """
    import random
    import time
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    rnd = random.Random(seed)
    if input_strategy is not None and zero_inputs:
        raise ValueError('input_strategy and zero_inputs are mutually exclusive')
    # Image pages have always held original bytes. This explicit strategy
    # retains the random distribution elsewhere and tags that input policy.
    if input_strategy is not None and input_strategy not in INPUT_STRATEGIES:
        raise ValueError(f'unknown input strategy: {input_strategy}')
    # A CALL from a 16-byte-aligned caller leaves ESP congruent to 12.
    # Opt in: preserve every old distribution and every return guard.
    # __alloca_probe_16/_8 (0x012F4E90/0x012F4EA6) align ESP+4; with
    # zero EAX this input exercises their zero-allocation return path.
    call_stack = input_strategy == 'zero-call-v1'
    zero_values = zero_inputs or call_stack
    page_strategy = MOCKZERO.get(input_strategy, input_strategy)
    inputs = SparseInputs(page_strategy) if page_strategy in PAGE_STRATEGIES else None
    mock_zero = input_strategy in MOCKZERO
    deadline = time.monotonic()+max_seconds if max_seconds is not None else None
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.ctl_set_tcg_buffer_size(TCG_BUFFER)  # default 1 GB per instance: workers reached 4 GB
    configure_fs(uc)
    # Unicorn's reset tag word reads 0x5555 (eight zero values), not an
    # empty x87 stack. Match the fixture's actual empty-stack contract.
    uc.reg_write(_xc.UC_X86_REG_FPTAG, 0xffff)
    uc.reg_write(_xc.UC_X86_REG_FPCW, 0x037f)
    uc.reg_write(_xc.UC_X86_REG_MXCSR, 0x1f80)
    # L22: every x87 instruction follows the host CPU (x87_host.Oracle;
    # Unicorn 2.1 sets no PE/DE/C1 bits and keeps reserved FLDCW bits).
    # Its flush hook must run before every other code hook.
    import x87_host
    oracle = x87_host.Oracle(lambda uc_, i, op: effective_address(uc_, i, op))
    uc.hook_add(UC_HOOK_CODE, oracle.on_flush)
    # L22: CPUID/RDTSC answers, selector reads and CPL3 POPFD (machine_events).
    import machine_events
    machine = machine_events.Machine(uc)
    uc.hook_add(UC_HOOK_CODE, machine.on_code)
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
        if zero_values:
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
    # Stack dwords written by `push imm32` and not overwritten since: a model
    # size argument found here is a constant of the code, not a random input.
    const_slots, pending_push = {}, [None]

    def forget_constants(addr, size):
        for slot in [a for a in const_slots if a < addr + size and addr < a + 4]:
            del const_slots[slot]

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
            push = pending_push[0]
            forget_constants(addr, size)
            if push is not None and (addr, size) == (push[0], 4):
                const_slots[addr] = push[1]
            pending_push[0] = None

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
        # L22: NaN/Inf float inputs are no longer rejected. The rejection
        # existed because Unicorn does not quiet a signalling NaN on x87 loads
        # and picks the x87 NaN for SSE; the x87 and SSE host oracles now
        # replace every such result with the hardware's (x87_host.Oracle).
        # The old test still runs and tags the case, so its effect is counted.
        if decoded[eip] in FLOAT_LOADS and size in (4, 8, 10):
            bits = int.from_bytes(bytes(uc_.mem_read(addr, size)), "little")
            if (((bits >> 23) & 0xFF) == 0xFF if size == 4 else
                    ((bits >> 52) & 0x7FF) == 0x7FF if size == 8 else
                    ((bits >> 64) & 0x7FFF) == 0x7FFF):
                retired.add("nan-input")

    count, bad = [0], []
    retired = set()  # retired NaN guards this case would have tripped

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
            ea = effective_address(uc_, i, o)
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
                                  o.mem.scale, o.mem.disp, max(o.size, 1),
                                  TEB_BASE if i.reg_name(o.mem.segment) == 'fs' else 0)
                                 for o in i.operands if o.type == 3 and (not o.mem.segment or i.reg_name(o.mem.segment) in ("es", "ds", "ss", "fs"))]                     if not (i.mnemonic in ("lea", "nop") or i.mnemonic.startswith("prefetch")) else []
            except (UcError, StopIteration):
                mem_ops[addr] = []
                x87_ops[addr] = None
        esp = uc_.reg_read(UC_X86_REG_ESP)
        need = {(esp - 0x40) & ~0xFFF, esp & ~0xFFF}  # implicit stack accesses
        for base, index, scale, disp, sz, segment in mem_ops[addr]:
            a = disp + segment
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
    entering = [False]  # a call was just made: the next instruction is the callee's entry
    terminal_call = [None]  # return PC of a root call at a noreturn site (terminal runs)
    padded_call = [False]   # that call is followed by int3 padding, not a known noreturn callee

    def padded_return(addr, size, _cache={}):
        """The straight-line code at a call's return address reaches an
        int3 stop (noreturn_sites) within 8 instructions, with no branch,
        return or call before it."""
        pc = addr + size
        for _ in range(8):
            if pc in noreturn_sites:
                return code(pc, 1) == b"\xcc"
            if not is_own(pc):
                return False
            i = next(nmd.disasm(code(pc, 15), pc, count=1), None)
            if i is None or i.mnemonic.startswith(("j", "ret", "call", "loop", "int")):
                return False
            pc += i.size
        return False
    stopped = [False]       # a terminal call ended the case
    frame_contracts = {a: c if isinstance(c, str) else (c[0], c[1], tuple(tuple(t) for t in c[2]))
                       for a, c in (frame_contracts or {}).items()}
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
        elif mn in ("rol","ror","rcl","rcr","shld","shrd"):
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
        v=op.mem.disp + (TEB_BASE if i.reg_name(op.mem.segment) == 'fs' else 0)
        for reg,scale in ((op.mem.base,1),(op.mem.index,op.mem.scale)):
            if reg:
                v+=uc_.reg_read(getattr(_xc,"UC_X86_REG_"+i.reg_name(reg).upper()))*scale
        return v&0xffffffff

    def on_frame(uc_, addr, size, _):
        # Must precede on_ext: the first root instruction after a closed
        # callee returns can itself be another call that needs recording.
        esp_now = uc_.reg_read(UC_X86_REG_ESP)
        if entering[0]:
            entering[0] = False
            contract = frame_contracts.get(addr)
            if contract == 'return-pc':
                # a call-free helper with no derivable exit (it moves ESP by a
                # value): it can reach the caller's return PC only by returning
                frames[-1] = (frames[-1][0], 0)
            elif contract is not None:
                regs_now = {"esp": esp_now, "ebp": uc_.reg_read(UC_X86_REG_EBP)}
                exit_esp = regs_now[contract[0]] + contract[1]
                for sign, k in contract[2]:
                    exit_esp += sign * struct.unpack("<I", bytes(uc_.mem_read((esp_now + k) & 0xFFFFFFFF, 4)))[0]
                frames[-1] = (frames[-1][0], exit_esp & 0xFFFFFFFF)
        while frames and addr == frames[-1][0] and esp_now >= frames[-1][1]:
            frames.pop()
        root_frame[0] = not frames
        # The guard is for runs that reach a noreturn function. The root's own
        # body is the code under test (e.g. `__exit` 0x012EDC4B returns
        # syntactically; its callee does the exit) and runs like any root.
        if (terminal_call[0] is not None and frames and frames[-1][0] == terminal_call[0]
                and (addr in noreturn_targets or padded_call[0]) and addr not in lifted):
            stopped[0] = True
            calls.append({"target": addr, "regs": {n: uc_.reg_read(r) for n, r in GPR}, "terminal": True})
            uc_.emu_stop()
            return
        if (terminal and end is not None and addr in noreturn_targets and addr != address
                and addr not in lifted and not frames and branch[0] is not None
                and branch[0].mnemonic == "jmp"):
            # L45: a root tail jump into a mocked noreturn entry (`_purecall`
            # 0x012EE1A1: jmp _abort) ends the case like a call there; no
            # return address is pushed, the registers are the callee's entry.
            stopped[0] = True
            calls.append({"target": addr, "regs": {n: uc_.reg_read(r) for n, r in GPR}, "terminal": True})
            uc_.emu_stop()
            return
        if addr in noreturn_targets and addr != address:
            bad.append(f"noreturn target at {addr:#010x}")
            uc_.emu_stop()

    def on_code(uc_, addr, size, _):
        # Unicorn has already decoded this instruction. Reading an arbitrary
        # 15-byte window can cross an unmapped next page even when the whole
        # instruction is mapped, breaking both premap and float checks.
        if stopped[0]:
            return
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
                    rets[addr] = next(nmd.disasm(code(addr, 15), addr)).mnemonic in ("ret", "retn", "repz ret")
                except (KeyError, StopIteration):
                    rets[addr] = False
            own_ret[0] = rets[addr]
            if (terminal and end is not None and padded_return(addr, size)
                    and next(nmd.disasm(code(addr, 15), addr)).mnemonic == "call"):
                # L45: a call whose return path runs straight into int3 (a
                # decode_body stop, possibly after a dead `pop esi`) does
                # not return by the compiler's own evidence, whatever its
                # callee (`__invoke_watson`, `ExitProcess` through the IAT):
                # a mocked callee ends the case like a known noreturn entry.
                terminal_call[0] = addr + size
                padded_call[0] = True
            if addr in noreturn_sites:
                if terminal and end is not None:
                    terminal_call[0] = addr + size
                else:
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
        pending_push[0] = None
        i = integer_ops.get(addr)
        if (i is not None and i.mnemonic == "push" and i.operands[0].type == 2 and i.operands[0].size == 4
                and uc_.reg_read(UC_X86_REG_EIP) == addr):
            pending_push[0] = ((esp_now - 4) & 0xFFFFFFFF, i.operands[0].imm & 0xFFFFFFFF)
        if addr in integer_ops and not model_entry(addr) and uc_.reg_read(UC_X86_REG_EIP) == addr:
            reason = check_integer_flags(uc_, integer_ops[addr])
            if reason:
                bad.append(f"{reason} at {addr:#010x}")
                uc_.emu_stop()
                return
        if body is not None and not model_entry(addr) and addr != RET:
            if addr not in flow:
                try:
                    flow[addr] = next(nmd.disasm(bytes(uc_.mem_read(addr, ins_len(addr))), addr)).mnemonic
                except (UcError, StopIteration):
                    flow[addr] = ""
            if flow[addr] == "call" and uc_.reg_read(UC_X86_REG_EIP) == addr:
                frames.append((addr + size, esp_now))
                entering[0] = True
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
        # L22: SSE operations with two NaN operands take the host's result
        # through the SSE oracle (all of SSE_BINARY is oracle-covered), so
        # `two_nans` no longer rejects them; it tags the case instead.
        if two_nans(uc_, addr):
            retired.add("two-nan")
        count[0] += 1
        if deadline is not None and count[0] % 256 == 1 and time.monotonic() >= deadline:
            bad.append("function time budget")
            uc_.emu_stop()
        if count[0] > max_insn:
            bad.append("runaway")
            uc_.emu_stop()

    heap = [HEAP_BASE]
    model_bytes = [0]  # bytes memset/memcpy touched in this run
    const_bytes = [0]  # of those, with sizes pushed as immediates

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
        forget_constants(a, len(data))
        ensure(a, len(data))
        uc.mem_write(a, data)
        written.update(range(a, a + len(data)))

    def alloc(size):
        a = heap[0]
        heap[0] = ((a + max(size, 1) + 15) & ~15) & 0xFFFFFFFF
        return a

    def model_entry(addr):
        return addr in PLATFORM and not (original_entry and addr == address and not frames)

    def on_platform(uc_, addr, size, _):
        """platform.rs, line for line: arguments, effects, then `ret`."""
        if not model_entry(addr):
            return
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
            # A size pushed as an immediate (sSave::ctor clears 0x80000 bytes)
            # is the same on every input: it gets its own, larger budget.
            constant = const_slots.get((esp + 12) & 0xFFFFFFFF) == n
            if constant:
                const_bytes[0] += n
            else:
                model_bytes[0] += n
            if (const_bytes[0] > CONST_MODEL_BYTES if constant
                    else n > 0x10000 or model_bytes[0] > MODEL_BYTES):
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
    pending_call = [None]  # return PC, caller ESP, outstanding arguments
    dmd = Cs(CS_ARCH_X86, CS_MODE_32)
    dmd.detail = True
    own = {}
    saved_pushes = saved_push_sites(address, body) if end is not None else set()
    local_allocations = local_alloc_sites(address, body) if end is not None else set()

    def on_ext(uc_, pc, size, _):
        if stopped[0]:
            return
        # a call to the function's own entry (recursion) leaves it like any
        # other call: the lifted code makes it through call_ext, so mock it
        recursion = pc == address and branch[0] is not None and branch[0].mnemonic == "call"
        if root_frame[0] and is_own(pc) and not recursion:
            if pending_call[0] is not None and pc == pending_call[0][0]:
                _, before, arguments = pending_call[0]
                consumed = (uc_.reg_read(UC_X86_REG_ESP) - before) & 0xffffffff
                pushed[0] = max(0, arguments - consumed)
                pending_call[0] = None
            if pc not in own:
                own[pc] = next(dmd.disasm(code(pc, 15), pc))  # the function's own code: from the image
            i = own[pc]
            branch[0] = i if i.mnemonic == "call" or i.mnemonic.startswith("j") else None
            if i.mnemonic == "call":  # its arguments: what was pushed since the previous call
                pending_call[0] = (i.address+i.size, uc_.reg_read(UC_X86_REG_ESP), pushed[0])
                pushed[2], pushed[0] = pushed[0], 0
            prologue = (i.mnemonic == "push" and i.operands[0].type == 1) or                 (i.mnemonic == "mov" and i.op_str == "ebp, esp") or                 (i.mnemonic in ("sub", "and") and i.op_str.startswith("esp, "))
            if not pushed[1] and not prologue:
                pushed[1] = True
            if pushed[1]:  # argument space: pushes and `sub esp, N` (float args stored with movss)
                if i.mnemonic == "push" and i.address not in saved_pushes:
                    pushed[0] += 4
                elif (i.mnemonic == "sub" and i.op_str.startswith("esp, ") and i.operands[1].type == 2
                      and i.address not in local_allocations):
                    pushed[0] += i.operands[1].imm
                elif i.mnemonic == "add" and i.op_str.startswith("esp, ") and i.operands[1].type == 2:
                    pushed[0] = max(0, pushed[0] - i.operands[1].imm)
                elif i.mnemonic == 'pop' and i.operands[0].type == 1:
                    pushed[0] = max(0, pushed[0] - 4)
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
        # Explicit immediate caller cleanup means these argument bytes are
        # not callee-popped. This is still a mock ABI inference, not execution
        # of the unavailable callee. Preserve its value in the recorded event.
        known_pop = import_pops()[1].get(pc)
        if known_pop is not None:
            pop = known_pop
        elif not direct and i.mnemonic == "call":
            # Caller cleanup of this call's arguments: right after it, or (L28)
            # merged into a later `add esp` past pushes and direct calls known
            # to leave their arguments (0x00E0E495: push ebx; push edi; call
            # eax; push x3; call sprintf; add esp, 0x10 -> 4 bytes are ours).
            after, later = i.address + i.size, 0
            for _ in range(24):
                if not is_own(after):
                    break
                cleanup = next(dmd.disasm(code(after, 15), after, count=1), None)
                if cleanup is None:
                    break
                writes_esp = any(cleanup.reg_name(r) == 'esp' for r in cleanup.regs_access()[1])
                if (cleanup.mnemonic == "add"
                        and cleanup.op_str.startswith("esp, ") and cleanup.operands[1].type == 2):
                    pop = max(0, pop - max(0, cleanup.operands[1].imm - later))
                    if later:
                        break
                elif (not later and cleanup.mnemonic == 'pop'
                      and cleanup.op_str in ('eax','ecx','edx')):
                    pop = max(0, pop - 4)
                elif cleanup.mnemonic == 'push':
                    later += 4
                elif (later and cleanup.mnemonic == 'call' and cleanup.operands[0].type == 2
                      and (cleanup.operands[0].imm & 0xffffffff) not in noreturn_targets
                      and static_pop(cleanup.operands[0].imm & 0xffffffff) == 0):
                    pass  # a callee that leaves its arguments to us
                elif later and not writes_esp and not cleanup.mnemonic.startswith('j')                         and cleanup.mnemonic not in ('call', 'ret', 'retn', 'repz ret', 'int3', 'ud2'):
                    pass
                else:
                    break
                after += cleanup.size
        ret = struct.unpack("<I", mread(esp, 4))[0]
        out = {"eax": rnd.getrandbits(32), "ecx": rnd.getrandbits(32), "edx": rnd.getrandbits(32),
               "xmm0": [struct.unpack("<I", struct.pack("<f", rnd.uniform(-1000, 1000)))[0] for _ in range(4)],
               "pop": pop}
        if zero_values:
            out.update(eax=0, ecx=0, edx=0, xmm0=[0] * 4)
        if inputs:
            out.update(inputs.returns(rnd))
        if mock_zero:
            out.update(eax=0, ecx=0, edx=0, xmm0=[0] * 4)
        for n, r in (("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX)):
            uc_.reg_write(r, out[n])
        uc_.reg_write(UC_X86_REG_XMM0, sum(v << (32 * k) for k, v in enumerate(out["xmm0"])))
        uc_.reg_write(UC_X86_REG_ESP, (esp + 4 + pop) & 0xFFFFFFFF)
        if ret in x87_push:  # the callee returns a float: push it on the way back
            v = struct.unpack("<I", struct.pack("<f", rnd.uniform(-1000, 1000)))[0]
            if inputs:
                v = inputs.float_bits(rnd)
            if mock_zero:
                v = 0
            out["st_push"] = v
            uc_.mem_write(X87_SLOT, struct.pack("<II", v, ret))
            ret = X87_TRAMP
        uc_.reg_write(UC_X86_REG_EIP, ret)
        calls.append({**ev, "out": out})

    if body is not None:
        uc.hook_add(UC_HOOK_CODE, on_frame)
    if end is not None:
        uc.hook_add(UC_HOOK_CODE, on_ext)
    if visited is not None:
        uc.hook_add(UC_HOOK_CODE, lambda uc_, pc, size, _: visited.add(pc))
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
    uc.hook_add(UC_HOOK_CODE, oracle.on_compute)  # after premap and the recorder's guards
    uc.mem_map(RET & ~0xFFF, 0x1000)
    regs_in = {}
    for name, r in GPR:
        v = rnd.getrandbits(32)
        if zero_values:
            v = 0
        if inputs:
            v = inputs.register(rnd, name)
        if name == "esp":
            v = (0x0F000000 + rnd.randrange(0x100) * 0x1000 + 0x800) & ~3
            if call_stack:
                v -= 4
        regs_in[name] = v
        uc.reg_write(r, v)
    # the return address at [esp]; the rest of the stack page is random
    on_unmapped(uc, None, regs_in["esp"], 4, 0, None)
    stack_page = regs_in["esp"] & ~0xFFF
    page = bytearray(pages_in[stack_page])
    struct.pack_into("<I", page, regs_in["esp"] - stack_page, RET)
    pages_in[stack_page] = bytes(page)
    uc.mem_write(stack_page, bytes(page))
    # L22 x87 entry contract (lift.x87_deficit): the caller left `x87_entry`
    # values on the x87 stack. Random finite doubles (drawn only for such
    # bodies, so other fixtures keep their streams), loaded by the host's
    # fld m64; recorded st0 first as 80-bit (mantissa, exponent).
    st_in = []
    if x87_entry:
        state = x87_host.initial_state()
        for _ in range(x87_entry):
            value = 0.0 if zero_inputs else rnd.uniform(-1000, 1000)
            if inputs:
                value = struct.unpack("<f", struct.pack("<I", inputs.float_bits(rnd)))[0]
            state, _, _ = x87_host.execute(state, 0xDD, 0x00, struct.pack("<d", value))
        x87_host.set_unicorn_state(uc, state)
        st_in = [list(struct.unpack_from("<QH", state, 28 + 10 * k)) for k in range(x87_entry)]
    try:
        uc.emu_start(address, RET)
    except UcError as e:
        # pages are mapped before each instruction (premap), so a fault here
        # is real; resuming would lose QEMU's lazy flags: never resume
        bad.append(f"{e} at {uc.reg_read(UC_X86_REG_EIP):#010x}")
    oracle.flush(uc)  # the last x87 instruction's host result
    if oracle.problem:
        bad.append(oracle.problem)
    machine.flush(uc)
    if machine.problem:
        bad.append(machine.problem)
    # Caller-frame helpers (frame_exit) are checked against their exact
    # derived exit ESP; every other function keeps the ordinary range.
    # A terminal case ends at the noreturn callee's entry, not at a return.
    contract = frame_exit(address, body) if not bad and body is not None and not stopped[0] else None
    stack_contract = None
    if not bad and caller_frame(contract):
        want = regs_in[contract[0]] + contract[1]
        for sign, k in contract[2]:
            slot = [reads.get((regs_in["esp"] + k + n) & 0xFFFFFFFF) for n in range(4)]
            if None in slot:
                bad.append(f"caller-frame slot [esp+{k:#x}] was not an input")
                break
            want += sign * int.from_bytes(bytes(slot), "little")
        if not bad and uc.reg_read(UC_X86_REG_ESP) != want & 0xFFFFFFFF:
            bad.append(f"returned with ESP not {frame_text(contract)} (caller-frame contract)")
        stack_contract = {"rule": "caller-frame-v1", "exit_esp": frame_text(contract)}
    elif not bad and not stopped[0] and not (4 <= ((uc.reg_read(UC_X86_REG_ESP) - regs_in["esp"]) & 0xFFFFFFFF) <= 0x404):
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
        oracle.flush(uc)
        if oracle.problem:
            bad.append(oracle.problem)
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
            **({"x87_oracle": x87_host.VERSION} if oracle.used else {}),
            **({"cpu_dependent": machine_events.host_brand()} if oracle.cpu_dependent else {}),
            **({"mxcsr": uc.reg_read(_xc.UC_X86_REG_MXCSR)} if oracle.mxcsr_touched else {}),
            **({"st_in": st_in} if st_in else {}),
            **machine.result(uc),
            **({"nan_guard_retired": sorted(retired)} if retired else {}),
            **({"calls": calls} if end is not None else {}), **({"zero_inputs": True} if zero_inputs else {}),
            **({"input_strategy": input_strategy} if input_strategy else {}),
            **({"stack_contract": stack_contract} if stack_contract else {}),
            **({"terminal": True} if stopped[0] else {})}
