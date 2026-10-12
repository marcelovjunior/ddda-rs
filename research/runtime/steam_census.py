"""Census of the Steam calls DDDA.exe makes (brief L26), read-only.

Finds every reference to a steam_api.dll import slot in the exe's code and
follows the returned interface pointer forward through straight-line code
to the virtual calls made on it: `call [vtable+off]` with `this` = the
interface. Prints one line per (interface, vtable offset) with its call
sites, plus every flat-API call site and the references the tracker could
not follow (to be read by hand in Ghidra).

The tracker is a linear sweep with per-register provenance that carries
the registers' state to forward branch targets; a site whose interface
pointer reaches the call through a stack slot or a global, or only
through a backward branch, is listed as unresolved.

Usage: python steam_census.py <DDDA.exe (unpacked)> [--json out.json]
"""
import argparse
import json
import struct
import sys

import capstone
from capstone import x86 as cx


def pe(path):
    b = open(path, 'rb').read()
    e = struct.unpack_from('<I', b, 0x3c)[0]
    nsec, = struct.unpack_from('<H', b, e + 6)
    opt = e + 24
    optsize, = struct.unpack_from('<H', b, e + 20)
    base, = struct.unpack_from('<I', b, opt + 28)
    imp_rva, = struct.unpack_from('<I', b, opt + 96 + 8)
    secs = []
    for k in range(nsec):
        s = opt + optsize + 40 * k
        name = b[s:s + 8].rstrip(b'\0').decode()
        vsize, va, rsize, raw = struct.unpack_from('<IIII', b, s + 8)
        secs.append((name, va, vsize, raw, rsize))

    def off(rva):
        for _, va, vs, raw, rs in secs:
            if va <= rva < va + max(vs, rs):
                return raw + rva - va
        raise ValueError(hex(rva))

    def cstr(rva):
        o = off(rva)
        return b[o:b.index(b'\0', o)].decode()

    imports = {}
    d = off(imp_rva)
    while True:
        oft, _, _, name_rva, ft = struct.unpack_from('<IIIII', b, d)
        if name_rva == 0:
            break
        dll = cstr(name_rva)
        k = 0
        while True:
            thunk, = struct.unpack_from('<I', b, off(oft or ft) + 4 * k)
            if thunk == 0:
                break
            fname = f'#{thunk & 0xffff}' if thunk & 0x80000000 else cstr(thunk + 2)
            imports[base + ft + 4 * k] = (dll, fname)
            k += 1
        d += 20
    return b, base, secs, off, imports


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('exe')
    ap.add_argument('--json')
    a = ap.parse_args()
    b, base, secs, off, imports = pe(a.exe)
    steam = {iat: n for iat, (dll, n) in imports.items() if dll.lower() == 'steam_api.dll'}
    text = [s for s in secs if s[0] == '.text'][0]
    _, va, vsize, raw, _ = text
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    code = b[raw:raw + vsize]
    start = base + va
    # pass 1 (no operand detail, addresses only): instructions that name an
    # import slot as an absolute memory operand
    needles = {struct.pack('<I', iat) for iat in steam}
    refs_at = []
    pos = 0
    while pos < len(code):
        got = False
        for addr, size, _, _ in md.disasm_lite(code[pos:pos + 0x10000], start + pos):
            o = addr - start
            if any(n in code[o:o + size] for n in needles):
                refs_at.append(addr)
            pos = o + size
            got = True
        if not got:
            pos += 1
    md.detail = True

    def window(addr, n=400):
        o = addr - start
        out = []
        for i in md.disasm(code[o:o + 8 * n], addr):
            out.append(i)
            if len(out) == n:
                break
        return out

    def mem_of(op):
        return op.mem if op.type == cx.X86_OP_MEM else None

    def reg_name(r):
        return md.reg_name(r) if r else None

    full = {'al': 'eax', 'ax': 'eax', 'bl': 'ebx', 'bx': 'ebx', 'cl': 'ecx', 'cx': 'ecx',
            'dl': 'edx', 'dx': 'edx', 'si': 'esi', 'di': 'edi', 'bp': 'ebp'}

    methods = {}  # (iface, off) -> sites
    flat = {}     # api -> sites
    unresolved = []
    refs = []
    for at in refs_at:
        w = window(at)
        if w and any(op.type == cx.X86_OP_MEM and op.mem.base == 0 and op.mem.index == 0
                     and (op.mem.disp & 0xffffffff) in steam for op in w[0].operands):
            refs.append(w)
    for w in refs:
        r = w[0]
        iat = [op.mem.disp & 0xffffffff for op in r.operands if op.type == cx.X86_OP_MEM][0]
        api = steam[iat]
        accessor = api.startswith('Steam') and not api.startswith('SteamAPI_')
        if not accessor:
            flat.setdefault(api, []).append(r.address)
            if r.mnemonic != 'call':
                unresolved.append((api, r.address, 'flat API through a register'))
            continue
        iface = 'I' + api
        # provenance: reg -> ('acc',) accessor pointer, ('if',) interface,
        # ('vt',) vtable, ('m', off) method pointer
        st = {}
        found = False
        if r.mnemonic == 'call':
            st['eax'] = ('if',)
        elif r.mnemonic == 'mov' and r.operands[0].type == cx.X86_OP_REG:
            st[reg_name(r.operands[0].reg)] = ('acc',)
        else:
            unresolved.append((api, r.address, f'{r.mnemonic} {r.op_str}'))
            continue
        pending = {}  # forward branch target -> register state at the branch
        dead = False  # after a ret or jmp: no fall-through
        for i in w[1:]:
            m, ops = i.mnemonic, i.operands
            if m == 'int3':  # the function's end (padding)
                break
            if i.address in pending:
                st = {**pending.pop(i.address), **({} if dead else st)}
                dead = False
            elif dead:
                continue
            if m.startswith('j') and ops and ops[0].type == cx.X86_OP_IMM and ops[0].imm > i.address:
                pending.setdefault(ops[0].imm, dict(st))
            if m in ('ret', 'jmp'):
                dead = True
                if not pending:
                    break
                continue
            if m == 'call':
                o = ops[0]
                if o.type == cx.X86_OP_REG:
                    p = st.get(reg_name(o.reg))
                    if p == ('acc',):
                        for x in ('eax', 'ecx', 'edx'):
                            st.pop(x, None)
                        st['eax'] = ('if',)
                        continue
                    if p and p[0] == 'm':
                        methods.setdefault((iface, p[1]), []).append(i.address)
                        found = True
                elif o.type == cx.X86_OP_MEM and o.mem.base and st.get(reg_name(o.mem.base)) == ('vt',) and not o.mem.index:
                    methods.setdefault((iface, o.mem.disp), []).append(i.address)
                    found = True
                elif o.type == cx.X86_OP_MEM and not o.mem.base and (o.mem.disp & 0xffffffff) == iat:
                    for x in ('eax', 'ecx', 'edx'):
                        st.pop(x, None)
                    st['eax'] = ('if',)
                    continue
                keep = {x: v for x, v in st.items() if x not in ('eax', 'ecx', 'edx')}
                st = keep
                if not st and not pending:
                    break
                continue
            if m in ('mov',) and len(ops) == 2 and ops[0].type == cx.X86_OP_REG:
                dst = full.get(reg_name(ops[0].reg), reg_name(ops[0].reg))
                s = ops[1]
                if s.type == cx.X86_OP_REG:
                    v = st.get(full.get(reg_name(s.reg), reg_name(s.reg)))
                    if v:
                        st[dst] = v
                    else:
                        st.pop(dst, None)
                    continue
                if s.type == cx.X86_OP_MEM and s.mem.base and not s.mem.index:
                    v = st.get(reg_name(s.mem.base))
                    if v == ('if',) and s.mem.disp == 0:
                        st[dst] = ('vt',)
                        continue
                    if v == ('vt',):
                        st[dst] = ('m', s.mem.disp)
                        continue
                if s.type == cx.X86_OP_MEM and not s.mem.base and (s.mem.disp & 0xffffffff) == iat:
                    st[dst] = ('acc',)
                    continue
                st.pop(dst, None)
                continue
            # any other write to a register clears its provenance
            for o in ops:
                if o.type == cx.X86_OP_REG and o.access & capstone.CS_AC_WRITE:
                    st.pop(full.get(reg_name(o.reg), reg_name(o.reg)), None)
            if m in ('pop',) and ops and ops[0].type == cx.X86_OP_REG:
                st.pop(reg_name(ops[0].reg), None)
            if not st and not pending:
                break
        if not found:
            unresolved.append((api, r.address, 'no virtual call followed'))
    for api in sorted(flat):
        print(f'flat {api}: ' + ' '.join(f'{x:#010x}' for x in sorted(flat[api])))
    for (iface, o) in sorted(methods):
        sites = sorted(set(methods[(iface, o)]))
        print(f'{iface} +{o:#x} slot {o // 4}: {len(sites)} site(s): ' + ' '.join(f'{x:#010x}' for x in sites))
    for api, at, why in unresolved:
        print(f'unresolved {api} at {at:#010x}: {why}')
    if a.json:
        json.dump({'flat': {k: sorted(v) for k, v in flat.items()},
                   'methods': [{'iface': i, 'offset': o, 'sites': sorted(set(s))} for (i, o), s in sorted(methods.items())],
                   'unresolved': [{'api': p, 'at': x, 'why': w} for p, x, w in unresolved]},
                  open(a.json, 'w'), indent=1)


if __name__ == '__main__':
    sys.exit(main())
