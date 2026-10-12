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
        for name in h.PAGE_STRATEGIES:
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

    def test_explicit_image_policy_preserves_original_bytes_and_random_inputs(self):
        code=bytes.fromhex('a1000240008b11c3').ljust(0x200,b'\xcc')+struct.pack('<I',0x12345678)
        ordinary=self.record(code)
        image=self.record(code,'image-v1')
        self.assertEqual(image.pop('input_strategy'),'image-v1')
        self.assertEqual(image,ordinary)

    def test_mockzero_keeps_random_memory_and_zeroes_mock_returns(self):
        code=bytes.fromhex('a1000240008b11c3').ljust(0x200,b'\xcc')+struct.pack('<I',0x12345678)
        ordinary=self.record(code)
        zero=self.record(code,'mockzero-v1')
        self.assertEqual(zero.pop('input_strategy'),'mockzero-v1')
        self.assertEqual(zero,ordinary)
        # wait: call mocked 0x400100; test eax,eax; jnz wait; mov eax,[ecx]; ret
        wait=bytes.fromhex('e8fb00000085c075f78b01c3')
        c=self.record(wait,'mockzero-v1',end=0x40000c)
        self.assertEqual(len(c['calls']),1)
        self.assertEqual({k:c['calls'][0]['out'][k] for k in ('eax','ecx','edx','xmm0')},
                         dict(eax=0,ecx=0,edx=0,xmm0=[0]*4))
        with self.assertRaisesRegex(h.Rejected,'runaway'):
            self.record(wait,end=0x40000c,max_insn=2000)
        # call mocked float 0x400100; fstp dword [ecx]; ret
        c=self.record(bytes.fromhex('e8fb000000d919c3'),'mockzero-v1',end=0x400008,x87_push={0x400005})
        self.assertEqual(c['calls'][0]['out']['st_push'],0)
        # The mock also returned ECX = 0, so the zero float lands at address 0.
        self.assertIn([0,'00000000'],c['writes'])
        # Page variants: the page strategy's inputs, zero mocked returns.
        for name,pages in (('small-mockzero-v1','small-v1'),('objects-mockzero-v1','objects-v1')):
            self.assertEqual(h.MOCKZERO[name],pages)
            zero=self.record(code,name)
            base=self.record(code,pages)
            self.assertEqual((zero.pop('input_strategy'),base.pop('input_strategy')),(name,pages))
            self.assertEqual(zero,base)
            c=self.record(wait,name,end=0x40000c)
            self.assertEqual({k:c['calls'][0]['out'][k] for k in ('eax','ecx','edx','xmm0')},
                             dict(eax=0,ecx=0,edx=0,xmm0=[0]*4))

    def test_original_model_entry_is_distinct_and_callees_still_use_models(self):
        a=0x00CF73F0
        for text,expected in [('b878563412c3',0x12345678),
                              ('e8fbfffffff3c3',h.HEAP_BASE)]:
            code=bytes.fromhex(text)
            body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,a)]
            with patch.object(h,'image',return_value=(a,[(a,code.ljust(4096,b'\xcc'))])):
                c=h.run_sparse(a,19,body=body,end=a+len(code),original_entry=True)
                self.assertEqual(c['regs_out']['eax'],expected)
                self.assertEqual(c['calls'],[])
                model=h.run_sparse(a,19,body=body,end=a+len(code))
                self.assertEqual(model['regs_out']['eax'],h.HEAP_BASE)
        f=dict(addr=a,body=body,calls=[],ext=True)
        self.assertNotEqual(trials.lift.vector_key(f),trials.lift.vector_key({**f,'original_entry':True}))

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

    def promote_fixture(self, root, caller_records):
        ext=lambda a,arms=('arm',):dict(addr=a,size=4,body=[a],calls=[],arms=list(arms),ext=True)
        free,mocked,calling,unwanted=ext(0x10),ext(0x20),ext(0x30,('x86::call_ext(m, cpu, 0x99)',)),ext(0x40)
        user=ext(0x60,('{ let t = 0x20u32; x86::call_ext(m, cpu, t); }','{ let t = 0x70u32; x86::call_ext(m, cpu, t); }'))
        plain=ext(0x70)
        caller=dict(addr=0x50,arms=['{ let t = 0x10u32; x86::call_ext(m, cpu, t); }',
                                    '{ let t = 0x20u32; x86::call_ext(m, cpu, t); }',
                                    '{ let t = 0x70u32; x86::call_ext(m, cpu, t); }',
                                    '(|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_ext(m, cpu, 0x30))(m, cpu);'])
        cases=[dict(addr=0x10,input_strategy='small-v1'),dict(addr=0x20),
               dict(addr=0x60,input_strategy='objects-mockzero-v1',calls=[{'target':0x20},{'target':0x70}])]
        for path,value in [('patterns/lift.json',[free,mocked,calling,unwanted,user,plain]),('vectors/lift.json',cases),
                           ('patterns/structured_candidates.json',[caller])]:
            trials.write(root/path,value)
        runs=[]
        def run(a,seed,**kw):
            runs.append((a,kw.get('input_strategy'),'end' in kw,0x20 in kw.get('lifted',())))
            if a==0x60 and not caller_records:
                raise h.Rejected('runaway')
            return dict(regs_in={},fresh=True)
        lifts=[]
        def relift(a,size,callable,ext=False):
            lifts.append((a,ext,0x20 in callable));return ['closed arm' if not ext else 'ext arm']
        with (patch.object(trials.lift,'seed_closed_contracts',return_value=set()),
              patch.object(trials.lift,'lift',side_effect=relift),
              patch.object(trials.lift,'release_body'),
              patch.object(trials.lift,'noreturn_entries',return_value=set()),
              patch.dict(trials.lift.BODY_INFO,{a:dict(body=[a]) for a in (0x10,0x20,0x30,0x40,0x60,0x70)}),
              patch.object(h,'run_sparse',side_effect=run),
              patch.object(h,'frame_exit',side_effect=lambda a,body:('ebp',4,()) if a in (0x10,0x20) else ('esp',4,())),
              contextlib.redirect_stdout(io.StringIO())):
            (root/'structured-random.jsonl').write_text('')
            with self.assertRaisesRegex(ValueError,'archive the trial logs'):
                trials.promote(root)
            (root/'structured-random.jsonl').unlink()
            trials.promote(root)
        return {f['addr']:f for f in trials.read(root/'patterns/lift.json')},trials.read(root/'vectors/lift.json'),runs

    def test_promotion_rerecords_mocking_callers(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
            selected,out,runs=self.promote_fixture(root,caller_records=True)
            for a in (0x10,0x20):
                self.assertNotIn('ext',selected[a])
                self.assertEqual(selected[a]['arms'],['closed arm'])
                self.assertEqual(selected[a]['frame_exit'],['ebp',4,[]])
            self.assertEqual(selected[0x30]['arms'],['x86::call_ext(m, cpu, 0x99)'])   # makes a call
            self.assertEqual(selected[0x40]['arms'],['arm'])                          # no candidate calls it
            self.assertTrue(selected[0x70]['ext'] and selected[0x70]['arms']==['arm'])   # ordinary and mocked
            self.assertTrue(selected[0x60]['ext'])
            self.assertEqual(selected[0x60]['arms'],['ext arm'])                      # re-lifted against 0x20 closed
            self.assertIn((0x10,'small-v1',False,False),runs)
            self.assertIn((0x60,'objects-mockzero-v1',True,True),runs)
            self.assertEqual(sorted({c['addr'] for c in out}),[0x10,0x20,0x60])
            self.assertTrue(all(c.get('fresh') and c['body_key']==trials.lift.vector_key(selected[c['addr']]) for c in out))
            self.assertEqual([c['seed'] for c in out if c['addr']==0x10],[(0x10<<8)^i for i in (1,2,3)])
            self.assertEqual(trials.frame_contracts(list(selected.values())),{0x10:['ebp',4,[]],0x20:['ebp',4,[]]})
            self.assertEqual(trials.read(root/'structured-promoted.json'),
                             dict(promoted=[0x10,0x20],rerecorded=[0x60],kept_ext=[]))

    def test_promotion_keeps_target_ext_when_a_caller_cannot_rerecord(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
            selected,out,runs=self.promote_fixture(root,caller_records=False)
            self.assertNotIn('ext',selected[0x10])
            self.assertTrue(selected[0x20]['ext'] and selected[0x20]['arms']==['arm'])
            self.assertEqual(selected[0x60]['arms'][0],'{ let t = 0x20u32; x86::call_ext(m, cpu, t); }')
            self.assertIn(dict(addr=0x20),out)
            self.assertIn(dict(addr=0x60,input_strategy='objects-mockzero-v1',calls=[{'target':0x20},{'target':0x70}]),out)
            self.assertEqual(trials.read(root/'structured-promoted.json'),
                             dict(promoted=[0x10],rerecorded=[],kept_ext=[0x20]))

if __name__=='__main__':unittest.main()
