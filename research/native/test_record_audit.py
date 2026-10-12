import copy
import json
from pathlib import Path
import tempfile
import unittest
from audit_records import compare, fingerprint


class RecordAuditTests(unittest.TestCase):
    def fixture(self):
        return dict(functions={'1': dict(hash='helper', ext=True),
                               '2': dict(hash='caller', ext=True),
                               '3': dict(hash='other', ext=True)},
                    cases={'1': ['a'], '2': ['b', 'c'], '3': ['d']},
                    mocked={'2': [1]})

    def test_additions_preserve_existing_records(self):
        before = self.fixture(); after = copy.deepcopy(before)
        after['functions']['4'] = dict(hash='new', ext=True)
        after['cases']['4'] = ['e']
        r = compare(before, after)
        self.assertTrue(r['ok']); self.assertEqual(r['unchanged'], 3)
        self.assertEqual(r['added'], ['0x00000004'])

    def test_promotion_requires_replacement_of_mocking_cases(self):
        before = self.fixture(); after = copy.deepcopy(before)
        after['functions']['1'] = dict(hash='closed-helper', ext=False)
        after['cases']['1'] = ['fresh-helper']
        self.assertEqual(compare(before, after)['stale_mock_cases'], ['0x00000002'])
        after['cases']['2'] = ['fresh-caller']
        self.assertEqual(compare(before, after)['still_mocking_promoted'], ['0x00000002'])
        after['mocked'] = {}
        r = compare(before, after)
        self.assertTrue(r['ok']); self.assertEqual(r['unchanged'], 1)
        self.assertEqual(r['affected_callers'], ['0x00000002'])

    def test_unrelated_change_or_removal_fails(self):
        before = self.fixture(); after = copy.deepcopy(before)
        after['cases']['3'] = ['different']
        self.assertFalse(compare(before, after)['ok'])
        self.assertEqual(compare(before, after)['unexpected_changes'], ['0x00000003'])
        del after['functions']['3']; del after['cases']['3']
        self.assertEqual(compare(before, after)['missing'], ['0x00000003'])

    def test_explicit_refresh_allows_only_stale_callers_of_existing_closed_target(self):
        before=self.fixture();before['functions']['1']['ext']=False
        after=copy.deepcopy(before);after['cases']['2']=['fresh'];after['mocked']={}
        self.assertFalse(compare(before,after)['ok'])
        self.assertTrue(compare(before,after,[1])['ok'])
        self.assertFalse(compare(before,before,[1])['ok'])
        after['cases']['3']=['unrelated']
        self.assertFalse(compare(before,after,[1])['ok'])
        with self.assertRaises(ValueError):compare(before,after,[3])

    def test_fingerprint_checks_pair_and_preserves_case_multiplicity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'patterns').mkdir(); (root/'vectors').mkdir()
            functions = root/'patterns/lift.json'
            cases = root/'vectors/lift.json'
            functions.write_text(json.dumps([dict(addr=1, ext=True, arms=['example'])]))
            cases.write_text(json.dumps([dict(addr=1, seed=2), dict(addr=1, seed=3)]))
            before = fingerprint(root)
            cases.write_text(json.dumps([dict(seed=3, addr=1), dict(seed=2, addr=1)]))
            self.assertTrue(compare(before, fingerprint(root))['ok'])
            cases.write_text(json.dumps([dict(addr=1, seed=2)] * 2))
            self.assertFalse(compare(before, fingerprint(root))['ok'])
            cases.write_text('[]')
            with self.assertRaisesRegex(ValueError, 'without cases'):
                fingerprint(root)
            cases.write_text(json.dumps([dict(addr=2)]))
            with self.assertRaisesRegex(ValueError, 'orphan'):
                fingerprint(root)
            functions.write_text(json.dumps([dict(addr=1), dict(addr=1)]))
            with self.assertRaisesRegex(ValueError, 'duplicate'):
                fingerprint(root)


if __name__ == '__main__':
    unittest.main()
