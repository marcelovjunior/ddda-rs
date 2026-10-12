"""L22: the recorder's x87 host oracle follows the CPU, not Unicorn.

Synthetic sequences only (no exe bytes): each runs natively on the host,
and in Unicorn with x87_host.Oracle installed as harness.run_sparse does
(flush hook first, compute hook last). Both must store the same status
word, control word and results.
"""
import struct
import unittest

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn import x86_const as xc

import probe_x87
import x87_host


def oracle_run(body, data):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.mem_map(0x1000, 0x2000)
    code = bytes.fromhex(body) + probe_x87.STORE_SW_CW
    uc.mem_write(0x1000, code)
    uc.mem_write(0x2000, bytes(data))
    uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
    uc.reg_write(xc.UC_X86_REG_FPTAG, 0xFFFF)  # as run_sparse: empty stack, 0x037F
    uc.reg_write(xc.UC_X86_REG_FPCW, 0x037F)
    oracle = x87_host.Oracle(lambda u, i, op: (u.reg_read(xc.UC_X86_REG_ECX) + op.mem.disp) & 0xFFFFFFFF)
    uc.hook_add(UC_HOOK_CODE, oracle.on_flush)
    uc.hook_add(UC_HOOK_CODE, oracle.on_compute)
    uc.emu_start(0x1000, 0x1000 + len(code))
    oracle.flush(uc)
    assert oracle.problem is None, oracle.problem
    return bytes(uc.mem_read(0x2000, 0x40))


class OracleTest(unittest.TestCase):
    def test_probe_sequences_match_native(self):
        for name, _, body, inputs in probe_x87.CASES:
            for data in inputs:
                with self.subTest(case=name, data=data.hex()):
                    self.assertEqual(oracle_run(body, data), probe_x87.native(body, data)[:0x40])

    def test_status_and_control_bits_unicorn_misses(self):
        # fld of a denormal single sets DE (0x02); FLDCW drops reserved bits.
        out = oracle_run("D94100D94104DED9D96908", struct.pack("<IIH", 1, 0x3F800000, 0xAFFF))
        sw, cw = struct.unpack_from("<HH", out, 0x30)
        self.assertEqual(sw & 0x3F, 0x02)
        self.assertEqual(cw, 0x0F7F)

    def test_unicorn_state_round_trip(self):
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        state = x87_host.initial_state()
        for modrm in (0xE8, 0xEE, 0xEB):  # fld1, fldz, fldpi
            state, _, _ = x87_host.execute(state, 0xD9, modrm)
        x87_host.set_unicorn_state(uc, state)
        words = lambda s: struct.unpack_from("<HxxHxxH", s, 0)  # FNSAVE fills reserved halves
        self.assertEqual(words(x87_host.unicorn_state(uc)), words(state))
        self.assertEqual(x87_host.unicorn_state(uc)[28:], state[28:])

    def test_encodings_mirror_rust(self):
        self.assertTrue(x87_host.supported(0xD9, 0x28))   # fldcw
        self.assertTrue(x87_host.supported(0xDE, 0xD9))   # fcompp
        self.assertFalse(x87_host.supported(0xD9, 0x20))  # fldenv: exposes host pointers
        self.assertFalse(x87_host.supported(0xDD, 0x30))  # fnsave


if __name__ == "__main__":
    unittest.main()


class ApproximationTest(unittest.TestCase):
    def test_oracle_takes_the_host_approximation(self):
        # movups xmm1,[ecx]; rcpps xmm0,xmm1; rsqrtps xmm2,xmm1; rcpss xmm3,[ecx+16]  (same bytes 32/64-bit)
        body = bytes.fromhex("0f10090f53c10f52d1f30f535910")
        operands = struct.pack("<4f", 3.0, 0.7, 123.25, 1e-3) + struct.pack("<f", 5.5)
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x1000, 0x2000)
        uc.mem_write(0x1000, body)
        uc.mem_write(0x2000, operands)
        uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
        uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)  # as run_sparse
        oracle = x87_host.Oracle(lambda u, i, op: (u.reg_read(xc.UC_X86_REG_ECX) + op.mem.disp) & 0xFFFFFFFF)
        uc.hook_add(UC_HOOK_CODE, oracle.on_flush)
        uc.hook_add(UC_HOOK_CODE, oracle.on_compute)
        uc.emu_start(0x1000, 0x1000 + len(body))
        oracle.flush(uc)
        self.assertIsNone(oracle.problem)
        got = [uc.reg_read(getattr(xc, f"UC_X86_REG_XMM{k}")).to_bytes(16, "little") for k in (0, 2, 3)]
        # natively: the same instructions, then store xmm0, xmm2, xmm3 after the operands
        code = (bytes.fromhex("0f57c00f57d20f57db") + body
                + bytes.fromhex("0f114120" "0f115130" "0f115940" "c3"))
        out = x87_host.native(code, operands.ljust(0x50, b"\0"))
        self.assertEqual(got, [out[0x20:0x30], out[0x30:0x40], out[0x40:0x50]])
        self.assertTrue(oracle.cpu_dependent)


class SseOracleTest(unittest.TestCase):
    # xorps xmm2/3/4; movss xmm0,[ecx]; movss xmm1,[ecx+4]; divss xmm0,xmm1;
    # addss xmm0,[ecx+8]; cvttss2si eax,xmm0; cvtsi2ss xmm2,eax; comiss xmm2,xmm1;
    # sqrtss xmm3,[ecx+0xc]; divss xmm1,[ecx+0x10]; cmpltps xmm4,xmm0; stmxcsr [ecx+0x20]
    BODY = bytes.fromhex("0f57d2" "0f57db" "0f57e4" "f30f1001" "f30f104904" "f30f5ec1"
                         "f30f584108" "f30f2cc0" "f30f2ad0" "0f2fd1" "f30f51590c"
                         "f30f5e4910" "0fc2e001" "0fae5920")
    DATA = struct.pack("<5f", 1.0, 3.0, 0.1, -1.0, 0.0).ljust(0x1C, b"\0") + struct.pack("<I", 0x1F80)

    def native(self):
        code = (bytes.fromhex("0fae592c" "0fae511c") + self.BODY
                + bytes.fromhex("0f114130" "0f114940" "0f115150" "0f115960" "0f116170"
                                "894124" "0fae5928" "0fae512c" "c3"))
        out = x87_host.native(code, self.DATA.ljust(0x80, b"\0"))
        xmm = [int.from_bytes(out[0x30 + 16 * k:0x40 + 16 * k], "little") for k in range(5)]
        word = lambda a: int.from_bytes(out[a:a + 4], "little")
        return xmm, word(0x24), word(0x20), word(0x28)

    def test_sse_oracle_follows_the_host(self):
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x1000, 0x2000)
        uc.mem_write(0x1000, self.BODY)
        uc.mem_write(0x2000, self.DATA)
        uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
        uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
        oracle = x87_host.Oracle(lambda u, i, op: (u.reg_read(xc.UC_X86_REG_ECX) + op.mem.disp) & 0xFFFFFFFF)
        uc.hook_add(UC_HOOK_CODE, oracle.on_flush)
        uc.hook_add(UC_HOOK_CODE, oracle.on_compute)
        uc.emu_start(0x1000, 0x1000 + len(self.BODY))
        oracle.flush(uc)
        self.assertIsNone(oracle.problem)
        got = ([uc.reg_read(getattr(xc, f"UC_X86_REG_XMM{k}")) for k in range(5)],
               uc.reg_read(xc.UC_X86_REG_EAX), int.from_bytes(uc.mem_read(0x2020, 4), "little"),
               uc.reg_read(xc.UC_X86_REG_MXCSR))
        want = self.native()
        self.assertEqual(got, want)
        self.assertEqual(want[3] & 0x3F, 0x25)  # PE, ZE and IE are set on the CPU
        self.assertTrue(oracle.mxcsr_touched)

    def test_unicorn_alone_sets_no_flags(self):
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x1000, 0x2000)
        uc.mem_write(0x1000, self.BODY)
        uc.mem_write(0x2000, self.DATA)
        uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
        uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
        uc.emu_start(0x1000, 0x1000 + len(self.BODY))
        self.assertEqual(uc.reg_read(xc.UC_X86_REG_MXCSR), 0x1F80)


class NanTest(unittest.TestCase):
    """L22: the NaN cases the recorder used to reject follow the hardware."""

    def run_both(self, body, data, setup=b""):
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x1000, 0x2000)
        uc.mem_write(0x1000, body)
        uc.mem_write(0x2000, data)
        uc.reg_write(xc.UC_X86_REG_ECX, 0x2000)
        uc.reg_write(xc.UC_X86_REG_MXCSR, 0x1F80)
        uc.reg_write(xc.UC_X86_REG_FPTAG, 0xFFFF)
        uc.reg_write(xc.UC_X86_REG_FPCW, 0x037F)
        oracle = x87_host.Oracle(lambda u, i, op: (u.reg_read(xc.UC_X86_REG_ECX) + op.mem.disp) & 0xFFFFFFFF)
        uc.hook_add(UC_HOOK_CODE, oracle.on_flush)
        uc.hook_add(UC_HOOK_CODE, oracle.on_compute)
        uc.emu_start(0x1000, 0x1000 + len(body))
        oracle.flush(uc)
        self.assertIsNone(oracle.problem)
        emulated = bytes(uc.mem_read(0x2000, 0x40))
        # natively: fninit and the default MXCSR around the same bytes
        code = bytes.fromhex("ddb180000000" "0fae5978" "dbe3" "0fae517c") + body + bytes.fromhex("0fae5178" "dda180000000" "c3")
        native = x87_host.native(code, (data.ljust(0x7C, b"\0") + struct.pack("<I", 0x1F80)).ljust(0x100, b"\0"))  # FNSAVE area 0x80..0xEC
        return emulated, native[:0x40]

    def test_signalling_nan_load_is_quieted(self):
        # fld dword [ecx]; fstp dword [ecx+4]
        emulated, native = self.run_both(bytes.fromhex("d901d95904"), struct.pack("<I", 0x7F800001))
        self.assertEqual(emulated[4:8], native[4:8])
        self.assertEqual(int.from_bytes(native[4:8], "little"), 0x7FC00001)

    def test_two_nan_sse_operands(self):
        # movups xmm0,[ecx]; movups xmm1,[ecx+16]; addps xmm0,xmm1; movups [ecx+32],xmm0
        data = struct.pack("<4I", 0x7FC00001, 0x7F800002, 1, 2) + struct.pack("<4I", 0x7FC00003, 0x7FC00004, 0x7FC00005, 3)
        emulated, native = self.run_both(bytes.fromhex("0f10010f1049100f58c10f114120"), data)
        self.assertEqual(emulated[32:48], native[32:48])
