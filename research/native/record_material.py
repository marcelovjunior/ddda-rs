"""L176 oracles: nDraw material parameter lookup 0x01066590 and its
binding writer 0x01066B10, run as the original's code in Unicorn on
synthetic material tables.

0x01066590 `this->find(slot, key)` (thiscall, `ret 8`): the `count`
indices at `this+0x78+4*slot` (count at `this+0x88+4*slot`) name 12-byte
entries at `this+0x30` whose first word holds a 12-bit key in bits 20..31;
a linear search for up to two indices, a binary search above. The cases
cover sorted, duplicate and unsorted tables, hits and misses, key words
with high bits set, and the binary search's wrap below index 0 (it then
reads the word before the index list). 0x01066B10 `this->bind(key, value)`
looks up slot 0 and fills the found entry from the table at 0x018D12CC.

Records every initial byte read, every final written byte and the
registers before and after (crates/mtdraw/tests/material.rs compares at
the ABI level). Reads the user's own exe; never runs the game; the output
is private (E:/ddda-re/notes/native-L176).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import argparse
import hashlib
import json
import random
import struct
from pathlib import Path

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
import unicorn.x86_const as x
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h

FIND, BIND = 0x1066590, 0x1066B10
LENGTHS = {FIND: 0xA7, BIND: 0x45}
GLOBAL_TABLE = 0x18D12CC
BASE, SIZE = 0xE000000, 0x80000
STACK, STACK_SIZE, SP = 0xF000000, 0x10000, 0xF008000
OWNER, LIST, ENTRIES, BINDINGS = (BASE + v for v in (0, 0x1000, 0x10000, 0x40000))
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0x1066000, 0x1000), (GLOBAL_TABLE & ~0xFFF, 0x1000),
                              (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        for entry, length in LENGTHS.items():
            uc.mem_write(entry, h.code(entry, length))
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        if self.active:
            self.remember(address, size)
            self.written.update(range(address, address + size))

    def snapshot(self, memory=True):
        uc = self.uc
        result = dict(regs=[uc.reg_read(r) for r in REGS])
        if memory:
            result['memory'] = [(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert any(e <= address < e + n for e, n in LENGTHS.items()), hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, entry, seed, count, mode, target):
        """mode: sorted | duplicates | unsorted; target: hit | below | above | between | high"""
        uc, rng = self.uc, random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        uc.mem_write(GLOBAL_TABLE & ~0xFFF, rng.randbytes(0x1000))
        words = {}
        slot = 0 if entry == BIND else seed % 4
        words[OWNER + 0x30] = ENTRIES
        words[OWNER + 0x78 + 4 * slot] = LIST
        words[OWNER + 0x88 + 4 * slot] = count
        total = max(count, 16) + 16
        keys = sorted(rng.sample(range(1, 0xFFF), total))
        if mode == 'duplicates':
            keys = sorted(rng.choice(keys[:max(count // 2, 1)]) for _ in range(total))
        order = list(range(total))
        if mode == 'unsorted':
            rng.shuffle(order)
        chosen = sorted(rng.sample(range(total), count)) if mode != 'unsorted' else order[:count]
        for k in range(total):
            words[ENTRIES + 12 * k] = (keys[k] << 20) | rng.getrandbits(20)
        for i, e in enumerate(chosen):
            words[LIST + 4 * i] = e
        # the word before the list: an entry index, read by the wrapped search
        words[LIST - 4] = rng.randrange(total)
        present = [keys[e] for e in chosen]
        if target == 'hit' and present:
            key = rng.choice(present)
        elif target == 'below':
            key = rng.randrange(0, min(keys))
        elif target == 'above':
            key = rng.randrange(max(keys) + 1, 0x1000)
        elif target == 'between' and present:
            gaps = [k for k in range(min(present), max(present)) if k not in set(present)]
            key = rng.choice(gaps) if gaps else rng.randrange(0x1000)
        else:
            key = rng.randrange(0x1000)
        key |= rng.getrandbits(20) << 12 if target == 'high' or seed % 3 == 0 else 0
        if entry == FIND:
            stack = [h.RET, slot, key]
        else:
            stack = [h.RET, key, rng.getrandbits(32)]
            words[GLOBAL_TABLE] = BINDINGS
        for i, v in enumerate(stack):
            words[SP + 4 * i] = v
        for a, v in words.items():
            uc.mem_write(a, struct.pack('<I', v))
        regs = [rng.getrandbits(32), OWNER, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for r, v in zip(REGS, regs):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.edges, self.last = {}, set(), set(), None
        before = self.snapshot(False)
        self.active = True
        try:
            uc.emu_start(entry, h.RET, count=20000)
        except UcError:
            self.active = False
            return None  # the wrapped search left the synthetic memory
        self.active = False
        if uc.reg_read(x.UC_X86_REG_EIP) != h.RET:
            return None  # the wrapped search did not end within the limit
        return dict(entry=entry, seed=seed, count=count, mode=mode, target=target, before=before,
                    initial=sorted(self.initial.items()), events=[], after=self.snapshot(),
                    edges=sorted(self.edges))


APPLY, APPLY_END, APPLY_CALLEE = 0x10655A0, 0x10658A2, 0x1062F20
SERIAL, THRESHOLD, SCALE = 0x18D2CE8, 0x1406CE0, 0x161C298
CONTEXT, SOURCE, MATERIAL = BASE + 0x10000, BASE + 0x8000, BASE + 0x100
LENGTHS[APPLY] = APPLY_END - APPLY


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def component(rng, threshold):
    """A colour component: the threshold itself, around it, NaN,
    infinities, denormals and zeros, values the conversion overflows."""
    t = struct.unpack('<f', struct.pack('<I', threshold))[0]
    pick = rng.randrange(10)
    if pick == 0:
        return threshold
    if pick == 1:
        return f32(rng.choice([float('inf'), float('-inf')]))
    if pick == 2:
        return 0x7FC00000 | rng.getrandbits(22) | (rng.getrandbits(1) << 31)
    if pick == 3:
        return rng.getrandbits(23) | (rng.getrandbits(1) << 31)
    if pick == 4:
        return f32(rng.uniform(-3e9, 3e9))
    return f32(rng.uniform(-0.5, 1.5) * t)


class StateOracle(Oracle):
    """0x010655A0 `this->apply(context, value)` (thiscall, `ret 8`): the
    material's states into the draw context's caches, then 0x01062F20
    (a guest call: stubbed, the stub records what it is given and returns
    a random EAX)."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0x1065000, 0x1000), (APPLY_CALLEE & ~0xFFF, 0x1000),
                              (SERIAL & ~0xFFF, 0x1000), (THRESHOLD & ~0xFFF, 0x1000),
                              (SCALE & ~0xFFF, 0x1000), (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(APPLY, h.code(APPLY, APPLY_END - APPLY))
        uc.mem_write(APPLY_CALLEE, bytes.fromhex('c20400'))  # the stub's own effect: ret 4
        for page in (THRESHOLD & ~0xFFF, SCALE & ~0xFFF):  # the float constants, as the image has them
            uc.mem_write(page, h.code(page, 0x1000))
        self.threshold = struct.unpack('<I', h.code(THRESHOLD, 4))[0]
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def snapshot(self, memory=True):
        result = super().snapshot(memory)
        result['xmm'] = [(self.uc.reg_read(x.UC_X86_REG_XMM0 + i)) & 0xFFFFFFFF for i in range(3)]
        return result

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address == APPLY_CALLEE:
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp, 8)
            self.events.append(dict(kind='call', target=address, **self.snapshot()))
            resume = struct.unpack('<I', uc.mem_read(sp, 4))[0]
            for reg in REGS[:3]:
                uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 8)  # thiscall, one argument
            self.events.append(dict(kind='return', **self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            return
        assert APPLY <= address < APPLY_END, hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, seed, warm, pending, records, count):
        """warm: None (the context's caches random) or the cache fields to
        change after a first, untraced run (every other cache then already
        holds what the material sets); records: whether bit 0x80 of the
        context's +0x5C is set; count: the material's source entries."""
        uc = self.uc
        rng = self.rng = random.Random(seed)
        self.active = False
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        uc.mem_write(SERIAL & ~0xFFF, rng.randbytes(0x1000))
        this, ctx = MATERIAL, CONTEXT

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def byte(a, v):
            uc.mem_write(a, bytes([v & 0xFF]))

        def read(a):
            return struct.unpack('<I', uc.mem_read(a, 4))[0]

        value = rng.choice([rng.getrandbits(8), rng.getrandbits(32)])
        index = rng.getrandbits(12) & (0x3F if seed % 2 else 0xFFF)
        word(this + 0x2C, rng.getrandbits(20) << 12 | index)
        for k in range(4):
            word(this + 0x40 + 4 * k, component(rng, self.threshold))
        word(this + 0x18, (rng.getrandbits(23) << 9) | count)
        word(this + 0x30, SOURCE)
        for k in range(count):
            word(SOURCE + 12 * k, (rng.randrange(0x400) << 20) | rng.getrandbits(20))
        shader, blend, mode, extra = read(this + 0x10), read(this + 0x28), read(this + 0x14) & 0xFF, read(this + 0x24)
        if warm is not None:
            # Run once untraced: the context's caches then hold exactly what
            # the material sets; `warm` names the fields to change after.
            word(ctx + 0x1C30, 0)  # no pending record: the warm run only fills the caches
            self.events, self.edges, self.last = [], set(), None  # (not the previous case's)
            for i, v in enumerate((h.RET, ctx, value)):
                word(SP + 4 * i, v)
            uc.reg_write(x.UC_X86_REG_ESP, SP)
            uc.reg_write(x.UC_X86_REG_ECX, this)
            uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
            uc.emu_start(APPLY, h.RET, count=100000)
            fields = {'shader': (0x1CC, 4, 1), 'value': (0x169, 1, 1), 'tag': (0x1D0, 4, 1),
                      'blend': (0x11C, 4, 1), 'mode': (0x114, 1, 1), 'texture': (0x118, 4, 1),
                      'colour': (0x110, 4, 1 << rng.randrange(32)), 'extra': (0x120, 4, 1),
                      'flag20': (0x154, 4, 1 << 20), 'nibble': (0x154, 4, 1 << 3),
                      'high': (0x154, 4, 1 << 11), 'flag19': (0x154, 4, 1 << 19)}
            for name in warm:
                offset, size, bit = fields[name]
                old = int.from_bytes(uc.mem_read(ctx + offset, size), 'little')
                uc.mem_write(ctx + offset, (old ^ bit).to_bytes(size, 'little'))
        word(ctx + 0x1C30, pending)
        word(ctx + 0x1C34, rng.choice([0, rng.getrandbits(32)]))
        byte(ctx + 0x5C, (rng.getrandbits(8) | 0x80) if records else rng.getrandbits(7))
        word(ctx + 0x6C, rng.randrange(64))
        for i, v in enumerate((h.RET, ctx, value)):
            word(SP + 4 * i, v)
        regs = [rng.getrandbits(32), this, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(APPLY, h.RET, count=100000)
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=APPLY, seed=seed, warm=warm, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))


SET_TARGET, SET_TARGET_END = 0xFEFB90, 0xFF009E
ALLOCATE, MEMCPY, ALLOCATOR = 0xDA7D70, 0x12EFEA0, 0x18D08F4
SLOT_TABLE, ONE_SCALE_PAGE, SIGN_PAGE = 0x1447B7C, 0x1406000, 0x161C000
LENGTHS[SET_TARGET] = SET_TARGET_END - SET_TARGET
TARGET, DEPTH, CHAIN, OWNER2, PASS_OWNER, PASS = (BASE + v for v in (0x100, 0x200, 0x300, 0x400, 0x500, 0x600))
TEMPLATE, STATE_TABLE, PREVIOUS, ARENA, FRESH = (BASE + v for v in (0x2000, 0x3000, 0x30000, 0x40000, 0x60000))


class TargetOracle(Oracle):
    """0x00FEFB90 `context->set_target(target, depth, flags, unused)`
    (thiscall, `ret 0x10`). 0x00DA7D70 (the arena allocation) is stubbed
    as a recorded guest call; memcpy (0x012EFEA0, a platform model in the
    runtime) is done by the stub itself, not recorded as a call."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        image_pages = (ONE_SCALE_PAGE, SIGN_PAGE, SLOT_TABLE & ~0xFFF)
        for address, size in ((0xFEF000, 0x2000), (ALLOCATE & ~0xFFF, 0x1000), (MEMCPY & ~0xFFF, 0x1000),
                              (SERIAL & ~0xFFF, 0x1000), (ALLOCATOR & ~0xFFF, 0x1000),
                              *((p, 0x1000) for p in image_pages),
                              (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(SET_TARGET, h.code(SET_TARGET, SET_TARGET_END - SET_TARGET))
        uc.mem_write(ALLOCATE, bytes.fromhex('c20400'))  # ret 4 (the stub's own effect)
        uc.mem_write(MEMCPY, bytes.fromhex('c3'))
        for page in image_pages:  # constants and the slot table, as the image has them
            uc.mem_write(page, h.code(page, 0x1000))
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        sp = uc.reg_read(x.UC_X86_REG_ESP)
        if address == ALLOCATE:
            self.remember(sp, 8)
            self.events.append(dict(kind='call', target=address, **self.snapshot()))
            resume = struct.unpack('<I', uc.mem_read(sp, 4))[0]
            uc.reg_write(x.UC_X86_REG_EAX, self.arena)
            for reg in REGS[1:3]:
                uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 8)
            self.events.append(dict(kind='return', **self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            return
        if address == MEMCPY:
            self.remember(sp, 16)
            resume, dst, src, n = struct.unpack('<4I', uc.mem_read(sp, 16))
            self.remember(src, n)
            data = bytes(uc.mem_read(src, n))
            uc.mem_write(dst, data)
            self.written.update(range(dst, dst + n))
            uc.reg_write(x.UC_X86_REG_EAX, dst)
            uc.reg_write(x.UC_X86_REG_ESP, sp + 4)
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            return
        assert SET_TARGET <= address < SET_TARGET_END, hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, seed, kind):
        """kind: 'same' (the context already has them), 'resize', 'keep'
        (the size unchanged), with arena and target variations by seed."""
        uc = self.uc
        rng = self.rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        uc.mem_write(SERIAL & ~0xFFF, rng.randbytes(0x1000))
        uc.mem_write(ALLOCATOR & ~0xFFF, rng.randbytes(0x1000))
        ctx = CONTEXT

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def half(a, v):
            uc.mem_write(a, struct.pack('<H', v & 0xFFFF))

        target = rng.choice([0, TARGET, TARGET, TARGET])
        depth = rng.choice([0, DEPTH, DEPTH])
        flags = rng.choice([rng.getrandbits(12), rng.getrandbits(32), 1, 0x40, 0x43, 0])
        tw, th = rng.choice([(rng.randrange(1, 4096), rng.randrange(1, 4096)), (0, 0), (rng.getrandbits(16), rng.getrandbits(15))])
        word(TARGET + 0x18, tw | th << 16 | (rng.getrandbits(1) << 31))
        word(TARGET + 0x20, rng.getrandbits(32))
        word(DEPTH + 0x18, rng.getrandbits(32))
        word(DEPTH + 0x10, CHAIN)
        word(CHAIN + 0x44, CHAIN + 0x10)
        word(CHAIN + 0x10, rng.choice([0, OWNER2]))
        word(ctx + 0x18, PASS_OWNER)
        word(PASS_OWNER + 0x54C, PASS)
        arena = rng.randrange(4)
        # a null block (no arena, or a failed allocation) only with nothing
        # to copy: the original's memcpy to address 0 would fault
        n = rng.choice([0, 1, 4, 16, 64]) if arena < 2 else 0
        half(PASS + 0x18, n)
        word(PASS + 0x24, TEMPLATE)
        word(ctx + 0x20, STATE_TABLE)
        word(ctx + 0xC90, PREVIOUS)
        size = (n * 4 + 0xF) & ~0xF
        if arena == 0:  # room left
            word(ctx + 0x24, ARENA)
            word(ctx + 0x28, ARENA + 0x10000)
        elif arena == 1:  # exhausted: a new arena
            word(ctx + 0x24, ARENA)
            word(ctx + 0x28, ARENA + size - 4 if size else ARENA - 1)
        elif arena == 2:  # no arena at all: the block is null
            word(ctx + 0x24, 0)
            word(ctx + 0x28, 0x10000)
        else:  # exhausted (even for an empty block), and the allocation fails
            word(ctx + 0x24, ARENA)
            word(ctx + 0x28, ARENA - 1)
        self.arena = FRESH if arena == 1 else 0
        for k in range(8):  # the two rectangles: small, zero-sized or wide
            word(ctx + 0xCC + 4 * k, rng.choice([rng.randrange(-64, 2048), 0, rng.getrandbits(32)]))
        if kind == 'keep':
            half(ctx + 0x10C, tw if target else (rng.getrandbits(16) if depth else 1))
            half(ctx + 0x10E, th if target else (rng.getrandbits(16) if depth else 1))
            if seed % 7 == 0:  # the same width, another height
                half(ctx + 0x10E, rng.getrandbits(16))
            if not target and depth:  # the depth surface's size
                word(DEPTH + 0x18, rng.getrandbits(16) << 16 | struct.unpack('<H', uc.mem_read(ctx + 0x10C, 2))[0])
                uc.mem_write(DEPTH + 0x1A, bytes(uc.mem_read(ctx + 0x10E, 2)))
        if kind == 'same':
            word(ctx + 0xB4, target)
            word(ctx + 0xC4, depth)
            state = rng.getrandbits(32)
            word(ctx + 0xC8, (state & ~0xFFF7) | ((flags & 0xFFF) << 4) | rng.choice([1, 1, 1, 9, 3]))
            if seed % 5 == 0:
                flags |= 0x1000  # bits above the twelve compared: no longer the same
        elif seed % 3 == 0:
            word(ctx + 0xB4, target)
        if seed % 4 == 0:
            word(ctx + 0x170, (struct.unpack('<I', uc.mem_read(TARGET + 0x20, 4))[0] >> 20) & 0xF if target else 0xD)
        for i, v in enumerate((h.RET, target, depth, flags, rng.getrandbits(32))):
            word(SP + 4 * i, v)
        regs = [rng.getrandbits(32), ctx, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(SET_TARGET, h.RET, count=200000)
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=SET_TARGET, seed=seed, kind=kind, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))


SORT, SORT_END = 0xFEDD60, 0xFEE0EF
LENGTHS[SORT] = SORT_END - SORT
RECORDS, SCRATCH = BASE + 0x1000, BASE + 0x40000


class SortOracle(TargetOracle):
    """0x00FEDD60 `sort(records, count, scratch) -> sorted` (stdcall, `ret
    0xc`); memcpy (0x012EFEA0, a platform model) is done by the stub."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0xFED000, 0x2000), (MEMCPY & ~0xFFF, 0x1000),
                              (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(SORT, h.code(SORT, SORT_END - SORT))
        uc.mem_write(MEMCPY, bytes.fromhex('c3'))
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address == MEMCPY:
            return super().code(uc, address, size, data)
        assert SORT <= address < SORT_END, hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, seed, count, keys, overlap):
        """keys: the key range (duplicates when small); overlap: the
        scratch overlapping the records by that many records (0: apart)"""
        uc = self.uc
        rng = self.rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        for k in range(count if count < 0x10000 else 0):
            uc.mem_write(RECORDS + 8 * k, struct.pack('<II', rng.randrange(keys) if keys else rng.getrandbits(32), k))
        scratch = RECORDS + 8 * overlap if overlap else SCRATCH
        for i, v in enumerate((h.RET, RECORDS, count & 0xFFFFFFFF, scratch)):
            uc.mem_write(SP + 4 * i, struct.pack('<I', v))
        regs = [rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(SORT, h.RET, count=20_000_000)
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=SORT, seed=seed, count=count, overlap=overlap, before=before,
                    initial=sorted(self.initial.items()), events=self.events, after=self.snapshot(),
                    edges=sorted(self.edges))


def record_sort(out):
    oracle = SortOracle()
    plan = [(count, keys, 0) for count in (-5, 0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 47, 63, 64, 65,
                                          100, 127, 128, 129, 200, 255, 256, 257, 511, 513, 1000)
            for keys in (0, 4, 1 << 16) for _ in range(3)]
    plan += [(count, 8, overlap) for count in (5, 16, 40, 70) for overlap in (1, 3, 16)]
    # huge counts: the width stops after 32 doublings (it wraps to 0, below 2)
    plan += [(0x7FFFFFFF, 0, 0), (0x80000000, 0, 0)]
    cases = [oracle.run(i + 1, *p) for i, p in enumerate(plan)]
    edges = {tuple(e) for c in cases for e in c['edges']}
    missing = [(hex(i.address), hex(t)) for i in branches(SORT)
               for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
    # Unreachable: a width of 2 or more means at least two records, so no
    # pass sees a non-positive count, no run is empty, and a run's merge
    # always ends through one of its tail copies (one record is written
    # per step and one run is exhausted before the last).
    assert missing == [('0xfedd97', '0xfeddf1'), ('0xfede41', '0xfee0b4'), ('0xfedeba', '0xfedf5b'),
                       ('0xfedf13', '0xfedf15'), ('0xfedf19', '0xfedf57'), ('0xfedf8a', '0xfee0b4'),
                       ('0xfedffc', '0xfee08b'), ('0xfee053', '0xfee055')], missing
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        uncovered_edges=missing, cases=cases), separators=(',', ':')))
    print(f'{len(cases)} sort cases -> {out}')


CULL_SPHERE, CULL_ORIENTED, CULL_BOX, COLOUR, CULL_BOX_ALL = 0xFF0780, 0xFF0AD0, 0xFF0D20, 0x1067790, 0xFF0E60
LENGTHS.update({CULL_SPHERE: 0x7F, CULL_ORIENTED: 0x23F, CULL_BOX: 0x131, COLOUR: 0x93, CULL_BOX_ALL: 0x129})
PLANES, SHAPE, COLOUR_ENTRIES = BASE + 0x2000, BASE + 0x3000, BASE + 0x5000


def any_float(rng):
    """Mostly ordinary values, sometimes zero, infinity, NaN or a denormal."""
    pick = rng.randrange(20)
    if pick == 0:
        return rng.choice([0, 0x80000000])
    if pick == 1:
        return f32(rng.choice([float('inf'), float('-inf')]))
    if pick == 2:
        return 0x7FC00000 | rng.getrandbits(22)
    if pick == 3:
        return rng.getrandbits(23)
    return f32(rng.uniform(-50.0, 50.0))


class CullOracle(TargetOracle):
    """The frustum tests 0x00FF0780 (sphere), 0x00FF0AD0 (transformed
    box), 0x00FF0D20 (box) and nDraw::Material::vf09 0x01067790 (the
    colour parameter, through the real lookup 0x01066590)."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        image_pages = (0xFF0000, 0x1066000, 0x1067000, 0x161C000, 0x1406000, 0x1629000)
        for address in image_pages:
            uc.mem_map(address, 0x1000)
            uc.mem_write(address, h.code(address, 0x1000))
        for address, size in ((BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert any(e <= address < e + n for e, n in LENGTHS.items()), hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, entry, seed):
        uc = self.uc
        rng = self.rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        ctx = CONTEXT

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        word(ctx + 0xCA0, PLANES | rng.getrandbits(4))
        exact = seed % 4 == 0  # integer planes and shapes: equalities happen exactly
        for k in range(24):  # six planes (a, b, c, d); normals mostly modest
            if exact:
                word(PLANES + 4 * k, f32(float(rng.choice([0, 0, 1, -1]) if k % 4 != 3 else rng.randrange(-8, 9))))
            else:
                word(PLANES + 4 * k, any_float(rng) if rng.randrange(8) == 0 else f32(rng.uniform(-1.5, 1.5) * (10 if k % 4 == 3 else 1)))
        args = []
        if entry == CULL_SPHERE and exact:
            for k in range(4):
                word(SHAPE + 4 * k, f32(float(rng.randrange(-6, 7) if k < 3 else rng.randrange(0, 8))))
            args = [SHAPE]
        elif entry in (CULL_BOX, CULL_BOX_ALL) and exact:
            for k in range(3):
                lo = rng.randrange(-8, 8)
                word(SHAPE + 4 * k, f32(float(lo)))
                word(SHAPE + 0x10 + 4 * k, f32(float(lo + 2 * rng.randrange(0, 5))))
            args = [SHAPE, rng.choice([0, rng.getrandbits(6)])]
        elif entry == CULL_ORIENTED and exact:
            for k in range(19):  # an axis-aligned transform with integer translation and extents
                v = 1.0 if k in (0, 5, 10, 15) else (float(rng.randrange(-6, 7)) if k in (12, 13, 14, 16, 17, 18) else 0.0)
                word(SHAPE + 4 * k, f32(abs(v) if k >= 16 else v))
            args = [SHAPE, rng.choice([0, rng.getrandbits(6)])]
        elif entry == CULL_SPHERE:
            for k in range(4):
                word(SHAPE + 4 * k, f32(rng.uniform(-20, 20)) if rng.randrange(6) else any_float(rng))
            if rng.randrange(3) == 0:
                word(SHAPE + 12, f32(rng.uniform(0, 200)))  # a large radius: wholly in front
            args = [SHAPE]
        elif entry in (CULL_BOX, CULL_BOX_ALL):
            for k in range(3):
                lo = rng.uniform(-30, 30)
                word(SHAPE + 4 * k, f32(lo) if rng.randrange(8) else any_float(rng))
                word(SHAPE + 0x10 + 4 * k, f32(lo + rng.uniform(0, 40)))
            args = [SHAPE, rng.choice([0, rng.getrandbits(6), 0x3F, rng.getrandbits(32)])]
        elif entry == CULL_ORIENTED:
            for k in range(19):
                word(SHAPE + 4 * k, f32(rng.uniform(-2, 2) * (10 if k in (3, 7, 11) else 1)) if rng.randrange(10) else any_float(rng))
            args = [SHAPE, rng.choice([0, rng.getrandbits(6), 0x3F, rng.getrandbits(32)])]
        else:  # the colour: a material with a slot 1 lookup table
            material = ctx
            count = rng.choice([0, 1, 2, 3, 7])
            word(material + 0x30, COLOUR_ENTRIES)
            word(material + 0x7C, LIST)
            word(material + 0x8C, count)
            keys = sorted(rng.sample(range(0x1000), 8))
            if rng.randrange(3) and count:
                keys[rng.randrange(count)] = 0x1EE
                keys = sorted(keys)
            for k in range(8):
                word(COLOUR_ENTRIES + 12 * k, keys[k] << 20 | rng.getrandbits(20))
                word(COLOUR_ENTRIES + 12 * k + 4, rng.choice([0, SHAPE | rng.getrandbits(4), SHAPE]))
                word(LIST + 4 * k, k)
            # (no signalling NaNs: Unicorn does not quiet them on x87 fld, the
            # hardware does; crates/mtdraw's material tests check that
            # against the CPU itself)
            for k in range(4):
                word(SHAPE + 4 * k, rng.choice([any_float(rng), 0x7FC00000 | rng.getrandbits(22), f32(rng.uniform(0, 1))]))
            args = [BASE + 0x6000]
        for i, v in enumerate([h.RET] + args):
            word(SP + 4 * i, v)
        regs = [rng.getrandbits(32), ctx, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        for i in range(8):
            uc.reg_write(x.UC_X86_REG_XMM0 + i, rng.getrandbits(128))
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        uc.emu_start(entry, h.RET, count=100000)
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=entry, seed=seed, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))


def record_cull(out):
    oracle = CullOracle()
    cases = [oracle.run(entry, i + 1) for i, entry in enumerate(
        e for e in (CULL_SPHERE, CULL_ORIENTED, CULL_BOX, COLOUR, CULL_BOX_ALL) for _ in range(500))]
    for entry in (CULL_SPHERE, CULL_ORIENTED, CULL_BOX, COLOUR, CULL_BOX_ALL):
        edges = {tuple(e) for c in cases if c['entry'] == entry for e in c['edges']}
        missing = [(hex(i.address), hex(t)) for i in branches(entry)
                   for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
        assert not missing, (hex(entry), missing)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} cull/colour cases -> {out}')


UPDATE, UPDATE_END, REFILL = 0x1062F20, 0x106302B, 0xFED890
LENGTHS[UPDATE] = UPDATE_END - UPDATE
GROUP_TABLE, KEYS, PASSES, BLOCKS = (BASE + v for v in (0x500, 0x800, 0x9000, 0x20000))


class UpdateOracle(TargetOracle):
    """0x01062F20 `this->update(context)` (thiscall, `ret 4`). The arena
    refill 0x00FED890 is a recorded guest call whose stub sets the new
    arena (its writes recorded as the call's effects); memcpy is done by
    the stub."""

    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.ctl_set_tcg_buffer_size(16 * 1024 * 1024)
        for address, size in ((0x1062000, 0x2000), (REFILL & ~0xFFF, 0x1000), (MEMCPY & ~0xFFF, 0x1000),
                              (BASE, SIZE), (STACK, STACK_SIZE), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(UPDATE, h.code(UPDATE, UPDATE_END - UPDATE))
        uc.mem_write(REFILL, bytes.fromhex('c20400'))
        uc.mem_write(MEMCPY, bytes.fromhex('c3'))
        self.active = False
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address == MEMCPY:
            return super().code(uc, address, size, data)
        if address == REFILL:
            sp = uc.reg_read(x.UC_X86_REG_ESP)
            self.remember(sp, 8)
            self.events.append(dict(kind='call', target=address, **self.snapshot()))
            resume, need = struct.unpack('<II', uc.mem_read(sp, 8))
            ctx = uc.reg_read(x.UC_X86_REG_ECX)
            arena = self.rng.choice([BLOCKS + 0x8000 * len(self.events), BLOCKS + 0x8000 * len(self.events), 0])
            limit = (arena + ((need + 0xFFF) & ~0xFFF)) if arena else 0
            effects = []
            for a, v in ((ctx + 0x24, arena), (ctx + 0x28, limit)):
                b = struct.pack('<I', v)
                self.remember(a, 4)
                uc.mem_write(a, b)
                self.written.update(range(a, a + 4))
                effects += [(a + i, b[i]) for i in range(4)]
            for reg in REGS[:3]:
                uc.reg_write(reg, self.rng.getrandbits(32))
            uc.reg_write(x.UC_X86_REG_ESP, sp + 8)
            self.events.append(dict(kind='return', effects=effects, **self.snapshot()))
            uc.reg_write(x.UC_X86_REG_EIP, resume)
            return
        assert UPDATE <= address < UPDATE_END, hex(address)
        if self.last is not None:
            self.edges.add((self.last, address))
        self.last = address

    def run(self, seed):
        uc = self.uc
        rng = self.rng = random.Random(seed)
        uc.mem_write(BASE, rng.randbytes(SIZE))
        uc.mem_write(STACK, rng.randbytes(STACK_SIZE))
        this, ctx = MATERIAL, CONTEXT

        def word(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        word(this + 0x74, BASE + 0x400)
        word(BASE + 0x410, GROUP_TABLE)
        for s in range(4):
            uc.mem_write(this + 0x50 + 8 * s, struct.pack('<H', rng.choice([rng.randrange(4), 0xFFFF, 0x8000, rng.randrange(4)])))
        for g in range(4):
            group = BASE + 0x600 + 0x40 * g
            word(GROUP_TABLE + 4 * g + 4, group)
            count = rng.choice([0, 1, 2, 5])
            word(group + 4, count << 18 | rng.getrandbits(18))
            word(group + 8, KEYS + 0x40 * g)
            for k in range(count):
                word(KEYS + 0x40 * g + 4 * k, rng.getrandbits(20) << 12 | rng.randrange(0x40))
        word(ctx + 0x18, PASSES)
        for key in range(0x40):
            word(PASSES + 4 * key, PASSES + 0x400 + 0x20 * key)
            uc.mem_write(PASSES + 0x400 + 0x20 * key + 0x18, struct.pack('<H', rng.choice([0, 1, 4, 16])))
            word(ctx + 0x1F8 + 8 * key, BASE + 0x50000 + 0x100 * key)
        roomy = rng.randrange(3)
        word(ctx + 0x24, BLOCKS)
        word(ctx + 0x28, BLOCKS + (0x10000 if roomy else rng.randrange(0x80)))
        for i, v in enumerate((h.RET, ctx)):
            word(SP + 4 * i, v)
        regs = [rng.getrandbits(32), this, rng.getrandbits(32), rng.getrandbits(32), SP,
                rng.getrandbits(32), rng.getrandbits(32), rng.getrandbits(32)]
        for reg, v in zip(REGS, regs):
            uc.reg_write(reg, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202 | (rng.getrandbits(12) & 0xCD5))
        self.initial, self.written, self.events, self.edges, self.last = {}, set(), [], set(), None
        before = self.snapshot(False)
        self.active = True
        try:
            uc.emu_start(UPDATE, h.RET, count=200000)
        except UcError:
            self.active = False
            return None  # a failed refill (null arena) and a copy into address 0
        self.active = False
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET
        return dict(entry=UPDATE, seed=seed, before=before, initial=sorted(self.initial.items()),
                    events=self.events, after=self.snapshot(), edges=sorted(self.edges))


def record_update(out):
    oracle = UpdateOracle()
    cases = [c for c in (oracle.run(i + 1) for i in range(1500)) if c is not None]
    edges = {tuple(e) for c in cases for e in c['edges']}
    missing = [(hex(i.address), hex(t)) for i in branches(UPDATE)
               for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
    assert not missing, missing
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} update cases, {sum(len(c["events"]) for c in cases)} call boundaries -> {out}')


def record_target(out):
    oracle = TargetOracle()
    cases = [oracle.run(i + 1, kind) for i, kind in enumerate(
        kind for kind in ('same', 'resize', 'keep') for _ in range(400))]
    edges = {tuple(e) for c in cases for e in c['edges']}
    missing = [(hex(i.address), hex(t)) for i in branches(SET_TARGET)
               for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
    # The slot table (constant, from the image) never starts with a zero
    # word, so neither copy loop is ever skipped.
    assert missing == [('0xfefcac', '0xfefd18'), ('0xfefce3', '0xfefd18')], missing
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} target cases, {sum(len(c["events"]) for c in cases)} call boundaries -> {out}')


def branches(entry):
    return [i for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(h.code(entry, LENGTHS[entry]), entry)
            if i.mnemonic.startswith('j') and i.mnemonic != 'jmp']


def record_state(out):
    oracle = StateOracle()
    warms = [None, [], ['shader'], ['value'], ['tag'], ['blend'], ['mode'], ['texture'], ['colour'],
             ['extra'], ['flag20'], ['nibble'], ['high'], ['flag19'], ['shader', 'colour', 'flag19']]
    plan = [(warm, pending, records, count) for warm in warms for pending in (0, 1, 0xDEAD)
            for records in (False, True) for count in (0, 1, 7, 64) for _ in range(3)]
    cases = [oracle.run(i + 1, *p) for i, p in enumerate(plan)]
    edges = {tuple(e) for c in cases for e in c['edges']}
    missing = [(hex(i.address), hex(t)) for i in branches(APPLY)
               for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
    assert not missing, missing
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} state cases, {sum(len(c["events"]) for c in cases)} call boundaries -> {out}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', default='E:/ddda-re/notes/native-L176/material_lookup.json')
    p.add_argument('--state', action='store_true', help='record 0x010655A0 (material_state.json beside --out)')
    p.add_argument('--target', action='store_true', help='record 0x00FEFB90 (set_target.json beside --out)')
    p.add_argument('--sort', action='store_true', help='record 0x00FEDD60 (record_sort.json beside --out)')
    p.add_argument('--cull', action='store_true', help='record the frustum tests and the colour (cull.json beside --out)')
    p.add_argument('--update', action='store_true', help='record 0x01062F20 (material_update.json beside --out)')
    a = p.parse_args()
    if a.state:
        record_state(Path(a.out).with_name('material_state.json'))
        return
    if a.target:
        record_target(Path(a.out).with_name('set_target.json'))
        return
    if a.sort:
        record_sort(Path(a.out).with_name('record_sort.json'))
        return
    if a.cull:
        record_cull(Path(a.out).with_name('cull.json'))
        return
    if a.update:
        record_update(Path(a.out).with_name('material_update.json'))
        return
    oracle = Oracle()
    cases, skipped, seed = [], 0, 0
    for entry in (FIND, BIND):
        for count in (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 31, 64, 100, 255, 400):
            for mode in ('sorted', 'duplicates', 'unsorted'):
                for target in ('hit', 'below', 'above', 'between', 'high'):
                    for _ in range(3):
                        seed += 1
                        case = oracle.run(entry, seed, count, mode, target)
                        if case is None:
                            skipped += 1
                        else:
                            cases.append(case)
    for entry in (FIND, BIND):
        edges = {tuple(e) for c in cases if c['entry'] == entry for e in c['edges']}
        missing = [(hex(i.address), hex(t)) for i in branches(entry)
                   for t in (i.address + i.size, int(i.op_str, 16)) if (i.address, t) not in edges]
        # A zero count returns at the first test, so the short search's
        # own zero test (0x10665BA) never takes its branch.
        expected = [('0x10665ba', '0x10665ec')] if entry == FIND else []
        assert missing == expected, (hex(entry), missing)
    out = Path(a.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(dict(
        source='DDDA.exe 2017-12-13 Steam',
        exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(),
        cases=cases), separators=(',', ':')))
    print(f'{len(cases)} cases ({skipped} wrapped searches left the synthetic memory, skipped) -> {out}')


if __name__ == '__main__':
    main()
