"""Brief L20 census: which IDirect3DDevice9 methods the exe calls, and where.

Read-only; run with the research Python (Capstone installed) on the
unpacked analysis copy of DDDA.exe. The output (call sites per vtable
slot) is private and is refused under this repository.

The renderer keeps the device at sRender+0x98 (sRender: the global
0x018D08F4) and copies it to its draw context's +4 (0x00DA6DBE). Calls go
`mov r,[dev]; mov r2,[r+slot*4]; push dev; call r2` with the loads far
apart, so a plain pattern scan finds nothing. This scan follows values
through registers and stack slots in one linear pass over .text:

- a value loaded from [reg+off] is an object of origin ('o', off), or
  ('g', global, off) when reg was loaded from a global;
- [object] is its vtable; [vtable+disp] a method pointer of slot disp/4;
- an indirect call through a method pointer, or `call [vtable+disp]`,
  with an object of the same kind pushed last (the COM this) is a hit.

Caller-saved registers are forgotten at calls, jumps and returns;
callee-saved ones survive jumps, returns and pops (functions keep the
device in esi/edi across their switch cases and epilogues). Hits are kept
for every origin; the device's are those with origin ('g', 0x018D08F4,
0x98), ('o', 0x98) in the renderer and resource classes, and ('o', 4) in
the renderer (0x00DA0000-0x00DC0000). Other origins are other COM classes.
Decoding goes in 16 KB chunks: Capstone's disasm() decodes a whole buffer
at once and needs about 12 GB for .text.
"""
import argparse
import json
from pathlib import Path
import struct
import sys

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_IMM, X86_OP_MEM, X86_OP_REG

FULL = {'al': 'eax', 'ah': 'eax', 'ax': 'eax', 'bl': 'ebx', 'bh': 'ebx', 'bx': 'ebx', 'cl': 'ecx',
        'ch': 'ecx', 'cx': 'ecx', 'dl': 'edx', 'dh': 'edx', 'dx': 'edx', 'si': 'esi', 'di': 'edi',
        'bp': 'ebp'}
CALLEE_SAVED = ('esi', 'edi', 'ebx', 'ebp')


def text_section(path):
    b = Path(path).read_bytes()
    pe = struct.unpack_from('<I', b, 0x3c)[0]
    opt = struct.unpack_from('<H', b, pe + 20)[0]
    base = struct.unpack_from('<I', b, pe + 24 + 28)[0]
    o = pe + 24 + opt
    vs, va, rs, ro = struct.unpack_from('<IIII', b, o + 8)
    return base + va, b[ro:ro + rs]


def stream(md, va, data, chunk=0x4000):
    off = 0
    while off < len(data):
        last, tail = None, off + chunk >= len(data)
        for ins in md.disasm(data[off:off + chunk], va + off):
            last = ins
            if not tail and ins.address + ins.size - va > off + chunk - 16:
                break
            yield ins
        if tail:
            return
        off = (last.address - va) if last and last.address - va > off else off + chunk - 16


def scan(va, data):
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    md.skipdata = True
    full = lambda r: FULL.get(r, r)
    tags, stack, lastpush, hits = {}, {}, None, []
    for i in stream(md, va, data):
        mn, ops = i.mnemonic, (i.operands if i.id else [])
        if i.id == 0:
            tags, stack = {}, {}
            continue
        if mn == 'push':
            lastpush = None
            if ops and ops[0].type == X86_OP_REG:
                lastpush = tags.get(full(i.reg_name(ops[0].reg)))
            elif ops and ops[0].type == X86_OP_MEM and not ops[0].mem.index:
                lastpush = ('dev', ('o', ops[0].mem.disp & 0xffffffff))
            continue
        if mn in ('ret', 'int3', 'jmp'):
            if mn == 'int3':
                tags, stack = {}, {}
            else:
                for r in ('eax', 'ecx', 'edx'):
                    tags.pop(r, None)
            continue
        if mn == 'call':
            o = ops[0]
            pushed_dev = lastpush and lastpush[0] == 'dev'
            if o.type == X86_OP_MEM and not o.mem.index:
                t = tags.get(full(i.reg_name(o.mem.base)) if o.mem.base else None)
                if t and t[0] == 'vt' and pushed_dev:
                    hits.append((i.address, t[1], o.mem.disp // 4))
            elif o.type == X86_OP_REG:
                t = tags.get(full(i.reg_name(o.reg)))
                if t and t[0] == 'm' and pushed_dev:
                    hits.append((i.address, t[1], t[2]))
            for r in ('eax', 'ecx', 'edx'):
                tags.pop(r, None)
            lastpush = None
            continue
        if ops and ops[0].type == X86_OP_REG:
            dst = full(i.reg_name(ops[0].reg))
            if mn in ('cmp', 'test') or (mn == 'pop' and dst in CALLEE_SAVED):
                continue
            new = None
            if mn == 'mov' and len(ops) == 2:
                s = ops[1]
                if s.type == X86_OP_REG:
                    new = tags.get(full(i.reg_name(s.reg)))
                elif s.type == X86_OP_MEM and not s.mem.index:
                    m = s.mem
                    b = full(i.reg_name(m.base)) if m.base else None
                    t = tags.get(b)
                    if b is None:
                        new = ('g', m.disp & 0xffffffff)
                    elif b in ('esp', 'ebp') and (b, m.disp) in stack:
                        new = stack[(b, m.disp)]
                    elif m.disp != 0 and not (t and t[0] == 'vt'):
                        origin = ('g', t[1], m.disp & 0xffffffff) if t and t[0] == 'g' else ('o', m.disp & 0xffffffff)
                        new = ('dev', origin)
                    elif m.disp == 0 and t and t[0] == 'dev':
                        new = ('vt', t[1])
                    elif t and t[0] == 'vt' and m.disp < 0x200 and m.disp % 4 == 0:
                        new = ('m', t[1], m.disp // 4)
            if dst == 'esp':
                stack = {}
            if new:
                tags[dst] = new
            else:
                tags.pop(dst, None)
        elif mn == 'mov' and len(ops) == 2 and ops[0].type == X86_OP_MEM and ops[1].type == X86_OP_REG:
            m = ops[0].mem
            b = full(i.reg_name(m.base)) if m.base else None
            if b in ('esp', 'ebp'):
                t = tags.get(full(i.reg_name(ops[1].reg)))
                if t:
                    stack[(b, m.disp)] = t
                else:
                    stack.pop((b, m.disp), None)
    return hits


def is_device(addr, origin):
    if origin == ('g', 0x018D08F4, 0x98):
        return True
    if origin == ('o', 0x98):
        return 0x00D00000 <= addr < 0x00E20000 or 0x01100000 <= addr < 0x01150000
    return origin == ('o', 4) and 0x00DA0000 <= addr < 0x00DC0000


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--exe', required=True, help='the unpacked analysis copy of DDDA.exe')
    ap.add_argument('--out', required=True, help='JSON output (private; not under this repo)')
    a = ap.parse_args()
    out = Path(a.out).resolve()
    if Path(__file__).resolve().parents[2] in out.parents:
        sys.exit('refusing to write under the repository')
    va, data = text_section(a.exe)
    hits = scan(va, data)
    device = {}
    for addr, origin, slot in hits:
        if is_device(addr, origin):
            device.setdefault(slot, []).append(addr)
    out.write_text(json.dumps({'device_slots': {s: sorted(v) for s, v in sorted(device.items())},
                               'all_hits': hits}, indent=1, default=list))
    for s, v in sorted(device.items()):
        print(f'slot {s:3}: {len(v):3} sites, first {", ".join(hex(x) for x in sorted(v)[:4])}')


if __name__ == '__main__':
    main()
