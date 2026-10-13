"""L184 s7 oracle: 0x012F05C0 (_ftol2_sse: ST0 in, truncated int in EAX, pops ST0), run
as the original's code in Unicorn. A tiny trampoline `fld qword ptr [VALUE]; call ftol`
loads ST0 (a double, which the test loads with fld m64 as well) and calls it.
Output: E:/ddda-re/notes/native-L184/ftol2_sse.json {value_address, cases:[... usual format +
'value' bits and after 'depth']}. Pyenv python."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x

ENTRY = 0x012F05C0
VALUE, TRAMP = 0x20000000, 0x21000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]

def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 5000
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    _, secs = h.image()
    for va, b in secs:
        s = va & ~0xFFF
        uc.mem_map(s, ((va + len(b) - s) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    for a, s in ((VALUE, 0x1000), (TRAMP, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
        uc.mem_map(a, s)
    # fld qword ptr [VALUE]; call ENTRY; (return to RET)
    call = struct.pack('<Bi', 0xE8, ENTRY - (TRAMP + 6 + 5))
    uc.mem_write(TRAMP, bytes([0xDD, 0x05]) + struct.pack('<I', VALUE) + call + bytes([0xE9]) + struct.pack('<i', h.RET - (TRAMP + 11 + 5)))
    uc.mem_write(0x019AA078, struct.pack('<I', 1))  # the CRT's SSE2 flag, set at start-up
    initial, written = {}, set()
    def remember(a, s):
        for i, b in enumerate(uc.mem_read(a, s)):
            initial.setdefault(a + i, b)
    def rd(uc_, acc, a, s, v, d):
        if a >= STACK or a == 0x019AA078: remember(a, s)
    def wr(uc_, acc, a, s, v, d):
        if a >= STACK:
            remember(a, s); written.update(range(a, a + s))
    uc.hook_add(UC_HOOK_MEM_READ, rd)
    uc.hook_add(UC_HOOK_MEM_WRITE, wr)
    uc.hook_add(UC_HOOK_CODE, lambda u, a, s, d: u.emu_stop() if a == h.RET else None)
    rng = random.Random(5)
    bits = lambda f: struct.unpack('<Q', struct.pack('<d', f))[0]
    cases = []
    for i in range(n):
        r = rng.random()
        if r < 0.4: v = rng.uniform(-1000, 1000)
        elif r < 0.55: v = rng.uniform(-3e9, 3e9)
        elif r < 0.65: v = rng.choice((2147483647.5, 2147483647.0, 2147483648.0, -2147483648.0, -2147483648.9, -2147483649.0, 0.5, -0.5, 0.9999999999, -0.0, 0.0, 1e-300, 1e300, -1e300, 4294967296.0))
        elif r < 0.7: v = float('inf') * rng.choice((1, -1))
        elif r < 0.73: v = float('nan')
        else: v = float(rng.randrange(-70000, 70000)) + rng.choice((0, 0.25, 0.5, 0.75, 0.999))
        uc.mem_write(VALUE, struct.pack('<d', v))
        uc.mem_write(STACK, bytes(rng.randrange(256) for _ in range(0x10000)))
        initial.clear(); written.clear()
        esp = SP - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for rr, vv in zip(REGS, start): uc.reg_write(rr, vv)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        # esp at the call: trampoline's call pushes its return into esp-4; the function's
        # entry esp is therefore esp-4 with the return address RET'-continuation on top
        before = dict(regs=[uc.reg_read(r) for r in REGS])
        uc.emu_start(TRAMP, h.RET, count=1000)
        fpsw = uc.reg_read(x.UC_X86_REG_FPSW)
        after = dict(regs=[uc.reg_read(r) for r in REGS], top=(fpsw >> 11) & 7)
        cases.append(dict(value=bits(v), before=before, after=after, entry_esp=esp - 4))
    json.dump(dict(cases=cases), open(out, 'w'))
    print(n, 'cases')

main()
