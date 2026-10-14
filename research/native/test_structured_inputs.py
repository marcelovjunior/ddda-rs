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
    RANGE_LAYOUT = dict(stride=20, first={'stack':8}, last={'stack':12}, result={'stack':4},
                        uint_offsets=[8,12], float_offsets=[16])
    def record(self, code, strategy=None, seed=19, **kwargs):
        if strategy == 'guarded-state-v1':
            kwargs.setdefault('initial_state',dict(pages={},heap=h.HEAP_BASE,registers={}))
        if strategy == 'record-range-v1':
            kwargs.setdefault('input_layout',self.RANGE_LAYOUT)
        elif strategy == 'sentinel-records-v1':
            kwargs.setdefault('input_layout',dict(stride=8, first={'register':'edx'},
                                                  sentinel_offset=0, sentinel=0xffffffff))
        elif strategy == 'body-call-v1':
            kwargs.setdefault('input_layout',dict(bodies={'0x400000':[0x400000]}))
        elif strategy == 'slot-sweep-v1':  # L48: a register index binding over small-v1
            kwargs.setdefault('input_layout',dict(index={'register':'eax'},value=1))
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

    def test_zero_call_stack_exercises_zero_allocation_alignment(self):
        # Synthetic alignment wrapper followed by a call-free stack mover:
        # eax += (esp+4-eax)&mask; then transfer the return PC to esp-eax.
        # With zero eax, aligned entry ESP requires allocation; a CALL from
        # an aligned caller (entry ESP = aligned-4) requires none.
        for mask in (7, 15):
            code = bytes.fromhex('51 8d4c2408 2bc8 83e1') + bytes([mask])
            code += bytes.fromhex('03c1 59 51 8d4c2404 2bc8 8bc1 59 94 8b00 890424 c3')
            with self.assertRaisesRegex(h.Rejected, 'misaligned stack'):
                self.record(code, zero_inputs=True)
            c = self.record(code, 'zero-call-v1')
            self.assertEqual(c['regs_in']['esp'] % 16, 12)
            self.assertEqual(c['regs_in']['eax'], 0)
            self.assertEqual(c['regs_out']['esp'], c['regs_in']['esp'] + 4)
            self.assertNotIn('stack_contract', c)
            self.assertEqual(c, self.record(code, 'zero-call-v1'))
            self.assertEqual(trials._inputs([c]), dict(input_strategy='zero-call-v1'))

    def test_range_inputs_supply_related_stack_arguments_and_keep_guest_writes(self):
        # Read first/end/result, write byte length and the first float to result;
        # overwrite first record then read that same word back into EAX.
        code=bytes.fromhex('8b4c2408 8b54240c 2bd1 8b442404 8910 f30f104110 f30f114004 c70178563412 8b01 c3')
        c=self.record(code,'record-range-v1',seed=1)
        from record_inputs import RecordRangeInputs
        r=RecordRangeInputs(self.RANGE_LAYOUT,1)
        self.assertEqual(c['regs_out']['eax'],0x12345678)
        self.assertIn([r.RESULT,struct.pack('<If',r.count*20,0.0).hex()],c['writes'])
        self.assertIn([r.first,'78563412'],c['writes'])
        self.assertEqual(c['input_layout'],self.RANGE_LAYOUT)
        self.assertEqual(trials._inputs([c]),dict(input_strategy='record-range-v1',input_layout=self.RANGE_LAYOUT))

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

    def test_retry_reruns_only_matching_failures(self):
        rec=lambda a,reason,cases=():dict(addr=a,body_key='k',requested_seeds=3,seconds=20,
                                         attempts=[dict(reason=reason)],cases=list(cases))
        prior=[rec(1,'memset of 0x80000 bytes (0x80000 in this run)'),rec(2,'runaway'),rec(3,'ok',[{}]),
               dict(rec(4,'memset of 0x60000 bytes'),requested_seeds=12)]
        self.assertEqual(trials.done_keys(prior,3,20),{(1,'k'),(2,'k'),(3,'k')})
        self.assertEqual(trials.done_keys(prior,3,20,'memset|memcpy'),{(2,'k'),(3,'k')})

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
            if a==0x60 and (not caller_records or
                           isinstance(caller_records,str) and kw.get('input_strategy') != caller_records):
                raise h.Rejected('runaway')
            return dict(regs_in={},fresh=True)
        lifts=[]
        def relift(a,size,callable,ext=False,x87_inputs=False):
            lifts.append((a,ext,0x20 in callable));return ['closed arm' if not ext else 'ext arm']
        with (patch.object(trials.lift,'seed_closed_contracts',return_value=set()),
              patch.object(trials.lift,'lift',side_effect=relift),
              patch.object(trials.lift,'release_body'),
              patch.object(trials.lift,'noreturn_entries',return_value=set()),
              patch.dict(trials.lift.BODY_INFO,{a:dict(body=[a]) for a in (0x10,0x20,0x30,0x40,0x60,0x70)}),
              patch.object(h,'run_sparse',side_effect=run),
              patch.object(h,'frame_exit',side_effect=lambda a,body:('ebp',4,()) if a in (0x10,0x20) else ('esp',4,())),
              patch.object(h,'moves_frame',return_value=False),
              contextlib.redirect_stdout(io.StringIO())):
            (root/'structured-random.jsonl').write_text('')
            with self.assertRaisesRegex(ValueError,'archive the trial logs'):
                trials.promote(root)
            (root/'structured-random.jsonl').unlink()
            trials.promote(root)
        return {f['addr']:f for f in trials.read(root/'patterns/lift.json')},trials.read(root/'vectors/lift.json'),runs

    def test_promotion_of_a_mocked_x87_helper_by_address(self):
        # L46: no candidate calls 0x80 (its caller cannot lift until it is
        # closed); --address names it. Its arms do x87 work, so a mock is no
        # stand-in: it is promoted although 0x90 mocks it, keeping its x87
        # entry contract, and 0x90 is re-lifted and re-recorded.
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
            helper=dict(addr=0x80,size=4,body=[0x80],calls=[],arms=['arm'],ext=True,x87_entry=1)
            caller=dict(addr=0x90,size=4,body=[0x90],calls=[],arms=['{ let t = 0x80u32; x86::call_ext(m, cpu, t); }'],ext=True)
            trials.write(root/'patterns/lift.json',[helper,caller])
            trials.write(root/'vectors/lift.json',[dict(addr=0x80),dict(addr=0x90,calls=[{'target':0x80}])])
            trials.write(root/'patterns/structured_candidates.json',[])
            seen=[]
            def relift(a,size,callable,ext=False,x87_inputs=False):
                seen.append((a,ext,x87_inputs));return ['{ st.register::<0xf0d9>(); }'] if not ext else ['ext arm']
            with (patch.object(trials.lift,'seed_closed_contracts',return_value=set()),
                  patch.object(trials.lift,'lift',side_effect=relift),
                  patch.object(trials.lift,'release_body'),
                  patch.object(trials.lift,'noreturn_entries',return_value=set()),
                  patch.dict(trials.lift.BODY_INFO,{0x80:dict(body=[0x80],x87_entry=1),0x90:dict(body=[0x90])}),
                  patch.object(h,'run_sparse',return_value=dict(regs_in={},fresh=True)),
                  patch.object(h,'frame_exit',return_value=('esp',4,())),
                  patch.object(h,'moves_frame',return_value=False),
                  contextlib.redirect_stdout(io.StringIO())):
                trials.promote(root)
                self.assertEqual(trials.read(root/'patterns/lift.json')[0].get('ext'),True)  # not a target
                trials.promote(root,targets=[0x80])
            selected={f['addr']:f for f in trials.read(root/'patterns/lift.json')}
            self.assertNotIn('ext',selected[0x80])
            self.assertEqual(selected[0x80]['x87_entry'],1)
            self.assertIn((0x80,False,True),seen)
            self.assertEqual(selected[0x90]['arms'],['ext arm'])
            self.assertTrue(all(c.get('fresh') for c in trials.read(root/'vectors/lift.json')))

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

    def test_promotion_tries_call_stack_distribution_for_affected_caller(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
            selected,out,runs=self.promote_fixture(root,caller_records='zero-call-v1')
            self.assertNotIn('ext',selected[0x20])
            self.assertIn((0x60,'zero-call-v1',True,True),runs)
            self.assertTrue(all(c.get('fresh') for c in out if c['addr']==0x60))

    def test_range_layout_is_reproduced_and_distinguishes_retry_keys(self):
        layout=dict(stride=4,first={'stack':4},last={'stack':8})
        case=dict(input_strategy='record-range-v1',input_layout=layout)
        self.assertEqual(trials._inputs([case]),case)
        row=dict(addr=1,body_key='k',requested_seeds=3,seconds=20,cases=[],input_layout=layout)
        self.assertEqual(trials.done_keys([row],3,20,layout=layout),{(1,'k')})
        self.assertFalse(trials.done_keys([row],3,20,layout={**layout,'stride':8}))

    def test_refresh_closed_is_all_or_nothing_and_preserves_unrelated_cases(self):
        for success in (False,True):
            with tempfile.TemporaryDirectory() as folder:
                root=Path(folder);(root/'patterns').mkdir();(root/'vectors').mkdir()
                functions=[dict(addr=1,arms=['closed'],calls=[],size=1,body=[1]),
                           dict(addr=2,ext=True,arms=['old'],calls=[],size=1,body=[2],x87_entry=1,insn_budget=30000)]
                cases=[dict(addr=1,seed=9),dict(addr=2,calls=[dict(target=1)])]
                trials.write(root/'patterns/lift.json',functions);trials.write(root/'vectors/lift.json',cases)
                with (patch.object(trials.lift,'seed_closed_contracts',return_value={1}),
                      patch.object(trials.lift,'noreturn_entries',return_value=set()),
                      patch.object(trials.lift,'lift',return_value=['new']),
                      patch.object(trials.lift,'release_body'),
                      patch.dict(trials.lift.BODY_INFO,{2:dict(body=[2])}),
                      patch.object(trials,'_rerecord_any',return_value=[dict(addr=2,calls=[])] if success else []),
                      contextlib.redirect_stdout(io.StringIO())):
                    if success:trials.refresh_closed(root)
                    else:
                        with self.assertRaisesRegex(ValueError,'untouched'):trials.refresh_closed(root)
                if success:
                    self.assertEqual(trials.read(root/'vectors/lift.json'),[cases[0],dict(addr=2,calls=[])])
                    self.assertEqual(trials.read(root/'patterns/lift.json')[0],functions[0])
                    self.assertNotIn('x87_entry',trials.read(root/'patterns/lift.json')[1])
                    self.assertNotIn('insn_budget',trials.read(root/'patterns/lift.json')[1])
                else:
                    self.assertEqual(trials.read(root/'vectors/lift.json'),cases)
                    self.assertEqual(trials.read(root/'patterns/lift.json'),functions)

if __name__=='__main__':unittest.main()


class PrepareTests(unittest.TestCase):
    def test_a_tail_of_its_jumpers_is_never_a_candidate(self):
        # L58 (head #9879): an entry the lifter decodes into its jumpers
        # (frame_tail) gets no standalone record
        import sqlite3
        import lift
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); (root/'patterns').mkdir()
            (root/'patterns/lift.json').write_text('[]')
            db = root/'inventory.sqlite'
            with contextlib.closing(sqlite3.connect(db)) as conn:
                conn.execute('CREATE TABLE functions(address INTEGER, namespace TEXT, name TEXT, size INTEGER, status TEXT)')
                conn.executemany('INSERT INTO functions VALUES (?,?,?,?,?)',
                                 [(0x1000, 'G', 'tail', 4, 'untouched'), (0x2000, 'G', 'fn', 4, 'untouched')])
                conn.commit()
            with patch.object(lift, 'DB', str(db)), patch.object(lift, 'lift', return_value=['return;']), \
                 patch.object(lift, 'inlined_tail', side_effect=lambda a: a == 0x1000), \
                 patch.object(lift, 'release_body'), patch.dict(lift.BODY_INFO, {0x2000: dict(body=[0x2000])}), \
                 contextlib.redirect_stdout(io.StringIO()):
                trials.prepare(root)
            self.assertEqual([c['addr'] for c in json.loads((root/'patterns/structured_candidates.json').read_text())], [0x2000])
            self.assertIn(str(0x1000), json.loads((root/'patterns/structured_rejections.json').read_text()))
