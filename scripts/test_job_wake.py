import copy
import unittest
from job_wake import analyze, parse, SIGNAL, WAIT


def fixture():
    return [
        ("Batch", dict(id=2, frame=10, ns=100, queued=1, nested=False)),
        ("Wake", dict(id=2, end_id=2, tid=1, site=SIGNAL, handle=7,
                      start=110, end=150, result=1)),
        ("Wake", dict(id=1, end_id=2, tid=4, site=WAIT, handle=7,
                      start=50, end=140, result=0)),
        ("Job", dict(id=2, tid=4, start=160, end=180, panicked=False)),
        ("End", dict(id=2, ns=200, lost=0, panicked=False)),
    ]


class WakeTests(unittest.TestCase):
    def test_old_wait_batch_and_worker_before_signal_return(self):
        row = analyze(fixture())["pairs"][0]
        self.assertEqual(row["signal_to_wait_ns"], 30)
        self.assertEqual(row["signal_return_to_wait_ns"], -10)
        self.assertEqual(row["wait_to_first_job_ns"], 20)
        self.assertEqual(row["signal_to_first_job_ns"], 50)

    def test_cross_thread_emission_order_is_irrelevant(self):
        self.assertEqual(analyze(fixture()), analyze(list(reversed(fixture()))))

    def test_missing_or_duplicate_records_are_rejected(self):
        for index in range(5):
            records = fixture()
            del records[index]
            self.assertFalse(analyze(records)["pairs"], index)
            records = fixture()
            records.append(copy.deepcopy(records[index]))
            self.assertFalse(analyze(records)["pairs"], index)

    def test_failed_unordered_lossy_and_nested(self):
        for index, field, value in [(1, "result", 0), (2, "result", 258),
                                    (1, "end_id", 3), (2, "end", 105),
                                    (3, "start", 130), (4, "lost", 1),
                                    (4, "panicked", True), (0, "nested", True)]:
            records = fixture()
            records[index][1][field] = value
            self.assertFalse(analyze(records)["pairs"], (index, field))

    def test_worker_can_wake_without_claiming_a_job(self):
        records = fixture()
        records[3][1]["tid"] = 1
        row = analyze(records)["pairs"][0]
        self.assertIsNone(row["wait_to_first_job_ns"])
        self.assertEqual(row["worker_jobs"], 0)

    def test_frame_filter(self):
        self.assertFalse(analyze(fixture(), 11)["pairs"])
        self.assertEqual(len(analyze(fixture(), 10)["pairs"]), 1)

    def test_parse_and_reject_truncation_or_multiple_processes(self):
        lines = ["critical version=1"]
        for kind, row in fixture():
            fields = ", ".join(f"{k}: {str(v).lower()}" for k, v in row.items())
            lines.append(f"{kind} {{ {fields} }}")
        self.assertEqual(parse(lines), fixture())
        for bad in [lines[1:], lines + ["End {"], lines + [lines[0]]]:
            with self.assertRaises(ValueError):
                parse(bad)


if __name__ == "__main__":
    unittest.main()
