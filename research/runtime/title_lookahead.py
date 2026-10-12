"""L21 look-ahead along virtual and pointer routes (read-only).

Closure from the title path's roots over (a) direct calls in the shared
inventory and (b) code addresses a reached function mentions as immediates:
a function pointer (an inventory entry in .text) or a vtable (a .rdata
address whose consecutive words are inventory entries, DTI.md's rule),
whose slots all become reachable. Each route stops at its first unverified
function. Output: those functions with the number of pointer/vtable hops
needed and the route's last verified function."""
import json, sqlite3, collections, os, struct, sys
import capstone
from capstone import x86 as X

EXE = r'E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe'
data = open(EXE, 'rb').read()
e = struct.unpack_from('<I', data, 0x3C)[0]
nsec = struct.unpack_from('<H', data, e + 6)[0]
opt = struct.unpack_from('<H', data, e + 20)[0]
base = struct.unpack_from('<I', data, e + 24 + 28)[0]
secs = {}
for i in range(nsec):
    name, vs, va, rs, rp = struct.unpack_from('<8sIIII', data, e + 24 + opt + 40 * i)
    secs[name.rstrip(b'\0').decode()] = (base + va, vs, rp, rs)

def read(va, n):
    for s, (v, vs, rp, rs) in secs.items():
        if v <= va and va + n <= v + min(vs, rs):
            return data[rp + va - v: rp + va - v + n]
    return None

text = secs['.text']
rdata = secs.get('.rdata')
def in_text(a): return text[0] <= a < text[0] + text[1]
def in_rdata(a): return rdata and rdata[0] <= a < rdata[0] + rdata[1]

c = sqlite3.connect('file:E:/ddda-re/notes/inventory/inventory.sqlite?mode=ro', uri=True)
fn = {a: (n, s, st) for a, n, s, st in c.execute('select address, name, size, status from functions')}
callees = collections.defaultdict(list)
for a, b in c.execute('select caller, callee from calls'):
    callees[a].append(b)
P = 'E:/ddda-re/notes/native/patterns'
novector = set(json.load(open(f'{P}/lift_novector.json')))

def reason(a):
    st = fn[a][2]
    if a in novector:
        return 'Codex: lifts, no fixture'
    return f'claude-6: not in the lift set (status {st})'

def vtable_slots(a):
    slots = []
    while True:
        w = read(a + 4 * len(slots), 4)
        if w is None:
            break
        t = struct.unpack('<I', w)[0]
        if t not in fn or not in_text(t):
            break
        slots.append(t)
    return slots if len(slots) >= 2 else []

md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
md.detail = True
def mentions(a):
    """Code addresses the function's instructions hold as immediates or
    displacements: function pointers and vtable slots."""
    size = fn[a][1] or 0
    body = read(a, size) if size else None
    out = []
    if not body:
        return out
    for ins in md.disasm(body, a):
        for op in ins.operands:
            v = None
            if op.type == X.X86_OP_IMM:
                v = op.imm & 0xFFFFFFFF
            elif op.type == X.X86_OP_MEM and op.mem.base == 0 and op.mem.index == 0:
                v = op.mem.disp & 0xFFFFFFFF
            if v is None:
                continue
            if v in fn and in_text(v) and not (ins.mnemonic == 'call' or ins.mnemonic.startswith('j')):
                out.append(('pointer', v))
            elif in_rdata(v):
                for k, t in enumerate(vtable_slots(v)):
                    out.append((f'vtable {v:#010x} slot {k}', t))
    return out

ROOTS = [0x004079E0, 0x00DF3AC0, 0x00DF1550, 0x00DF0D20, 0x0041B400, 0x0041DD60, 0x00DB1A50,
         0x00531800, 0x00DAC090, 0x00DBA9A0, 0x00DB9F40, 0x00DBDEF0, 0x00DBDF10, 0x00CFE6B0,
         0x0041FFE0, 0x0041FE60, 0x011A4610]
VTABLES = {0x0156CD80: 'aInitialize', 0x0156F7A8: 'aTitle'}
# (hops, via, how)
seen = {}
order = collections.deque()
def add(a, hops, via, how):
    if a not in fn:
        return
    if a not in seen or seen[a][0] > hops:
        seen[a] = (hops, via, how)
        order.append(a)
for r in ROOTS:
    add(r, 0, None, 'root')
for v, name in VTABLES.items():
    for k, t in enumerate(vtable_slots(v)):
        add(t, 0, v, f'{name} slot {k}')
limit = int(sys.argv[1]) if len(sys.argv) > 1 else 3
while order:
    a = order.popleft()
    hops, _, _ = seen[a]
    if fn[a][2] != 'verified':
        continue  # a route stops at its first unverified function
    for b in callees.get(a, []):
        add(b, hops, a, 'call')
    if hops < limit:
        for how, t in mentions(a):
            add(t, hops + 1, a, how)

rows = [(h, a, via, how) for a, (h, via, how) in seen.items() if fn[a][2] != 'verified']
rows.sort()
print(f'reached {len(seen)} functions with up to {limit} pointer/vtable hops; {len(rows)} unverified')
cnt = collections.Counter(reason(a).split(':')[0] for _, a, _, _ in rows)
print(dict(cnt))
for h, a, via, how in rows:
    n, s, st = fn[a]
    print(f'h{h} {a:#010x} {n:32.32} {s or 0:6} {reason(a):45.45} <- {via:#010x} ({how})' if via else f'h{h} {a:#010x} {n} root')
