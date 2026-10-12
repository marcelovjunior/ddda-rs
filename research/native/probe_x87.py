"""L22 probe: which side of the x87 quarantines is right on real hardware.

Runs short x87 sequences, taken from the quarantined bodies, three ways:

1. natively on the host CPU as one instruction sequence (no FSAVE/FRSTOR
   between instructions; the probe only saves/restores the host state
   around the whole sequence),
2. one instruction at a time through x87_host.execute (the FRSTOR/FNSAVE
   route the lifted Rust uses, crates/ddnative-base/src/x87.rs),
3. in Unicorn (the recorder's emulator).

Memory operands are [ecx+d8] / [rcx+d8], which encode identically in 32-
and 64-bit mode, so the same bytes run in all three. Prints a JSON report;
the private copy goes under LIFT_DATA. No exe bytes are used.

    E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/probe_x87.py [out.json]
"""
import json
import struct
import sys

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn import x86_const as xc

import x87_host

SAVE, RESTORE = bytes.fromhex("DDB180000000"), bytes.fromhex("DDA180000000")  # fnsave/frstor [rcx+0x80]
FNINIT = bytes.fromhex("DBE3")
STORE_SW_CW = bytes.fromhex("DD7930D97932")  # fnstsw [rcx+0x30]; fnstcw [rcx+0x32]

# (name, the quarantined body it comes from, x87 bytes after fninit, inputs at +0)
CASES = [
    ("fldcw reserved bits", "0x012FC159 __ctrlfp fldcw at 0x012FC17B",
     "D96900", [struct.pack("<H", v) for v in (0xAFFF, 0xFFFF, 0x137F, 0x1F7F, 0xE07F, 0x0F7F)]),
    ("fld denormal m32, fcompp, fnstsw", "0x01314930/0x013149B0 loop body",
     "D94100D94104DED9", [struct.pack("<II", a, b) for a, b in
                          ((0x00000001, 0x3F800000), (0x3F800000, 0x007FFFFF), (0x3F800000, 0x40000000))]),
    ("fild, fidiv, fstp m32", "0x013179A0 at 0x013179BA",
     "DB4100DA7104D95908", [struct.pack("<ii", a, b) for a, b in ((1, 3), (7, 2), (-5, 7), (12, 4))]),
    ("fld m32, fabs, fsub m32, fstp m32", "0x01317A00 at 0x01317A83",
     "D94100D9E1D86104D95908", [struct.pack("<ff", a, b) for a, b in ((1.1, 0.3), (2.0, 1.0), (1e-30, 3.0))]),
    ("fld m64, fsubr m64, fstp m64", "0x012FCEBC at 0x012FCF9E",
     "DD4100DC6908DD5910", [struct.pack("<dd", a, b) for a, b in ((1.25, 0.1), (3.0, 1.0), (1e300, 1e-300))]),
    ("fld m64, fsin, fstp m32", "0x00F276C0 (large-argument FSIN)",
     "DD4100D9FED95908", [struct.pack("<d", v) for v in (1e10, 123456789.0, 3.0e18, 0.5)]),
]


def native(body, data):
    code = SAVE + FNINIT + bytes.fromhex(body) + STORE_SW_CW + RESTORE + b"\xC3"
    buf = bytearray(0x100)
    buf[:len(data)] = data
    return x87_host.native(code, buf)


def stepwise(body, data):
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    mem = bytearray(0x40)
    mem[:len(data)] = data
    state = x87_host.initial_state()
    for i in md.disasm(bytes.fromhex(body), 0):
        opcode, modrm, op = x87_host.encoding(i)
        if op is None:
            state, _, _ = x87_host.execute(state, opcode, modrm)
            continue
        a, size = op.mem.disp, op.size
        state, out, _ = x87_host.execute(state, opcode, modrm, mem[a:a + size])
        if i.mnemonic in ("fst", "fstp", "fist", "fistp", "fnstcw", "fnstsw"):
            mem[a:a + size] = out[:size]
    cw, sw = struct.unpack_from("<HxxH", state, 0)
    mem[0x30:0x34] = struct.pack("<HH", sw, cw)
    return bytes(mem)


def emulated(body, data):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.mem_map(0x1000, 0x2000)
    code = FNINIT + bytes.fromhex(body) + STORE_SW_CW
    uc.mem_write(0x1000, code)
    uc.mem_write(0x2000, bytes(data))
    uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
    uc.emu_start(0x1000, 0x1000 + len(code))
    return bytes(uc.mem_read(0x2000, 0x40))


def main():
    report = []
    for name, source, body, inputs in CASES:
        for data in inputs:
            n, s, u = native(body, data)[:0x40], stepwise(body, data), emulated(body, data)
            row = dict(case=name, source=source, input=data.hex(),
                       native=dict(sw=f"{struct.unpack_from('<H', n, 0x30)[0]:#06x}", cw=f"{struct.unpack_from('<H', n, 0x32)[0]:#06x}", out=n[8:0x18].hex()),
                       host_stepwise_equal=n == s,
                       unicorn=dict(sw=f"{struct.unpack_from('<H', u, 0x30)[0]:#06x}", cw=f"{struct.unpack_from('<H', u, 0x32)[0]:#06x}", out=u[8:0x18].hex()),
                       unicorn_equal=n == u)
            report.append(row)
            print(json.dumps(row))
    summary = dict(cases=len(report), host_stepwise_equal=sum(r["host_stepwise_equal"] for r in report),
                   unicorn_equal=sum(r["unicorn_equal"] for r in report))
    print(json.dumps(summary))
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w") as stream:
            json.dump(dict(summary=summary, rows=report), stream, indent=1)


if __name__ == "__main__":
    main()
