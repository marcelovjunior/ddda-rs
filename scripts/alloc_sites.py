"""Rank DDPLAY_ALLOC_STACKS cumulative rows and optionally resolve a saved PDB.

python scripts/alloc_sites.py private-run.log --exe C:/.../ddplay.exe
Does not attach to or start any process. Use the exact run's exe/PDB pair.
Only each site's latest cumulative row is counted, never summed over time.
"""
import argparse
import re
from pathlib import Path

ROW = re.compile(r'alloc-site id=(\d+) bytes=(\d+) calls=(\d+) base=([0-9a-f]+) pcs=([0-9a-f,]+)')


def latest(text):
    sites = {}
    for match in ROW.finditer(text):
        site, size, calls, base, pcs = match.groups()
        sites[(int(base, 16), int(site))] = (int(size), int(calls), tuple(int(pc, 16) for pc in pcs.split(',') if int(pc, 16)))
    # Contending first insertions may occupy different slots; merge exact stacks.
    stacks = {}
    for (base, _), (size, calls, pcs) in sites.items():
        old = stacks.get((base, pcs), (0, 0))
        stacks[(base, pcs)] = (old[0] + size, old[1] + calls)
    return sorted(((size, calls, base, pcs) for (base, pcs), (size, calls) in stacks.items()), reverse=True)


def resolver(exe, base):
    import ctypes as c
    import ctypes.wintypes as w
    from sample_profile import SYMBOL_INFOW, sig
    dbg = c.WinDLL('dbghelp', use_last_error=True)
    handle = w.HANDLE(0x140)
    sig(dbg.SymInitializeW, w.BOOL, w.HANDLE, w.LPCWSTR, w.BOOL)
    sig(dbg.SymLoadModuleExW, c.c_uint64, w.HANDLE, w.HANDLE, w.LPCWSTR, w.LPCWSTR, c.c_uint64, w.DWORD, c.c_void_p, w.DWORD)
    sig(dbg.SymFromAddrW, w.BOOL, w.HANDLE, c.c_uint64, c.POINTER(c.c_uint64), c.POINTER(SYMBOL_INFOW))
    if not dbg.SymInitializeW(handle, str(exe.parent), False):
        raise c.WinError(c.get_last_error())
    if not dbg.SymLoadModuleExW(handle, None, str(exe), None, base, 0, None, 0):
        raise c.WinError(c.get_last_error())
    def name(pc):
        info = SYMBOL_INFOW()
        info.SizeOfStruct = SYMBOL_INFOW.Name.offset + c.sizeof(w.WCHAR)
        # SYMBOL_INFOW with a single WCHAR has trailing struct padding on x64.
        info.SizeOfStruct = (info.SizeOfStruct + 7) & ~7
        info.MaxNameLen = 1024
        displacement = c.c_uint64()
        if dbg.SymFromAddrW(handle, pc, c.byref(displacement), c.byref(info)):
            return f'{info.Name}+0x{displacement.value:x}'
        return f'0x{pc:x}'
    return name


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('log', type=Path)
    parser.add_argument('--exe', type=Path)
    args = parser.parse_args()
    text = args.log.read_text(errors='replace')
    rows = latest(text)
    if not rows:
        raise SystemExit('No allocation stacks: run needs DDPLAY_ALLOC_LOG=1 DDPLAY_ALLOC_STACKS=1 DDRUNTIME_LOAD_LOG=1')
    bases = {row[2] for row in rows}
    if args.exe and len(bases) != 1:
        raise SystemExit('Split multi-process logs before symbolizing')
    name = resolver(args.exe.resolve(), next(iter(bases))) if args.exe else lambda pc: f'0x{pc:x}'
    for rank, (size, calls, base, pcs) in enumerate(rows[:10], 1):
        print(f'{rank}. requested_bytes={size} calls={calls} image_base=0x{base:x}')
        for pc in pcs:
            print(f'   {name(pc)}')
    overflow = re.findall(r'alloc-site-overflow bytes=(\d+)', text)
    print(f'Unattributed bytes (bounded table): {overflow[-1] if overflow else "unknown"}')
    print('Cumulative requested bytes, not retained memory. Stack census adds overhead; do not use its run for speed claims.')


if __name__ == '__main__':
    main()
