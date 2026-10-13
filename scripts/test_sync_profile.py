import unittest
from sync_profile import snapshots, difference


def block(frames=4, calls=2, ns=200):
    return [f"sync-census frames={frames} rows=1 overflow=0 cumulative=1",
            f"sync-row tid=0x1000 target=0x1234 site=0xdbe66d arg0=0x7 calls={calls} ns={ns} max_ns=150 contended=1 service_contended=0 blocks=1 timeouts=0 hist_us=[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] name=KERNEL32.dll!WaitForMultipleObjects",
            f"sync-census-end frames={frames}"]


class CensusTests(unittest.TestCase):
    def test_complete_only(self):
        self.assertEqual(len(list(snapshots(block() + block()[:-1]))), 1)

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
