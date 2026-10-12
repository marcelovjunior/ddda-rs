"""Discover supplementary entry points from the player's PE; emit private TSV.

No Ghidra connection or writes to input files. Re-run against the SAME base
export and lifted manifest for identical output. Evidence is JSON per row.
Sizes are decoded reachable bytes when available, otherwise a one-byte hint.
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'research/runtime'))
from measure import PE
sys.path.insert(0, str(ROOT / 'research/native'))
from capstone import Cs, CS_ARCH_X86, CS_MODE_32


class Image(PE):
    def section(self, a, n=1):
        return next((s for s in self.sections if s['va'] <= a and
                     a+n <= s['va']+min(s['virtual_size'], s['raw_size'])), None)

    def executable(self, a, n=1):
        s = self.section(a, n)
        return bool(s and s['flags'] & 0x20000000)

    def read(self, a, n):
        s = self.section(a)
        if not s:
            raise KeyError(a)
        n = min(n, s['va']+min(s['virtual_size'], s['raw_size'])-a)
        o = self.offset(a)
        return self.data[o:o+n]

    def words(self, name):
        for s in self.sections:
            if s['name'] == name:
                for a in range(s['va'], s['va']+min(s['virtual_size'], s['raw_size'])-3, 4):
                    yield a, self.word(a)


def decoder():
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    return md


def code_entry(pe, md, a):
    """Require a decodable first block ending in a control transfer.

    This is corroboration of an independently identified entry, NOT a code
    scanner: x86 data often decodes. No arbitrary byte offsets are admitted.
    """
    pc = a
    for _ in range(20000):
        if not pe.executable(pc):
            return False
        i = next(md.disasm(pe.read(pc, 15), pc, count=1), None)
        if i is None or not pe.executable(pc, i.size):
            return False
        if i.mnemonic in ('int3', 'ud2', 'hlt', 'in', 'out', 'cli', 'sti', 'iret', 'iretd'):
            return False
        if i.mnemonic in ('ret', 'call') or i.mnemonic.startswith('j'):
            return True
        pc += i.size
    return False


def function_boundary(pe, md, a):
    """Whether `a` starts where a function can: after padding (int3/nop),
    after an instruction that does not fall through (ret, jmp, int3)
    ending exactly at `a`, or right after the previous function's case
    table (two code pointers within 64 KB, or eight switch-index bytes
    below 0x40, as 0x0040D330 and 0x00648AF0 follow their neighbours'
    tables). An immediate that only looks like code (0x00520003 inside an
    `add eax, 4`, the packed ids 0x00620000/0x00740000 and ASCII such as
    0x00706465 stored by 0x00444C20) starts mid-instruction (L45 triage)."""
    if not pe.executable(a - 8, 8):
        return False
    before = pe.read(a - 8, 8)
    if before[-1] in (0xCC, 0x90):
        return True
    for k in range(1, 16):
        i = next(md.disasm(pe.read(a - k, 15), a - k, count=1), None)
        if i is not None and i.address + i.size == a and i.mnemonic in ('ret', 'retn', 'jmp', 'int3'):
            return True
    pointers = struct.unpack('<II', before)
    if all(pe.executable(p) and abs(p - a) < 0x10000 for p in pointers):
        return True
    return all(b < 0x40 for b in before)


def initializers(pe, md):
    # DDDA 2017-12-13 Steam: table arguments and loop bounds at
    # 0x012EDA8A/8F/94 and 0x012EDAAB/B0, indirect call 0x012EDAC2.
    expected = {0x12EDA8A: ('push', 0x14034D0), 0x12EDA8F: ('push', 0x14034B4),
                0x12EDA94: ('call', 0x12EDA3A), 0x12EDAAB: ('mov', 0x139D490),
                0x12EDAB0: ('mov', 0x14034B0)}
    for a, (mn, value) in expected.items():
        i = next(md.disasm(pe.read(a, 15), a, count=1), None)
        if not i or i.mnemonic != mn or i.operands[-1].type != 2 or i.operands[-1].imm != value:
            raise ValueError(f'initializer driver differs at {a:#x}; research this image first')
    for kind, lo, hi in [('c_initializer', 0x14034B4, 0x14034D0),
                         ('cpp_initializer', 0x139D490, 0x14034B0)]:
        for slot in range(lo, hi, 4):
            a = pe.word(slot)
            if a:
                yield a, dict(source=kind, slot=slot, driver=0x12EDA5E)


def mt_registrations(pe, md, roots):
    """MT RTTI: initializer push arguments; mov ecx,descriptor; call ctor.

    Registration ABI from 0x00CF5780 and research/dti/dti_registry.py.
    Only forward-decoded instructions from proven initializer roots count.
    """
    out = {}
    for root in sorted(roots):
        pushes, this = [], None
        for i in md.disasm(pe.read(root, 256), root):
            if i.mnemonic == 'push' and i.operands[0].type == 2:
                pushes.append(i.operands[0].imm & 0xffffffff)
            elif i.mnemonic == 'mov' and i.op_str.startswith('ecx, ') and i.operands[1].type == 2:
                this = i.operands[1].imm & 0xffffffff
            elif i.mnemonic == 'call':
                if i.operands[0].type == 2 and i.operands[0].imm == 0xCF5780 and this and pushes:
                    try:
                        name = pe.string(pushes[-1])
                        if re.fullmatch(r'[A-Za-z_][A-Za-z_0-9:<> ,*]{0,199}', name) and any(
                                s['va'] <= this < s['va']+s['virtual_size'] and s['flags'] & 0x80000000 for s in pe.sections):
                            out[this] = dict(name=name, registration=i.address, initializer=root)
                    except (ValueError, struct.error):
                        pass
                break
            else:
                break
    return out


def vtables(pe, md, roots, known=frozenset(), stores=None, vptr_stores=None):
    regs = mt_registrations(pe, md, roots)
    stubs = {}
    for s in pe.sections:
        if not s['flags'] & 0x20000000:
            continue
        data = pe.data[s['raw']:s['raw']+min(s['virtual_size'], s['raw_size'])]
        for m in re.finditer(rb'\xB8(....)\xC3', data, re.S):
            dti = struct.unpack('<I', m[1])[0]
            if dti in regs:
                stubs[s['va']+m.start()] = dti
    tables = {}
    for slot, value in pe.words('.rdata'):
        if value in stubs and stores and slot-16 in stores and pe.section(slot-16, 20) and all(pe.executable(pe.word(slot-16+4*k)) for k in range(5)):
            dti = stubs[value]
            tables[slot-16] = dict(rtti='MtDTI', descriptor=dti, getdti=value,
                                   stores=sorted(stores[slot-16]), **regs[dti])
        # MSVC PE32 Complete Object Locator + hierarchy + self base descriptor.
        try:
            if not pe.section(value, 20) or pe.word(value) != 0:
                continue
            td, ch = pe.word(value+12), pe.word(value+16)
            name = pe.string(td+8)
            if not name.startswith(('.?AV', '.?AU')) or not name.endswith('@@') or len(name) > 200:
                continue
            if pe.word(ch) != 0 or not 1 <= pe.word(ch+8) <= 4096:
                continue
            bases = pe.word(ch+12)
            if pe.word(pe.word(bases)) != td or not pe.executable(pe.word(slot+4)):
                continue
            tables[slot+4] = dict(rtti='MSVC', descriptor=value, name=name)
        except (ValueError, struct.error):
            continue
    # L22: a table without RTTI (e.g. a COM interface such as the movie
    # player's IStream at 0x01436E78, stored by its constructor at
    # 0x00F7C4AB). Anchor: an object-vtable store of a read-only address at
    # any 4-aligned offset (primary, secondary or member vptr, `vptr_stores`), and
    # corroboration: at least one slot of the run is already a known
    # function. The run is cut as below and at any other stored table.
    vptr_stores = stores if vptr_stores is None else vptr_stores
    for start in sorted(vptr_stores or ()):
        section = pe.section(start, 4)
        if start in tables or not section or section['name'] != '.rdata':
            continue
        run = []
        for k in range(600):
            slot = start+4*k
            if (k and (slot in tables or slot in vptr_stores)) or not pe.section(slot, 4) or pe.section(slot, 4)['name'] != '.rdata':
                break
            a = pe.word(slot)
            if not pe.executable(a) or not code_entry(pe, md, a):
                break
            run.append(a)
        if any(a in known for a in run):
            tables[start] = dict(rtti='none', stores=sorted(vptr_stores[start]))
    starts = sorted(tables)
    for idx, start in enumerate(starts):
        end = starts[idx+1] if idx+1 < len(starts) else 0xffffffff
        for k in range(600):
            slot = start+4*k
            section = pe.section(slot, 4)
            if slot >= end or not section or section['name'] != '.rdata':
                break
            a = pe.word(slot)
            if not pe.executable(a):
                break
            # Pointer-looking strings after MT tables caused bogus Ghidra
            # entries before (dti_labels.py). Require alignment for unknown
            # MT targets except the independently anchored getDTI stub.
            if tables[start]['rtti'] == 'MtDTI' and a not in stubs and a % 16:
                # Already exported CRT entries can be unaligned.
                if a not in known:
                    break
            if not code_entry(pe, md, a):
                break
            # L45: text after a table (the extension tags "xml", "pcf" or the
            # UTF-16 "config" after 0x01407180) names code mid-instruction
            if a not in known and not function_boundary(pe, md, a):
                break
            yield a, dict(source='vtable', table=start, slot=slot, index=k, **tables[start])


def inside(extents, a):
    """Whether `a` lies strictly inside a known function's extent."""
    import bisect
    k = bisect.bisect_right(extents, (a, float('inf'))) - 1
    return k >= 0 and extents[k][0] < a < extents[k][0] + extents[k][1]


def discover(pe, known, lifted, extents=()):
    md = decoder()
    evidence, rejected = collections.defaultdict(list), []
    init = list(initializers(pe, md))
    candidates = list(init)
    stores = collections.defaultdict(set)
    vptr_stores = collections.defaultdict(set)
    for f in lifted:
        for pc in f.get('body', []):
            if not pe.executable(pc):
                raise ValueError(f'lifted instruction outside installed executable: {pc:#x}')
            i = next(md.disasm(pe.read(pc, 15), pc, count=1), None)
            if (i and i.mnemonic == 'mov' and len(i.operands) == 2 and
                    i.operands[0].type == 3 and i.operands[0].size == 4 and
                    i.operands[0].mem.base and not i.operands[0].mem.index and
                    i.operands[1].type == 2):
                disp = i.operands[0].mem.disp
                if disp == 0:
                    stores[i.operands[1].imm & 0xffffffff].add(pc)
                # L22: secondary vptrs (COM interfaces of multiply inherited
                # objects, e.g. the XAPO table 0x0142AFE8 at [esi+4] by
                # 0x00DD50EC) and vptrs of embedded member objects (0x0142AFA0
                # at [edi+0x4DD78] by 0x00DD619B) anchor RTTI-less tables only.
                if disp >= 0 and disp % 4 == 0:
                    vptr_stores[i.operands[1].imm & 0xffffffff].add(pc)
            # address-taken code: an immediate (push imm32 / mov ..., imm32)
            # naming an executable address outside every known function, e.g.
            # the CRT's FlsAlloc stand-in 0x012F953C stored by __mtinit at
            # 0x012F98B8, or the FLS callback pushed at 0x012F9940
            if (i and i.mnemonic in ('push', 'mov') and i.operands[-1].type == 2 and i.operands[-1].size == 4):
                v = i.operands[-1].imm & 0xffffffff
                if (pe.executable(v) and v not in known and not inside(extents, v)
                        and function_boundary(pe, md, v)):
                    candidates.append((v, dict(source='address_taken', function=f['addr'], instruction=pc)))
            if i and i.mnemonic == 'call' and i.operands[0].type == 2:
                if i.operands[0].imm == i.address+i.size:
                    continue  # call/pop position acquisition, not a function
                candidates.append((i.operands[0].imm & 0xffffffff,
                                   dict(source='call', caller=f['addr'], instruction=pc)))
    candidates += list(vtables(pe, md, {a for a, _ in init}, known, stores, vptr_stores))
    for a, ev in candidates:
        if a in known:
            continue
        if not code_entry(pe, md, a):
            rejected.append(dict(address=a, **ev))
        elif ev not in evidence[a]:
            evidence[a].append(ev)
    # The existing recursive decoder supplies byte counts, not distance to
    # the next entry (padding/shared tails must not inflate the denominator).
    import lift
    entries = frozenset(known | evidence.keys())
    rows = []
    for a, ev in sorted(evidence.items()):
        size, size_kind = 1, 'hint'
        try:
            ins, _, _, _ = lift.decode_body(a, 1, entries=entries, noreturn=frozenset(), read=pe.read, is_code=pe.executable)
            size, size_kind = sum(i.size for i in ins), 'reachable'
        except lift.Unsupported:
            pass
        rows.append(dict(address=f'0x{a:08X}', size=size, size_kind=size_kind,
                         sources=','.join(sorted({e['source'] for e in ev})),
                         evidence=json.dumps(sorted(ev, key=lambda e: json.dumps(e, sort_keys=True)), sort_keys=True, separators=(',', ':'))))
    return rows, rejected


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--exe', type=Path, default=Path('E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe'))
    ap.add_argument('--export', type=Path, default=Path('E:/ddda-re/notes/inventory/functions.tsv'))
    ap.add_argument('--lifted', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    if args.out.resolve().is_relative_to(ROOT):
        ap.error('derived entries belong in private data, outside the repository')
    if args.out.resolve() in {p.resolve() for p in (args.exe, args.export, args.lifted)}:
        ap.error('output must not overwrite an input')
    with args.export.open(encoding='utf-8') as f:
        rows_in = list(csv.DictReader(f, delimiter='\t'))
    known = {int(r['address'], 16) for r in rows_in}
    extents = sorted((int(r['address'], 16), int(r['size'] or 1)) for r in rows_in)
    with args.lifted.open() as f:
        lifted = json.load(f)
    pe = Image(args.exe)
    rows, rejected = discover(pe, known, lifted, extents)
    with args.out.open('w', encoding='utf-8', newline='') as f:
        w = csv.DictWriter(f, fieldnames=['address', 'size', 'size_kind', 'sources', 'evidence'], delimiter='\t')
        w.writeheader(); w.writerows(rows)
    counts = collections.Counter(s for r in rows for s in r['sources'].split(','))
    report = dict(image_sha256=hashlib.sha256(pe.data).hexdigest(),
                  export_sha256=hashlib.sha256(args.export.read_bytes()).hexdigest(),
                  lifted_sha256=hashlib.sha256(args.lifted.read_bytes()).hexdigest(),
                  discovered=len(rows), per_source={s: counts[s] for s in ('c_initializer', 'cpp_initializer', 'vtable', 'call', 'address_taken')},
                  size_kinds=dict(collections.Counter(r['size_kind'] for r in rows)), rejected=rejected)
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k != 'rejected'}, indent=2))


if __name__ == '__main__':
    main()
