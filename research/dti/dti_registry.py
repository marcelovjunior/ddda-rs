"""Harvest MtDTI registrations from the unpacked DDDA.exe.

Each class registers with a static-init stub:
    push a6; push a5; push a4      (imm8/imm32)
    push size; push parentDTI; push nameStr
    mov ecx, thisDTI
    call MtDTI::MtDTI (0x00CF5780)
    [push dtor; mov dword [thisDTI], dtiVtable; call atexit]
Writes JSON records: dti, name, parent, parent_name, size, args, dti_vtable.
"""
import json, struct, sys
EXE, XREFS, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
CTOR = 0x00CF5780
d = open(EXE, 'rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
nsec, optsz = struct.unpack_from('<H', d, pe + 6)[0], struct.unpack_from('<H', d, pe + 20)[0]
secs = []
for i in range(nsec):
    _, vs, va, rs, ro = struct.unpack_from('<8sIIII', d, pe + 24 + optsz + 40 * i)
    secs.append((0x400000 + va, max(vs, rs), ro))
def off(va):
    for s, n, ro in secs:
        if s <= va < s + n:
            return ro + (va - s)
    return None
def u32(va): return struct.unpack_from('<I', d, off(va))[0]
def cstr(va):
    o = off(va)
    if o is None: return None
    e = d.index(b'\0', o)
    return d[o:e].decode('latin-1') if e - o < 200 else None

def parse_site(call):
    """Walk backwards from the call collecting mov ecx + pushes."""
    if d[off(call)] != 0xE8 or call + 5 + struct.unpack_from('<i', d, off(call) + 1)[0] != CTOR:
        return None
    p = call
    ecx = None
    pushes = []
    # Try decoding a forward window that ends exactly at the call.
    for start in range(call - 40, call):
        q, ps, ec, ok = start, [], None, True
        while q < call:
            b = d[off(q)]
            if b == 0x68: ps.append(u32(q + 1)); q += 5
            elif b == 0x6A: ps.append(struct.unpack_from('<b', d, off(q) + 1)[0] & 0xFFFFFFFF); q += 2
            elif b == 0xB9: ec = u32(q + 1); q += 5
            else: ok = False; break
        if ok and q == call and ec is not None and len(ps) >= 3:
            if len(ps) > len(pushes): pushes, ecx = ps, ec
    if ecx is None: return None
    args = list(reversed(pushes))  # first arg = last pushed
    rec = {'site': hex(call), 'dti': hex(ecx), 'name': cstr(args[0]),
           'parent': hex(args[1]), 'size': args[2], 'args': [hex(a) for a in args[3:]]}
    # after the call: optional push imm32 (dtor), then C7 05 <dti> <vtbl>
    q = call + 5
    for _ in range(3):
        b = d[off(q)]
        if b == 0x68: q += 5
        elif b == 0xC7 and d[off(q) + 1] == 0x05 and u32(q + 2) == ecx:
            rec['dti_vtable'] = hex(u32(q + 6)); break
        else: break
    return rec

recs, bad = [], []
for line in open(XREFS):
    if line.startswith('From '):
        site = int(line.split()[1], 16)
        r = parse_site(site)
        (recs if r and r['name'] else bad).append(r or hex(site))
by_dti = {r['dti']: r for r in recs}
for r in recs:
    p = by_dti.get(r['parent'])
    r['parent_name'] = p['name'] if p else None
json.dump({'ctor': hex(CTOR), 'classes': recs, 'unparsed_sites': bad}, open(OUT, 'w'), indent=1)
print(f'parsed {len(recs)} classes, {len(bad)} unparsed sites, '
      f'{sum(1 for r in recs if r["parent_name"])} with resolved parent, '
      f'{sum(1 for r in recs if "dti_vtable" in r)} with DTI vtable')
