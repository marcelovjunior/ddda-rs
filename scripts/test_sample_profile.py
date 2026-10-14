"""Synthetic attribution fixtures only; no game data or process attachment."""
import contextlib
import io
import json
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest

import sample_profile as profile


A = 'RNvNtNtCsExample_9ddlift_279generated10lifted_00110f_00db82c0'
B = '_RNvNtNtCsExample_13ddlift_hot_009generated10lifted_00410f_00df0d20'
C = 'ddlift_01::generated::lifted_003::f_00401000'


class AttributionTests(unittest.TestCase):
    def test_alert_is_a_wake_not_a_wait(self):
        self.assertFalse(any(w in "NtAlertThreadByThreadIdEx" for w in profile.WAITS))
        self.assertTrue(any(w in "ZwWaitForAlertByThreadId" for w in profile.WAITS))


    def test_symbol_forms(self):
        for symbol in (A, '_' + A, A[3:], A + '+0x10',
                       'ddlift_27::generated::lifted_001::f_00DB82C0',
                       'ddlift_27::f_00db82c0::h123abc', 'f_00db82c0'):
            with self.subTest(symbol=symbol):
                self.assertEqual(profile.lifted_address(symbol), '0x00db82c0')
        self.assertEqual(profile.lifted_address(B), '0x00df0d20')

    def test_helpers_are_not_lifted(self):
        for symbol in ('ddnative_base::x86::Flags::alu_sub',
                       'RINvNtNtCsExample_13ddnative_base3x864sse29comiss_hwKb1_ECsOther_9ddlift_09',
                       'core::helper<ddlift_01::f_00401000>',
                       'ddlift_01::generated::f_004010000',
                       'ddlift_01::generated::not_f_00401000'):
            with self.subTest(symbol=symbol):
                self.assertIsNone(profile.lifted_address(symbol))

    def test_helper_ownership_inclusive_and_chains(self):
        stacks = [[helper, A, B, C] for helper in
                  ('fast_u32', 'Flags::alu_sub', 'x87::memory', 'sse2::comiss_hw')]
        stacks += [[A, B, C], [B, C], ['unresolved']]
        own, incl, chains = profile.attribution(stacks)
        self.assertEqual(own, {'0x00db82c0': 5, '0x00df0d20': 1, '(no lifted frame)': 1})
        self.assertEqual(incl, {'0x00db82c0': 5, '0x00df0d20': 6, '0x00401000': 6})
        self.assertEqual(chains[2], {'0x00df0d20 -> 0x00db82c0': 5,
                                   '0x00401000 -> 0x00df0d20': 1})
        self.assertEqual(chains[3], {'0x00401000 -> 0x00df0d20 -> 0x00db82c0': 5})

    def test_recursion_counts_inclusive_once(self):
        own, incl, chains = profile.attribution([[A, A, B]])
        self.assertEqual(incl['0x00db82c0'], 1)
        self.assertEqual(own['0x00db82c0'], 1)
        self.assertEqual(chains[2]['0x00db82c0 -> 0x00db82c0'], 1)

    def test_filter_exact_names(self):
        filters = profile.thread_filter('guest main thread, guest thread 0x1070,')
        self.assertTrue(profile.selected_thread('guest main thread', filters))
        self.assertTrue(profile.selected_thread('guest thread 0x1070', filters))
        self.assertFalse(profile.selected_thread('guest thread 0x10700', filters))
        self.assertTrue(profile.selected_thread('anything', set()))

    def test_offline_cli_roundtrip(self):
        capture = {'version': 1, 'pid': 1, 'rounds': 1, 'wall': 1,
                   'symbols': {'1': A, '2': B},
                   'threads': [{'tid': 7, 'name': 'guest main thread', 'cpu': 0.5,
                                'stacks': [[1, 2]]}]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'synthetic.json'
            path.write_text(json.dumps(capture), encoding='utf-8')
            result = subprocess.run([sys.executable, profile.__file__, '--input', str(path),
                                     '--threads', 'guest main thread'],
                                    capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('0x00df0d20 -> 0x00db82c0', result.stdout)
        self.assertIn('nearest lifted function', result.stdout)

    def test_report_waits_empty_exited_thread_and_filter(self):
        capture = {'pid': 1, 'rounds': 4, 'wall': 1,
                   'symbols': {'1': 'fast_u32', '2': A, '3': 'NtWaitForSingleObject'},
                   'threads': [{'tid': 7, 'name': 'guest main thread', 'cpu': None,
                                'stacks': [[1, 2], [2], [3, 2], []]},
                               {'tid': 8, 'name': 'excluded', 'cpu': 1, 'stacks': [[2]]}]}
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            profile.report(capture, 25, {'guest main thread'})
        result = out.getvalue()
        self.assertIn('busy 2/4 samples, wait 1, empty 1', result)
        self.assertIn('100.0%        2  0x00db82c0', result)
        self.assertIn('cpu unknown', result)
        self.assertNotIn('excluded', result)

    def test_callers_preserved_and_unresolved_not_invented(self):
        capture = {'pid': 1, 'rounds': 2, 'wall': 1,
                   'symbols': {'1': 'memcpy_repmovs_amd', '2': 'memcpy', '3': A,
                               '4': 'crt.dll!<unresolved function RVA 0x1000>'},
                   'threads': [{'tid': 7, 'name': 'worker', 'cpu': None,
                                'stacks': [[1, 2, 3], [4, 3]]}]}
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            profile.report(capture, 25, set(), callers=2)
        text = out.getvalue()
        self.assertIn('callers of memcpy_repmovs_amd (50.0%, 1 samples)', text)
        self.assertIn('unresolved function RVA 0x1000', text)
        self.assertNotIn('_NLG_Return2', text)


if __name__ == '__main__':
    unittest.main()
