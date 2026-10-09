"""Harvests MtDTI reflected properties (field name, type, offset) from DDDA.exe.

Every MtObject class registers its reflected fields in vtable slot 3
(`vf03`, e.g. sMain::vf03 at 0x00DBEB70): for each field it fills a stack
record

    [+0x00] name (char*)   [+0x04] type code | flags   [+0x08] owner (this)
    [+0x0C] &this->field, or a getter function   [+0x10..] setter, count ...

and passes it to the property-list allocator 0x00CF73F0 (or by pointer to
0x004027A0), after calling the
parent class's vf03. This script follows that pattern statically with
capstone, for every class in the DTI registry, and writes

    E:\\ddda-re\\notes\\dti-props.json    {class: {parent, size, props: [...]}}

Usage:
    python dti_props.py                # scan, write the json, print stats
    python dti_props.py show uPlayer   # inherited layout of one class
Type codes are MtType (the same as XFS; crates/mtxfs).
"""
import json
import os
import sys

import capstone
from capstone import x86

sys.path.insert(0, os.path.dirname(__file__))
import peimg

EXE = 'E:/ddda-re/bin/DDDA.exe.unpacked.exe'
REG = 'E:/ddda-re/notes/dti-registry.json'
OUT = 'E:/ddda-re/notes/dti-props.json'
ALLOC = 0x00CF73F0
# the same record passed by pointer to a wrapper (uPlayer::vf03 0x00B55DE0)
ADD_PROPERTY = 0x004027A0

TYPES = {0x01: 'class', 0x02: 'classref', 0x03: 'bool', 0x04: 'u8', 0x05: 'u16', 0x06: 'u32', 0x07: 'u64',
         0x08: 's8', 0x09: 's16', 0x0A: 's32', 0x0B: 's64', 0x0C: 'f32', 0x0D: 'f64', 0x0E: 'string',
         0x0F: 'color', 0x10: 'point', 0x11: 'size', 0x12: 'rect', 0x13: 'matrix', 0x14: 'vector3',
         0x15: 'vector4', 0x16: 'quaternion', 0x17: 'property', 0x18: 'event', 0x19: 'group',
         0x1A: 'pagebegin', 0x1B: 'pageend', 0x1C: 'event32', 0x1D: 'array', 0x1E: 'propertylist',
         0x1F: 'groupend', 0x20: 'cstring', 0x21: 'time', 0x22: 'float2', 0x23: 'float3', 0x24: 'float4',
         0x25: 'float3x3', 0x26: 'float4x3', 0x27: 'float4x4', 0x28: 'easecurve', 0x29: 'line',
         0x2A: 'linesegment', 0x2B: 'ray', 0x2C: 'plane', 0x2D: 'sphere', 0x2E: 'capsule', 0x2F: 'aabb',
         0x30: 'obb', 0x31: 'cylinder', 0x32: 'triangle', 0x33: 'cone', 0x34: 'torus', 0x35: 'ellipsoid',
         0x36: 'range', 0x37: 'rangef', 0x38: 'rangeu16', 0x39: 'hermitecurve', 0x3A: 'enumlist',
         0x3B: 'float3x4', 0x3C: 'linesegment4', 0x3D: 'aabb4', 0x3E: 'oscillator', 0x3F: 'variable',
         0x40: 'vector2', 0x41: 'matrix33', 0x42: 'rect3d_xz', 0x43: 'rect3d', 0x44: 'rect3d_collision',
         0x45: 'plane_xz', 0x46: 'ray_y', 0x47: 'pointf', 0x48: 'sizef', 0x49: 'rectf', 0x80: 'resource'}

img = peimg.Img(EXE)
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
md.detail = True


def cstr(va):
    try:
        o = img.off(va)
        if o is None:
            return None
        end = img.d.index(b'\0', o, o + 128)
        s = img.d[o:end]
        if 1 <= len(s) and all(32 <= c < 127 for c in s):
            return s.decode()
    except ValueError:
        return None
    return None


def scan_vf03(fn):
    """Field records registered by one vf03."""
    o = img.off(fn)
    if o is None:
        return []
    this = {'ecx'}
    lea = {}       # reg -> offset from this
    slots = {}     # esp/ebp displacement -> value
    props = []
    for ins in md.disasm(img.d[o:o + 0x10000], fn):
        m, ops = ins.mnemonic, ins.operands
        if m == 'ret' or m == 'int3':
            break
        if m in ('mov', 'lea') and len(ops) == 2 and ops[0].type == x86.X86_OP_REG:
            dst = ins.reg_name(ops[0].reg)
            src = ops[1]
            this.discard(dst)
            lea.pop(dst, None)
            if m == 'mov' and src.type == x86.X86_OP_REG and ins.reg_name(src.reg) in this:
                this.add(dst)
            elif m == 'lea' and src.type == x86.X86_OP_MEM and src.mem.index == 0 and \
                    ins.reg_name(src.mem.base) in this:
                lea[dst] = src.mem.disp
            continue
        if m == 'mov' and len(ops) == 2 and ops[0].type == x86.X86_OP_MEM and ops[0].size == 4:
            base = ins.reg_name(ops[0].mem.base) if ops[0].mem.base else None
            if base in ('esp', 'ebp'):
                d = (base, ops[0].mem.disp)
                src = ops[1]
                if src.type == x86.X86_OP_IMM:
                    slots[d] = ('imm', src.imm & 0xFFFFFFFF)
                elif src.type == x86.X86_OP_REG:
                    r = ins.reg_name(src.reg)
                    slots[d] = ('off', lea[r]) if r in lea else ('this',) if r in this else ('reg', r)
            continue
        if m == 'call' and ops and ops[0].type == x86.X86_OP_IMM and ops[0].imm in (ALLOC, ADD_PROPERTY):
            # the record: the slot holding a name string, then its neighbours
            best = None
            for (b, disp), v in slots.items():
                if v[0] == 'imm' and (b, disp + 4) in slots and slots[(b, disp + 4)][0] == 'imm':
                    name = cstr(v[1])
                    if name and (best is None or disp < best[1]):
                        best = (b, disp, name)
            if best:
                b, disp, name = best
                code = slots[(b, disp + 4)][1]
                field = slots.get((b, disp + 12))
                extra = [slots.get((b, disp + k)) for k in (16, 20)]
                p = {'name': name, 'type': code & 0xFF, 'flags': code >> 8,
                     'type_name': TYPES.get(code & 0xFF, f'0x{code & 0xFF:02x}')}
                if field and field[0] == 'off':
                    p['offset'] = field[1]
                elif field and field[0] == 'this':
                    p['offset'] = 0
                elif field and field[0] == 'imm' and field[1]:
                    p['getter'] = field[1]
                setters = [e[1] for e in extra if e and e[0] == 'imm' and 0x401000 <= e[1] < 0x1400000]
                if setters:
                    p['setter'] = setters[0]
                props.append(p)
            for k in list(slots):  # each record is rebuilt before the next call
                slots.pop(k)
    return props


def scan():
    reg = json.load(open(REG))['classes']
    by_name = {c['name']: c for c in reg}
    out = {}
    vf03_owner = {}
    for c in reg:
        slots = c.get('vtable_slots') or []
        if len(slots) < 4:
            continue
        fn = int(slots[3], 16)
        parent = c.get('parent_name')
        pslots = (by_name.get(parent) or {}).get('vtable_slots') or []
        inherited = len(pslots) > 3 and pslots[3] == slots[3]
        props = [] if inherited else scan_vf03(fn)
        out[c['name']] = {'parent': parent, 'size': c.get('size'), 'vf03': slots[3],
                          'inherits_vf03': inherited, 'props': props}
    json.dump(out, open(OUT, 'w'), indent=1)
    n = sum(len(v['props']) for v in out.values())
    with_props = sum(1 for v in out.values() if v['props'])
    print(f'{len(out)} classes scanned, {with_props} register fields, {n} fields -> {OUT}')


def show(name):
    data = json.load(open(OUT))
    chain = []
    while name in data and name not in chain:  # some registry parents loop back
        chain.append(name)
        name = data[name]['parent']
    rows = []
    for cls in reversed(chain):
        for p in data[cls]['props']:
            rows.append((p.get('offset', -1), cls, p))
    print(' -> '.join(reversed(chain)), f'(size {data[chain[0]]["size"]:#x})')
    for off, cls, p in sorted(rows, key=lambda r: (r[0] < 0, r[0])):
        where = f'+{off:#06x}' if off >= 0 else (f'get {p["getter"]:#x}' if 'getter' in p else '   ?   ')
        print(f'  {where:>12}  {p["type_name"]:10} {p["name"]:32} [{cls}] flags {p["flags"]:#x}')


if __name__ == '__main__':
    if len(sys.argv) > 2 and sys.argv[1] == 'show':
        show(sys.argv[2])
    else:
        scan()
