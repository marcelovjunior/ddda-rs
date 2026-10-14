import unittest
from profile_waits import state, summarize

GUEST = 'RNvNtNtCsExample_13ddlift_hot_009generated10lifted_00310f_00dac090'


class WaitTests(unittest.TestCase):
    def fixture(self):
        return {'wall': 10, 'symbols': {'1': 'ZwWaitForAlertByThreadId', '2': GUEST,
                '3': 'ddruntime::threads::wait_until<ddruntime::threads::wait_event::closure_env$3>',
                '4': 'NtAlertThreadByThreadIdEx', '5': 'ddgpu::encode::Pool::finish<T>',
                '6': 'ddplay.exe!<unresolved RVA 0x123>', '7': 'ddruntime::threads::signal_key',
                '8': 'std::sync::mpmc::Receiver::recv<d3d9cmd::deferred::Command>'},
                'threads': [{'tid': 1, 'name': 'guest thread 0x1034', 'cpu': 5,
                             'stacks': [[1, 3, 2], [4, 7, 2], [6], []]}]}

    def test_separates_wake_unknown_and_empty_and_keeps_cpu_denominator(self):
        row = summarize(self.fixture())[0]
        self.assertEqual(row['classes'], {'wait': 1, 'wake': 1, 'unknown': 1, 'empty': 1})
        self.assertEqual(row['cpu_percent'], 50)
        self.assertEqual(row['groups'][0]['percent'], 25)

    def test_guest_function_is_not_reported_as_guest_return_or_handle(self):
        group = summarize(self.fixture())[0]['groups'][0]
        self.assertEqual(group['guest_function'], '0x00dac090')
        self.assertEqual(group['guest_host_pc'], '0x2')
        self.assertIsNone(group['guest_return'])
        self.assertIsNone(group['wait_handle'])

    def test_backend_empty_receive_is_not_producer_backpressure(self):
        data = self.fixture(); data['threads'][0].update(name='d3d9-backend', stacks=[[1, 8]])
        group = summarize(data)[0]['groups'][0]
        self.assertEqual(group['mechanism'], 'backend command queue receive')
        self.assertIsNone(group['guest_function'])

    def test_encode_result_wait_and_no_cpu_sample(self):
        data = self.fixture(); data['threads'][0].update(name='wgpu-finish', stacks=[[1, 5]], cpu=None)
        row = summarize(data)[0]
        self.assertIsNone(row['cpu_percent'])
        self.assertEqual(row['groups'][0]['mechanism'], 'finisher waiting for encode results')

    def test_nested_lock_is_not_event_wait_and_present_throttle_is_not_send(self):
        data = self.fixture()
        data['symbols']['9'] = 'ddruntime::threads::holds_lock'
        data['threads'][0]['stacks'] = [[1, 9, 3, 2]]
        self.assertEqual(summarize(data)[0]['groups'][0]['mechanism'], 'guest execution lock / ownership check')
        data['symbols']['9'] = 'd3d9cmd::deferred::DeferredBackend::command'
        data['threads'][0]['stacks'] = [[1, 8, 9, 2]]
        self.assertEqual(summarize(data)[0]['groups'][0]['mechanism'], 'producer waiting for Present retirement')

    def test_finisher_barrier_keeps_present_and_query_context_separate(self):
        data = self.fixture()
        data['symbols'].update({'9': 'ddgpu::WgpuBackend::wait_finisher',
                                '10': 'ddgpu::impl$4::present', '11': 'ddgpu::impl$4::query_result'})
        data['threads'][0].update(name='d3d9-backend', stacks=[[1, 9, 10], [1, 9, 11]])
        groups = summarize(data)[0]['groups']
        self.assertEqual(len(groups), 2)
        self.assertTrue(groups[0]['mechanism'].endswith('::present'))
        self.assertTrue(groups[1]['mechanism'].endswith('::query_result'))

    def test_leaf_only_classification_does_not_mistake_wait_caller_for_wait(self):
        self.assertEqual(state('ddruntime::threads::wait_until'), 'nonwait')
        self.assertEqual(state('NtWaitForSingleObject'), 'wait')
        self.assertEqual(state('NtAlertThreadByThreadIdEx'), 'wake')


if __name__ == '__main__':
    unittest.main()
