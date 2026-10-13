import unittest
from load_timeline import add_mark, movie_spans, parse, phases, render, render_legacy


class TimelineTests(unittest.TestCase):
    def test_out_of_order_and_unrelated_lines(self):
        events = parse("noise\nload-log t=2.0 event=first_window\nload-log t=0 event=process_start")
        self.assertEqual([(e.name, d) for e, d in phases(events)], [("process_start", None), ("first_window", 2.0)])

    def test_missing_origin_is_not_invented(self):
        self.assertIsNone(list(phases(parse("load-log t=20 event=main_entry")))[0][1])
        self.assertIn("no process-start anchor", render([]))

    def test_movie_pairing_uses_reader_and_stream(self):
        events = parse("load-log t=1 event=movie_start reader=1 stream=3\nload-log t=2 event=movie_start reader=2 stream=4\nload-log t=5 event=movie_end reader=1 stream=3")
        self.assertEqual(movie_spans(events), [(("1", "3"), 1, 5), (("2", "4"), 2, None)])

    def test_manual_marks_are_explicit(self):
        events = []
        add_mark(events, "main_menu=12.5")
        self.assertIn("head_observation", render(events))
        for bad in ["main_menu=-1", "main_menu=nan", "main menu=1"]:
            with self.assertRaises(ValueError):
                add_mark(events, bad)

    def test_totals_are_not_phase_boundaries(self):
        events = parse("load-log t=0 event=process_start\nload-log t=1 event=io_totals read_ns=250000000 bytes=42\nload-log t=2 event=first_window")
        self.assertEqual(len(list(phases(events))), 2)
        self.assertIn("read=0.250000 s", render(events))

    def test_frames_do_not_invent_loading_screen(self):
        events = parse("load-log t=1 event=frame_totals draws=10 presents=1\nload-log t=3 event=frame_totals draws=4010 presents=3")
        result = render(events)
        self.assertIn("| 3.000 | 2.000 | 2 | 2000.0 |", result)
        self.assertIn("Unmeasured UI milestones", result)

    def test_zlib_entry_buckets_are_totals(self):
        events = parse("load-log t=0 event=process_start\nload-log t=1 event=zlib_entry_totals short_output_calls=12 short_output_ns=500000000\nload-log t=2 event=first_present")
        self.assertEqual(len(list(phases(events))), 2)
        self.assertIn("short_output=0.500000 s", render(events))

    def test_legacy_clocks_remain_explicit(self):
        result = render_legacy("ddplay stage-probe t=99s draws/frame=4765 fps=11.0")
        self.assertIn("NOT process start", result)
        self.assertIn("| 99 | 4765 | 11.0 |", result)

    def test_phase_duration_ignores_intervening_milestones(self):
        events = parse("load-log t=0 event=process_start\nload-log t=5 event=first_window\nload-log t=8 event=main_menu")
        self.assertIn("| Boot to main menu | 8.000 |", render(events))
        self.assertIn("| Loading screen | unmeasured |", render(events))


if __name__ == "__main__":
    unittest.main()
