"""L45 synthetic fixtures for table jumps whose index is bounded on the
paths into the jump (no exe bytes).

The CRT memcpy variants select their tail moves with `neg ecx; jmp
[ecx*4 + T]` after `cmp ecx, 8; jb`, and with `sub ecx, 4; jb` before an
underflow table; switches compare against a register loaded with a
constant. The targets lie past the function's inventory extent.
"""
import struct
import unittest
from unittest.mock import patch

import harness as h
import lift


def image_fixture(code, tables):
    image = {0x1000: code, **tables}

    def read(addr, count):
        for start, data in image.items():
            if start <= addr < start + len(data):
                return data[addr - start:addr - start + count]
        raise KeyError(addr)

    is_code = lambda a, n=1: 0x1000 <= a and a + n <= 0x1000 + len(code)
    return read, is_code


def body(code, tables, size=None, ext=True):
    read, is_code = image_fixture(code, tables)
    return lift.decode_body(0x1000, size or len(code), entries=frozenset({0x1000}), noreturn=frozenset(),
                            read=read, is_code=is_code, ext=ext)


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *values)


class EdgeBoundTests(unittest.TestCase):
    def test_negated_index_after_cmp_jb_reaches_code_past_the_extent(self):
        # cmp ecx,8; jb +1; ret; neg ecx; jmp [ecx*4+0x2020]; ret x9
        code = bytes.fromhex("83f9087201c3f7d9ff248d20200000") + b"\xc3" * 9
        # slots -7..0 at 0x2004..0x2020 select 0x1010..0x1017
        tables = {0x2004: dwords(*range(0x1010, 0x1018))}
        ins, switches, _, _ = body(code, tables, size=0x10)
        self.assertEqual(sorted(switches[0x1008]), list(range(0x1010, 0x1018)))
        self.assertTrue(switches[0x1008].proven)
        self.assertTrue(set(range(0x1010, 0x1018)) <= {i.address for i in ins})
        self.assertEqual(lift.TABLE_PROOF[0x1000], {0x1008: True})

    def test_underflow_after_sub_jb(self):
        # sub ecx,4; jb +1; ret; jmp [ecx*4+0x2010]; ret x4
        code = bytes.fromhex("83e9047201c3ff248d10200000") + b"\xc3" * 4
        tables = {0x2000: dwords(0x100D, 0x100E, 0x100F, 0x1010)}
        _, switches, _, _ = body(code, tables)
        self.assertEqual(sorted(switches[0x1006]), [0x100D, 0x100E, 0x100F, 0x1010])
        self.assertTrue(switches[0x1006].proven)

    def test_register_bound_holding_a_constant(self):
        # mov edx,3; cmp eax,edx; ja default; jmp [eax*4+0x2000]; ret (default); ret x4
        code = bytes.fromhex("ba03000000" "39d0" "7707" "ff248500200000") + b"\xc3" * 5
        tables = {0x2000: dwords(0x1011, 0x1012, 0x1013, 0x1014, 0)}
        _, switches, _, _ = body(code, tables)
        self.assertEqual(sorted(switches[0x1009]), [0x1011, 0x1012, 0x1013, 0x1014])
        self.assertTrue(switches[0x1009].proven)

    def test_register_bound_set_on_one_path_only_is_unproven(self):
        # test ecx,ecx; je skip; mov edx,3; skip: cmp eax,edx; ja default; jmp [eax*4+0x2000]
        code = bytes.fromhex("85c9" "7405" "ba03000000" "39d0" "7707" "ff248500200000") + b"\xc3" * 5
        tables = {0x2000: dwords(0x1015, 0x1016, 0x1017, 0x1018, 0)}
        body(code, tables)
        self.assertEqual(lift.TABLE_PROOF[0x1000], {0x100D: False})

    def test_and_with_a_register_keeps_the_earlier_bound(self):
        # cmp eax,3; ja default; and eax,ecx; jmp [eax*4+0x2000]; ret; ret (default); ret x3
        code = bytes.fromhex("83f803" "770a" "21c8" "ff248500200000") + b"\xc3" * 5
        tables = {0x2000: dwords(0x100E, 0x1010, 0x1011, 0x1012, 0)}
        _, switches, _, _ = body(code, tables)
        self.assertEqual(sorted(switches[0x1007]), [0x100E, 0x1010, 0x1011, 0x1012])
        self.assertTrue(switches[0x1007].proven)

    def test_input_index_is_unproven(self):
        # mov ecx,[esp+4]; jmp [ecx*4+0x2000]; ret; ret
        code = bytes.fromhex("8b4c2404" "ff248d00200000") + b"\xc3\xc3"
        tables = {0x2000: dwords(0x100B, 0x100C, 0)}
        _, switches, _, _ = body(code, tables)
        self.assertEqual(sorted(switches[0x1004]), [0x100B, 0x100C])
        self.assertFalse(switches[0x1004].proven)


class VerificationGapTests(unittest.TestCase):
    def gaps(self, records, code, tables):
        read, is_code = image_fixture(code, tables)
        decode = lift.decode_body

        def fixture_decode(addr, size, ext=False):
            return decode(addr, size, entries=frozenset({0x1000}), noreturn=frozenset(),
                          read=read, is_code=is_code, ext=ext)
        with patch.object(lift, "decode_body", fixture_decode), \
                patch.object(lift, "function_entries", lambda: frozenset({0x1000, 0x3000})), \
                patch.object(h, "import_pops", lambda: ({}, {})):
            return lift.verification_gaps(records)

    def test_unbound_direct_target_unproven_table_and_stale_body(self):
        table_arm = "0x1004 => { { let t = m.u32(ecx); if matches!(t, 0x100B) { pc = t; continue; } } }"
        code = bytes.fromhex("8b4c2404" "ff248d00200000") + b"\xc3\xc3"
        tables = {0x2000: dwords(0x100B, 0x100C, 0)}
        records = [
            dict(addr=0x1000, size=len(code), ext=True, body=[0x1000, 0x1004, 0x100B, 0x100C],
                 arms=[table_arm, "x86::call_ext(m, cpu, 0x5000)", "x86::call_ext(m, cpu, 0x3000)"]),
        ]
        gaps = self.gaps(records, code, tables)
        self.assertEqual(gaps, {0x1000: ["unbound target 0x00005000", "unproven table at 0x00001004"]})

    def test_stale_body_and_a_clean_record(self):
        # the negated-index fixture: proven, but a record lifted before the
        # rule lacks the tail code
        code = bytes.fromhex("83f9087201c3f7d9ff248d20200000") + b"\xc3" * 9
        tables = {0x2004: dwords(*range(0x1010, 0x1018))}
        arm = "0x1008 => { { let t = m.u32(ecx); if matches!(t, 0x1017) { pc = t; continue; } } }"
        full = [0x1000, 0x1003, 0x1005, 0x1006, 0x1008] + list(range(0x1010, 0x1018))
        stale = dict(addr=0x1000, size=0x10, ext=True, arms=[arm], body=full[:5] + [0x1017])
        self.assertEqual(self.gaps([stale], code, tables),
                         {0x1000: ["stale body: 7 table-reached instructions from 0x00001010 not lifted"]})
        self.assertEqual(self.gaps([dict(stale, body=full)], code, tables), {})


if __name__ == "__main__":
    unittest.main()
