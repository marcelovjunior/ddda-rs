"""Stage 3: resolve each class's vtable (getDTI at slot 4) and emit Ghidra labels.

Output CSV columns: address,kind,namespace,name,source
kind: data | function. namespace is the MtDTI class name.
A virtual method is named after the most-base class whose vtable has that
function at that slot (the class that introduced or last overrode it).
"""
import csv, json, re, struct, sys, collections
sys.path.insert(0, __import__('os').path.dirname(__file__))
from peimg import Img
im = Img('E:/ddda-re/bin/DDDA.exe.unpacked.exe')
reg = json.load(open('E:/ddda-re/notes/dti-registry.json'))
classes = reg['classes']
by_dti = {int(c['dti'], 16): c for c in classes}
by_name = {c['name']: c for c in classes}
text = [s for s in im.secs if s[0] == '.text'][0]; rdata = [s for s in im.secs if s[0] == '.rdata'][0]
d = im.d
def in_text(v): return text[1] <= v < text[1] + text[2]

stub_to_dti = {}
for m in re.finditer(rb'\xB8(....)\xC3', d[text[4]:text[4] + min(text[2], text[3])], re.S):
    v = struct.unpack('<I', m.group(1))[0]
    if v in by_dti: stub_to_dti[text[1] + m.start()] = v
cands = collections.defaultdict(list)          # dti -> [vtable starts]
for i in range(0, min(rdata[2], rdata[3]) - 3, 4):
    v = struct.unpack_from('<I', d, rdata[4] + i)[0]
    if v in stub_to_dti:
        cands[stub_to_dti[v]].append(rdata[1] + i - 16)
all_starts = sorted({s for l in cands.values() for s in l})

def vt_len(start):
    nxt = next((s for s in all_starts if s > start), None)
    n = 0
    while in_text(im.u32(start + 4 * n)) and (nxt is None or start + 4 * n < nxt) and n < 600:
        n += 1
    return n

stats = collections.Counter()
for c in classes:
    dti = int(c['dti'], 16)
    cs = cands.get(dti, [])
    vc = int(c['vtable_ctor'], 16) if c.get('vtable_ctor') else None
    if len(cs) == 1: c['vtable'] = hex(cs[0]); stats['single'] += 1
    elif vc in cs: c['vtable'] = hex(vc); stats['multi_ctor_match'] += 1
    elif cs: c['vtable'] = None; c['vtable_ambiguous'] = [hex(x) for x in cs]; stats['ambiguous'] += 1
    else: c['vtable'] = None; stats['no_vtable'] += 1
    if c['vtable']:
        st = int(c['vtable'], 16)
        c['vtable_slots'] = [hex(im.u32(st + 4 * k)) for k in range(vt_len(st))]

# vt_len stops at the first word outside .text, but data that follows a vtable
# can still look like a .text address: resource classes are followed by their
# extension string ('tex\0' = 0x00786574), others by ASCII/UTF-16/UTF-8 strings.
# Trim the run at the first slot that is (a) past slot 4 (getDTI), (b) not a
# getDTI stub, (c) used by only one vtable, and (d) unaligned (48,579 of the
# 48,633 real slots are 16-byte aligned; the other 54 are _purecall, shared by
# many vtables), or a printable word not preceded by a function boundary, or a
# resource class's 2-3 letter extension string.
def _boundary(w):
    o = im.off(w)
    return d[o - 1] in (0xCC, 0x90, 0xC3) or d[o - 3] == 0xC2 or d[o - 5] == 0xE9 or d[o - 2] == 0xEB
def _printable(w):
    return all(x == 0 or 0x20 <= x < 0x7F for x in w.to_bytes(4, 'little'))
_mult = collections.Counter(int(f, 16) for c in classes for f in set(c.get('vtable_slots') or []))
_stubs = set(stub_to_dti)
for c in classes:
    s = c.get('vtable_slots')
    if not s:
        continue
    is_res = re.match(r'r[A-Z]', c['name']) is not None
    for i, f in enumerate(s):
        w = int(f, 16)
        if i < 5 or w in _stubs or _mult[w] >= 2:
            continue
        raw = w.to_bytes(4, 'little')
        if (w % 16 or (_printable(w) and not _boundary(w))
                or (is_res and raw[3] == 0 and re.fullmatch(rb'[a-z0-9]{2,3}', raw.rstrip(b'\0')))):
            c['vtable_slots'] = s[:i]
            c['vtable_trailing'] = raw.rstrip(b'\0').decode('latin-1')
            stats['vtable_trimmed'] += 1
            break

# depth for "most-base introducer" rule
def chain(c):
    out = []
    while c:
        out.append(c); c = by_dti.get(int(c['parent'], 16))
        if len(out) > 64: break
    return out
rows = []
def add(addr, kind, ns, name, src): rows.append((addr, kind, ns, name, src))
slot_owner = {}
for c in classes:
    n = c['name']
    add(c['dti'], 'data', n, 'DTI', 'dti_ctor_call')
    add(c['dti_vtable'], 'data', n, 'DTI_vftable', 'dti_ctor_call')
    if c.get('new_instance'): add(c['new_instance'], 'function', n, 'DTI_newInstance', 'dti_vftable_slot1')
    if c.get('vtable'):
        add(c['vtable'], 'data', n, 'vftable', 'getdti_slot4')
        anc = chain(c)
        for k, f in enumerate(c['vtable_slots']):
            # owner = most-base ancestor (incl. self) whose vtable has f at slot k
            owner = n
            for a in anc[1:]:
                sl = a.get('vtable_slots')
                if sl and k < len(sl) and sl[k] == f: owner = a['name']
                elif sl is not None: break
            key = int(f, 16)
            if key not in slot_owner: slot_owner[key] = (owner, k)
for f, (owner, k) in slot_owner.items():
    nm = 'getDTI' if k == 4 else ('dtor_deleting' if k == 0 else f'vf{k:02d}')
    add(hex(f), 'function', owner, nm, 'vtable_slot')
# constructor: only when its body installs the resolved class vtable
for c in classes:
    if c.get('ctor') and c.get('vtable'):
        body = im.dis(int(c['ctor'], 16), 250, stop_ret=False)
        if any(i.mnemonic == 'mov' and i.op_str.endswith(', ' + c['vtable']) for i in body):
            add(c['ctor'], 'function', c['name'], 'ctor', 'newInstance_call_verified')
            stats['ctor_verified'] += 1
json.dump(reg, open('E:/ddda-re/notes/dti-registry.json', 'w'), indent=1)
with open('E:/ddda-re/notes/dti-labels.csv', 'w', newline='') as fh:
    w = csv.writer(fh); w.writerow(['address', 'kind', 'namespace', 'name', 'source']); w.writerows(rows)
print(dict(stats)); print('labels:', len(rows), ' distinct virtual functions named:', len(slot_owner))
u = by_name['uPlayer']; print('uPlayer vtable', u['vtable'], 'slots', len(u.get('vtable_slots', [])))
