"""L49 caller-state-v1: a helper recorded from its caller's frame (synthetic x86)."""
import gc
import unittest
from unittest.mock import patch

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
import caller_state as cs
import constructed_inputs as c

# caller 0x400000: push ebp; mov ebp,esp; mov dword [ebp-8],0x1234; call 0x400100; pop ebp; ret
CALLER = "55 8bec c745f834120000 e8f1000000 5d c3"
# helper 0x400100: mov eax,[ebp-8]; ret (it reads its caller's frame)
HELPER = "8b45f8 c3"


class CallerStateTests(unittest.TestCase):
    def tearDown(self):
        gc.collect()

    def setUp(self):
        self.code = {0x400000: bytes.fromhex(CALLER), 0x400100: bytes.fromhex(HELPER)}
        data = bytearray(b"\xcc" * 4096)
        for a, b in self.code.items():
            data[a - 0x400000:a - 0x400000 + len(b)] = b
        data = bytes(data)
        for mock in (patch.object(h, "image", return_value=(0x400000, [(0x400000, data)])),
                     patch.object(h, "code", side_effect=lambda a, n: data[a-0x400000:a-0x400000+n]),
                     patch.object(h, "executable", side_effect=lambda a, length=1: 0x400000 <= a < 0x401000)):
            mock.start()
            self.addCleanup(mock.stop)

    def run_body(self, f, seed, **kw):
        code = self.code[f["addr"]]
        body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(code, f["addr"])]
        return h.run_sparse(f["addr"], seed, body=body, end=f["addr"] + len(code), **kw)

    def accepted(self):
        return dict(addr=0x400000, seed=1, body_key="test", **self.run_body(dict(addr=0x400000), 1))

    def test_the_helper_reads_what_its_caller_wrote(self):
        case = self.accepted()
        self.assertEqual([e["target"] for e in case["calls"]], [0x400100])
        state, recipe = cs.capture(dict(addr=0x400000), case, 0x400100, self.run_body, "test")
        result = self.run_body(dict(addr=0x400100), 2, initial_state=state)
        self.assertEqual(result["regs_out"]["eax"], 0x1234)
        self.assertEqual(result["regs_in"]["esp"], state["esp"])
        self.assertEqual(result["regs_in"]["ebp"], case["calls"][0]["regs"]["ebp"])
        self.assertEqual(result["regs_out"]["esp"], result["regs_in"]["esp"] + 4)
        self.assertEqual(recipe["case_hash"], c.digest(case))
        self.assertEqual(recipe["rule"], "caller-state-v1")

    def test_refusals(self):
        case = self.accepted()
        with self.assertRaisesRegex(ValueError, "does not mock"):
            cs.capture(dict(addr=0x400000), case, 0x400200, self.run_body, "test")
        with self.assertRaisesRegex(ValueError, "case/body mismatch"):
            cs.capture(dict(addr=0x400000), case, 0x400100, self.run_body, "stale")
        case["regs_out"]["eax"] ^= 1
        with self.assertRaisesRegex(h.Rejected, "replay differs"):
            cs.capture(dict(addr=0x400000), case, 0x400100, self.run_body, "test")
        state, _ = cs.capture(dict(addr=0x400000), self.accepted(), 0x400100, self.run_body, "test")
        for esp in (state["esp"] + 2, 0x00500000):
            with self.assertRaisesRegex(ValueError, "stack pointer"):
                c.validate_state(dict(state, esp=esp))


if __name__ == "__main__":
    unittest.main()
