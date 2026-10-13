"""Sampling CPU profile of a running ddplay (or any x64 process with a PDB).

Suspends each thread of the target at a fixed rate, walks its stack with
dbghelp (StackWalk64 over the image's unwind tables), and resumes it. The
report, per busy thread: the hottest leaf functions (self time), the
hottest lifted game functions (nearest caller, including runtime helpers),
inclusive guest addresses and 2/3-level lifted caller chains. Percentages
are sample shares, not exact CPU-time measurements.

    python scripts/sample_profile.py --name ddplay.exe --seconds 60 --hz 100 \
        --threads "guest main thread,guest thread 0x1070" \
        --save-stacks E:/ddda-re/notes/runs/profile_L131.json > profile.txt
    python scripts/sample_profile.py --input E:/ddda-re/notes/runs/profile_L131.json

Store captures outside the repository; JSON retains raw symbols and leaf-first
stacks for offline re-attribution. --threads uses exact thread descriptions.

Version 2 also saves module paths/bases/identities and DbgHelp displacement,
size, flags and range evidence. Export-only Size is not a function bound.
For CRT private implementations and accurate OS-wait names add:
    --fetch-symbols vcruntime140.dll,ntdll.dll,win32u.dll
Downloads occur AFTER sampling, into --symbol-cache (outside the repo).
Re-resolve an existing version-2 capture without a live process:
    --input capture.json --resymbolize --fetch-symbols vcruntime140.dll
    --save-stacks resolved.json --callers 12
Image identity mismatches are left unresolved. Without matching private
symbols, a PE unwind range/export thunk can identify some functions; the
remaining code is explicitly unresolved, never assigned to a nearby export.

Diagnostics only: it pauses the target briefly per sample (a few tens of
microseconds), so frame rates measured during a profile are lower.
"""

import argparse
import collections
import ctypes
import ctypes.wintypes as wt
import sys
import json
from functools import lru_cache
import re
import struct
from pathlib import Path
import time
from profile_symbols import PEImage, fetch_pdb

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


class MODULEENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("th32ModuleID", wt.DWORD),
                ("th32ProcessID", wt.DWORD), ("GlblcntUsage", wt.DWORD),
                ("ProccntUsage", wt.DWORD), ("modBaseAddr", ctypes.c_void_p),
                ("modBaseSize", wt.DWORD), ("hModule", wt.HMODULE),
                ("szModule", wt.WCHAR * 256), ("szExePath", wt.WCHAR * 260)]


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
sig(k32.Module32FirstW, wt.BOOL, wt.HANDLE, ctypes.POINTER(MODULEENTRY32W))
sig(k32.Module32NextW, wt.BOOL, wt.HANDLE, ctypes.POINTER(MODULEENTRY32W))
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
sig(dbg.SymLoadModuleExW, ctypes.c_uint64, wt.HANDLE, wt.HANDLE, wt.LPCWSTR, wt.LPCWSTR,
    ctypes.c_uint64, wt.DWORD, ctypes.c_void_p, wt.DWORD)
sig(dbg.SymCleanup, wt.BOOL, wt.HANDLE)
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


def modules_of(pid):
    snap = k32.CreateToolhelp32Snapshot(0x8, pid)
    entry = MODULEENTRY32W(dwSize=ctypes.sizeof(MODULEENTRY32W))
    ok, out = k32.Module32FirstW(snap, ctypes.byref(entry)), []
    while ok:
        out.append({'base': entry.modBaseAddr, 'size': entry.modBaseSize,
                    'path': entry.szExePath})
        ok = k32.Module32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return out


def resolve_capture(capture, search_path=None, fetch_modules='', cache=None):
    """Resolve saved module images offline; never attach or start a process.

    Keep every DbgHelp nearest symbol/displacement as evidence, but only give
    it function ownership if a PDB size or PE unwind/thunk range supports it.
    """
    modules = capture.get('modules', [])
    if not modules:
        raise ValueError('capture has no module metadata; capture again with --save-stacks')
    wanted = {name.lower() for name in fetch_modules.split(',') if name}
    images, paths = {}, [search_path] if search_path else []
    for module in modules:
        try:
            image = PEImage(module['path'])
            identity = {'timestamp': image.timestamp, 'size': image.size, 'pdb': list(image.pdb) if image.pdb else None}
            if module.get('identity', identity) != identity:
                raise ValueError('image identity changed since capture')
            module['identity'] = identity
            images[module['base']] = image
            paths.append(str(Path(module['path']).parent))
            if Path(module['path']).name.lower() in wanted:
                try:
                    paths.append(str(fetch_pdb(image, cache).parent))
                except (OSError, ValueError) as error:
                    print(f'public PDB unavailable for {module["path"]}: {error}', file=sys.stderr)
        except (OSError, ValueError, struct.error) as error:
            print(f'image unavailable for {module["path"]}: {error}', file=sys.stderr)
    # A private DbgHelp session loads only on-disk images at captured bases.
    handle = wt.HANDLE(0x147)
    dbg.SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS)
    if not dbg.SymInitializeW(handle, ';'.join(dict.fromkeys(paths)), False):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        for module in modules:
            if module['base'] in images:
                dbg.SymLoadModuleExW(handle, None, module['path'], None,
                                     module['base'], module['size'], None, 0)
        symbols, details = {}, {}
        for pc in sorted({pc for thread in capture['threads'] for stack in thread['stacks'] for pc in stack}):
            module = next((m for m in modules if m['base'] <= pc < m['base'] + m['size']), None)
            image = images.get(module['base']) if module else None
            info, displacement = SYMBOL_INFOW(), ctypes.c_uint64()
            info.SizeOfStruct, info.MaxNameLen = 88, 1024
            found = dbg.SymFromAddrW(handle, pc, ctypes.byref(displacement), ctypes.byref(info)) if image else False
            detail = {'pc': pc}
            if module:
                detail.update(module=module['path'], base=module['base'], rva=pc - module['base'])
            if found:
                detail.update(name=info.Name, address=info.Address, displacement=displacement.value,
                              size=info.Size, flags=info.Flags, tag=info.Tag)
            if image:
                symbol = {'name': info.Name, 'size': info.Size, 'flags': info.Flags,
                          'rva': info.Address - module['base']} if found else {}
                label, evidence = image.bounded_name(pc - module['base'], symbol)
                # Preserve raw Rust names for lifted attribution and pretty().
                if evidence in ('unresolved', 'unwind-only'):
                    label = Path(module['path']).name + '!' + label
                detail['evidence'] = evidence
                symbols[str(pc)] = label
            else:
                symbols[str(pc)] = f'?{pc:#x}'
                detail['evidence'] = 'image-unavailable'
            details[str(pc)] = detail
        capture['symbols'], capture['symbol_details'] = symbols, details
        capture['version'] = 2
    finally:
        dbg.SymCleanup(handle)


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


@lru_cache(maxsize=65536)
def lifted_address(symbol):
    """Recognize the actual function suffix, not a crate in helper generics.

    DbgHelp emits v0 with either R or _R and sometimes partially decoded
    paths. Match before pretty(), which is only a display heuristic.
    """
    match = re.search(r"(?:10|::|^)f_([0-9a-fA-F]{8})(?:E|::h[0-9a-fA-F]+)?(?:[.+][^ ]*|\s*\+\s*0x[0-9a-fA-F]+)?$", symbol)
    if match and ("ddlift" in symbol or "lifted_" in symbol or symbol.startswith("f_")):
        return "0x" + match[1].lower()
    return None


def thread_filter(value):
    return {part.strip() for part in value.split(",") if part.strip()}


def selected_thread(name, filters):
    return not filters or name in filters


def attribution(stacks):
    """Input is leaf-first. Exclusive ownership includes ALL non-lifted callees.

    Inclusive counts a guest address once per sample (including recursion).
    Chains end at the nearest lifted frame and read caller -> callee; helpers
    are omitted and recursive lifted frames are deliberately retained.
    """
    guest, inclusive = collections.Counter(), collections.Counter()
    chains = {depth: collections.Counter() for depth in (2, 3)}
    for stack_names in stacks:
        frames = [address for frame in stack_names if (address := lifted_address(frame))]
        guest[frames[0] if frames else "(no lifted frame)"] += 1
        inclusive.update(set(frames))
        for depth, counter in chains.items():
            if len(frames) >= depth:
                counter[" -> ".join(reversed(frames[:depth]))] += 1
    return guest, inclusive, chains


def report(capture, top, filters, callers=0):
    wall = capture["wall"]
    print(f"pid {capture['pid']}: {capture['rounds']} rounds in {wall:.1f} s "
          f"({capture['rounds'] / wall:.0f}/s)")
    print("Percentages/counts use non-wait samples per thread, not measured CPU seconds. "
          "Nearest ownership includes runtime helpers; incomplete stacks remain unattributed.")
    if 'symbol_details' in capture:
        evidence = collections.Counter(detail['evidence'] for detail in capture['symbol_details'].values())
        print('Symbol range evidence (distinct PCs): ' + ', '.join(f'{key}={value}' for key, value in sorted(evidence.items())))
        print('Unresolved leaves remain in the denominator; their wait/busy state is unknown.')
    symbols = capture["symbols"]
    for thread in sorted(capture["threads"], key=lambda t: -(t["cpu"] or 0)):
        if not selected_thread(thread["name"], filters):
            continue
        named = [[symbols[str(pc)] for pc in stack] for stack in thread["stacks"]]
        valid = [stack for stack in named if stack]
        busy = [stack for stack in valid if not any(w in stack[0] for w in WAITS)]
        waiting = len(valid) - len(busy)
        c = thread["cpu"]
        cpu_label = "unknown" if c is None else f"{c:.1f} s ({100 * c / wall:.0f}%)"
        print(f"\n== thread {thread['tid']} {thread['name']!r}: cpu {cpu_label} of {wall:.1f} s, "
              f"busy {len(busy)}/{len(named)} samples, wait {waiting}, empty {len(named) - len(valid)}")
        if not busy:
            continue
        guest, inclusive, chains = attribution(busy)
        sections = [("self (host leaf)", collections.Counter(pretty(s[0]) for s in busy)),
                    ("nearest lifted function (self + non-lifted callees)", guest),
                    ("inclusive lifted functions", inclusive)]
        sections += [(f"lifted call chains ({depth} levels, caller -> callee)", counter)
                     for depth, counter in chains.items()]
        sections.append(("inclusive (host frames)", collections.Counter(f for s in busy for f in set(map(pretty, s)))))
        for title, counter in sections:
            print(f"  -- {title}")
            for name, count in counter.most_common(top):
                print(f"  {100 * count / len(busy):5.1f}%  {count:7d}  {name}")
        display = [list(map(pretty, stack)) for stack in busy]
        helper = ('_NLG_', '__NLG_', 'memset', 'memcpy', 'memmove', 'Rtl', 'malloc',
                  'free', '__rust_', 'alloc::', 'core::ptr::', '?')
        for name, count in collections.Counter(s[0] for s in display).most_common(callers):
            owners = collections.Counter(next((frame for frame in stack[1:]
                                               if frame != name and not frame.startswith(helper)
                                               and '<unresolved' not in frame), '(none)')
                                         for stack in display if stack[0] == name)
            print(f'  -- callers of {name} ({100 * count / len(busy):.1f}%, {count} samples)')
            for owner, n in owners.most_common(8):
                print(f'  {100 * n / count:5.1f}%  {n:7d}  {owner}')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="ddplay.exe")
    ap.add_argument("--pid", type=int)
    ap.add_argument("--seconds", type=float, default=20)
    ap.add_argument("--hz", type=float, default=500, help="sampling rounds per second (all threads per round)")
    ap.add_argument("--depth", type=int, default=64)
    ap.add_argument("--top", type=int, default=25)
    ap.add_argument("--callers", type=int, default=0, help="nearest non-CRT/heap caller of the N hottest leaves")
    ap.add_argument("--symbols", help="directories with the PDBs (the exe names only its PDB's file name)")
    ap.add_argument("--threads", default="", help="comma-separated exact thread descriptions; default all")
    ap.add_argument("--save-stacks", type=Path, help="save raw symbols and stacks as private JSON for replay")
    ap.add_argument("--input", type=Path, help="re-report a --save-stacks JSON without attaching")
    ap.add_argument("--resymbolize", action="store_true", help="resolve --input again using captured module paths/bases")
    ap.add_argument("--fetch-symbols", default="", help="comma-separated DLL names whose exact public PDBs to fetch from Microsoft")
    ap.add_argument("--symbol-cache", type=Path, default=Path.home() / '.cache' / 'ddda-symbols')
    a = ap.parse_args()
    if a.seconds <= 0 or a.hz <= 0 or a.depth <= 0 or a.top <= 0:
        ap.error("seconds, hz, depth and top must be positive")
    filters = thread_filter(a.threads)
    if a.input:
        capture = json.loads(a.input.read_text(encoding="utf-8"))
        if a.resymbolize or 'symbols' not in capture:
            resolve_capture(capture, a.symbols, a.fetch_symbols, a.symbol_cache)
        if a.save_stacks:
            a.save_stacks.write_text(json.dumps(capture), encoding='utf-8')
        report(capture, a.top, filters, a.callers)
        return
    pid = a.pid or find_pid(a.name)
    hproc = k32.OpenProcess(PROCESS_ACCESS, False, pid)
    if not hproc:
        sys.exit(f"OpenProcess({pid}) failed: {ctypes.get_last_error()}")
    dbg.SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_FAIL_CRITICAL_ERRORS)
    if not dbg.SymInitializeW(hproc, a.symbols, True):
        sys.exit(f"SymInitialize failed: {ctypes.get_last_error()}")

    handles, names, cpu0, last, active = {}, {}, {}, {}, set()
    modules = {module['base']: module for module in modules_of(pid)}
    samples = collections.defaultdict(list)  # tid -> [stack]
    t_end, period, rounds = time.perf_counter() + a.seconds, 1.0 / a.hz, 0
    started = time.perf_counter()
    while time.perf_counter() < t_end:
        t0 = time.perf_counter()
        if rounds % 100 == 0:
            modules.update({module['base']: module for module in modules_of(pid)})
            for tid in threads_of(pid):
                if tid not in handles:
                    h = k32.OpenThread(THREAD_ACCESS, False, tid)
                    if h:
                        handles[tid], names[tid], cpu0[tid] = h, thread_name(h), cpu_time(h)
            # sample only threads that used CPU since the last check
            now = {tid: cpu_time(h) for tid, h in handles.items()}
            # Re-read descriptions: threads may be named after initial discovery.
            names.update({tid: thread_name(h) for tid, h in handles.items()})
            active = {tid for tid in handles if selected_thread(names[tid], filters)
                      and (rounds == 0 or now[tid] - last.get(tid, 0) > 0.002)}
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

    capture = {"version": 2, "pid": pid, "rounds": rounds, "wall": wall,
               "modules": list(modules.values()),
               "threads": [{"tid": tid, "name": names.get(tid, ""), "cpu": cpu.get(tid),
                            "stacks": stacks} for tid, stacks in samples.items()]}
    # Save raw evidence before any symbol-server/network work, outside the
    # sampling window. Resolution can be repeated even after the target exits.
    if a.save_stacks:
        a.save_stacks.write_text(json.dumps(capture), encoding="utf-8")
    dbg.SymCleanup(hproc)
    resolve_capture(capture, a.symbols, a.fetch_symbols, a.symbol_cache)
    if a.save_stacks:
        a.save_stacks.write_text(json.dumps(capture), encoding="utf-8")
    report(capture, a.top, filters, a.callers)
    for h in handles.values():
        k32.CloseHandle(h)
    k32.CloseHandle(hproc)


if __name__ == "__main__":
    main()
