"""Stage 2: find each DTI class's constructor and vtable.

Method A (constructor): DTI vtable slot 1 = newInstance -> allocates `size`
bytes then calls the class constructor; the constructor's last
`mov dword [reg], imm32` with imm32 in .rdata is the most-derived vtable.
Method B (getDTI): every MtObject-derived vtable has a slot whose target is
`mov eax, <thisDTI>; ret`. Find those stubs, then the .rdata words that point
at them; the vtable is the run of code pointers containing that word.
Both are recorded; agreement is the confidence signal.
"""
import json, re, struct, sys
sys.path.insert(0, __import__('os').path.dirname(__file__))
from peimg import Img
im = Img('E:/ddda-re/bin/DDDA.exe.unpacked.exe')
reg = json.load(open('E:/ddda-re/notes/dti-registry.json'))
classes = reg['classes']
by_dti = {int(c['dti'], 16): c for c in classes}
text = [s for s in im.secs if s[0] == '.text'][0]
rdata = [s for s in im.secs if s[0] == '.rdata'][0]
def in_text(v): return text[1] <= v < text[1] + text[2]
def in_rdata(v): return rdata[1] <= v < rdata[1] + rdata[2]

# --- Method B: getDTI stubs (B8 imm32 C3) returning a known DTI
stubs = {}
d = im.d
t0, tn = text[4], min(text[2], text[3])
for m in re.finditer(rb'\xB8(....)\xC3', d[t0:t0 + tn], re.S):
    v = struct.unpack('<I', m.group(1))[0]
    if v in by_dti:
        stubs.setdefault(v, []).append(text[1] + m.start())
stub_to_dti = {s: dti for dti, ss in stubs.items() for s in ss}
# .rdata words pointing at stubs
r0, rn = rdata[4], min(rdata[2], rdata[3])
vt_b = {}
for i in range(0, rn - 3, 4):
    v = struct.unpack_from('<I', d, r0 + i)[0]
    if v in stub_to_dti:
        slot_va = rdata[1] + i
        # walk back to vtable start: preceding words are code pointers
        s = slot_va
        while in_text(im.u32(s - 4)) and (s - 4) not in vt_b.get('_starts', ()):
            s -= 4
            if slot_va - s > 4 * 400: break
        vt_b.setdefault(stub_to_dti[v], []).append({'vtable': s, 'getdti_slot': (slot_va - s) // 4})

# --- Method A: newInstance -> ctor -> vtable store
def ctor_of(dti_vt):
    ni = im.u32(dti_vt + 4)
    if not in_text(ni): return None, None
    ins = im.dis(ni, 30)
    calls = [i for i in ins if i.mnemonic == 'call']
    # pattern: call getAllocator; call [edx+0x1c] (alloc); call ctor
    direct = [int(i.op_str, 16) for i in calls if i.op_str.startswith('0x')]
    direct = [c for c in direct if c != 0xCF5E20]
    return ni, (direct[0] if direct else None)

def vtable_from_ctor(ctor):
    last = None
    for i in im.dis(ctor, 200, stop_ret=False):
        if i.mnemonic == 'ret': break
        m = re.match(r'dword ptr \[(e\w\w)\], (0x[0-9a-f]+)$', i.op_str)
        if i.mnemonic == 'mov' and m and in_rdata(int(m.group(2), 16)):
            last = int(m.group(2), 16)
    return last

agree = disagree = only_a = only_b = none = 0
for c in classes:
    dti = int(c['dti'], 16)
    ni, ctor = ctor_of(int(c['dti_vtable'], 16))
    va = vtable_from_ctor(ctor) if ctor else None
    vb = [x['vtable'] for x in vt_b.get(dti, [])]
    c['new_instance'] = hex(ni) if ni else None
    c['ctor'] = hex(ctor) if ctor else None
    c['vtable_ctor'] = hex(va) if va else None
    c['vtable_getdti'] = [hex(x) for x in vb]
    c['getdti_stub'] = [hex(x) for x in stubs.get(dti, [])]
    if va and va in vb: agree += 1; c['vtable'] = hex(va)
    elif va and vb: disagree += 1
    elif va: only_a += 1; c['vtable'] = hex(va)
    elif len(vb) == 1: only_b += 1; c['vtable'] = hex(vb[0])
    else: none += 1
json.dump(reg, open('E:/ddda-re/notes/dti-registry.json', 'w'), indent=1)
print(f'getDTI stubs found for {len(stubs)} classes')
print(f'vtable: agree={agree} disagree={disagree} ctor-only={only_a} getdti-only={only_b} none={none}')
for n in ('uPlayer', 'sPawnManager', 'rModel', 'cAIFSM', 'nNetwork::Session'):
    x = [c for c in classes if c['name'] == n][0]
    print(n, x['ctor'], x['vtable_ctor'], x['vtable_getdti'])
