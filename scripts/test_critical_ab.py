import io
import unittest
from critical_ab import summarize


def trace(*records):
    return io.BytesIO(('critical version=1\n' + '\n'.join(records) + '\n').encode())


class StreamingTests(unittest.TestCase):
    def records(self):
        return [
            'Batch { id: 1, frame: 10, present: 8, queued: 2, ns: 0, nested: false }',
            'Job { id: 1, start: 0, end: 20, target: 4, resolved: 8, vtable: 12, tid: 1, panicked: false }',
            'Job { id: 1, start: 40, end: 60, target: 4, resolved: 8, vtable: 12, tid: 2, panicked: false }',
            'Join { id: 1, start: 10, end: 70, result: 0 }',
            'End { id: 1, ns: 80, panicked: false, lost: 0 }',
        ]

    def test_solo_tail_excludes_gap_and_work_is_separate(self):
        r = summarize(trace(*self.records()), 10, 10)
        self.assertEqual(r['frames'][10]['solo_tail_ns'], 20)
        self.assertEqual(r['frames'][10]['join_ns'], 60)
        self.assertEqual(r['frames'][10]['after_last_ns'], 10)
        rank = next(iter(r['families'].values()))
        self.assertEqual(rank['work_ns'], 40)
        self.assertEqual(rank['calls'], 2)
        self.assertEqual(rank['last_count'], 1)

    def test_window_and_missing_end(self):
        self.assertEqual(summarize(trace(*self.records()), 11, 12)['frames'], {})
        r = summarize(trace(*self.records()[:-1]), 10, 10)
        self.assertEqual(r['frames'], {})
        self.assertEqual(r['rejected']['unfinished batch'], 1)

    def test_loss_and_duplicate_callback_rejected(self):
        lines = self.records()
        lines[-1] = lines[-1].replace('lost: 0', 'lost: 1')
        self.assertEqual(summarize(trace(*lines), 10, 10)['frames'], {})
        lines = self.records()
        lines.insert(2, lines[1])
        self.assertEqual(summarize(trace(*lines), 10, 10)['frames'], {})

    def test_only_unterminated_eof_is_allowed(self):
        with self.assertRaises(ValueError):
            summarize(trace('bad line'), 10, 10)
        r = summarize(io.BytesIO(trace(*self.records()).read() + b'End {'), 10, 10)
        self.assertEqual(r['unterminated_eof_bytes'], 5)
        self.assertEqual(len(r['frames']), 1)


if __name__ == '__main__':
    unittest.main()
