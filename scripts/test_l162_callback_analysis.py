import hashlib
import tempfile
import unittest
from pathlib import Path

from l162_callback_analysis import stacks, trace


class AttributionTests(unittest.TestCase):
    def test_only_visible_shadow_under_dispatcher_is_attributed(self):
        symbols = {str(i): name for i, name in enumerate([
            "mem::fast_u32", "f_00ee2c90", "f_00db7620", "f_00dbd8d0", "f_00dbdc90"
        ])}
        result = stacks(dict(symbols=symbols, threads=[dict(stacks=[
            [0, 1, 2, 3], [0, 1, 2, 4], [0, 2, 3], [0, 1, 2]
        ])]))
        self.assertEqual(result["main"]["counts"]["db7620"], 2)
        self.assertEqual(result["main"]["counts"]["shadow"], 1)
        self.assertEqual(result["worker"]["counts"]["shadow"], 1)
        self.assertEqual(result["main"]["helpers"]["fast_load"], 1)

    def fixture(self, root):
        job = dict(id=1, tid=4096, target=0xDB7620, resolved=0xEE2C90,
                   vtable=0x160DF98, object=123, start=10, end=30)
        text = "critical version=1\nJob { " + ", ".join(f"{k}: {v}" for k, v in job.items()) + " }\n"
        path = root / "sample.trace"
        path.write_bytes(text.encode())
        selection = dict(source_sha256=hashlib.sha256(text.encode()).hexdigest(), batches=[
            dict(batch=1, frame=1, queued=1, last_job=job, last_job_solo_tail_ns=12)
        ])
        return path, selection

    def test_trace_and_selected_tail(self):
        with tempfile.TemporaryDirectory() as root:
            path, selection = self.fixture(Path(root))
            result = trace(path, selection)
            self.assertEqual(result["family"]["0160DF98/main/1000"]["total_ns"], 20)
            self.assertEqual(result["tails"][0][1]["solo_ns"], 12)

    def test_wrong_source_hash_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            path, selection = self.fixture(Path(root))
            selection["source_sha256"] = "wrong"
            with self.assertRaisesRegex(ValueError, "hash"):
                trace(path, selection)

    def test_missing_callback_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            path, selection = self.fixture(Path(root))
            selection["batches"][0]["queued"] = 2
            with self.assertRaisesRegex(ValueError, "counts"):
                trace(path, selection)


if __name__ == "__main__":
    unittest.main()
