"""Read-only R01 measurements. Outputs are private; never emits game bytes.

Run with the research Python (Capstone installed). See docs/RUNTIME.md.
Static graph counts are potential targets, not dynamic invocation counts.
"""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_32


class PE:
    def __init__(self, path):
        self.data = Path(path).read_bytes()
        pe = self.u32(0x3c)
        assert self.data[pe:pe+4] == b'PE\0\0'
        opt = pe + 24
        assert self.u16(opt) == 0x10b, 'requires PE32'
        self.base = self.u32(opt+28)
        self.entry = self.base + self.u32(opt+16)
        self.size = self.u32(opt+56)
        self.characteristics = self.u16(pe+22)
        self.directories = [struct.unpack_from('<II', self.data, opt+96+k*8) for k in range(16)]
        start = opt + self.u16(pe+20)
        self.sections = []
        for k in range(self.u16(pe+6)):
            off = start+k*40
            vs, rva, rs, raw = struct.unpack_from('<IIII', self.data, off+8)
            self.sections.append(dict(name=self.data[off:off+8].rstrip(b'\0').decode(),
                                      va=self.base+rva, virtual_size=vs, raw_size=rs,
                                      raw=raw, flags=self.u32(off+36)))

    def u16(self, o): return struct.unpack_from('<H', self.data, o)[0]
    def u32(self, o): return struct.unpack_from('<I', self.data, o)[0]
    def offset(self, va):
        rva = va-self.base
        if 0 <= rva < 0x400: return rva
        for s in self.sections:
            if s['va'] <= va < s['va']+s['raw_size']:
                return s['raw']+va-s['va']
        raise ValueError(f'not file-backed: {va:#x}')
    def word(self, va): return self.u32(self.offset(va))
    def string(self, va):
        off = self.offset(va)
        return self.data[off:self.data.index(b'\0', off)].decode(errors='replace')
    def imports(self):
        rva, _ = self.directories[1]
        result = {}
        if not rva: return result
        off = self.offset(self.base+rva)
        while True:
            original, _, _, name, first = struct.unpack_from('<IIIII', self.data, off)
            if not name: break
            dll = self.string(self.base+name)
            k = 0
            while (v := self.word(self.base+(original or first)+4*k)):
                symbol = f'ordinal:{v & 0xffff}' if v & 0x80000000 else self.string(self.base+v+2)
                result[self.base+first+4*k] = dll+'!'+symbol
                k += 1
            off += 20
        return result


def reachable(roots, graph):
    seen, todo = set(), list(roots)
    while todo:
        a = todo.pop()
        if a in seen: continue
        seen.add(a)
        todo.extend(graph.get(a, ()))
    return seen


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--inventory', type=Path, default=Path('E:/ddda-re/notes/inventory'))
    ap.add_argument('--lifted', type=Path, default=Path('E:/ddda-re/notes/native/patterns/lift.json'))
    ap.add_argument('--exe', type=Path, default=Path('E:/ddda-re/bin/DDDA.exe.unpacked.exe'))
    ap.add_argument('--installed', type=Path, help='optional installed PE for section equality check')
    ap.add_argument('--out', type=Path, required=True, help='private output JSON, outside the repo')
    ap.add_argument('--root', action='append', type=lambda s:int(s,0), default=[])
    args = ap.parse_args()
    repo = Path(__file__).resolve().parents[2]
    out = args.out.resolve()
    if out.is_relative_to(repo) or out.is_relative_to(Path('E:/ddda-re/notes/native').resolve()):
        ap.error('output must be outside the repo and the shared native directory')
    db = sqlite3.connect((args.inventory/'inventory.sqlite').resolve().as_uri()+'?mode=ro', uri=True)
    db.execute('PRAGMA query_only=ON')
    rows = {a:dict(name=ns+'::'+name,size=size,status=status,thunk=bool(thunk))
            for a,name,ns,size,status,thunk in db.execute('select address,name,namespace,size,status,thunk from functions')}
    db.close()
    graph = collections.defaultdict(set)
    with (args.inventory/'calls.tsv').open() as f:
        for row in csv.DictReader(f, delimiter='\t'):
            graph[int(row['caller'],16)].add(int(row['callee'],16))
    with args.lifted.open() as f: lifted = {x['addr']:x for x in json.load(f)}
    pe = PE(args.exe); imports = pe.imports()
    md = Cs(CS_ARCH_X86,CS_MODE_32)
    import_sites = collections.defaultdict(list)
    indirect, atomic = {}, []
    # Inventory spans are hints: these sites/counts are a linear-span census,
    # not recursive-descent ownership or a proof every byte is executable.
    for a,row in rows.items():
        try: off = pe.offset(a)
        except ValueError: continue
        sites = []
        for address, _, mnemonic, operands in md.disasm_lite(pe.data[off:off+row['size']],a):
            if mnemonic.startswith('lock ') or mnemonic in ('xchg','cmpxchg','cmpxchg8b','xadd'):
                atomic.append([a,address,mnemonic,operands])
            if mnemonic in ('call','jmp') and not operands.startswith('0x'):
                sites.append(address)
            for match in re.finditer(r'\[(0x[0-9a-f]+)\]', operands):
                target = int(match[1],16)
                if target in imports:
                    import_sites[target].append([a,address,mnemonic])
        if sites: indirect[a] = sites
    # Conservative, all-image pointer-run superset: NOT claimed RTTI vtables.
    # Four or more contiguous aligned pointers to known function entries.
    runs = []
    for s in pe.sections:
        if s['flags'] & 0x20000000: continue
        run = []
        for k in range(0,s['raw_size']-3,4):
            v = pe.u32(s['raw']+k)
            if v in rows: run.append((s['va']+k,v))
            else:
                if len(run)>=4: runs.append(run)
                run=[]
        if len(run)>=4: runs.append(run)
    candidates = {a for run in runs for _,a in run}
    roots = args.root or [0x4079e0,0xdf0d20,0x41dd60,0x12efa14]
    summaries = {}
    def describe(found):
        known = found & rows.keys()
        return dict(nodes=len(found),known=len(known),lifted=len(known & lifted.keys()),
                    inventory_status=dict(collections.Counter(rows[a]['status'] for a in known)),
                    missing_entries=[a for a in sorted(known) if a not in lifted],
                    unresolved_targets=sorted(found-rows.keys()),
                    indirect_sites=sum(len(indirect.get(a,())) for a in known),
                    internal_edge_count=sum(len(graph[a]&known) for a in known),
                    edges_to_missing=sum(len((graph[a]&known)-lifted.keys()) for a in known))
    for root in roots: summaries[hex(root)]=describe(reachable([root],graph))
    # Bounds read from 0x012EDA8A..0x012EDAC9, not inferred from pointer runs.
    initializers = [pe.word(a) for a in range(0x139d490,0x14034b0,4) if pe.word(a)]
    c_initializers = [pe.word(a) for a in range(0x14034b4,0x14034d0,4) if pe.word(a)]
    main_vtable = [pe.word(0x155adc0+4*k) for k in range(12)]  # ctor store at 0x0041B420
    report=dict(exe_sha256=hashlib.sha256(pe.data).hexdigest(),base=pe.base,entry=pe.entry,
                image_size=pe.size,characteristics=pe.characteristics,sections=pe.sections,
                directories=pe.directories,lifted_count=len(lifted),
                closed_count=sum(not f.get('ext') for f in lifted.values()),
                imports={a:dict(name=name,sites=import_sites[a]) for a,name in imports.items()},
                roots=summaries,rows=rows,atomic_sites=atomic,
                pointer_runs=len(runs),pointer_targets=len(candidates),
                pointer_target_closure=describe(reachable(candidates|set(roots),graph)),
                pointer_run_starts=[run[0][0] for run in runs],
                initializers=dict(slots=(0x14034b0-0x139d490)//4,non_null=len(initializers),
                                  known=len(set(initializers)&rows.keys()),targets=initializers,
                                  c_targets=c_initializers),
                main_vtable=main_vtable,
                boot_with_main_vtable=describe(reachable({0x4079e0,0x41b400}|set(main_vtable),graph)),
                input_sha256={str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in
                              (args.inventory/'functions.tsv',args.inventory/'calls.tsv',args.lifted)})
    if args.installed:
        installed=PE(args.installed)
        report['installed']=dict(sha256=hashlib.sha256(installed.data).hexdigest(),
                                 entry=installed.entry,image_size=installed.size,
                                 imports_equal=installed.imports()==imports,sections={})
        for s in pe.sections:
            other=next((t for t in installed.sections if t['name']==s['name']),None)
            report['installed']['sections'][s['name']]=bool(other and
                pe.data[s['raw']:s['raw']+s['raw_size']]==installed.data[other['raw']:other['raw']+other['raw_size']])
    out.parent.mkdir(parents=True,exist_ok=True)
    out.write_text(json.dumps(report,indent=2))
    print(json.dumps({k:v for k,v in report.items() if k not in
                      ('rows','imports','atomic_sites','roots','pointer_target_closure','pointer_run_starts',
                       'initializers','boot_with_main_vtable')},indent=2))
    for root,s in summaries.items(): print(root,{k:v for k,v in s.items() if not isinstance(v,list)})
    print('pointer superset',{k:v for k,v in report['pointer_target_closure'].items() if not isinstance(v,list)})
    print('imports',len(imports),'atomic candidate sites',len(atomic),'private output',out)


if __name__ == '__main__': main()
