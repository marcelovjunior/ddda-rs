"""Census of the application options DDDA.exe registers on sApp (brief L34),
read-only.

Every option is an sApp::Item made by 0x00DF1090 through one of the
registrars 0x00DF2B80 (type 1, a choice), 0x00DF2BC0 (3, f32 range),
0x00DF2C00 (4, s32 range), 0x00DF2C40 (5, bool); type 2 (flags) has no
registrar in this build. Each registrar takes 15 stack words: the section
mask, a 40-byte property descriptor (name, type word, object, setter,
getter...) and the type's extra words. The script finds every call to a
registrar, emulates the straight-line code that sets up its arguments
(from the previous call in the same function) with unicorn, and prints the
words with the strings and choice lists they point to.

Usage: python options_census.py <DDDA.exe> [--json out.json]
"""
import argparse
import json
import struct
import sys

import capstone
import unicorn
from unicorn import x86_const as R

REGISTRARS = {0x00DF2B80: 1, 0x00DF2BC0: 3, 0x00DF2C00: 4, 0x00DF2C40: 5}
# argument bytes each registrar pops (its `ret N`)
POPS = {0x00DF2B80: 0x3c, 0x00DF2BC0: 0x3c, 0x00DF2C00: 0x3c, 0x00DF2C40: 0x34}
SECTIONS = 0x0142C068  # (name, mask) x 7: GAME, GRAPHICS, DISPLAY, ETC, DEVELOP, SOUNDS, CPU
MARK = {'esi': 0x5E000000, 'edi': 0xED000000, 'ebp': 0xEB000000, 'ecx': 0xEC000000, 'edx': 0xED500000, 'eax': 0xEA000000}


def pe(path):
    b = open(path, 'rb').read()
    e = struct.unpack_from('<I', b, 0x3c)[0]
    nsec, = struct.unpack_from('<H', b, e + 6)
    opt = e + 24
    optsize, = struct.unpack_from('<H', b, e + 20)
    base, = struct.unpack_from('<I', b, opt + 28)
    secs = []
    for k in range(nsec):
        s = opt + optsize + 40 * k
        vsize, va, rsize, raw = struct.unpack_from('<IIII', b, s + 8)
        secs.append((va, vsize, raw, rsize))
    return b, base, secs


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('exe')
    ap.add_argument('--json')
    a = ap.parse_args()
    b, base, secs = pe(a.exe)

    def off(va):
        for v, vs, raw, rs in secs:
            if v <= va - base < v + max(vs, rs):
                return raw + va - base - v
        return None

    def u32(va):
        o = off(va)
        return None if o is None else struct.unpack_from('<I', b, o)[0]

    def cstr(va):
        o = off(va)
        if o is None:
            return None
        end = b.index(b'\0', o)
        t = b[o:end]
        return t.decode('latin1') if end - o < 200 and all(32 <= c < 127 for c in t) else None

    sections = [(cstr(u32(SECTIONS + 8 * k)), u32(SECTIONS + 8 * k + 4)) for k in range(7)]
    text_va, text_size, text_raw, _ = secs[0]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    code = b[text_raw:text_raw + text_size]
    start = base + text_va
    # every call to a registrar
    calls = []
    pos = 0
    while pos < len(code):
        got = False
        for addr, size, mn, ops in md.disasm_lite(code[pos:pos + 0x10000], start + pos):
            if mn == 'call' and ops.startswith('0x') and int(ops, 16) in REGISTRARS:
                calls.append((addr, int(ops, 16)))
            pos = addr + size - start
            got = True
        if not got:
            pos += 1
    mu = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_32)
    img_size = max(v + max(vs, rs) for v, vs, raw, rs in secs)
    img_size = (img_size + 0xFFF) & ~0xFFF
    mu.mem_map(base, img_size)
    for v, vs, raw, rs in secs:
        mu.mem_write(base + v, b[raw:raw + rs])
    STACK = 0x0F000000
    mu.mem_map(STACK - 0x100000, 0x200000)
    def ret_bytes(target):
        """The bytes a callee pops (its first `ret N`), for skipping calls."""
        o = off(target)
        if o is None:
            return 0
        for addr, size, mn, ops in md.disasm_lite(b[o:o + 0x400], target):
            if mn == 'ret':
                return int(ops, 16) if ops else 0
        return 0

    sites = {site for site, _ in calls}
    state = {}

    def hook(uc, address, size, _):
        """Step over every call: a registrar call is a site (stop there),
        any other callee is skipped as if it returned 1 and popped its
        arguments."""
        if address in sites and address != state['from']:
            uc.emu_stop()
            return
        insn = bytes(uc.mem_read(address, size))
        target = None
        if insn[0] == 0xE8:
            target = (address + 5 + struct.unpack('<i', insn[1:5])[0]) & 0xFFFFFFFF
        elif insn[0] == 0xFF and (insn[1] >> 3) & 7 == 2:
            target = 0
        if target is None:
            return
        n = ret_bytes(target) if target else 0
        uc.reg_write(R.UC_X86_REG_ESP, uc.reg_read(R.UC_X86_REG_ESP) + n)
        uc.reg_write(R.UC_X86_REG_EAX, 1)
        uc.reg_write(R.UC_X86_REG_EIP, address + size)

    mu.hook_add(unicorn.UC_HOOK_CODE, hook)
    out = []
    prev = None  # the previous site, and the bytes its registrar pops
    calls.sort()
    for site, reg in calls:
        # set-up code: straight on from the previous site when it is close
        # (same registration block: registers carry over), else from the
        # last branch or call before this site
        if prev is not None and 0 < site - prev[0] < 0x200:
            begin = prev[0] + 5
            mu.reg_write(R.UC_X86_REG_ESP, mu.reg_read(R.UC_X86_REG_ESP) + prev[1])
        else:
            begin = site - 0x200
            for addr, size, mn, ops in md.disasm_lite(code[site - start - 0x200:site - start], site - 0x200):
                if mn in ('call', 'ret', 'jmp') or mn.startswith('j'):
                    begin = addr + size
            regs = {'ebx': 0, **MARK}
            for k, v in regs.items():
                mu.reg_write(getattr(R, 'UC_X86_REG_' + k.upper()), v)
            mu.reg_write(R.UC_X86_REG_ESP, STACK)
            mu.mem_write(STACK - 0x100000, bytes(1) * 0x200000)
        state['from'] = begin
        prev = (site, POPS[reg])
        try:
            mu.emu_start(begin, site)
        except unicorn.UcError as e:
            out.append({'site': site, 'error': str(e), 'begin': begin})
            prev = None
            continue
        esp = mu.reg_read(R.UC_X86_REG_ESP)
        n = POPS[reg] // 4
        words = list(struct.unpack(f'<{n}I', mu.mem_read(esp, 4 * n)))
        rec = {'site': site, 'registrar': reg, 'type': REGISTRARS[reg], 'begin': begin, 'words': words}
        mask = words[0] & 0xFFFF
        rec['sections'] = [n for n, m in sections if m & mask]
        rec['name'] = cstr(words[1])
        rec['type_word'] = words[2]
        rec['object'] = words[3]
        rec['setter'] = words[4]
        rec['getter'] = words[6]
        t = REGISTRARS[reg]
        if t == 1:
            lst, k, items = words[11], 0, []
            while lst and u32(lst) is not None and u32(lst + 8 * k):
                items.append((cstr(u32(lst + 8 * k)), u32(lst + 8 * k + 4)))
                k += 1
            rec['choices'] = items
            rec['extra'] = words[12]
        elif t == 3:
            rec['range'] = [struct.unpack('<f', struct.pack('<I', w))[0] for w in words[11:13]]
        elif t == 4:
            rec['range'] = [struct.unpack('<i', struct.pack('<I', w))[0] for w in words[11:13]]
        # the last two words: the item's +0x34 and its byte +7
        rec['tail'] = words[-2:]
        out.append(rec)
    print('sections:', sections)
    for r in out:
        if 'error' in r:
            print(f"{r['site']:#010x}: emulation failed: {r['error']}")
            continue
        ex = r.get('choices') or r.get('range') or ''
        print(f"{r['site']:#010x} type {r['type']} {'/'.join(r['sections'])} {r['name']!r} typeword {r['type_word']:#x} "
              f"obj {r['object']:#x} set {r['setter']:#x} get {r['getter']:#x} {ex} tail {[hex(w) for w in r['tail']]}")
        print('    words', ' '.join(f'{w:08x}' for w in r['words']))
    if a.json:
        json.dump(out, open(a.json, 'w'), indent=1)


if __name__ == '__main__':
    sys.exit(main())
