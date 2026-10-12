"""Sampling CPU profile of a running ddplay (or any x64 process with a PDB).

Suspends each thread of the target at a fixed rate, walks its stack with
dbghelp (StackWalk64 over the image's unwind tables), and resumes it. The
report, per busy thread: the hottest leaf functions (self time), the
hottest lifted game functions (the nearest `f_XXXXXXXX` frame: which
original function the time belongs to) and inclusive frames.

    python scripts/sample_profile.py --name ddplay.exe --seconds 20 > profile.txt

Diagnostics only: it pauses the target briefly per sample (a few tens of
microseconds), so frame rates measured during a profile are lower.
"""

import argparse
import collections
import ctypes
import ctypes.wintypes as wt
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
dbg = ctypes.WinDLL("dbghelp", use_last_error=True)

TH32CS_SNAPTHREAD = 0x4
TH32CS_SNAPPROCESS = 0x2
THREAD_ACCESS = 0x0002 | 0x0008 | 0x0040 | 0x0400  # suspend/resume, get context, query, query limited
PROCESS_ACCESS = 0x0400 | 0x0010  # query information, vm read
CONTEXT_FULL = 0x10000B
IMAGE_FILE_MACHINE_AMD64 = 0x8664
ADDR_MODE_FLAT = 3
SYMOPT_UNDNAME, SYMOPT_DEFERRED_LOADS, SYMOPT_FAIL_CRITICAL_ERRORS = 0x2, 0x4, 0x200


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ThreadID", wt.DWORD),
                ("th32OwnerProcessID", wt.DWORD), ("tpBasePri", wt.LONG), ("tpDeltaPri", wt.LONG),
                ("dwFlags", wt.DWORD)]


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD),
                ("th32DefaultHeapID", ctypes.c_void_p), ("th32ModuleID", wt.DWORD), ("cntThreads", wt.DWORD),
                ("th32ParentProcessID", wt.DWORD), ("pcPriClassBase", wt.LONG), ("dwFlags", wt.DWORD),
                ("szExeFile", wt.WCHAR * 260)]


class ADDRESS64(ctypes.Structure):
    _fields_ = [("Offset", ctypes.c_uint64), ("Segment", wt.WORD), ("Mode", wt.DWORD)]


class STACKFRAME64(ctypes.Structure):
    _fields_ = [("AddrPC", ADDRESS64), ("AddrReturn", ADDRESS64), ("AddrFrame", ADDRESS64),
                ("AddrStack", ADDRESS64), ("AddrBStore", ADDRESS64), ("FuncTableEntry", ctypes.c_void_p),
                ("Params", ctypes.c_uint64 * 4), ("Far", wt.BOOL), ("Virtual", wt.BOOL),
                ("Reserved", ctypes.c_uint64 * 3), ("KdHelp", ctypes.c_byte * 256)]


class SYMBOL_INFOW(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.ULONG), ("TypeIndex", wt.ULONG), ("Reserved", ctypes.c_uint64 * 2),
                ("Index", wt.ULONG), ("Size", wt.ULONG), ("ModBase", ctypes.c_uint64), ("Flags", wt.ULONG),
                ("Value", ctypes.c_uint64), ("Address", ctypes.c_uint64), ("Register", wt.ULONG),
                ("Scope", wt.ULONG), ("Tag", wt.ULONG), ("NameLen", wt.ULONG), ("MaxNameLen", wt.ULONG),
                ("Name", wt.WCHAR * 1024)]


def sig(f, res, *args):
    f.restype, f.argtypes = res, list(args)


sig(k32.CreateToolhelp32Snapshot, wt.HANDLE, wt.DWORD, wt.DWORD)
sig(k32.Thread32First, wt.BOOL, wt.HANDLE, ctypes.POINTER(THREADENTRY32))
sig(k32.Thread32Next, wt.BOOL, wt.HANDLE, ctypes.POINTER(THREADENTRY32))
sig(k32.Process32FirstW, wt.BOOL, wt.HANDLE, ctypes.POINTER(PROCESSENTRY32W))
sig(k32.Process32NextW, wt.BOOL, wt.HANDLE, ctypes.POINTER(PROCESSENTRY32W))
sig(k32.OpenThread, wt.HANDLE, wt.DWORD, wt.BOOL, wt.DWORD)
sig(k32.OpenProcess, wt.HANDLE, wt.DWORD, wt.BOOL, wt.DWORD)
sig(k32.CloseHandle, wt.BOOL, wt.HANDLE)
sig(k32.SuspendThread, wt.DWORD, wt.HANDLE)
sig(k32.ResumeThread, wt.DWORD, wt.HANDLE)
sig(k32.GetThreadContext, wt.BOOL, wt.HANDLE, ctypes.c_void_p)
sig(k32.GetThreadDescription, ctypes.c_long, wt.HANDLE, ctypes.POINTER(ctypes.c_wchar_p))
sig(k32.LocalFree, ctypes.c_void_p, ctypes.c_void_p)
sig(k32.GetThreadTimes, wt.BOOL, wt.HANDLE, *[ctypes.POINTER(wt.FILETIME)] * 4)
sig(dbg.SymSetOptions, wt.DWORD, wt.DWORD)
sig(dbg.SymInitializeW, wt.BOOL, wt.HANDLE, wt.LPCWSTR, wt.BOOL)
sig(dbg.SymFromAddrW, wt.BOOL, wt.HANDLE, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(SYMBOL_INFOW))
sig(dbg.StackWalk64, wt.BOOL, wt.DWORD, wt.HANDLE, wt.HANDLE, ctypes.POINTER(STACKFRAME64), ctypes.c_void_p,
    ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)
FUNCTION_TABLE = ctypes.cast(dbg.SymFunctionTableAccess64, ctypes.c_void_p)
MODULE_BASE = ctypes.cast(dbg.SymGetModuleBase64, ctypes.c_void_p)


def find_pid(name):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    e = PROCESSENTRY32W(dwSize=ctypes.sizeof(PROCESSENTRY32W))
    ok, found = k32.Process32FirstW(snap, ctypes.byref(e)), []
    while ok:
        if e.szExeFile.lower() == name.lower():
            found.append(e.th32ProcessID)
        ok = k32.Process32NextW(snap, ctypes.byref(e))
    k32.CloseHandle(snap)
    if len(found) != 1:
        sys.exit(f"{len(found)} processes named {name}: pass --pid")
    return found[0]


def threads_of(pid):
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    e = THREADENTRY32(dwSize=ctypes.sizeof(THREADENTRY32))
    ok, out = k32.Thread32First(snap, ctypes.byref(e)), []
    while ok:
        if e.th32OwnerProcessID == pid:
            out.append(e.th32ThreadID)
        ok = k32.Thread32Next(snap, ctypes.byref(e))
    k32.CloseHandle(snap)
    return out


def thread_name(h):
    p = ctypes.c_wchar_p()
    if k32.GetThreadDescription(h, ctypes.byref(p)) >= 0 and p.value:
        name = p.value
        k32.LocalFree(p)
        return name
    return ""


def cpu_time(h):
    c, e, k, u = (wt.FILETIME() for _ in range(4))
    k32.GetThreadTimes(h, ctypes.byref(c), ctypes.byref(e), ctypes.byref(k), ctypes.byref(u))
    ft = lambda f: (f.dwHighDateTime << 32 | f.dwLowDateTime) / 1e7
    return ft(k) + ft(u)


CTX_BUF = ctypes.create_string_buffer(0x4D0 + 16)
CTX = (ctypes.addressof(CTX_BUF) + 15) & ~15


def stack(hproc, hthread, depth):
    ctypes.memset(CTX, 0, 0x4D0)
    ctypes.c_uint32.from_address(CTX + 0x30).value = CONTEXT_FULL
    if not k32.GetThreadContext(hthread, CTX):
        return []
    rip = ctypes.c_uint64.from_address(CTX + 0xF8).value
    rsp = ctypes.c_uint64.from_address(CTX + 0x98).value
    rbp = ctypes.c_uint64.from_address(CTX + 0xA0).value
    f = STACKFRAME64()
    f.AddrPC.Offset, f.AddrPC.Mode = rip, ADDR_MODE_FLAT
    f.AddrStack.Offset, f.AddrStack.Mode = rsp, ADDR_MODE_FLAT
    f.AddrFrame.Offset, f.AddrFrame.Mode = rbp, ADDR_MODE_FLAT
    pcs = []
    while len(pcs) < depth and dbg.StackWalk64(IMAGE_FILE_MACHINE_AMD64, hproc, hthread, ctypes.byref(f), CTX,
                                                None, FUNCTION_TABLE, MODULE_BASE, None):
        if f.AddrPC.Offset == 0:
            break
        pcs.append(f.AddrPC.Offset)
    return pcs or [rip]


def pretty(name):
    """Readable path of a Rust v0 symbol (`_R...`): its identifiers joined
    by `::` (crate hashes and generic arguments dropped). Others unchanged."""
    if not name.startswith("_R"):
        return name
    out, k = [], 2
    while k < len(name):
        if name[k].isdigit():
            j = k
            while j < len(name) and name[j].isdigit():
                j += 1
            n = int(name[k:j])
            if name[j:j + 1] == "_":
                j += 1
            ident = name[j:j + n]
            if n and len(ident) == n and (ident[0].isalpha() or ident[0] == "_"):
                out.append(ident)
                k = j + n
                continue
        k += 1
    return "::".join(out[-4:]) or name


WAITS = ("NtWait", "ZwWait", "NtDelayExecution", "NtRemoveIoCompletion", "NtUserMsgWaitForMultipleObjects",
         "NtUserGetMessage", "NtAlertThreadByThreadId", "NtWaitForAlertByThreadId", "ZwRemoveIoCompletion",
         "NtSignalAndWaitForSingleObject", "NtUserWaitMessage")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="ddplay.exe")
    ap.add_argument("--pid", type=int)
    ap.add_argument("--seconds", type=float, default=20)
    ap.add_argument("--hz", type=float, default=500, help="sampling rounds per second (all threads per round)")
    ap.add_argument("--depth", type=int, default=64)
    ap.add_argument("--top", type=int, default=25)
    ap.add_argument("--symbols", help="directories with the PDBs (the exe names only its PDB's file name)")
    a = ap.parse_args()
    pid = a.pid or find_pid(a.name)
    hproc = k32.OpenProcess(PROCESS_ACCESS, False, pid)
    if not hproc:
        sys.exit(f"OpenProcess({pid}) failed: {ctypes.get_last_error()}")
    dbg.SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS)
    if not dbg.SymInitializeW(hproc, a.symbols, True):
        sys.exit(f"SymInitialize failed: {ctypes.get_last_error()}")

    handles, names, cpu0, last, active = {}, {}, {}, {}, set()
    samples = collections.defaultdict(list)  # tid -> [stack]
    t_end, period, rounds = time.perf_counter() + a.seconds, 1.0 / a.hz, 0
    started = time.perf_counter()
    while time.perf_counter() < t_end:
        t0 = time.perf_counter()
        if rounds % 100 == 0:
            for tid in threads_of(pid):
                if tid not in handles:
                    h = k32.OpenThread(THREAD_ACCESS, False, tid)
                    if h:
                        handles[tid], names[tid], cpu0[tid] = h, thread_name(h), cpu_time(h)
            # sample only threads that used CPU since the last check
            now = {tid: cpu_time(h) for tid, h in handles.items()}
            active = {tid for tid in handles if rounds == 0 or now[tid] - last.get(tid, 0) > 0.002}
            last = now
        for tid, h in [(t, handles[t]) for t in active if t in handles]:
            if k32.SuspendThread(h) == 0xFFFFFFFF:
                k32.CloseHandle(h)
                del handles[tid]
                continue
            try:
                samples[tid].append(stack(hproc, h, a.depth))
            finally:
                k32.ResumeThread(h)
        rounds += 1
        rest = period - (time.perf_counter() - t0)
        if rest > 0:
            time.sleep(rest)
    wall = time.perf_counter() - started
    cpu = {tid: cpu_time(h) - cpu0[tid] for tid, h in handles.items()}

    # resolve while the target still runs (dbghelp reads its modules)
    sym_cache = {}
    info = SYMBOL_INFOW()

    def sym(pc):
        if pc not in sym_cache:
            info.SizeOfStruct, info.MaxNameLen = 88, 1024
            disp = ctypes.c_uint64()
            sym_cache[pc] = pretty(info.Name) if dbg.SymFromAddrW(hproc, pc, ctypes.byref(disp), ctypes.byref(info)) else f"?{pc:#x}"
        return sym_cache[pc]

    probe = sorted({pc for st in samples.values() for s in st for pc in s})
    named = sum(not sym(pc).startswith("?") for pc in probe)
    print(f"symbols: {named} of {len(probe)} distinct addresses named")
    print(f"pid {pid}: {rounds} rounds in {wall:.1f} s ({rounds / wall:.0f}/s), {len(samples)} threads")
    order = sorted(samples, key=lambda t: -cpu.get(t, 0))
    for tid in order:
        stacks = samples[tid]
        named = [[sym(pc) for pc in s] for s in stacks]
        busy = [s for s in named if not any(w in s[0] for w in WAITS)]
        c = cpu.get(tid)
        if not busy or (c is not None and c < 0.02 * wall):
            continue
        print(f"\n== thread {tid} {names.get(tid, '')!r}: cpu {c:.1f} s of {wall:.1f} s "
              f"({100 * c / wall:.0f}%), busy {len(busy)}/{len(named)} samples")
        leaf = collections.Counter(s[0] for s in busy)
        guest = collections.Counter(next((f for f in s if "::f_" in f), "(no lifted frame)") for s in busy)
        incl = collections.Counter(f for s in busy for f in set(s))
        for title, counter in (("self", leaf), ("nearest lifted function", guest), ("inclusive", incl)):
            print(f"  -- {title}")
            for name, n in counter.most_common(a.top):
                print(f"  {100 * n / len(busy):5.1f}%  {name}")
    for h in handles.values():
        k32.CloseHandle(h)


if __name__ == "__main__":
    main()
