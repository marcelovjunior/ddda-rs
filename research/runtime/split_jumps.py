"""L21: verified functions that jump (jmp/jcc, not call) to the entry of another
inventory function that is not verified: Ghidra splits whose tail is not
lifted (L21 stops 11 and 17). A case is live when the jumping record's
lifted body (LIFT_DATA, default the shared data) does not cover the target.
Read-only: the exe, the inventory and lift.json."""
import sqlite3, struct, capstone, collections, json, os
data = open(r'E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe', 'rb').read()
e = struct.unpack_from('<I', data, 0x3C)[0]; n = struct.unpack_from('<H', data, e + 6)[0]
opt = struct.unpack_from('<H', data, e + 20)[0]; base = struct.unpack_from('<I', data, e + 52)[0]
secs = [struct.unpack_from('<8sIIII', data, e + 24 + opt + 40 * i) for i in range(n)]
def off(va):
    for name, vs, vaddr, rs, rp in secs:
        if vaddr <= va - base < vaddr + min(vs, rs):
            return rp + va - base - vaddr
c = sqlite3.connect('file:E:/ddda-re/notes/inventory/inventory.sqlite?mode=ro', uri=True)
fn = {a: (nm, s or 0, st) for a, nm, s, st in c.execute('select address,name,size,status from functions')}
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
hits = collections.defaultdict(list)
for a, (nm, size, st) in fn.items():
    if st != 'verified' or not size:
        continue
    o = off(a)
    if o is None:
        continue
    for ins in md.disasm(data[o:o + size], a):
        if ins.mnemonic.startswith('j') and ins.op_str.startswith('0x'):
            t = int(ins.op_str, 16)
            if t != a and t in fn and fn[t][2] != 'verified' and not (a <= t < a + size):
                hits[t].append((a, ins.address, ins.mnemonic))
lift = {f['addr']: f for f in json.load(open(os.environ.get('LIFT_DATA', 'E:/ddda-re/notes/native') + '/patterns/lift.json'))}
def covered(s, t):
    return any(a <= t < b for a, b in lift.get(s, {}).get('body_ranges', []))
print(len(hits), 'unverified function entries reached by jumps from verified functions')
for t, src in sorted(hits.items()):
    nm, size, st = fn[t]
    live = t not in lift and any(not covered(s, t) for s, _, _ in src)
    print(f'{"LIVE" if live else "ok  "} {t:#010x} {nm:28.28} {size:6} {st:12} <- '
          + ', '.join(f'{s:#010x}@{j:#010x} {m}' for s, j, m in src[:4]) + (' ...' if len(src) > 4 else ''))
