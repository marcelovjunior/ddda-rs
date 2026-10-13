import unittest
from pipeline_summary import summarize


class SummaryTests(unittest.TestCase):
    def test_counts_totals_boundaries_and_incomplete_lines(self):
        lines = [
            "other log line",
            "DDGPU_PIPELINE kind=module cache=object-miss frame=20 phase=unknown ms=1.5",
            "DDGPU_PIPELINE kind=module cache=object-miss frame=21 phase=unknown ms=2.5",
            "DDGPU_PIPELINE kind=pipeline cache=warmup frame=1 phase=unknown ms=100",
            "DDGPU_PIPELINE truncated",
        ]
        rows = summarize(lines, [(0, "boot"), (10, "title"), (20, "loading")])
        self.assertEqual(rows[("loading", "module", "object-miss")], [2, 4.0, 2.5])
        self.assertEqual(rows[("boot", "pipeline", "warmup")], [1, 100.0, 100.0])
        self.assertEqual(len(rows), 2)

    def test_recorded_labels_are_preserved_without_overrides(self):
        rows = summarize(["DDGPU_PIPELINE kind=ir cache=hit frame=100 phase=gameplay ms=1.0"])
        self.assertEqual(rows[("gameplay", "ir", "hit")], [1, 1.0, 1.0])


if __name__ == "__main__":
    unittest.main()
