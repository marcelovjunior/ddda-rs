"""L184 s9 oracle: 0x00EA5320 `model->draw_mesh(...)` with its small callees
run as REAL original code: 0x00FED940 (view matrix address), 0x00FEEF20
(primitive state flush) and, inside it, 0x00FEE320 (commit_primitive_state,
x87 through the host oracle). Everything else is a stub with recorded
effects: the filter 0x00EA42E0, the material calls 0x00FF14D0 / 0x00FED190 /
0x00EA23F0, the arena grow 0x00FED890, the submit 0x00FF1920, the listener
callback (its 16-byte sphere argument is recorded), and fee320's own
callees (page allocator 0x00DA7D70, 0x00E132B0; the CRT memcpy it does
natively). Output: notes/native-L184/draw_mesh_real.json in the format of
research/native/record_l177.py (calls with target/regs/args/memory/effects).
Never starts the game. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import hashlib
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h
import record_l177 as base
from record_l177 import BASE, CommitOracle, ENTRIES, CALLEES, REGS, STACK, STACK_SIZE, SP
import unicorn.x86_const as x

DRAW_MESH, DRAW_MESH_LENGTH = 0xEA5320, 0x747
CALLBACK = 0x1200000
REAL = {DRAW_MESH: DRAW_MESH_LENGTH, 0xFED940: 0xD, 0xFEEF20: 0x54, 0xFEE320: 0xBF7}
# stubs: address -> number of stack arguments
STUBS = {0xDA7D70: 1, 0xE132B0: 7, 0x12EFEA0: 3, 0xEA42E0: 3, 0xFF14D0: 0, 0xFED190: 2, 0xEA23F0: 2,
         0xFED890: 1, 0xFF1920: 0, CALLBACK: 4}
for a in list(CALLEES):
    del CALLEES[a]
LATE = {0xEA42E0: 3, CALLBACK: 4}  # pages mapped after the base constructor
CALLEES.update({k: v for k, v in STUBS.items() if k not in LATE})
for a in (0xFED940, 0xFEEF20):
    ENTRIES.pop(a, None)
ENTRIES.update(REAL)


class MeshReal(CommitOracle):
    def __init__(self):
        super().__init__()
        for a, n in ((0xEA4000, 0x1000), (CALLBACK, 0x1000)):
            self.uc.mem_map(a, n)
        for a, n in LATE.items():
            CALLEES[a] = n
            self.uc.mem_write(a, bytes([0xC3]))

    def code(self, uc, a, size, data):
        self.visited.add(a)
        if a not in CALLEES:
            assert any(s <= a < s + n for s, n in REAL.items()), hex(a)
            return
        argc = CALLEES[a]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        args = [self.get(esp + 4 * i) for i in range(argc + 1)]
        self.remember(esp, 4 * (argc + 1))
        call = dict(target=a, regs=[uc.reg_read(r) for r in REGS], args=args,
                    memory=[(b, uc.mem_read(b, 1)[0]) for b in sorted(self.written)])
        if a == CALLBACK:
            self.remember(args[4], 16)
            call['buffer'] = [(args[4] + i, uc.mem_read(args[4] + i, 1)[0]) for i in range(16)]
        rng = self.rng
        effects = []

        def effect(address, value):
            self.remember(address, 4)
            self.word(address, value)
            effects.extend(enumerate(struct.pack('<I', value & 0xFFFFFFFF), address))
            self.written.update(range(address, address + 4))
        if a == 0xDA7D70:
            result = BASE + 0x140000 + (len(self.calls) % 0x60) * 0x1000
            # (a failed allocation, result 0, is covered by the commit's own vectors)
        elif a == 0xE132B0:
            result = BASE + 0x90000 if self.seed % 11 else 0
        elif a == 0x12EFEA0:
            dest, source, length = args[1:]
            self.remember(source, length)
            self.remember(dest, length)
            copied = bytes(uc.mem_read(source, length))
            uc.mem_write(dest, copied)
            effects = list(enumerate(copied, dest))
            self.written.update(range(dest, dest + length))
            result = dest
        else:
            result = rng.getrandbits(32)
            context = self.context
            if a == 0xEA42E0:
                result = 0xFFFFFFFF if rng.random() < 0.3 else 0
            elif a == 0xFED890:
                ctx = uc.reg_read(x.UC_X86_REG_ECX)
                effect(ctx + 0x24, BASE + 0x1A0000 + 0x4000 * (len(self.calls) % 4))
                effect(ctx + 0x28, BASE + 0x1A0000 + 0x4000 * (len(self.calls) % 4) + 0x3000)
            elif a == 0xFED190:
                ctx = uc.reg_read(x.UC_X86_REG_ECX)
                if rng.random() < 0.5:
                    # a material switch re-points the packet snapshots
                    for field, options in ((0x194, (0xC0000, 0xC0400)), (0x19C, (0xC0100, 0xC0500)), (0x1A0, (0xC0200, 0xC0600))):
                        if rng.random() < 0.6:
                            effect(ctx + field, BASE + rng.choice(options))
            elif a == 0xFF14D0 or a == 0xFF1920 or a == 0xEA23F0:
                if rng.random() < 0.3:
                    ctx = uc.reg_read(x.UC_X86_REG_ECX)
                    effect(ctx + 0x154, rng.getrandbits(32) & ~0x2 | rng.choice((0, 1)))
        cleanup = 4 if a == 0x12EFEA0 else 4 * (argc + 1)
        uc.reg_write(x.UC_X86_REG_EAX, result)
        uc.reg_write(x.UC_X86_REG_ECX, 0xC1A0)
        uc.reg_write(x.UC_X86_REG_EDX, 0xD1A0)
        uc.reg_write(x.UC_X86_REG_ESP, esp + cleanup)
        uc.reg_write(x.UC_X86_REG_EIP, args[0])
        call.update(effects=effects, result=result & 0xFFFFFFFF, return_esp=esp + cleanup)
        self.calls.append(call)

    def mesh_case(self, seed):
        self.active = False
        self.seed = seed
        self.rng = rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(BASE, bytes(base.SIZE))
        uc.mem_write(0, bytes(4096))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        word, get = self.word, self.get
        # ---- the draw context and its state-commit data (as record_l177.run) ----
        context = BASE + 0x20000
        self.context = context
        model_tbl, bank, descs, desc = BASE + 0x40000, BASE + 0x50000, BASE + 0x60000, BASE + 0x61000
        resource, registry, dependency, keys = BASE + 0x70000, BASE + 0x80000, BASE + 0x90000, BASE + 0x91000
        buffer = BASE + 0x100000
        word(0x18D08F4, BASE + 0xA0000)
        word(0x18D12CC, registry)
        word(context + 0x20, bank)
        word(context + 24, descs)
        word(descs + 0x568, desc)
        word(desc + 24, [5, 8, 12, 1025][(seed // 17) % 4])
        word(desc + 36, BASE + 0xB0000)
        uc.mem_write(BASE + 0xB0000, rng.randbytes(8192))
        word(context + 0x24, buffer)
        word(context + 0x28, buffer + rng.choice([0, 16, 32, 48, 80, 96, 128, 0x10000, 0x20000]))
        for i in range(4096):
            word(context + 0x1F8 + i * 8, resource + (i % 16) * 256)
            word(bank + i * 8, rng.getrandbits(32))
        for i in range(16):
            word(resource + i * 256 + 16, rng.choice([0, 1, 8, 9, 0x38, 0x39]))
            word(resource + i * 256 + 32, 0x20 if rng.randrange(2) else 0)
            word(resource + i * 256 + 36, i % 2)
            word(registry + 0x3C + i * 8, resource + i * 256)
        for i in range(4):
            word(context + 0x1BC + i * 4, (seed + i) % 16)
        for i in range(24):
            word(context + 0xB4 + i * 4, rng.getrandbits(32))
        word(context + 0xB4, resource if seed % 5 else 0)
        word(context + 0xC8, 8 if seed % 4 == 0 else 0)
        word(context + 0x110, rng.getrandbits(32))
        word(context + 0x114, ((seed % 2) << 8) | (0x1000 if seed % 3 == 0 else 0))
        for i in range(3):
            word(context + 0x118 + i * 4, resource + i * 256)
        for idx in range(4):
            word(model_tbl + 0x1C + idx * 40 + 24, 0 if (seed + idx) % 5 == 0 else (((seed + idx) % 16) << 20) | ((seed + idx + 1) % 16) | ((((seed + idx) + 2) % 16) << 10))
        for field, offset in [(0x194, 0xC0000), (0x19C, 0xC0100), (0x1A0, 0xC0200)]:
            word(context + field, BASE + offset)
        # alternative packet snapshots the material stub may switch to
        for k, (a0, a1) in enumerate(((0xC0400, 0xC0500), (0xC0600, 0xC0000))):
            pass
        for packet in (0xC0000, 0xC0100, 0xC0200, 0xC0400, 0xC0500, 0xC0600):
            word(BASE + packet + 8, resource)
            word(BASE + packet + 0x10, resource + 256)
        word(BASE + 0xC0200, resource if seed % 5 else 0)
        word(BASE + 0xC0008, resource)
        word(BASE + 0xC0010, resource + 256)
        for field, bankoff in [(0x20C0, 0x1EC8), (0x2248, 0x2050)]:
            word(context + field, get(bank + bankoff) if rng.randrange(2) else 0)
        word(context + 0x154, ((seed % 256) << 11) | (((seed // 4) % 256) << 3) | (rng.randrange(2) << 19) | (rng.randrange(2) << 20) | rng.randrange(8))
        word(context + 0x158, rng.getrandbits(32))
        word(context + 0x15C, dependency if seed % 7 else 0)
        word(dependency + 32, [0, 1, 2, 7, 16][seed % 5])
        word(dependency + 28, keys)
        for i in range(16):
            word(keys + i * 4, i)
        word(context + 0xCC8, BASE + 0xD0000 | (seed % 16))
        for off in [0, 12]:
            word(BASE + 0xD0000 + off, [0, 0x3F800000, 0x7FC12345, 0x7F800123, 1, 0x80000000][seed % 6])
        word(context + 0x17C, [0x3F123456, 0x7F800123, 1, 0x80000000][seed % 4])
        word(context + 0x198, BASE + 0xC0300 if seed % 3 else 0)
        word(BASE + 0xC0308, dependency if seed % 2 else 0)
        for f, v in ((0x161C220, 0), ):
            pass
        # ---- the flush (0xFEEF20): slot, index, flags ----
        slot = rng.randrange(4096)
        word(context + 0x1CC, slot | (rng.getrandbits(20) << 12))
        word(bank + slot * 8, model_tbl)
        word(context + 0x48, rng.getrandbits(32))
        # ---- the model, mesh and primitives (as record_l173_mesh) ----
        dm, mesh, materials, position = BASE + 0x110000, BASE + 0x120000, BASE + 0x121000, BASE + 0x122000
        prims, stream, vertices, lods, bounds = BASE + 0x123000, BASE + 0x124000, BASE + 0x125000, BASE + 0x126000, BASE + 0x127000
        listener, vtable, view_block = BASE + 0x128000, BASE + 0x129000, BASE + 0x12A000
        word(0x161C280, 0x80000000)  # the sign mask (XORPS negation) the exe holds there
        word(0x18D2CE8, rng.getrandbits(32))
        word(0x18D1D20, BASE + 0x12B000)
        word(BASE + 0x12B000 + 0x38, rng.choice((1, 2, 3)))
        word(0x18D2C80, listener)
        word(listener, vtable)
        word(vtable + 0x34, CALLBACK)
        # view matrix: ctx[0xc88] & ~15 + 0x70
        word(context + 0xC88, view_block | rng.choice((0, 1, 5, 15)))
        for i in range(16):
            word(view_block + 0x70 + 4 * i, struct.unpack('<I', struct.pack('<f', rng.choice((1.0, 0.0, -0.5, 2.25, rng.uniform(-3, 3)))))[0])
        f = lambda v: struct.unpack('<I', struct.pack('<f', v))[0]
        for off, v in {0x14C: vertices if seed % 2 else 0, 0x44: BASE + 0x130000, 0x48: 0, 0x5C: rng.choice((0, 0x10, 0x40, 0x50, 0x100, 0x150, 0x80)),
                       0x6C: seed % 7, 0x74: 0x7FFE, 0x164: rng.choice((0x10, 0xE, 3, 0x10)), 0x168: 0x10000, 0x178: 17, 0x124: 0, 0x138: 0, 0x134: 0}.items():
            word(context + off, v)
        word(context + 0x16A, rng.choice((0x3, 0x813, 0x1, 0x10, 0x800, 0x2)))
        uc.mem_write(context + 0x169, bytes([rng.choice((0xFF, 0, 1, 2, 3))]))
        for k, v in enumerate((0.25, -0.5, 0.75)):
            word(context + 0x184 + 4 * k, f(v))
        word(mesh + 0x70, prims)
        count = rng.choice((0, 1, 2, 4, 6))
        word(mesh + 0x74, count)
        word(mesh + 0x9C, vertices)
        word(mesh + 0xA0, stream if seed % 4 else 0)
        scales = [rng.choice((0.5, 1.0, 1.5, 2.0, 3.0)) for _ in range(4)]
        for row in range(4):
            for col in range(4):
                word(dm + 0xB0 + 16 * row + 4 * col, f(scales[row] if row == col else (0.125 if col == 3 else 0.0)))
        word(dm + 0xF0, lods)
        word(lods + 0xE0, 10)
        word(lods + 0xE4, 100)
        word(dm + 0x100, 0x13572468)
        word(dm + 0x104, rng.choice((0, 1)))
        word(dm + 0x108, (rng.choice((-1, 1, 2, 4)) & 15) << 9 | (3 << 25))
        word(dm + 0x10C, seed % 16)
        word(dm + 0x110, 0xFFFFFFFF)
        word(dm + 0x154, seed % 19 - 9)
        word(dm + 0x15C, 1 if seed % 7 == 0 else 0)
        word(dm + 0x16C, f(0.25))
        for k, v in enumerate((1.25, 2.5, float((seed % 5) * 50) + 0.75)):
            word(position + 4 * k, f(v))
        for k in range(8):
            word(materials + 4 * k, BASE + 0x12C000 + 64 * k)
        for k in range(count):
            prim = prims + 48 * k
            bound = bounds + 32 * k
            lod_bits = (7, 1, 2, 4, 3, 7)[k % 6]
            word(prim, 1)
            word(prim + 4, k | ((k // 2) << 12) | (lod_bits << 24))
            word(prim + 8, (1 | (4 if rng.randrange(2) else 0)) | (k << 8) | (3 << 16))
            word(prim + 16, 0x5A5A0000 + k)
            word(prim + 24, 0x101 + k)
            word(prim + 28, 0x202 + k)
            word(prim + 32, 0x303 + k)
            word(prim + 44, bound)
            for j, v in enumerate((1.25 + k, 2.5 - k, 3.75, 0.875)):
                word(bound + 16 + 4 * j, f(v))
        if rng.random() < 0.3:
            # the context already holds the first primitive's stream binding
            word(context + 0x138, 0x5A5A0000)
            word(context + 0x124, get(mesh + 0xA0))
            uc.mem_write(context + 0x134, bytes([rng.choice((3, 3, 5))]))
        if rng.random() < 0.15:
            word(position + 8, f(5e7))
        pass_arg = 0x3F if rng.randrange(3) == 0 else 2
        # a pass filter that rejects every other primitive is a stub result
        esp = SP
        self.initial, self.written, self.visited, self.calls = {}, set(), set(), []
        for r in REGS:
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ECX, dm)
        uc.reg_write(x.UC_X86_REG_ESP, SP)
        for i, v in enumerate([h.RET, context, mesh, materials, position, pass_arg, rng.getrandbits(32)]):
            word(SP + i * 4, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x7F | ([0, 0x200, 0x300][(seed // 4) % 3]) | ((seed % 4) << 10))
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80 | ((seed % 4) << 13) | ([0, 0x40, 0x8000, 0x8040][(seed // 4) % 4]))
        # the six arguments are all valid on every path
        self.remember(SP, 28)
        self.entry = DRAW_MESH
        import x87_host
        from record_l175 import effective_address
        self.host = x87_host.Oracle(effective_address)
        before = self.snapshot(False)
        self.active = True
        try:
            uc.emu_start(DRAW_MESH, h.RET, count=400000)
        except Exception:
            print('fault EIP/ESP/EDI/EBP', hex(uc.reg_read(x.UC_X86_REG_EIP)), hex(uc.reg_read(x.UC_X86_REG_ESP)), hex(uc.reg_read(x.UC_X86_REG_EDI)), hex(uc.reg_read(x.UC_X86_REG_EBP)),
                  'calls', [(hex(c['target']), list(map(hex, c['args']))) for c in self.calls], flush=True)
            raise
        self.host.flush(uc)
        assert self.host.problem is None, self.host.problem
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        self.active = False
        return dict(entry=DRAW_MESH, seed=seed, before=before, initial=sorted(self.initial.items()), after=self.snapshot(),
                    calls=self.calls, visited=sorted(self.visited), x87_oracle=base.x87_host.VERSION)


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--cases', type=int, default=1500)
    args = parser.parse_args()
    oracle = MeshReal()
    cases = []
    for seed in range(args.cases):
        try:
            cases.append(oracle.mesh_case(seed))
        except Exception:
            print('failed seed', seed, flush=True)
            raise
    out = Path('E:/ddda-re/notes/native-L184')
    out.mkdir(exist_ok=True)
    path = out / 'draw_mesh_real.json'
    path.write_text(json.dumps(dict(code_sha256=hashlib.sha256(h.code(DRAW_MESH, DRAW_MESH_LENGTH)).hexdigest(), cases=cases)))
    visited = set().union(*(set(c['visited']) for c in cases))
    print(len(cases), 'cases;', len(visited), 'instructions visited;', sum(len(c['calls']) for c in cases), 'calls')
