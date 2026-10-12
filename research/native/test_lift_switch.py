"""Synthetic recognizer fixtures; no bytes or tables copied from DDDA.exe.

Run with the pipeline Python: -m unittest discover -s research/native -p test_lift_switch.py
Original-exe differential coverage is recorded separately by lift.py vectors.
"""
import struct
import unittest
from unittest.mock import patch

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness
import lift


class SwitchTests(unittest.TestCase):
    def fixture(self, prefix=b"", entries=(0x1007, 0x1008), jump=None):
        # jmp [eax*4 + 0x2000]; nop; ret (all synthetic addresses).
        jump = jump if jump is not None else bytes.fromhex("ff248500200000")
        code = prefix + jump + b"\x90\xc3"
        image = {0x1000: code, 0x2000: struct.pack("<" + "I" * len(entries), *entries) + b"\0" * 4}

        def read(addr, count):
            for start, data in image.items():
                if start <= addr < start + len(data):
                    return data[addr - start:addr - start + count]
            raise KeyError(addr)

        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.detail = True
        ins = list(md.disasm(code, 0x1000))
        k = next(k for k, i in enumerate(ins) if i.mnemonic == "jmp")
        return code, image, read, ins, k

    def targets(self, fixture):
        code, _, read, ins, k = fixture
        return lift.switch_targets(ins, k, 0x1000, 0x1000 + len(code), read)

    def test_unbounded_table_and_duplicates(self):
        self.assertEqual(self.targets(self.fixture(entries=(0x1007, 0x1007, 0x1008))),
                         [0x1007, 0x1007, 0x1008])

    def test_stops_at_first_outside_target(self):
        self.assertEqual(self.targets(self.fixture(entries=(0x1007, 0x9000, 0x1008))), [0x1007])

    def test_rejects_instruction_interior_empty_and_unmapped(self):
        for entries in ((0x1001,), (0x9000,)):
            with self.subTest(entries=entries), self.assertRaises(lift.Unsupported):
                self.targets(self.fixture(entries=entries))
        f = self.fixture()
        del f[1][0x2000]
        with self.assertRaises(lift.Unsupported):
            self.targets(f)

    def test_rejects_other_indirect_forms(self):
        for jump in ("ffe0", "ff20", "ff2480", "ff244500200000", "64ff248500200000"):
            with self.subTest(jump=jump), self.assertRaises(lift.Unsupported):
                self.targets(self.fixture(jump=bytes.fromhex(jump)))

    def test_ext_pass_mocks_indirect_jumps_closed_stays_strict(self):
        code, _, read, _, _ = self.fixture(jump=bytes.fromhex("ffe0"))
        with patch.object(harness, "executable", return_value=True), patch.object(harness, "code", read):
            self.assertIn('x86::call_ext(m, cpu, t)', ' '.join(lift.lift(0x1000, len(code), ext=True)))
            with self.assertRaises(lift.Unsupported):
                lift.lift(0x1000, len(code))

    def test_checks_unsigned_bounds(self):
        for branch, limit in ((0x77, 1), (0x73, 2)):
            prefix = bytes((0x83, 0xf8, limit, branch, 8))  # cmp eax,N; ja/jae ret
            self.assertEqual(self.targets(self.fixture(prefix, (0x100c, 0x100d))), [0x100c, 0x100d])
            for entries in ((0x100d,), (0x100c, 0x100d, 0x100d)):
                with self.assertRaises(lift.Unsupported):
                    self.targets(self.fixture(prefix, entries))

    def test_byte_remap_bound_is_not_dword_count(self):
        prefix = bytes.fromhex("83f803770f0fb68000300000")  # cmp eax,3; ja ret; movzx eax,[eax+3000]
        f = self.fixture(prefix, (0x1013, 0x1014))
        f[1][0x3000] = bytes((0, 1, 1, 0))
        self.assertEqual(self.targets(f), [0x1013, 0x1014])
        for invalid in (bytes((0, 1, 2, 0)), bytes((0, 1))):
            f[1][0x3000] = invalid
            with self.assertRaises(lift.Unsupported):
                self.targets(f)

    def test_codegen_reads_mem_and_dispatches_to_case_leaders(self):
        code, _, read, _, _ = self.fixture()
        with patch.object(harness, "executable", return_value=True), patch.object(harness, "code", read):
            arms = lift.lift(0x1000, len(code))
        rust = lift.rust_fn(0x1000, "synthetic", arms)
        self.assertIn("pc = m.fast_u32(pt, cpu.eax.wrapping_mul(4).wrapping_add(0x2000)); continue;", rust)
        self.assertIn("0x1007 =>", rust)
        self.assertIn("0x1008 =>", rust)
        self.assertIn("_ => unreachable!()", rust)
        self.assertNotIn("call_ext", rust)


class SparseBoundaryTests(unittest.TestCase):
    def test_vector_worker_routes_opt_in_inputs(self):
        for mode in ("0", "1"):
            for end in (None, 0x400020):
                with self.subTest(mode=mode, end=end), patch.dict(lift.os.environ, {"LIFT_ZERO_INPUTS": mode}), \
                        patch.object(harness, "run_sparse", return_value={}) as run:
                    self.assertEqual(lift._vectors_for((0x400000, 1, end)),
                                     (0x400000, [{"addr": 0x400000}]))
                    self.assertEqual(run.call_args.kwargs.get("zero_inputs", False), mode == "1")
                    self.assertEqual(run.call_args.kwargs.get("end"), end)

    def run_fixture(self, code, **kwargs):
        # Synthetic image: preserve executable bytes while other pages are zero.
        data = code.ljust(0x1000, b"\x90")
        with patch.object(harness, "image", return_value=(0x400000, [(0x400000, data)])):
            return harness.run_sparse(0x400000, 17, **kwargs)

    def test_zero_inputs_keep_image_and_record_memory_reads(self):
        # mov eax,[0x2000]; inc eax; ret
        code = bytes.fromhex("a10020000040c3")
        case = self.run_fixture(code, zero_inputs=True)
        self.assertTrue(case["zero_inputs"])
        self.assertEqual(case["regs_out"]["eax"], 1)
        self.assertEqual(case["regs_out"]["esp"], case["regs_in"]["esp"] + 4)
        self.assertTrue(all(v == 0 for k, v in case["regs_in"].items() if k != "esp"))
        self.assertIn([0x2000, "00000000"], case["reads"])
        random_case = self.run_fixture(code)
        self.assertNotIn("zero_inputs", random_case)
        self.assertNotEqual(random_case["regs_in"]["eax"], 0)

    def test_zero_mock_returns_are_recorded(self):
        # call 0x400010; ret; padding; ret (callee's static pop remains zero).
        code = bytes.fromhex("e80b000000c3") + b"\x90" * 10 + b"\xc3"
        case = self.run_fixture(code, zero_inputs=True, end=0x400006)
        self.assertEqual(len(case["calls"]), 1)
        self.assertEqual(case["calls"][0]["target"], 0x400010)
        self.assertEqual(case["calls"][0]["out"],
                         {"eax": 0, "ecx": 0, "edx": 0, "xmm0": [0] * 4, "pop": 0})
        self.assertEqual(case["regs_out"]["eax"], 0)

    def test_closed_run_rejects_execution_of_sparse_data(self):
        # mov eax,0x2000; jmp eax -- random/zero data must never become code.
        with self.assertRaisesRegex(harness.Rejected, "execution outside the image"):
            self.run_fixture(bytes.fromhex("b800200000ffe0"), zero_inputs=True)


if __name__ == "__main__":
    unittest.main()

