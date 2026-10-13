import unittest
from sync_profile import snapshots, difference


def block(frames=4, calls=2, ns=200):
    return [f"sync-census frames={frames} rows=1 overflow=0 cumulative=1",
            f"sync-row tid=0x1000 target=0x1234 site=0xdbe66d arg0=0x7 calls={calls} ns={ns} max_ns=150 contended=1 service_contended=0 blocks=1 timeouts=0 hist_us=[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] name=KERNEL32.dll!WaitForMultipleObjects",
            f"sync-census-end frames={frames}"]


class CensusTests(unittest.TestCase):
    def test_complete_only(self):
        self.assertEqual(len(list(snapshots(block() + block()[:-1]))), 1)

    def test_truncated_final_row_discards_only_unfinished_snapshot(self):
        partial = block()[:1] + [block()[1].split(" name=")[0][:-12]]
        self.assertEqual(len(list(snapshots(block() + partial))), 1)
        self.assertEqual(len(list(snapshots(block() + partial + block()))), 2)

    def test_malformed_complete_snapshot_is_still_rejected(self):
        malformed = block()[:1] + ["sync-row tid=0x1000 calls=2"] + block()[-1:]
        with self.assertRaisesRegex(ValueError, "malformed sync-row"):
            list(snapshots(block() + malformed))

    def test_sparse_rows_retain_cumulative_state_and_resume_without_double_count(self):
        sparse_empty = ["sync-census frames=8 rows=1 sparse=1", "sync-census-end frames=8"]
        resumed = block(12, 5, 800)
        resumed[0] += " sparse=1"
        first, idle, last = list(snapshots(block() + sparse_empty + resumed))
        self.assertEqual(difference(first, idle), (4, []))
        frames, rows = difference(idle, last)
        self.assertEqual((frames, rows[0]["calls"], rows[0]["ns"]), (4, 3, 600))
        self.assertEqual(next(iter(first["rows"].values()))["calls"], 2)

    def test_unfinished_sparse_snapshot_cannot_silently_lose_updates(self):
        later = ["sync-census frames=12 rows=1 sparse=1", "sync-census-end frames=12"]
        for mode in [" sparse=0", " sparse=1"]:
            incomplete = block(8, 5, 800)[:-1]
            incomplete[0] += mode
            with self.assertRaisesRegex(ValueError, "uninterrupted"):
                list(snapshots(block() + incomplete + later))
            self.assertEqual(len(list(snapshots(block() + incomplete))), 1)
            self.assertEqual(len(list(snapshots(block() + incomplete + block(12, 5, 800) + later))), 3)

    def test_sparse_capture_requires_full_baseline(self):
        sparse = ["sync-census frames=12 rows=1 sparse=1", "sync-census-end frames=12"]
        with self.assertRaisesRegex(ValueError, "full snapshot baseline"):
            list(snapshots(sparse))

    def test_interval(self):
        first, last = list(snapshots(block() + block(8, 5, 800)))
        frames, rows = difference(first, last)
        self.assertEqual((frames, rows[0]["calls"], rows[0]["ns"]), (4, 3, 600))
        self.assertEqual(rows[0]["site"], 0xdbe66d)

    def test_regression_rejected(self):
        first, last = list(snapshots(block(8, 5, 800) + block()))
        with self.assertRaises(ValueError):
            difference(first, last)

    def test_unrelated_lines(self):
        self.assertEqual(len(list(snapshots(["normal log"] + block() + ["other log"]))), 1)


if __name__ == "__main__":
    unittest.main()
