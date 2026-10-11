"""Synthetic fixture consistency and opt-in strategy regression checks."""
import gc
import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

from capstone import Cs,CS_ARCH_X86,CS_MODE_32
import harness as h
import structured_inputs as trials


class InputTests(unittest.TestCase):
    def record(self, code, strategy=None, seed=19, **kwargs):
        image=code.ljust(0x1000,b'\xcc')
        body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,0x400000)]
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])), \
             patch.object(h,'code',side_effect=lambda a,n:image[a-0x400000:a-0x400000+n]), \
             patch.object(h,'static_pop',return_value=0):
            return h.run_sparse(0x400000,seed,body=body,size=len(code),input_strategy=strategy,**kwargs)

    def tearDown(self): gc.collect()

    def test_page_repeatability_and_distribution(self):
        for name in h.INPUT_STRATEGIES:
            s=h.SparseInputs(name)
            self.assertEqual(s.page(7,0x21000000),s.page(7,0x21000000))
            words=struct.unpack('<1024I',s.page(7,0x21000000))
            self.assertTrue(all(v<=31 or s.OBJECT_BASE<=v<s.OBJECT_BASE+0x20000 for v in words))
        s=h.SparseInputs('small-v1')
        self.assertNotEqual(s.page(7,0x21000000),s.page(8,0x21000000))
        self.assertNotEqual(s.page(7,0x21000000),s.page(7,0x21001000))

    def test_counted_loop_and_repeated_load_consistency(self):
        # mov eax,[ecx]; mov edx,[ecx]; test eax,eax; jz ret;
        # dec eax; jnz dec; ret. Original count is preserved in EDX.
        code=bytes.fromhex('8b018b1185c074034875fdc3')
        c=self.record(code,'small-v1')
        self.assertLessEqual(c['regs_out']['edx'],31)
        self.assertEqual(c['regs_out']['eax'],0)
        self.assertEqual(c['input_strategy'],'small-v1')
        self.assertEqual(c,self.record(code,'small-v1'))

    def test_original_image_and_guest_writes_are_unchanged(self):
        # Absolute image load, overlapping byte store/reload through ECX.
        code=bytes.fromhex('a100024000c6015a8b11c3').ljust(0x200,b'\xcc')+struct.pack('<I',0x12345678)
        for strategy in h.INPUT_STRATEGIES:
            c=self.record(code,strategy)
            self.assertEqual(c['regs_out']['eax'],0x12345678)
            self.assertEqual(c['regs_out']['edx']&0xff,0x5a)

    def test_object_root_and_mock_return_are_recorded(self):
        # call mocked 0x400100; mov eax,[eax]; ret
        c=self.record(bytes.fromhex('e8fb0000008b00c3'),'zero-objects-v1',end=0x400008)
        self.assertEqual(c['regs_out']['eax'],0)
        event=c['calls'][0]
        self.assertEqual(event['target'],0x400100)
        self.assertGreaterEqual(event['out']['eax'],h.SparseInputs.OBJECT_BASE)
        self.assertTrue(any(a==event['out']['eax'] and v=='00000000' for a,v in c['reads']))

    def test_opt_in_validation_and_stray_return_rejection(self):
        self.assertNotIn('input_strategy',self.record(b'\xc3'))
        with self.assertRaises(ValueError):self.record(b'\xc3','typo')
        with self.assertRaises(ValueError):self.record(b'\xc3','small-v1',zero_inputs=True)
        # No strategy may paper over the function overwriting its return slot.
        for strategy in h.INPUT_STRATEGIES:
            with self.assertRaisesRegex(h.Rejected,'instead of the caller'):
                self.record(bytes.fromhex('c7042400000000c3'),strategy)

    def test_instruction_deadline_rejects_without_resuming(self):
        with self.assertRaisesRegex(h.Rejected,'function time budget'):
            self.record(bytes.fromhex('ebfe'),'small-v1',max_seconds=0)

    def test_trial_preserves_failure_reason_and_seed(self):
        f=dict(addr=0x400000,size=1,body=[0x400000],calls=[],ext=True)
        with patch.object(h,'run_sparse',side_effect=h.Rejected('runaway')):
            r=trials.trial((f,'small-v1',3,20))
        self.assertEqual(r['cases'],[])
        self.assertEqual([a['reason'] for a in r['attempts']],['runaway']*3)
        self.assertEqual([a['seed'] for a in r['attempts']],[(0x400000<<8)^i for i in (1,2,3)])

    def test_adoption_preserves_baseline_and_rejects_stale_boundary(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
            old=dict(addr=1,body=[1],calls=[],size=1,arms=['old'])
            new=dict(addr=2,body=[2],calls=[],size=1,arms=['new'],ext=True)
            oldcase=dict(addr=1,fixture='unchanged')
            for path,value in [('patterns/lift.json',[old]),('vectors/lift.json',[oldcase]),
                               ('patterns/structured_candidates.json',[new]),
                               ('patterns/lift_novector.json',[2,3]),
                               ('patterns/structured_boundary.json',trials.boundary([old]))]:
                trials.write(root/path,value)
            record=dict(addr=2,body_key=trials.lift.vector_key(new),cases=[dict(addr=2)])
            path=root/'structured-small-v1.jsonl'
            path.write_text(json.dumps({**record,'body_key':'stale'})+'\n'+json.dumps(record)+'\n'+json.dumps(record)+'\n')
            trials.write(root/'structured_quarantine.json',[2])
            with contextlib.redirect_stdout(io.StringIO()):trials.adopt(root,'small-v1')
            self.assertEqual(trials.read(root/'patterns/lift.json'),[old])
            self.assertEqual(trials.read(root/'vectors/lift.json'),[oldcase])
            trials.write(root/'structured_quarantine.json',[])
            with contextlib.redirect_stdout(io.StringIO()):trials.adopt(root,'small-v1')
            self.assertEqual(trials.read(root/'patterns/lift.json'),[old,new])
            self.assertEqual(trials.read(root/'vectors/lift.json'),[oldcase,dict(addr=2)])
            self.assertEqual(trials.read(root/'patterns/lift_novector.json'),[3])
            with contextlib.redirect_stdout(io.StringIO()):trials.adopt(root,'small-v1')
            self.assertEqual(trials.read(root/'structured-small-v1-adopted.json'),[2])
            self.assertEqual(trials.read(root/'vectors/lift.json'),[oldcase,dict(addr=2)])
            with self.assertRaisesRegex(ValueError,'boundary changed'):
                trials.check_boundary(root,[{**old,'body':[1,3]},new])


if __name__=='__main__':unittest.main()
