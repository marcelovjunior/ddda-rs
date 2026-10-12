"""Sample the host stacks of a running runtime program (L21: where a boot
that does not stop is spending its time).

    python stack_sampler.py WINPID SECONDS INTERVAL OUT [SYMBOL_DIR]

Every INTERVAL seconds each thread of the process is suspended, its stack
walked with dbghelp (the debug build's PDB) and resumed. A debug build
keeps one host frame per lifted function (`f_00XXXXXX`), so the lifted
frames of a stack are the guest call stack (innermost first), and a
`ddruntime::` frame below them is the service the guest is in (a wait,
the message pump, Present). OUT gets one block per sample and, at the end,
the innermost lifted function and service per thread, counted.

Reads the process only (it never writes its memory).
"""
import ctypes as C
import collections
import re
import sys
import time
from ctypes import wintypes as W

k32 = C.WinDLL('kernel32', use_last_error=True)
dbg = C.WinDLL('dbghelp', use_last_error=True)

PROCESS_QUERY_INFORMATION, PROCESS_VM_READ = 0x0400, 0x0010
THREAD_ALL = 0x0002 | 0x0008 | 0x0010 | 0x0040  # suspend/resume, get context, query
TH32CS_SNAPTHREAD = 0x4
CONTEXT_FULL_AMD64 = 0x10000B
MACHINE_AMD64 = 0x8664


class THREADENTRY32(C.Structure):
    _fields_ = [('dwSize', W.DWORD), ('cntUsage', W.DWORD), ('th32ThreadID', W.DWORD),
                ('th32OwnerProcessID', W.DWORD), ('tpBasePri', W.LONG), ('tpDeltaPri', W.LONG),
                ('dwFlags', W.DWORD)]


class ADDRESS64(C.Structure):
    _fields_ = [('Offset', C.c_uint64), ('Segment', W.WORD), ('Mode', C.c_int)]


class STACKFRAME64(C.Structure):
    _fields_ = [('AddrPC', ADDRESS64), ('AddrReturn', ADDRESS64), ('AddrFrame', ADDRESS64),
                ('AddrStack', ADDRESS64), ('AddrBStore', ADDRESS64), ('FuncTableEntry', C.c_void_p),
                ('Params', C.c_uint64 * 4), ('Far', W.BOOL), ('Virtual', W.BOOL),
                ('Reserved', C.c_uint64 * 3), ('KdHelp', C.c_byte * 512)]


class SYMBOL_INFOW(C.Structure):
    _fields_ = [('SizeOfStruct', W.ULONG), ('TypeIndex', W.ULONG), ('Reserved', C.c_uint64 * 2),
                ('Index', W.ULONG), ('Size', W.ULONG), ('ModBase', C.c_uint64), ('Flags', W.ULONG),
                ('Value', C.c_uint64), ('Address', C.c_uint64), ('Register', W.ULONG),
                ('Scope', W.ULONG), ('Tag', W.ULONG), ('NameLen', W.ULONG), ('MaxNameLen', W.ULONG),
                ('Name', W.WCHAR * 1024)]


k32.OpenProcess.restype = W.HANDLE
k32.OpenThread.restype = W.HANDLE
k32.CreateToolhelp32Snapshot.restype = W.HANDLE
dbg.SymFunctionTableAccess64.restype = C.c_void_p
dbg.SymGetModuleBase64.restype = C.c_uint64
dbg.SymFromAddrW.argtypes = [W.HANDLE, C.c_uint64, C.POINTER(C.c_uint64), C.POINTER(SYMBOL_INFOW)]
dbg.StackWalk64.argtypes = [W.DWORD, W.HANDLE, W.HANDLE, C.POINTER(STACKFRAME64), C.c_void_p,
                            C.c_void_p, C.c_void_p, C.c_void_p, C.c_void_p]
FTA = C.cast(dbg.SymFunctionTableAccess64, C.c_void_p)
GMB = C.cast(dbg.SymGetModuleBase64, C.c_void_p)


def threads(pid):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    e = THREADENTRY32(dwSize=C.sizeof(THREADENTRY32))
    out = []
    ok = k32.Thread32First(snap, C.byref(e))
    while ok:
        if e.th32OwnerProcessID == pid:
            out.append(e.th32ThreadID)
        ok = k32.Thread32Next(snap, C.byref(e))
    k32.CloseHandle(snap)
    return out


names = {}


def name(proc, addr):
    if addr not in names:
        s = SYMBOL_INFOW(SizeOfStruct=88, MaxNameLen=1024)  # sizeof(SYMBOL_INFOW) with Name[1]
        d = C.c_uint64()
        names[addr] = s.Name if dbg.SymFromAddrW(proc, addr, C.byref(d), C.byref(s)) else f'{addr:#x}'
    return names[addr]


def stack(proc, tid, depth=4000):
    th = k32.OpenThread(THREAD_ALL, False, tid)
    if not th:
        return None
    raw = C.create_string_buffer(1232 + 16)
    ctx = (C.addressof(raw) + 15) & ~15
    C.c_uint32.from_address(ctx + 0x30).value = CONTEXT_FULL_AMD64
    pcs = []
    if k32.SuspendThread(th) != 0xFFFFFFFF:
        try:
            if k32.GetThreadContext(th, C.c_void_p(ctx)):
                f = STACKFRAME64()
                f.AddrPC.Offset, f.AddrPC.Mode = C.c_uint64.from_address(ctx + 0xF8).value, 3
                f.AddrFrame.Offset, f.AddrFrame.Mode = C.c_uint64.from_address(ctx + 0xA0).value, 3
                f.AddrStack.Offset, f.AddrStack.Mode = C.c_uint64.from_address(ctx + 0x98).value, 3
                while len(pcs) < depth and dbg.StackWalk64(MACHINE_AMD64, proc, th, C.byref(f), C.c_void_p(ctx),
                                                           None, FTA, GMB, None):
                    if not f.AddrPC.Offset:
                        break
                    pcs.append(f.AddrPC.Offset)
        finally:
            k32.ResumeThread(th)
    k32.CloseHandle(th)
    return pcs


LIFTED = re.compile(r'f_([0-9a-f]{8})$')  # legacy (::f_) and v0 (…10f_) manglings


def summarize(proc, pcs):
    """(guest frames innermost first, the innermost ddruntime frame below
    the innermost lifted one or above all, the top host frame)"""
    syms = [name(proc, pc) for pc in pcs]
    guest, service = [], None
    for s in syms:
        m = LIFTED.search(s)
        if m:
            if not guest or guest[-1] != m.group(1):  # facade + shard wrappers
                guest.append(m.group(1))
        elif not guest and service is None and 'ddruntime' in s and 'dispatch' not in s:
            service = s
    return guest, service, syms[0] if syms else '?'


def main():
    pid, seconds, interval, out = int(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
    symdir = sys.argv[5] if len(sys.argv) > 5 else None
    proc = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not proc:
        sys.exit(f'OpenProcess({pid}) failed: {C.get_last_error()}')
    dbg.SymSetOptions(0x2 | 0x4)  # UNDNAME | DEFERRED_LOADS
    if not dbg.SymInitializeW(proc, symdir, True):
        sys.exit(f'SymInitialize failed: {C.get_last_error()}')
    inner = collections.defaultdict(collections.Counter)
    with open(out, 'w', encoding='utf-8') as w:
        t0 = time.time()
        while time.time() - t0 < seconds:
            tids = threads(pid)
            if not tids:
                w.write('process gone\n')
                break
            w.write(f'--- t={time.time() - t0:.0f}s\n')
            for tid in tids:
                pcs = stack(proc, tid)
                if not pcs:
                    continue
                guest, service, top = summarize(proc, pcs)
                if not guest and service is None:
                    continue  # a host-only thread (runtime workers, Rust std)
                key = (f'0x{guest[0]}' if guest else '-') + (f' in {service}' if service else '')
                inner[tid][key] += 1
                chain = ' <- '.join(f'0x{g}' for g in guest[:12])
                w.write(f'tid {tid} depth {len(guest)} top {top}\n  service {service}\n  guest {chain}'
                        f'{" ..." if len(guest) > 12 else ""}\n  outer {" <- ".join("0x" + g for g in guest[-4:])}\n')
            w.flush()
            time.sleep(interval)
        w.write('\n=== innermost lifted function (and service) per thread, by samples\n')
        for tid, c in inner.items():
            w.write(f'tid {tid}\n')
            for k, n in c.most_common(12):
                w.write(f'  {n:4} {k}\n')


if __name__ == '__main__':
    main()
