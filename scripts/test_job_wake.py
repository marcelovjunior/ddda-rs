import copy
import unittest
import tempfile
from pathlib import Path
import hashlib
import gzip
from job_wake import analyze, analyze_file, parse, SIGNAL, WAIT


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
    def test_gzip_source_hash_is_original_trace(self):
        text = "critical version=1\n" + "\n".join(
            kind + " { " + ", ".join(f"{k}: {str(v).lower()}" for k, v in row.items()) + " }"
            for kind, row in fixture()) + "\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace.gz"
            with gzip.open(path, "wb") as out:
                out.write(text.encode())
            result = analyze_file(path)
            self.assertEqual(result["pairs"], analyze(fixture())["pairs"])
            self.assertEqual(result["source_sha256"], hashlib.sha256(text.encode()).hexdigest())
            self.assertEqual(result["source_hash_kind"], "decompressed-trace")

    def test_file_window_preserves_out_of_order_wait_pair_and_hash(self):
        records = fixture()
        other = copy.deepcopy(records)
        for kind, row in other:
            row["id"] += 10
            if "end_id" in row:
                row["end_id"] += 10
            if kind == "Batch":
                row["frame"] = 11
        text = "critical version=1\n" + "\n".join(
            kind + " { " + ", ".join(f"{k}: {str(v).lower()}" for k, v in row.items()) + " }"
            for kind, row in reversed(records + other)) + "\n"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            path.write_bytes(text.encode())
            result = analyze_file(path, 10, 10)
            expected = analyze(records)
            for key, value in expected.items():
                self.assertEqual(result[key], value)
            self.assertEqual(result["source_sha256"], hashlib.sha256(text.encode()).hexdigest())
            self.assertFalse(analyze_file(path, 12, 13)["pairs"])
            with self.assertRaises(ValueError):
                analyze_file(path, 11, 10)
            with path.open("a") as out:
                out.write("Job {")
            with self.assertRaises(ValueError):
                analyze_file(path, 10, 10)
            result = analyze_file(path, 10, 10, allow_truncated_tail=True)
            self.assertEqual(result["discarded_tail_bytes"], 5)
            self.assertEqual(result["pairs"], expected["pairs"])
            with path.open("a") as out:
                out.write("\n")
            with self.assertRaises(ValueError):
                analyze_file(path, 10, 10, allow_truncated_tail=True)

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

    def test_per_batch_tail_and_participation(self):
        records = fixture()
        records[0][1]["participants"] = 7
        records.append(("Join", dict(id=2, start=155, end=195, result=0)))
        row = analyze(records)["batches"][0]
        self.assertEqual(row["participants"], 7)
        self.assertEqual(row["signaled_lanes_with_main"], 2)
        self.assertEqual(row["jobs_per_participant"], 1 / 7)
        self.assertEqual(row["last_job_solo_tail_ns"], 20)
        self.assertEqual(row["last_job_to_batch_end_ns"], 20)
        self.assertEqual(row["join_waits"][0]["last_job_overlap_ns"], 20)
        self.assertEqual(row["join_waits"][0]["after_last_callback_ns"], 15)
        self.assertEqual(row["join_waits"][0]["until_last_worker_wait_return_ns"], 0)
        summary = analyze(records)["join_summary"]
        self.assertEqual(summary["joins_per_frame"], 1)
        self.assertEqual(summary["mean_ns_per_frame"]["duration_ns"], 40)
        self.assertEqual(row["no_callback_gaps_while_callbacks_unstarted"],
                         [dict(tid=4, start=110, end=160, ns=50)])

    def test_late_wake_overlap_is_clipped_to_join(self):
        records = fixture()
        records[3][1]["tid"] = 1  # Worker wakes after the main callback.
        records[2][1]["end"] = 198
        records.append(("Join", dict(id=2, start=170, end=195, result=0)))
        join = analyze(records)["batches"][0]["join_waits"][0]
        self.assertEqual(join["until_last_worker_wait_return_ns"], 25)
        self.assertEqual(join["after_last_callback_ns"], 15)
        self.assertEqual(join["last_job_overlap_ns"], 10)
        self.assertIsNone(analyze([])["join_summary"]["joins_per_frame"])

    def test_gaps_stop_when_all_callbacks_started_and_tail_is_clipped(self):
        records = fixture()
        records[0][1]["queued"] = 3
        records[3][1].update(start=145, end=150)
        records.extend([
            ("Job", dict(id=2, tid=1, start=165, end=190, panicked=False)),
            ("Job", dict(id=2, tid=4, start=170, end=195, panicked=False)),
        ])
        row = analyze(records)["batches"][0]
        self.assertEqual(row["last_job_solo_tail_ns"], 5)
        self.assertEqual([g["ns"] for g in row["no_callback_gaps_while_callbacks_unstarted"]],
                         [35, 20])

    def test_incomplete_wakes_do_not_infer_participant_count(self):
        records = fixture()
        del records[2]
        self.assertFalse(analyze(records)["batches"])

    def test_legacy_trace_does_not_confuse_signals_with_active_count(self):
        row = analyze(fixture())["batches"][0]
        self.assertIsNone(row["participants"])
        self.assertIsNone(row["jobs_per_participant"])
        self.assertEqual(row["signaled_lanes_with_main"], 2)

    def test_invalid_active_count_is_rejected(self):
        for count in [0, 9]:
            records = fixture()
            records[0][1]["participants"] = count
            self.assertFalse(analyze(records)["batches"])

    def test_batch_summary_counts_jobs_without_claiming_parallel_speedup(self):
        result = analyze(fixture())["batch_summary"]
        self.assertEqual(result["count"], 1)
        self.assertEqual(result["queued_histogram"], {1: 1})
        self.assertEqual(result["participants_histogram"], {"None": 1})
        self.assertEqual(result["batches_with_at_least_eight_jobs"], 0)
        self.assertEqual(result["batches_with_jobs_at_least_participants"], 0)
        self.assertEqual(result["last_job_solo_tail"]["p50_ns"], 20)
        self.assertEqual(result["callback_gaps"]["p50_ns"], 50)

    def test_bad_join_does_not_generate_tail_attribution(self):
        records = fixture()
        records.append(("Join", dict(id=2, start=155, end=195, result=258)))
        self.assertFalse(analyze(records)["batches"])

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
