"""L22 synthetic fixtures for the constant-loop instruction budget.

A loop whose trip count is a proven constant terminates identically on
every input, so its body may record with a larger run_sparse budget; a loop
of the same shape bounded by an input keeps the default cap.
"""
import unittest
from unittest.mock import patch

import harness as h
import lift


def body(code):
    read = lambda a, n: code[a - 0x1000:a - 0x1000 + n] if 0x1000 <= a < 0x1000 + len(code) else (_ for _ in ()).throw(KeyError(a))
    is_code = lambda a, n=1: 0x1000 <= a and a + n <= 0x1000 + len(code)
    ins, _, _, _ = lift.decode_body(0x1000, len(code), entries=frozenset({0x1000}), noreturn=frozenset(),
                                    read=read, is_code=is_code)
    return ins


# xor ecx,ecx; L: inc ecx; cmp ecx,0x10000; jb L; ret
CONSTANT = bytes.fromhex("31c94181f90000010072f7c3")
# mov ecx,[eax]; L: inc ecx; cmp ecx,0x10000; jb L; ret  (start is an input)
INPUT_START = bytes.fromhex("8b084181f90000010072f7c3")
# xor ecx,ecx; L: inc ecx; cmp ecx,[eax]; jb L; ret  (limit is an input)
INPUT_LIMIT = bytes.fromhex("31c941" "3b08" "72fb" "c3")


class BudgetTests(unittest.TestCase):
    def test_constant_bound_gets_a_budget(self):
        self.assertEqual(lift.constant_loop_budget(body(CONSTANT)), lift.INSN_BUDGET + 0x10000 * 3)

    def test_input_bounds_do_not(self):
        self.assertIsNone(lift.constant_loop_budget(body(INPUT_START)))
        self.assertIsNone(lift.constant_loop_budget(body(INPUT_LIMIT)))

    def test_shapes(self):
        # mov edx,0x7fff; L: dec edx; jns L; ret  -> 0x8000 trips of 2
        self.assertEqual(lift.constant_loop_budget(body(bytes.fromhex("baff7f00004a79fdc3"))),
                         lift.INSN_BUDGET + 0x8000 * 2)
        # counter written twice in the loop: not proven
        self.assertIsNone(lift.constant_loop_budget(body(bytes.fromhex("31c9" "41" "41" "81f900000100" "72f6" "c3"))))
        # a call in the loop: not proven
        self.assertIsNone(lift.constant_loop_budget(body(bytes.fromhex("31c9" "41" "e800000000" "81f900000100" "72f2" "c3"))))
        # beyond the ceiling: not granted
        self.assertIsNone(lift.constant_loop_budget(body(bytes.fromhex("31c94181f90000001072f7c3"))))

    def test_recording_uses_the_budget_only_when_proven(self):
        for code, proven in ((CONSTANT, True), (INPUT_START, False)):
            sections = [(0x400000, code.ljust(4096, b"\xcc"))]
            with patch.object(h, "image", return_value=(0x400000, sections)), \
                    patch.object(h, "executable", return_value=True), \
                    patch.object(lift, "function_entries", return_value=frozenset({0x400000})):
                lift.lift(0x400000, len(code), ext=True)
                info = lift.BODY_INFO[0x400000]
                self.assertEqual("insn_budget" in info, proven)
                budget = info.get("insn_budget") or lift.INSN_BUDGET
                if proven:
                    case = h.run_sparse(0x400000, 3, zero_inputs=True, end=0x400000 + len(code),
                                        body=info["body"], max_insn=budget)
                    self.assertEqual(case["regs_out"]["ecx"], 0x10000)
                with self.assertRaisesRegex(h.Rejected, "runaway"):
                    # the default cap rejects; an input-bound loop never gets more
                    h.run_sparse(0x400000, 3, zero_inputs=True, end=0x400000 + len(code), body=info["body"],
                                 max_insn=lift.INSN_BUDGET if proven else budget)
            lift._BODY_CACHE.clear()


if __name__ == "__main__":
    unittest.main()
