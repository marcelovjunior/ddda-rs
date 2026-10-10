import unittest

from perf_trace import spans, summarize


def event(phase, name, time, thread=1):
    return {"ph": phase, "name": name, "ts": time, "tid": thread, "pid": 1}


class TraceAccounting(unittest.TestCase):
    def test_nested_parallel_spans_are_not_added_to_frame_duration(self):
        records = spans([
            event("B", "update", 0), event("B", "system", 2_000),
            event("B", "render", 3_000, 2), event("E", "system", 6_000),
            event("E", "render", 9_000, 2), event("E", "update", 10_000),
        ])
        result = summarize(records, 0, 10_000)
        rows = {r["name"]: r for r in result["spans"]}
        self.assertEqual(result["frames"], 1)
        self.assertEqual(result["frame_median_ms"], 10)
        self.assertEqual(rows["update"]["exclusive_ms"], 6)
        self.assertEqual(rows["render"]["inclusive_ms"], 6)
        self.assertEqual(rows["system"]["inclusive_ms"], 4)

    def test_window_clips_children_and_excludes_partial_frame_denominator(self):
        records = spans([
            event("B", "update", 0), event("B", "system", 2_000),
            event("E", "system", 6_000), event("E", "update", 10_000),
        ])
        result = summarize(records, 4_000, 9_000)
        rows = {r["name"]: r for r in result["spans"]}
        self.assertEqual(result["frames"], 0)
        self.assertEqual(rows["update"]["inclusive_ms"], 5)
        self.assertEqual(rows["update"]["exclusive_ms"], 3)
        self.assertEqual(rows["system"]["inclusive_ms"], 2)
        self.assertIsNone(rows["update"]["inclusive_ms_per_frame"])

    def test_same_named_systems_stay_separate_across_threads(self):
        records = spans([event("B", "system", 0), event("B", "system", 0, 2),
                         event("E", "system", 1_000), event("E", "system", 2_000, 2)])
        self.assertEqual(len(summarize(records, 0, 2_000)["spans"]), 2)

    def test_broken_nesting_and_incomplete_capture_rejected(self):
        for events in ([event("E", "a", 10)], [event("B", "a", 0)],
                       [event("B", "a", 0), event("E", "b", 10)]):
            with self.assertRaises(ValueError):
                spans(events)


if __name__ == "__main__":
    unittest.main()
