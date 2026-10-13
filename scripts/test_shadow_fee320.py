import copy
import unittest
from shadow_fee320 import summarize, validate


def sample(capture=False):
    result = dict(version=1, origin='runtime-lift', entry=0xFEE320, frame=10, object=123,
                  capture=capture, panicked=False, overflow=False,
                  shadow_ns=1000, leaf_ns=600, boundary_ns=200, calls=2, records=[])
    if capture:
        call = dict(input=[0]*84, output=[0]*84, overflow=False, panicked=False,
                    events=[['r', 123, [1, 2]], ['enter', 99, [0]*84],
                            ['w', 42, [3]], ['enter', 90, [0]*84],
                            ['leave', 90, [0]*84], ['leave', 99, [0]*84]])
        result['records'] = [call, copy.deepcopy(call)]
    return result


class OracleTests(unittest.TestCase):
    def test_inclusive_and_opaque_callee_cost_not_double_counted(self):
        result = summarize([sample()], 10, 1)
        self.assertEqual(result['inclusive_share'], .6)
        self.assertEqual(result['excluding_external_share'], .4)

    def test_capture_timing_never_reported_as_cost(self):
        result = summarize([sample(True)], 10, 1)
        self.assertFalse(result['timing_usable'])
        self.assertNotIn('inclusive_share', result)

    def test_missing_duplicate_and_mixed_frames_rejected(self):
        for samples, first, frames in [([sample()], 9, 2), ([sample(), sample()], 10, 2),
                                       ([sample(), sample(True)], 10, 2)]:
            with self.assertRaises(ValueError):
                summarize(samples, first, frames)

    def test_incomplete_call_or_shadow_rejected(self):
        for flag in ['overflow', 'panicked']:
            for root in [True, False]:
                s = sample(True)
                (s if root else s['records'][0])[flag] = True
                with self.assertRaises(ValueError):
                    validate(s)
        s = sample(True)
        s['records'].pop()
        with self.assertRaises(ValueError):
            validate(s)

    def test_unbalanced_callee_rejected(self):
        s = sample(True)
        s['records'][0]['events'][-1][1] = 80
        with self.assertRaises(ValueError):
            validate(s)
        s = sample(True)
        s['records'][0]['events'].pop()
        with self.assertRaises(ValueError):
            validate(s)

    def test_malformed_state_bytes_and_times_rejected(self):
        for mutate in [lambda s:s['records'][0]['input'].pop(),
                       lambda s:s['records'][0]['events'][0][2].append(256),
                       lambda s:s.update(origin='synthetic-lift'),
                       lambda s:s.update(boundary_ns=700),
                       lambda s:s.update(leaf_ns=1001)]:
            s = sample(True)
            mutate(s)
            with self.assertRaises(ValueError):
                validate(s)


if __name__ == '__main__':
    unittest.main()
