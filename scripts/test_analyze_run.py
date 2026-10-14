"""Synthetic format tests plus read-only regressions on local head captures.

No private captures are copied into the repository. Set DDDA_ANALYST_EXAMPLES
to the scratchpad directory and DDDA_ANALYST_NOTES to the private notes dir to
override the maintainer defaults. Local integration tests skip if absent.
"""
import contextlib
import io
import os
from pathlib import Path
import tempfile
import unittest

import analyze_run as ar


def rate(t, frames, fps):
    return f'ddplay rate t={t}s frames={frames} {fps} FPS, 10 ms/frame'


PROFILE = """== WAITS thread 12 'render': waiting 10% of 100 samples
   90.0%  in ddruntime::threads::GuestLock::acquire_locked
   10.0%  from guest ignored
== thread 12 'render': cpu 8.0 s of 10.0 s (80%), busy 90/100 samples
  -- self
   10.0%  RtlAllocateHeap
  -- inclusive
   34.0%  ddgpu::WgpuBackend::try_draw
   12.0%  ddgpu::WgpuBackend::ring_fill
   15.0%  ddruntime::d3d9draw::snapshot
== thread 99 'main': cpu 1.0 s of 10.0 s (10%), busy 20/100 samples
  -- inclusive
   80.0%  ddnative::f_00123456
"""


class ParserTests(unittest.TestCase):
    def test_weighted_fps_and_rounded_duplicate_times(self):
        run = ar.parse_run('\n'.join([rate(0, 100, 20), rate(0, 105, 10), rate(1, 115, 20)]))
        self.assertAlmostEqual(run.buckets['0-20s'].fps, 15)
        self.assertEqual(run.excluded, 1)

    def test_stages_revisits_and_offset(self):
        text = '\n'.join(['ddruntime stage: menu t=10', rate(0, 1, 10), rate(1, 11, 10),
                          'ddruntime stage: game play t=12s', rate(2, 21, 10),
                          'ddruntime stage: menu t=13.0', rate(3, 31, 10)])
        run = ar.parse_run(text, -10)
        self.assertEqual(list(run.buckets), ['menu #1', 'game play #1', 'menu #2'])
        self.assertTrue(all(b.fps == 10 for b in run.buckets.values()))

    def test_windows_markers_and_truncation(self):
        text = '\n'.join(['ddplay marker: O', rate(19, 10, 10), 'ddplay marker: O',
                          rate(20, 20, 10), 'ddplay rate t=21s frames=', 'ddplay marker: O'])
        run = ar.parse_run(text)
        self.assertEqual(run.markers, [('O', None, 19), ('O', 19, 20), ('O', 20, None)])
        self.assertEqual(run.buckets['20-40s'].fps, 10)

    def test_gap_reset_and_zero_excluded(self):
        text = '\n'.join([rate(0, 10, 10), rate(10, 110, 100), rate(11, 1, 10),
                          rate(12, 11, 0), rate(13, 21, 10)])
        run = ar.parse_run(text)
        self.assertEqual(run.excluded, 4)
        self.assertEqual(run.buckets['0-20s'].frames, 10)

    def test_wait_sections_before_and_after_busy(self):
        threads = ar.parse_profile(PROFILE)
        t = ar.candidate(threads)
        self.assertEqual((t.name, t.busy, t.wait), ('render', 90, 10))
        self.assertEqual(len(t.waits), 1)
        self.assertEqual(len(t.inclusive), 3)
        self.assertEqual(threads[1].wait, 80)
        after = PROFILE + "\n== WAITS thread 99 'main': waiting 79% of 100 samples\n  100%  in sleep_for\n"
        self.assertEqual(ar.parse_profile(after)[1].wait, 79)

    def test_raw_and_truncated_symbols(self):
        cases = {
            'RNvNtCsEXAMPLE_9ddruntime8d3d9draw8snapshot': 'd3d9draw',
            '_RNvCsEXAMPLE_5ddgpu11WgpuBackend9ring_fill': 'ddgpu/wgpu',
            'RNvNtCsEXAMPLE_9ddlift_279generated10lifted_00110f_00123456': 'lifted game code',
            'ddruntime::threads::GuestLock::acquire_locked': 'threads/locks',
            'RNvNtCsEXAMPLE_7ddaudio6render': 'audio',
            'RtlAllocateHeap': 'allocations',
            'unknown_truncated': 'other',
            'core::array::map<ddgpu::impl$0::constants::closure': 'ddgpu/wgpu',
        }
        for symbol, group in cases.items():
            with self.subTest(symbol=symbol):
                self.assertEqual(ar.subsystem(symbol), group)

    def test_cumulative_lock_snapshots_not_summed(self):
        text = '''lock stats at 10 s:
guest lock by service: calls, held ms, waited ms
 10 4.0 .4 1.0 20.0 2.0 3.0 Old
 10 4.0 0.4 1.0 20.0 2.0 3.0 A
lock stats at 20 s:
guest lock by service: calls, held ms, waited ms
 20 9.0 0.4 1.0 25.0 1.2 3.0 A
 20 9.0 0.4 1.0 30.0 1.5 3.0 B
 30 9.0 0.4
'''
        self.assertEqual(ar.parse_locks(text), [(30, 'B', 20, 20), (25, 'A', 20, 20)])

    def test_gpu_variants_and_missing_values(self):
        text = '''timestamp, utilization.gpu [%], memory.used [MiB]
2026/10/13 00:00:00, 20 %, 200 MiB
2026-10-13 00:00:01, 40, 20, 200
2026/10/13 00:00:02, [N/A], 200
2026/10/13 00:00:03, nan, 200
2026/10/13 00:00:04, 300, 200
2026/10/13 00:00:05,
'''
        self.assertEqual(ar.parse_gpu(text), [20, 40])

    def test_bound_is_qualified_and_gpu_optional(self):
        t = ar.candidate(ar.parse_profile(PROFILE))
        self.assertIn('CPU graphics translation', ar.likely_bound(t, [20, 40]))
        self.assertIn('GPU unmeasured', ar.likely_bound(t, []))
        self.assertIn('mixed', ar.likely_bound(t, [95]))
        self.assertIn('undetermined', ar.likely_bound(None, []))

    def test_thread_ancestors_do_not_hide_lock_cost(self):
        t = ar.Thread('1', 'render', inclusive=[
            (100, 'ddruntime::threads::run_thread'),
            (100, 'std::backtrace<ddruntime::threads::create_thread>'),
            (11, 'ddruntime::threads::GuestLock::acquire_locked')])
        self.assertEqual(ar.cost_entries(t, 'threads/locks'), [
            (11, 'ddruntime::threads::GuestLock::acquire_locked')])

    def test_comparison_and_cli_utf16(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            profile, before, after = [base / name for name in ('profile', 'before', 'after')]
            profile.write_text(PROFILE, encoding='utf-16')
            before.write_text(rate(0, 10, 10) + '\n' + rate(1, 20, 10))
            after.write_text(rate(0, 10, 20) + '\n' + rate(1, 30, 20))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                ar.main([str(before), str(profile), '--compare', str(after), '--after-profile', str(profile)])
            self.assertIn('10.00 -> 20.00 FPS (+10.00, +100.0%)', output.getvalue())
            self.assertIn('NOT additive', output.getvalue())

    def test_empty_inputs(self):
        a = ar.Analysis(ar.parse_run(''), ar.parse_profile(''), [], [])
        self.assertIn('undetermined', ar.report(a, 'empty'))
        self.assertIsNone(ar.candidate(ar.parse_profile("== thread 1 'x': cpu 0.0 s of 0.0 s (0%), busy 0/0 samples")))


SCRATCH = Path(os.environ.get('DDDA_ANALYST_EXAMPLES',
    'C:/Users/janne/AppData/Local/Temp/claude/E--/4c653da2-9fb2-4573-befb-25cbe65ad0fd/scratchpad'))
NOTES = Path(os.environ.get('DDDA_ANALYST_NOTES', 'E:/ddda-re/notes'))


class LocalCaptureTests(unittest.TestCase):
    def capture(self, base, name):
        path = base / name
        if not path.is_file():
            self.skipTest(f'private capture absent: {path}')
        return ar.read_text(path)

    def test_quick_run_profile_gpu(self):
        run = ar.parse_run(self.capture(SCRATCH, 'run_quick1.log'))
        thread = ar.candidate(ar.parse_profile(self.capture(SCRATCH, 'profile_quick1.txt')))
        gpu = ar.parse_gpu(self.capture(SCRATCH, 'gpu_quick1.log'))
        self.assertGreater(run.rates, 100)
        self.assertGreater(len(run.buckets), 5)
        self.assertEqual(thread.name, 'guest thread 0x1034')
        self.assertEqual(thread.cpu, 84)
        self.assertAlmostEqual(thread.busy, 100 * 1061 / 1157)
        self.assertGreater(len(gpu), 10)
        self.assertEqual(gpu[:3], [6, 63, 26])

    def test_real_waits(self):
        threads = ar.parse_profile(self.capture(SCRATCH, 'profile_waits3.txt'))
        t = ar.candidate(threads)
        self.assertEqual((t.name, t.cpu), ('guest thread 0x1034', 88))
        sleeper = next(t for t in threads if t.name == 'guest thread 0x1038')
        self.assertEqual(sleeper.wait, 40)
        self.assertEqual(sleeper.waits[0], (99.1, 'ddruntime::threads::sleep_for'))

    def test_real_raw_v0(self):
        threads = ar.parse_profile(self.capture(NOTES, 'profile_gameplay_render.txt'))
        t = ar.candidate(threads)
        self.assertEqual(t.name, 'guest thread 0x1034')
        groups = {ar.subsystem(s) for _, s in t.inclusive}
        self.assertTrue(set(ar.GROUPS[:-1]).issubset(groups))

    def test_real_lock_wait_order(self):
        rows = ar.parse_locks(self.capture(NOTES, 'lockstats_gameplay.txt'))
        self.assertEqual(rows[0][:2], (27917.6, 'KERNEL32.dll!EnterCriticalSection'))
        self.assertEqual(rows[1][:2], (16153.8, 'KERNEL32.dll!WaitForSingleObject'))


if __name__ == '__main__':
    unittest.main()
