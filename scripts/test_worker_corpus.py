"""Reject corrupt/incoherent captures; test attribution independent of Windows."""
import unittest
import worker_corpus as c


class CorpusTests(unittest.TestCase):
    def test_split_parts_and_helpers(self):
        self.assertEqual(c.address('RNvNtC9ddlift_019generated10lifted_01210f_010a35d0_p1'),'0x010a35d0')
        self.assertIsNone(c.address('core::call<f_010a35d0>'))
        self.assertEqual(c.address('ddlift_00::lifted_012::f_010a35d0::h1234'),'0x010a35d0')

    def test_waits_unknowns_and_recursion(self):
        sample=dict(symbols={'1':'f_00000001','2':'f_00000002','3':'NtWaitForSingleObject','4':'unknown'},threads=[
            dict(name='guest main thread',stacks=[[1,1,2],[3,1],[4,2],[]]),dict(name='render worker',stacks=[[1]])])
        counts,meta=c.raw_counts(sample)
        self.assertEqual(meta,dict(busy=2,wait=1,empty=1,truncated=False))
        self.assertEqual(counts['self'],{'0x00000001':1});self.assertEqual(counts['inclusive'],{'0x00000001':1,'0x00000002':2})

    def test_text_sections_do_not_double_count_host_inclusive(self):
        text="""== thread 1 'guest main thread': cpu 1 s, busy 8/10 samples, wait 2, empty 0
  -- self (host leaf)
   12.5% 1 f_00000001
  -- nearest lifted function (self + non-lifted callees)
   50.0% 4 0x00000001
  -- inclusive lifted functions
   100.0% 8 0x00000001
  -- inclusive (host frames)
   100.0% 8 f_00000001
== thread 2 'guest thread 0x1034': cpu 1 s, busy 9/10 samples, wait 1, empty 0
  -- inclusive lifted functions
   100.0% 9 0x00000001
"""
        counts,meta=c.text_counts(text)
        self.assertEqual([counts[k]['0x00000001'] for k in ('self','nearest','inclusive')],[1,4,8])
        self.assertEqual(meta,dict(busy=8,wait=2,empty=0,truncated=True))

    def call(self,events):return dict(input=[0]*84,output=[0]*84,events=events,overflow=False,panicked=False)

    def test_pages_and_nested_seams(self):
        call=self.call([['r',10,[1,2]],['w',11,[3]],['enter',99,[0]*84],['enter',98,[0]*84],
                        ['w',10,[4]],['leave',98,[0]*84],['leave',99,[0]*84],['r',10,[4,3]]])
        result=c.materialize(call)
        self.assertEqual(result['initial'],[(10,1),(11,2)]);self.assertEqual(result['final'],[(10,4),(11,3)])
        self.assertEqual(len(result['seams']),1);self.assertEqual(result['seams'][0]['writes'],[[10,[4]]])
        self.assertEqual(result['seams'][0]['memory'],[(10,1),(11,3)])

    def test_reject_incoherent_even_across_seam(self):
        for events in [[['r',1,[1]],['r',1,[2]]],[['r',1,[1]],['enter',3,[0]*84],['leave',3,[0]*84],['r',1,[2]]]]:
            with self.assertRaisesRegex(ValueError,'incoherent'):c.materialize(self.call(events))

    def test_write_before_read_is_not_an_initial_value(self):
        r=c.materialize(self.call([['w',10,[9]],['r',10,[9]]]))
        self.assertEqual(r['initial'],[]);self.assertEqual(r['final'],[(10,9)])

    def test_reject_truncation_and_unbalanced(self):
        call=self.call([]);call['overflow']=True
        with self.assertRaises(ValueError):c.materialize(call)
        for events in [[['enter',3,[0]*84]],[['leave',3,[0]*84]],[['probe',1,4,True,None]], [['r',0xffffffff,[1,2]]]]:
            with self.assertRaises(ValueError):c.materialize(self.call(events))

    def test_reject_malformed_cpu_and_memory(self):
        bad=self.call([]);bad['input']=[0]*83
        with self.assertRaises(ValueError):c.materialize(bad)
        for event in [['r',1,[256]],['w',1,[]],['r',-1,[0]],['r',True,[0]],['unknown',1]]:
            with self.assertRaises(ValueError):c.materialize(self.call([event]))

    def test_extraction_requires_supported_call_seams(self):
        text='x86::call_local(m, cpu, 0x00FEDD60, super::lifted::f_00fedd60); x86::call_lifted(m,cpu,0xABC);'
        result=c.opaque_calls(text,'test')
        self.assertEqual(result,'x86::call_ext(m, cpu, 0x00FEDD60); x86::call_ext(m,cpu,0xABC);')
        with self.assertRaisesRegex(ValueError,'unhandled'):c.opaque_calls('super::lifted::f_1234(m,c)','test')


if __name__=='__main__':unittest.main()
