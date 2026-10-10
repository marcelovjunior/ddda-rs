import unittest

from perf_trace import spans, summarize, stream_summary


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

    def test_parallel_update_threads_require_explicit_frame_owner(self):
        records = spans([event("B", "update", 0), event("B", "update", 0, 2),
                         event("E", "update", 1_000), event("E", "update", 2_000, 2)])
        with self.assertRaises(ValueError):
            summarize(records, 0, 2_000)
        result = summarize(records, 0, 2_000, frame_thread=(1, 1))
        self.assertEqual(result["frames"], 1)
        self.assertEqual(result["frame_median_ms"], 1)
        self.assertEqual(result["frame_threads"], [(1, 1), (1, 2)])

    def test_bevy_empty_formatted_fields_preserve_frame_marker(self):
        records = spans([event("B", "update: ", 0), event("E", "update: ", 1_000)])
        self.assertEqual(summarize(records, 0, 1_000)["frames"], 1)

    def test_streamed_clipping_and_parallel_child_accounting(self):
        events = [event("B", "update: ", 0), event("B", "system", 2_000),
                  event("B", "render", 3_000, 2), event("E", "system", 6_000),
                  event("E", "render", 9_000, 2), event("E", "update: ", 10_000)]
        result = stream_summary(iter(events), 4_000, 9_000)
        rows = {r["name"]: r for r in result["spans"]}
        self.assertEqual(rows["update"]["inclusive_ms"], 5)
        self.assertEqual(rows["update"]["exclusive_ms"], 3)
        self.assertEqual(rows["render"]["inclusive_ms"], 5)
        self.assertEqual(result["frames"], 0)

    def test_terminal_open_scope_is_explicitly_reported_not_a_frame(self):
        events = [event("B", "bevy_app", 0), event("B", "update", 1_000),
                  event("E", "update", 3_000)]
        with self.assertRaises(ValueError):
            stream_summary(iter(events), 0, 4_000)
        result = stream_summary(iter(events), 0, 4_000, allow_open_scopes=True)
        self.assertEqual(result["frames"], 1)
        self.assertEqual(result["frame_median_ms"], 2)
        self.assertEqual(len(result["ignored_terminal_open_scopes"]), 1)
        self.assertEqual(result["ignored_terminal_open_scopes"][0]["name"], "bevy_app")
        self.assertEqual([row["name"] for row in result["spans"]], ["update"])

    def test_complete_child_record_is_subtracted_from_threaded_parent(self):
        child = {**event("X", "system", 2_000), "dur": 4_000}
        events = [event("B", "update", 0), child, event("E", "update", 10_000)]
        for result in (summarize(spans(events), 0, 10_000),
                       stream_summary(iter(events), 0, 10_000)):
            rows = {row["name"]: row for row in result["spans"]}
            self.assertEqual(rows["update"]["exclusive_ms"], 6)
            self.assertEqual(rows["system"]["inclusive_ms"], 4)

    def test_broken_nesting_and_incomplete_capture_rejected(self):
        for events in ([event("E", "a", 10)], [event("B", "a", 0)],
                       [event("B", "a", 0), event("E", "b", 10)]):
            with self.assertRaises(ValueError):
                spans(events)


if __name__ == "__main__":
    unittest.main()
