"""Bounded synthetic record ranges; layouts are evidence supplied by the caller.

No function-address dispatch, inferred game state, or branch-dependent repair.
All bytes and related pointers are chosen before execution starts.
"""
import random
import struct


class RecordRangeInputs:
    BASE = 0x30000ff0  # deliberately exercise records spanning page boundaries
    RESULT = 0x30040000
    COUNTS = (3, 5, 8, 17, 33)

    def __init__(self, layout, seed):
        if not isinstance(layout, dict) or set(layout) - {
                'stride', 'first', 'last', 'result', 'float_offsets', 'uint_offsets'}:
            raise ValueError('invalid record-range layout')
        stride = layout.get('stride')
        if type(stride) is not int or not 4 <= stride <= 4096 or stride % 4:
            raise ValueError('record stride must be a multiple of four in 4..4096')
        occupied = set()
        for name in ('first', 'last', 'result'):
            if name == 'result' and name not in layout:
                continue
            binding = layout.get(name)
            if not isinstance(binding, dict) or len(binding) != 1:
                raise ValueError('range bindings require one stack offset or register')
            kind, value = next(iter(binding.items()))
            if kind == 'stack':
                valid = type(value) is int and 4 <= value <= 128 and value % 4 == 0
            else:
                valid = kind == 'register' and value in ('eax','ebx','ecx','edx','esi','edi','ebp')
            if not valid or (kind, value) in occupied:
                raise ValueError('invalid or overlapping range binding')
            occupied.add((kind, value))
        fields = layout.get('float_offsets', []) + layout.get('uint_offsets', [])
        if any(type(o) is not int or o < 0 or o % 4 or o+4 > stride for o in fields):
            raise ValueError('record field outside stride or unaligned')
        if len(set(fields)) != len(fields):
            raise ValueError('overlapping record fields')
        self.layout = layout
        self.count = self.COUNTS[((seed & 255)-1) % len(self.COUNTS)]
        self.first, self.last = self.BASE, self.BASE + self.count * stride
        rng = random.Random(seed)
        data = bytearray(self.count * stride)
        variant = seed % 4
        for index in range(self.count):
            for offset in range(0, stride, 4):
                struct.pack_into('<I', data, index*stride+offset, rng.randrange(32))
            for offset in layout.get('uint_offsets', []):
                struct.pack_into('<I', data, index*stride+offset, index % 3 if variant == 0 else 0)
            value = (float(index) if variant == 1 else -float(index) if variant == 2
                     else 0.0 if variant == 3 else float(index % 5 - 2))
            for offset in layout.get('float_offsets', []):
                struct.pack_into('<f', data, index*stride+offset, value)
        self.data = bytes(data)
        values = dict(first=self.first, last=self.last, result=self.RESULT)
        self.stack, self.registers = {}, {}
        for name in ('first', 'last', 'result'):
            if name in layout:
                kind, location = next(iter(layout[name].items()))
                (self.stack if kind == 'stack' else self.registers)[location] = values[name]

    def page(self, seed, page):
        data = bytearray(4096)
        lo, hi = max(page, self.first), min(page+4096, self.last)
        if lo < hi:
            data[lo-page:hi-page] = self.data[lo-self.first:hi-self.first]
        return bytes(data)

    def register(self, rnd, name):
        return self.registers.get(name, 0)

    def returns(self, rnd):
        return dict(eax=0, ecx=0, edx=0, xmm0=[0]*4)

    def float_bits(self, rnd):
        return 0
