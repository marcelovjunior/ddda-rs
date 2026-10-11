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
