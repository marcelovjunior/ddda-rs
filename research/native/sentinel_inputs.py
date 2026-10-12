"""Finite sentinel-terminated records, fully generated before execution.

The layout supplies the observed stride, entry binding and terminator field.
No game-address rules, runtime repairs or inferred object fields.
"""
import struct


class SentinelInputs:
    BASE = 0x30000ff0
    COUNTS = (1, 3, 8, 0, 17)

    def __init__(self, layout, seed):
        if not isinstance(layout, dict) or set(layout) != {'stride','first','sentinel_offset','sentinel'}:
            raise ValueError('invalid sentinel-record layout')
        stride, offset, marker = (layout[k] for k in ('stride','sentinel_offset','sentinel'))
        if type(stride) is not int or not 4 <= stride <= 4096 or stride % 4:
            raise ValueError('record stride must be a multiple of four in 4..4096')
        if type(offset) is not int or offset < 0 or offset % 4 or offset+4 > stride:
            raise ValueError('sentinel field outside stride or unaligned')
        if type(marker) is not int or not 0 <= marker <= 0xffffffff:
            raise ValueError('sentinel must be a 32-bit value')
        binding=layout['first']
        if not isinstance(binding, dict) or len(binding) != 1:
            raise ValueError('first binding requires one stack offset or register')
        kind, location=next(iter(binding.items()))
        valid=(type(location) is int and 4 <= location <= 128 and location % 4 == 0
               if kind == 'stack' else kind == 'register' and
               location in ('eax','ebx','ecx','edx','esi','edi','ebp'))
        if not valid:
            raise ValueError('invalid first binding')
        self.count=self.COUNTS[((seed & 255)-1) % len(self.COUNTS)]
        self.first=self.BASE
        self.end=self.first+(self.count+1)*stride
        data=bytearray((self.count+1)*stride)
        # Payload fields are zero. A zero marker requires a distinct data key.
        for index in range(self.count):
            struct.pack_into('<I',data,index*stride+offset,int(marker == 0))
        struct.pack_into('<I',data,self.count*stride+offset,marker)
        self.data=bytes(data)
        self.stack,self.registers={},{}
        (self.stack if kind == 'stack' else self.registers)[location]=self.first

    def page(self, seed, page):
        data=bytearray(4096)
        lo,hi=max(page,self.first),min(page+4096,self.end)
        if lo < hi:
            data[lo-page:hi-page]=self.data[lo-self.first:hi-self.first]
        return bytes(data)

    def register(self, rnd, name):
        return self.registers.get(name,0)

    def returns(self, rnd):
        return dict(eax=0,ecx=0,edx=0,xmm0=[0]*4)

    def float_bits(self, rnd):
        return 0
