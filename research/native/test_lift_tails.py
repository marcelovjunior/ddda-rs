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

    def test_register_bound_from_push_imm_pop(self):
        # push 3; sar eax,4; pop edx; cmp eax,edx; ja default; jmp [eax*4+0x2000]; ret x5
        code = bytes.fromhex("6a03" "c1f804" "5a" "39d0" "7707" "ff248500200000") + b"\xc3" * 5
        tables = {0x2000: dwords(0x1012, 0x1013, 0x1014, 0x1015, 0)}
        _, switches, _, _ = body(code, tables)
        self.assertEqual(sorted(switches[0x100A]), [0x1012, 0x1013, 0x1014, 0x1015])
        self.assertTrue(switches[0x100A].proven)
        # the pop takes a register's push instead: no constant
        code = bytes.fromhex("6a03" "50" "5a" "39d0" "7707" "ff248500200000") + b"\xc3" * 5
        body(code, {0x2000: dwords(0x1010, 0x1011, 0x1012, 0x1013, 0)})
        self.assertEqual(lift.TABLE_PROOF[0x1000], {0x1008: False})

    def slots(self, code, tables, jump):
        read, is_code = image_fixture(code, tables)
        ins, switches, _, _ = lift.decode_body(0x1000, len(code), entries=frozenset({0x1000}), noreturn=frozenset(),
                                               read=read, is_code=is_code, ext=True)
        found = {i.address: i for i in ins}
        return lift.edge_slots(found, jump, 0x1000, switches, read)

    def test_l48_memory_slot_bound(self):
        # cmp dword [esp+4],2; movss [esp+0x10],xmm0; ja default; mov ecx,[esp+4];
        # jmp [ecx*4+0x2000]; ret x5 -- the flag-neutral store sits between
        code = bytes.fromhex("837c240402" "f30f11442410" "770b" "8b4c2404" "ff248d00200000") + b"\xc3" * 5
        tables = {0x2000: dwords(0x1019, 0x101A, 0x101B, 0)}
        self.assertEqual(self.slots(code, tables, 0x1011), [0, 1, 2])
        # a store to the slot after the compare ends the proof
        code = bytes.fromhex("837c240402" "770f" "89442404" "8b4c2404" "ff248d00200000") + b"\xc3" * 5
        self.assertIsNone(self.slots(code, {0x2000: dwords(0x1016, 0x1017, 0x1018, 0)}, 0x100F))

    def test_l48_byte_remap_behind_a_register_bound(self):
        # mov ecx,3; cmp eax,ecx; ja default; movzx eax,byte [eax+0x3000];
        # jmp [eax*4+0x2000] -- only the bytes T[0..3] = 1,0,1,0 are slots
        code = bytes.fromhex("b903000000" "39c8" "770e" "0fb68000300000" "ff248500200000") + b"\xc3" * 4
        tables = {0x2000: dwords(0x1018, 0x1019, 0x101A, 0), 0x3000: bytes((1, 0, 1, 0, 2))}
        self.assertEqual(self.slots(code, tables, 0x1010), [0, 1])

    def test_l48_lea_constant_and_and_with_a_constant_register(self):
        # mov edx,3; lea ecx,[edx+1]; cmp eax,ecx; ja default; jmp [eax*4+0x2000]
        code = bytes.fromhex("ba03000000" "8d4a01" "39c8" "7707" "ff248500200000") + b"\xc3" * 6
        tables = {0x2000: dwords(*range(0x1018, 0x101D), 0)}
        self.assertEqual(self.slots(code, tables, 0x100C), [0, 1, 2, 3, 4])
        # mov ecx,1; and eax,ecx; jmp [eax*4+0x2000]
        code = bytes.fromhex("b901000000" "21c8" "ff248500200000") + b"\xc3" * 2
        self.assertEqual(self.slots(code, {0x2000: dwords(0x100E, 0x100F, 0)}, 0x1007), [0, 1])

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


class FrameTailTests(unittest.TestCase):
    def test_a_jump_to_an_entry_that_pops_its_jumpers_frame_is_local(self):
        # 0x1000: push ebp; push edi; jmp 0x1010 (entry: pop edi; pop ebp; ret)
        # 0x1020: push esi; jmp 0x1030 (entry: ret, an ordinary tail call)
        code = bytearray(b"\xcc" * 0x40)
        code[0x00:0x04] = bytes.fromhex("5557eb0c")
        code[0x10:0x13] = bytes.fromhex("5f5dc3")
        code[0x20:0x23] = bytes.fromhex("56eb0d")
        code[0x30:0x31] = bytes.fromhex("c3")
        read, is_code = image_fixture(bytes(code), {})
        entries = frozenset({0x1000, 0x1010, 0x1020, 0x1030})
        decode = lambda a: {i.address for i in
                            lift.decode_body(a, 1, entries, frozenset(), read, is_code, ext=True)[0]}
        self.assertEqual(decode(0x1000), {0x1000, 0x1001, 0x1002, 0x1010, 0x1011, 0x1012})
        self.assertEqual(decode(0x1020), {0x1020, 0x1021})
        self.assertTrue(lift.frame_tail(0x1010, entries, frozenset(), read, is_code))
        self.assertFalse(lift.frame_tail(0x1030, entries, frozenset(), read, is_code))

    def test_a_jump_into_an_inlined_tail_is_emitted_locally(self):
        # L46: push ebp; push edi; jmp 0x1010 (frame tail: pop edi; pop ebp; ret).
        # The lifted body must continue there, not tail-call the entry.
        code = bytearray(b"\xcc" * 0x20)
        code[0x00:0x04] = bytes.fromhex("5557eb0c")
        code[0x10:0x13] = bytes.fromhex("5f5dc3")
        read, is_code = image_fixture(bytes(code), {})
        with patch.object(h, "code", read), patch.object(h, "executable", is_code), \
                patch.object(lift, "function_entries", lambda: frozenset({0x1000, 0x1010})), \
                patch.object(lift, "noreturn_entries", lambda: frozenset()):
            arms = lift.lift(0x1000, 4, ext=True)
            lift.release_body(0x1000, 4)
        text = " ".join(arms)
        self.assertIn("pc = 0x1010; continue;", text)
        self.assertNotIn("call_ext", text)

    def test_frame_tails_with_calls_and_underestimated_pops(self):
        # L46: 0x1010: call 0x1030 (ret); pop edx; ret -- a frame tail with a
        # call (the CRT x87 tail 0x01300749 reports errors through calls).
        # 0x1020: push 1; push 1; call 0x1038 (an unknown-pop callee); ret --
        # the underestimated pop gives a negative exit, which is no tail.
        code = bytearray(b"\xcc" * 0x48)
        code[0x10:0x17] = bytes.fromhex("e81b0000005ac3")
        code[0x20:0x2a] = bytes.fromhex("6a016a01e80f000000c3")
        code[0x30:0x31] = bytes.fromhex("c3")
        code[0x38:0x3a] = bytes.fromhex("ffe0")  # jmp eax: no proven pop
        read, is_code = image_fixture(bytes(code), {})
        entries = frozenset({0x1010, 0x1020, 0x1030, 0x1038})
        self.assertTrue(lift.frame_tail(0x1010, entries, frozenset(), read, is_code))
        self.assertFalse(lift.frame_tail(0x1020, entries, frozenset(), read, is_code))


class Eh4ScopeTests(unittest.TestCase):
    def scope(self, level_store, records):
        # push 0x3000 (scope table); call 0x1100 (__SEH_prolog4); <level store>; ret
        code = bytes.fromhex("6800300000" "e8f6000000") + level_store + b"\xc3"
        code = code.ljust(0x40, b"\xcc") + b"\xc3" * 0x10  # filter/handler code at 0x1040..
        header = dwords(0xFFFFFFFE, 0, 0xFFFFFFCC, 0)
        read, is_code = image_fixture(code, {0x3000: header + b"".join(dwords(*r) for r in records)
                                             + dwords(0xFFFFFFFE, 0, 0xFFFFFFD8)})
        with patch.object(lift, "eh4_helpers", lambda: (frozenset({0x1100}), frozenset())):
            ins = lift.decode_body(0x1000, 1, frozenset({0x1000, 0x1100}), frozenset(), read, is_code, ext=True)[0]
        lift.TABLE_PROOF.pop(0x1000, None)
        return {i.address for i in ins}

    def test_filter_and_handler_of_stored_levels_are_body_code(self):
        # and dword ptr [ebp-4], 0: level 0 -> filter 0x1040, handler 0x1041
        body = self.scope(bytes.fromhex("8365fc00"), [(0xFFFFFFFE, 0x1040, 0x1041), (0, 0, 0x1042)])
        self.assertTrue({0x1040, 0x1041} <= body)
        self.assertNotIn(0x1042, body)  # level 1 is never entered
        # a level from a register: records in order up to the invalid header
        body = self.scope(bytes.fromhex("31f6" "8975fc"), [(0xFFFFFFFE, 0x1040, 0x1041), (0, 0, 0x1042)])
        self.assertTrue({0x1040, 0x1041, 0x1042} <= body)

    def test_invalid_records_give_nothing(self):
        # enclosing level not below its own index; handler outside code
        body = self.scope(bytes.fromhex("31f6" "8975fc"), [(0, 0, 0x1041)])
        self.assertNotIn(0x1041, body)
        body = self.scope(bytes.fromhex("8365fc00"), [(0xFFFFFFFE, 0, 0x5000)])
        self.assertEqual(body, {0x1000, 0x1005, 0x100A, 0x100E})


class SlotSweepTests(unittest.TestCase):
    # mov ecx,[esp+4]; jmp [ecx*4+0x401000]; ret x3 (slots 0..2), then int3 padding
    CODE = bytes.fromhex("8b4c2404" "ff248d00104000") + b"\xc3" * 3

    def image(self):
        data = self.CODE.ljust(0x1000, b"\xcc") + dwords(0x40000B, 0x40000C, 0x40000D, 0xCCCCCCCC)
        read = lambda a, n: data[a - 0x400000:a - 0x400000 + n]
        return data, read, lambda a, n=1: 0x400000 <= a and a + n <= 0x400000 + len(self.CODE)

    def test_end_evidence_and_index_binding(self):
        _, read, is_code = self.image()
        lift.decode_body(0x400000, len(self.CODE), frozenset({0x400000}), frozenset(), read, is_code, ext=True)
        ends = lift.TABLE_ENDS.pop(0x400000)
        self.assertEqual(ends, {0x400004: {"table": 0x401000, "slots": 3, "index": {"stack": 4},
                                           "end": {"value": 0xCCCCCCCC, "kind": "not code"}}})
        self.assertEqual(lift.TABLE_PROOF.pop(0x400000), {0x400004: False})

    def test_strategy_sets_the_index_input(self):
        data, read, is_code = self.image()
        body = [0x400000, 0x400004, 0x40000B, 0x40000C, 0x40000D]
        with patch.object(h, "image", return_value=(0x400000, [(0x400000, data)])), \
                patch.object(h, "code", side_effect=read), patch.object(h, "executable", side_effect=is_code):
            for slot in range(3):
                seen = set()
                c = h.run_sparse(0x400000, 7, end=0x40000E, body=body, input_strategy="slot-sweep-v1",
                                 input_layout=dict(index={"stack": 4}, value=slot), visited=seen)
                self.assertIn(0x40000B + slot, seen)
                self.assertEqual(c["input_strategy"], "slot-sweep-v1")
            seen = set()
            h.run_sparse(0x400000, 7, end=0x40000E, body=body, input_strategy="slot-sweep-v1",
                         input_layout=dict(index={"register": "ecx"}, value=2), visited=seen)
            with self.assertRaises(ValueError):
                h.run_sparse(0x400000, 7, end=0x40000E, body=body, input_strategy="slot-sweep-v1",
                             input_layout=dict(index={"stack": 2}, value=0))

    def test_gate_counts_a_fully_swept_table(self):
        _, read, is_code = self.image()
        arm = "0x400004 => { { let t = m.u32(ecx); if matches!(t, 0x40000B) { pc = t; continue; } } }"
        record = dict(addr=0x400000, size=len(self.CODE), ext=True, arms=[arm],
                      body=[0x400000, 0x400004, 0x40000B, 0x40000C, 0x40000D],
                      table_sweep={"0x00400004": {"table": 0x401000, "slots": 3, "index": {"stack": 4},
                                                  "end": {"value": 0xCCCCCCCC, "kind": "not code"}}})
        decode = lift.decode_body
        with patch.object(lift, "decode_body", lambda a, s, *x, ext=False: decode(
                    a, s, frozenset({0x400000}), frozenset(), read, is_code, ext=ext)), \
                patch.object(lift, "function_entries", lambda: frozenset({0x400000})), \
                patch.object(lift, "noreturn_entries", lambda: frozenset()), \
                patch.object(h, "code", read), patch.object(h, "executable", is_code), \
                patch.object(h, "import_pops", lambda: ({}, {})):
            partial = lift.verification_gaps([dict(record, slot_coverage={"0x00400004": [0, 2]})])
            full = lift.verification_gaps([dict(record, slot_coverage={"0x00400004": [0, 1, 2]})])
        self.assertEqual(partial, {0x400000: ["unproven table at 0x00400004"]})
        self.assertEqual(full, {})


class VerificationGapTests(unittest.TestCase):
    def test_named_targets_include_lifted_callees_and_ext_constants(self):
        arm = ("{ super::lifted::f_012f71e0(m, cpu); } "
               "(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_ext(m, cpu, 0x12F9D82))(m, cpu);")
        self.assertEqual(lift.LIFTED_CALLEE.findall(arm), ["012f71e0"])
        self.assertEqual(lift.EXT_CONSTANT.findall(arm), ["12F9D82"])

    def gaps(self, records, code, tables):
        read, is_code = image_fixture(code, tables)
        decode = lift.decode_body

        def fixture_decode(addr, size, entries=None, noreturn=None, read_=None, is_code_=None, ext=False):
            return decode(addr, size, entries=frozenset({0x1000}), noreturn=frozenset(),
                          read=read, is_code=is_code, ext=ext)
        with patch.object(lift, "decode_body", fixture_decode), \
                patch.object(lift, "function_entries", lambda: frozenset({0x1000, 0x3000})), \
                patch.object(lift, "noreturn_entries", lambda: frozenset()), \
                patch.object(h, "code", read), patch.object(h, "executable", is_code), \
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
                         {0x1000: ["stale body: 7 instructions from 0x00001010 not lifted"]})
        self.assertEqual(self.gaps([dict(stale, body=full)], code, tables), {})


if __name__ == "__main__":
    unittest.main()
