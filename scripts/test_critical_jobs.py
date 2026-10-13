import unittest
from critical_jobs import address, analyze, category, join_metrics, timeline


class AttributionTests(unittest.TestCase):
    def test_tail_and_longest_are_distinct(self):
        batch = dict(id=1, present=2, queued=2, ns=0, nested=False)
        end = dict(ns=110, panicked=False, lost=0)
        def job(start, finish, target):
            return dict(start=start, end=finish, target=target, resolved=0,
                        vtable=0, tid=4, panicked=False)
        jobs = [job(0, 90, 10), job(80, 100, 20)]
        join = dict(start=85, end=105, result=0)
        row = join_metrics(batch, end, jobs, [join])
        self.assertEqual(row["longest_target"], 10)
        self.assertEqual(row["last_target"], 20)
        self.assertEqual(row["wait_after_other_jobs_ns"], 15)
        self.assertEqual(row["last_job_wait_overlap_ns"], 10)
        self.assertEqual(row["after_last_completion_ns"], 5)
        join["start"] = 102
        row = join_metrics(batch, end, jobs, [join])
        self.assertEqual(row["last_job_wait_overlap_ns"], 0)
        self.assertEqual(row["after_last_completion_ns"], 3)
        for field in ("lost", "panicked"):
            bad = dict(end, **{field: 1})
            with self.assertRaises(ValueError):
                join_metrics(batch, bad, jobs, [join])
        with self.assertRaises(ValueError):
            join_metrics(batch, end, jobs[:1], [join])

    def test_truncated_batch_is_not_admitted(self):
        result = timeline(["critical version=1", "Batch { id: 1, present: 0, scheduler: 4, queued: 2, ns: 0, nested: false }"])
        self.assertEqual(result["joins"], [])
        self.assertEqual(result["rejected"]["unfinished batch"], 1)

    def test_symbols_and_parts(self):
        self.assertEqual(address("RNvNt10f_00db7620"), 0xDB7620)
        self.assertEqual(address("lift::f_00fee320_p1"), 0xFEE320)
        self.assertIsNone(address("unresolved 0x00db7620"))
        self.assertEqual(category("ddnative_base::mem::impl$3::fast_set_u32"), "fast_store")
        self.assertEqual(category("RINv3x87NtB6memory"), "x87")

    def test_dispatcher_only_is_not_a_job(self):
        data = {"symbols": {"1": "wait", "2": "f_00dbdc90", "3": "f_00dbdef0",
                            "4": "fast_u32", "5": "f_00fee320_p1", "6": "f_00fee320",
                            "7": "f_00db7620", "8": "?unknown"},
                "threads": [{"name": "worker", "tid": 42, "cpu": 1.0,
                             "stacks": [[1, 2, 3], [4, 5, 6, 7, 2, 3],
                                        [8, 7, 2, 3], [4, 7], []]}]}
        result = analyze(data)
        self.assertEqual(result["totals"], {"dispatcher_samples": 3,
                         "no_visible_job": 1, "visible_job_samples": 2})
        root = result["outermost_visible_lifted"][0]
        self.assertEqual(root["address"], 0xDB7620)
        self.assertEqual(root["samples"], 2)
        self.assertEqual(root["self_categories"], {"fast_load": 1, "unresolved": 1})
        self.assertEqual(result["paths"][0]["addresses"], [0xDB7620, 0xFEE320])
        # Unresolved top PCs remain unresolved, not assigned to DB7620 self time.
        self.assertEqual(result["nearest_visible_lifted"][1]["self_symbols"][0]["value"], "?unknown")


if __name__ == "__main__":
    unittest.main()
