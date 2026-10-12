"""L22 synthetic fixtures for ext-mode open jump tables (no exe bytes).

Closed bodies keep the strict recognizer; ext bodies read the slot at run
time and leave the body for unknown targets, so table discovery only
decides which case code is decoded and verified.
"""
import struct
import unittest

import lift


def body(code, tables, entries=(0x1000,), ext=True):
    image = {0x1000: code, **tables}

    def read(addr, count):
        for start, data in image.items():
            if start <= addr < start + len(data):
                return data[addr - start:addr - start + count]
        raise KeyError(addr)

    is_code = lambda a, n=1: 0x1000 <= a and a + n <= 0x1000 + len(code)
    return lift.decode_body(0x1000, len(code), entries=frozenset(entries), noreturn=frozenset(),
                            read=read, is_code=is_code, ext=ext)


def dwords(*values):
    return struct.pack("<" + "I" * len(values), *values)


class OpenTableTests(unittest.TestCase):
    def test_unbounded_byte_remap(self):
        # movzx ecx, byte [eax+0x3000]; jmp [ecx*4+0x2000]; ret; ret
        code = bytes.fromhex("0fb68800300000ff248d00200000c3c3")
        tables = {0x2000: dwords(0x100E, 0x100F, 0), 0x3000: bytes((1, 0, 9))}
        with self.assertRaises(lift.Unsupported):
            body(code, tables, ext=False)
        _, switches, _, _ = body(code, tables)
        self.assertEqual(switches[0x1007], [0x100E, 0x100F])

    def test_case_label_entry_under_a_proven_bound(self):
        # cmp eax,1; ja default; jmp [eax*4+0x2000]; ret; ret (also an entry); nop; ret
        code = bytes.fromhex("83f8017708ff248500200000c3c390c3")
        tables = {0x2000: dwords(0x100C, 0x100D, 0)}
        with self.assertRaises(lift.Unsupported):
            body(code, tables, entries=(0x1000, 0x100D), ext=False)
        _, switches, _, _ = body(code, tables, entries=(0x1000, 0x100D))
        self.assertEqual(switches[0x1005], [0x100C, 0x100D])

    def test_mask_bound_skips_an_invalid_slot(self):
        # and eax,1; jmp [eax*4+0x2000]; ret  (slot 0 is not code)
        code = bytes.fromhex("83e001ff248500200000c3")
        _, switches, _, _ = body(code, {0x2000: dwords(0xDEAD0000, 0x100A, 0x100A)})
        self.assertEqual(switches[0x1003], [0x100A])

    def test_bounds(self):
        from capstone import Cs, CS_ARCH_X86, CS_MODE_32
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.detail = True
        def bound(text):
            ins = list(md.disasm(bytes.fromhex(text), 0x1000))
            k = len(ins) - 1
            return lift.table_bound(ins, k, ins[k].operands[0].mem.index)
        self.assertEqual(bound("bf0a00000039f87707ff248500200000"), 11)  # mov edi,10; cmp eax,edi; ja
        self.assertEqual(bound("83f8097307ff248500200000"), 9)           # cmp eax,9; jae
        self.assertEqual(bound("83e00301c1ff248500200000"), 4)           # and eax,3; add ecx,eax
        self.assertIsNone(bound("83e00340ff248500200000"))               # and eax,3; inc eax
        self.assertIsNone(bound("ff248500200000"))

    def test_bound_across_neutral_instructions(self):
        # cmp eax,1; ja default; push ebx; push edi; jmp [eax*4+0x2000]; ret; ret; ret (default)
        code = bytes.fromhex("83f8017709" "53" "57" "ff248500200000" "c3c3c3")
        tables = {0x2000: dwords(0x100E, 0x100F, 0x1010)}
        for ext in (False, True):
            _, switches, _, _ = body(code, tables, ext=ext)
            self.assertEqual(switches[0x1007], [0x100E, 0x100F])  # bound 2, not the table-only 3
        # a write to the index between the bound and the jump is not neutral
        code = bytes.fromhex("83f8017708" "40" "ff248500200000" "c3c3c3")  # inc eax
        _, switches, _, _ = body(code, {0x2000: dwords(0x100D, 0x100E, 0x100F)})
        self.assertEqual(switches[0x1006], [0x100D, 0x100E, 0x100F])  # table-only, no bound

    def test_no_forward_slot_is_a_runtime_jump(self):
        # jmp [ecx*4+0x2000] with slot 0 not code (a negative index selects earlier slots)
        code = bytes.fromhex("ff248d00200000c3")
        ins, switches, _, _ = body(code, {0x2000: dwords(0xDEAD0000)})
        self.assertIsInstance(switches[0x1000], lift.RuntimeTargets)
        self.assertEqual(switches[0x1000], [i.address for i in ins])


if __name__ == "__main__":
    unittest.main()
