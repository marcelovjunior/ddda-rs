import unittest
from load_timeline import add_mark, movie_spans, parse, phases, render, render_legacy, interval, ranked_intervals


class TimelineTests(unittest.TestCase):
    def test_boot_details_do_not_split_milestones(self):
        events = parse("load-log t=1 event=movie_end\n"
                       "load-log t=2 event=boot_detail kind=storage mode=7 timer_bits=0x42f00000\n"
                       "load-log t=3 event=boot_detail kind=gui gui_state=2\n"
                       "load-log t=8 event=main_menu state=0x00531a20")
        self.assertEqual([(e.name, d) for e, d in phases(events)],
                         [("movie_end", None), ("main_menu", 7)])
        self.assertEqual(events[1].fields["timer_bits"], "0x42f00000")

    def test_rank_uses_transition_deltas_and_retains_availability(self):
        events = parse("load-log t=2 event=interval from=boot to=title wall_ns=2000000000 zlib_ns=3000000000 gpu_available=false state=0x00532110\n"
                       "load-log t=3 event=io_totals read_ns=99999999999\n"
                       "load-log t=7 event=interval from=title to=area_request wall_ns=5000000000 dti=0x0198cdf0 pipeline_ns=unavailable")
        rows = ranked_intervals(events)
        self.assertEqual([r["wall_ns"] for r in rows], [5000000000, 2000000000])
        self.assertEqual(rows[0]["fields"]["pipeline_ns"], "unavailable")
        self.assertEqual(rows[0]["fields"]["dti"], "0x0198cdf0")
        self.assertEqual(rows[1]["fields"]["gpu_available"], "false")
        self.assertEqual(rows[1]["fields"]["zlib_ns"], "3000000000")
        self.assertNotIn("read_ns", rows[0]["fields"])

    def test_rank_does_not_invent_missing_transitions(self):
        self.assertEqual(ranked_intervals(parse("load-log t=1 event=io_totals read_ns=1")), [])
        rows = ranked_intervals(parse("load-log t=3 event=interval wall_ns=1000000000\nload-log t=2 event=interval wall_ns=1000000000"))
        self.assertEqual([r["end_s"] for r in rows], [2, 3])

    def test_rank_rejects_invalid_durations_but_allows_timestamp_rounding(self):
        for value in ["-1", "nan", "", "2000000001"]:
            with self.assertRaises(ValueError):
                ranked_intervals(parse(f"load-log t=1 event=interval wall_ns={value}"))
        self.assertEqual(len(ranked_intervals(parse("load-log t=1 event=interval wall_ns=1000000400"))), 1)

    def test_independent_snapshots_exclude_future_samples(self):
        events = parse("load-log t=1 event=io_totals bytes=40\nload-log t=1.1 event=archive_totals zlib_ns=20\nload-log t=2 event=io_totals bytes=100\nload-log t=3 event=archive_totals zlib_ns=999")
        result = interval(events, 1.5, 2.5)
        self.assertEqual(result["delta"]["io_totals.bytes"], 60)
        self.assertEqual(result["delta"]["archive_totals.zlib_ns"], 0)
        self.assertEqual(result["sample_age_s"]["archive_totals"], 1.4)

    def test_missing_counter_baseline_is_not_invented(self):
        events = parse("load-log t=2 event=gpu_totals pipelines=7")
        self.assertEqual(interval(events, 0, 3)["delta"], {})
        self.assertEqual(interval(events, 1, 3)["delta"], {})
        events = parse("load-log t=0 event=process_start\nload-log t=2 event=gpu_totals pipelines=7")
        self.assertEqual(interval(events, 0, 3)["delta"], {"gpu_totals.pipelines": 7})

    def test_nested_helpers_have_distinct_counters(self):
        events = parse("load-log t=0 event=process_start\nload-log t=1 event=zlib_helper address=0x00cf2cb0 calls=3 ns=123\nload-log t=1.1 event=archive_totals zlib_ns=150")
        result = interval(events, 0, 2)["delta"]
        self.assertEqual(result["zlib_helper:0x00cf2cb0.ns"], 123)
        self.assertEqual(result["archive_totals.zlib_ns"], 150)
        self.assertEqual(len(list(phases(events))), 1)

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
