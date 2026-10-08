"""Tiny helper: map VAs of the unpacked DDDA.exe and disassemble with capstone."""
import struct, capstone
class Img:
    def __init__(self, path):
        self.d = open(path, 'rb').read()
        pe = struct.unpack_from('<I', self.d, 0x3c)[0]
        n, osz = struct.unpack_from('<H', self.d, pe + 6)[0], struct.unpack_from('<H', self.d, pe + 20)[0]
        self.secs = []
        for i in range(n):
            nm, vs, va, rs, ro = struct.unpack_from('<8sIIII', self.d, pe + 24 + osz + 40 * i)
            self.secs.append((nm.rstrip(b'\0').decode(), 0x400000 + va, vs, rs, ro))
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    def off(self, va):
        for _, s, vs, rs, ro in self.secs:
            if s <= va < s + min(vs, rs): return ro + va - s
        return None
    def sec(self, va):
        for nm, s, vs, rs, ro in self.secs:
            if s <= va < s + vs: return nm
    def u32(self, va): return struct.unpack_from('<I', self.d, self.off(va))[0]
    def cstr(self, va):
        o = self.off(va)
        if o is None: return None
        e = self.d.index(b'\0', o); return self.d[o:e].decode('latin-1')
    def dis(self, va, n=40, stop_ret=True):
        out = []
        for ins in self.md.disasm(self.d[self.off(va):self.off(va) + 400], va):
            out.append(ins)
            if len(out) >= n or (stop_ret and ins.mnemonic in ('ret', 'jmp') and ins.op_str.startswith(('0x', '')) and ins.mnemonic == 'ret'): break
        return out
