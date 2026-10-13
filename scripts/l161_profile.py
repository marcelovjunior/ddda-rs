"""Offline L161 codegen census. Raw captures and assembly stay outside the repo.

Run with the research Python (capstone). Counts are static instruction/sites,
not dynamic frequencies; nearest ownership includes non-lifted helper time.
Split generated parts are attributed to their owning guest entry.
"""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import sys

import sample_profile as profile
from profile_symbols import PEImage


def address(symbol):
    match = re.search(r'f_([0-9a-fA-F]{8})(?:_p\d+)?(?:E|::h[0-9a-fA-F]+)?$', symbol)
    return '0x' + match[1].lower() if match and ('ddlift' in symbol or 'lifted_' in symbol) else None


def census(capture):
    groups = {}
    for group in ('main', 'jobs'):
        near, inclusive, direct = (collections.Counter() for _ in range(3))
        helper_leaves = collections.defaultdict(collections.Counter)
        samples = 0
        for thread in capture['threads']:
            wanted = (thread['name'] == 'guest main thread' if group == 'main' else
                      thread['name'] in [f'guest thread 0x{x:x}' for x in range(0x1070, 0x1085, 4)])
            if not wanted:
                continue
            for stack in thread['stacks']:
                names = [capture['symbols'][str(pc)] for pc in stack]
                if not names or any(w in names[0] for w in profile.WAITS):
                    continue
                samples += 1
                owners = [a for name in names if (a := address(name))]
                if owners:
                    near[owners[0]] += 1
                    inclusive.update(set(owners))
                    leaf = names[0]
                    kind = ('memory' if 'ddnative_base::mem::' in leaf else
                            'flags' if 'Flags::' in leaf else
                            'dispatch' if 'call_ext' in leaf or 'ddruntime::dispatch::' in leaf else
                            'x87' if 'x87::' in leaf else 'other')
                    helper_leaves[owners[0]][kind] += 1
                if a := address(names[0]):
                    direct[a] += 1
        groups[group] = dict(samples=samples, nearest=dict(near), inclusive=dict(inclusive), self=dict(direct),
                             helper_leaves={a: dict(counts) for a, counts in helper_leaves.items()})
    return groups


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('capture', type=Path)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--source-ref', default='3e72b984', help='git pin of generated Rust to inspect')
    args = ap.parse_args()
    repo = Path(__file__).resolve().parents[1]
    if args.output.resolve().is_relative_to(repo):
        ap.error('output must be outside the repository')
    args.output.mkdir(parents=True, exist_ok=True)
    capture = json.loads(args.capture.read_text())
    groups = census(capture)
    total = collections.Counter()
    for group in groups.values():
        total.update(group['nearest'])
    top = [a for a, _ in total.most_common(10)]
    bodies = {}
    pattern = 'pub fn f_(' + '|'.join(a[2:] for a in top) + ')'
    paths = subprocess.check_output(['git', 'grep', '-l', '-E', pattern, args.source_ref, '--',
                                    'crates/ddlift'], cwd=repo, text=True).splitlines()
    for path in paths:
        text = subprocess.check_output(['git', 'show', path], cwd=repo, text=True)
        for a in top:
            marker = f'pub fn f_{a[2:]}('
            start = text.find(marker)
            if start >= 0:
                end = text.find('// Translated from', start)
                bodies[a] = (path.split(':', 1)[1], text[start:end if end >= 0 else len(text)])
    import capstone
    sys.path.insert(0, str(repo / 'research/native'))
    import harness
    import lean_emission
    guest_md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    guest_md.detail = True
    database = sqlite3.connect('file:E:/ddda-re/notes/inventory/inventory.sqlite?mode=ro', uri=True)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    module = next(m for m in capture['modules'] if m['path'].endswith('ddplay.exe'))
    pe = PEImage(module['path'])
    binary = Path(module['path']).read_bytes()
    functions = {}
    for detail in capture['symbol_details'].values():
        a = address(detail.get('name', ''))
        if a in top and detail['evidence'] in ('pdb-size', 'unwind-range'):
            detail = dict(detail)
            if not detail['size']:
                rva = detail['address'] - module['base']
                bounds = next(((start, end) for start, end in pe.ranges if start == rva), None)
                if bounds is None:
                    continue
                detail['size'] = bounds[1] - bounds[0]
            functions[detail['address']] = (a, detail)
    rows = {}
    for a in top:
        path, body = bodies[a]
        rows[a] = dict(source=path, bytes=len(body), gpr_refs=len(re.findall(r'cpu\.(?:eax|ebx|ecx|edx|esi|edi|esp|ebp)\b', body)),
                       accessor_sites=len(re.findall(r'm\.fast_(?:u8|u16|u32|set_u8|set_u16|set_u32)\(', body)),
                       stack_refs=len(re.findall(r'cpu\.esp', body)),
                       flag_writes=len(re.findall(r'cpu\.fl(?: =|\.alu_|\.inc_dec)', body)),
                       call_ext_direct=len(re.findall(r'x86::call_ext\(m, cpu, 0x', body)),
                       call_ext_all=body.count('x86::call_ext('), assembly=[])
        extent = database.execute('SELECT size FROM functions WHERE address=?', (int(a, 16),)).fetchone()[0]
        guest = list(guest_md.disasm(harness.code(int(a, 16), extent), int(a, 16)))
        rows[a]['guest_extent_bytes'] = extent
        rows[a]['guest_linear_instructions'] = len(guest)
        rows[a]['guest_esp_memory_instructions'] = sum(any(op.type == capstone.CS_OP_MEM and
            op.mem.base == capstone.x86_const.X86_REG_ESP for op in i.operands) for i in guest)
        rows[a]['guest_direct_calls'] = sum(i.mnemonic == 'call' and i.operands[0].type == capstone.CS_OP_IMM for i in guest)
        rows[a]['residual_dead_flag_statements'] = sum(
            line.count('cpu.fl') - lean_emission.dead_flags(line).count('cpu.fl')
            for line in body.splitlines() if lean_emission.ARM_HEAD.match(line))
    for _, (a, d) in sorted(functions.items()):
        rva = d['address'] - module['base']
        for start, _, raw, raw_size in pe.sections:
            if start <= rva and rva + d['size'] <= start + raw_size:
                code = binary[raw + rva - start:raw + rva - start + d['size']]
                ins = list(md.disasm(code, rva))
                output = '\n'.join(f'{i.address:x}: {i.mnemonic} {i.op_str}' for i in ins)
                (args.output / f'{a[2:]}_{rva:x}.asm').write_text(output)
                rows[a]['assembly'].append(dict(name=d['name'], rva=rva, size=d['size'], instructions=len(ins),
                    calls=sum(i.mnemonic == 'call' for i in ins),
                    host_stack_operands=sum('[rsp' in i.op_str for i in ins)))
                # Cpu is the third Windows x64 argument, R8. These ten
                # sampled entry bodies copy it into a preserved register.
                base = next((i.op_str.split(',')[0] for i in ins[:30]
                             if i.mnemonic == 'mov' and i.op_str.endswith(', r8')), None)
                if base:
                    pattern = re.compile(r'\[' + base + r' \+ 0x(10[048c]|11[048c])\]')
                    hits = [i for i in ins if pattern.search(i.op_str)]
                    rows[a]['assembly'][-1].update(cpu_base_register=base,
                        gpr_context_operands=len(hits), gpr_context_stores=sum(
                            i.mnemonic == 'mov' and pattern.search(i.op_str.split(',')[0]) is not None for i in hits))
                break
    result = dict(capture=str(args.capture), capture_sha256=hashlib.sha256(args.capture.read_bytes()).hexdigest(),
                  image=module, image_sha256=hashlib.sha256(binary).hexdigest(), source_ref=args.source_ref,
                  groups=groups, top=top, functions=rows)
    (args.output / 'census.json').write_text(json.dumps(result, indent=2))
    for a in top:
        print(a, [(g, groups[g]['nearest'].get(a, 0), groups[g]['inclusive'].get(a, 0), groups[g]['self'].get(a, 0)) for g in groups], rows[a])


if __name__ == '__main__':
    main()
