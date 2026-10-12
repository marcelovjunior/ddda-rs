"""Synthetic original x86 tests: setup state is input, never a patched method."""
import gc
import struct
import unittest
from unittest.mock import patch

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
import constructed_inputs as c
import structured_inputs as s


class ConstructedTests(unittest.TestCase):
    def tearDown(self):
        gc.collect()

    def fixture(self, setup, method):
        self.setup, self.method = bytes.fromhex(setup), bytes.fromhex(method)
        data = bytearray(b'\xcc'*4096)
        data[:len(self.setup)] = self.setup
        data[256:256+len(self.method)] = self.method
        data = bytes(data)
        for mock in (patch.object(h, 'image', return_value=(0x400000, [(0x400000, data)])),
                     patch.object(h, 'code', side_effect=lambda a, n: data[a-0x400000:a-0x400000+n]),
                     patch.object(h, 'executable', side_effect=lambda a, length=1: 0x400000 <= a < 0x401000)):
            mock.start()
            self.addCleanup(mock.stop)

    def run_body(self, f, seed, **kw):
        code = self.setup if f['addr'] == 0x400000 else self.method
        body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(code, f['addr'])]
        return h.run_sparse(f['addr'], seed, body=body, end=f['addr']+len(code), **kw)

    def build(self):
        f = dict(addr=0x400000)
        case = dict(addr=f['addr'], seed=1, body_key='test',
                    **self.run_body(f, 1, input_strategy='guarded-objects-v1'))
        state, recipe = c.construct(f, case, {'ecx': ['regs_in', 'ecx']}, self.run_body, 'test')
        return state, recipe, case

    def test_constructor_store_becomes_method_read_and_no_setup_write_leaks(self):
        # constructor writes [this+4]=9; method increments it and returns it.
        self.fixture('c7410409000000 c3', 'ff4104 8b4104 c3')
        state, recipe, case = self.build()
        result = self.run_body(dict(addr=0x400100), 2, initial_state=state, input_strategy='zero-call-v1')
        self.assertEqual(result['regs_out']['eax'], 10)
        address = case['regs_in']['ecx']+4
        reads = {a+k: b for a, v in result['reads'] for k, b in enumerate(bytes.fromhex(v))}
        self.assertEqual(bytes(reads[address+k] for k in range(4)), struct.pack('<I', 9))
        self.assertEqual(recipe['case_hash'], c.digest(case))
        self.assertNotIn('image_budget', result)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def chain(self):
        phases=[]
        for address,seed in ((0x400000,1),(0x400100,2)):
            f=dict(addr=address)
            accepted=dict(addr=address,seed=seed,body_key='test',
                          **self.run_body(f,seed,input_strategy='zero-call-v1'))
            phases.append((f,accepted,'test'))
        return c.construct_chain(phases,{},self.run_body)

    def test_sequence_retains_disjoint_pages_and_written_facts(self):
        self.fixture('c7050000002003000000 c3','c7050010002004000000 c3')
        state,recipe=self.chain()
        self.assertEqual(state['pages'][0x20000000][:4],struct.pack('<I',3))
        self.assertEqual(state['pages'][0x20001000][:4],struct.pack('<I',4))
        self.assertEqual(state['writes'][0x20000000],3)
        self.assertEqual(state['writes'][0x20001000],4)
        self.assertEqual(len(recipe['setups']),2)
        self.assertEqual(recipe['rule'],'constructed-chain-v1')

    def test_sequence_retains_legacy_random_distribution(self):
        self.fixture('c3','c3')
        phases=[]
        for address,seed in ((0x400000,1),(0x400100,2)):
            f=dict(addr=address)
            accepted=dict(addr=address,seed=seed,body_key='test',**self.run_body(f,seed))
            phases.append((f,accepted,'test'))
        c.construct_chain(phases,{},self.run_body)

    def test_sequence_reproves_bounded_producer_on_prior_state(self):
        self.fixture('c7050000002003000000 c3', 'b9401f0000 31c0 40 49 75fc c3')
        phases=[]
        for address,seed,strategy in ((0x400000,1,'zero-call-v1'),
                                      (0x400100,2,'guarded-bounded-v1')):
            f=dict(addr=address)
            accepted=dict(addr=address,seed=seed,body_key='test',
                          **self.run_body(f,seed,input_strategy=strategy))
            phases.append((f,accepted,'test'))
        self.assertEqual(phases[1][1]['regs_out']['eax'],8000)
        self.assertIn('image_budget',phases[1][1])
        state,_=c.construct_chain(phases,{},self.run_body)
        self.assertEqual(state['writes'][0x20000000],3)

    def test_sequence_orders_overwrites_but_rejects_changed_read_dependencies(self):
        self.fixture('c7050000002003000000 c3','c7050000002004000000 c3')
        state,_=self.chain()
        self.assertEqual(state['writes'][0x20000000],4)
        self.assertEqual(state['pages'][0x20000000][:4],struct.pack('<I',4))
        self.fixture('c7050000002003000000 c3','a100000020 40 a304000020 c3')
        with self.assertRaisesRegex(h.Rejected,'replay differs'):
            self.chain()

    def test_sequence_rejects_register_rebinding_and_keeps_heap_reservations(self):
        self.fixture('c3','c3')
        state,_,accepted=self.build()
        with self.assertRaisesRegex(ValueError,'accepted register distribution'):
            c.construct(dict(addr=0x400000),accepted,{},self.run_body,'test',initial_state=state)
        state={**state,'registers':{},'heap':h.HEAP_BASE+32}
        actual,_=c.construct(dict(addr=0x400000),accepted,{},self.run_body,'test',initial_state=state)
        self.assertEqual(actual['heap'],h.HEAP_BASE+32)

    def test_capture_is_observational_and_replay_mismatch_refuses(self):
        self.fixture('c7410409000000 c3', 'c3')
        state, _, case = self.build()
        capture = {}
        self.assertEqual(self.run_body(dict(addr=0x400000), 1, capture_state=capture,
                                      input_strategy='guarded-objects-v1'),
                         {k: v for k, v in case.items() if k not in ('addr', 'seed', 'body_key')})
        case['regs_out']['eax'] ^= 1
        with self.assertRaisesRegex(h.Rejected, 'replay differs'):
            c.construct(dict(addr=0x400000), case, {}, self.run_body, 'test')
        with self.assertRaisesRegex(ValueError, 'case/body mismatch'):
            c.construct(dict(addr=0x400000), case, {}, self.run_body, 'stale')
        with self.assertRaisesRegex(ValueError, 'checked setup recipe'):
            s._inputs([dict(construction={'rule': 'constructed-v1'})])

    def test_checked_setup_writes_can_bound_a_method_loop(self):
        # Setup stores a count; method reads it through its bound object.
        self.fixture('c74104401f0000 c3', '8b5104 31c0 40 4a 75fc c3')
        state, _, case = self.build()
        address = case['regs_in']['ecx'] + 4
        self.assertEqual(bytes(state['writes'][address+k] for k in range(4)),
                         struct.pack('<I', 8000))
        result = self.run_body(dict(addr=0x400100), 2, initial_state=state,
                               input_strategy='guarded-stores-v1')
        self.assertEqual(result['regs_out']['eax'], 8000)
        self.assertGreater(result['image_budget']['instructions'], 20000)
        # Captured but unwritten bytes are not proof facts.
        with self.assertRaisesRegex(h.Rejected, 'runaway'):
            self.run_body(dict(addr=0x400100), 2,
                          initial_state={**state, 'writes': {}},
                          input_strategy='guarded-stores-v1')
        with self.assertRaisesRegex(ValueError, 'differs from captured'):
            c.validate_state({**state, 'writes': {address: 1}})

    def test_explicit_setup_return_binding_is_a_proof_value(self):
        self.fixture('b8401f0000 c3','31c0 40 49 75fc c3')
        f=dict(addr=0x400000)
        accepted=dict(addr=f['addr'],seed=1,body_key='test',
                      **self.run_body(f,1,input_strategy='zero-call-v1'))
        state,_=c.construct(f,accepted,{'ecx':['regs_out','eax']},self.run_body,'test')
        result=self.run_body(dict(addr=0x400100),2,initial_state=state,input_strategy='guarded-stores-v1')
        self.assertEqual(result['regs_out']['eax'],8000)
        self.assertIn('image_budget',result)

    def test_method_overwrite_invalidates_setup_count(self):
        self.fixture('c74104401f0000 c3',
                     'c7410402000000 8b5104 31c0 40 4a 75fc c3')
        state, _, _ = self.build()
        result = self.run_body(dict(addr=0x400100), 2, initial_state=state,
                               input_strategy='guarded-stores-v1', max_insn=5)
        self.assertEqual(result['regs_out']['eax'], 2)
        self.assertEqual(result['image_budget']['instructions'], 10)

    def test_constructed_input_does_not_relax_instruction_or_return_guards(self):
        self.fixture('c3', 'ebfe')
        state, _, _ = self.build()
        with self.assertRaisesRegex(h.Rejected, 'instruction limit|runaway'):
            self.run_body(dict(addr=0x400100), 2, initial_state=state,
                          input_strategy='guarded-stack-v1', max_insn=10)
        self.method = bytes.fromhex('c7042400000000 c3')
        # New fixture also checks the original return target guard.
        self.fixture('c3', self.method.hex())
        state, _, _ = self.build()
        with self.assertRaisesRegex(h.Rejected, 'instead of the caller|outside the image'):
            self.run_body(dict(addr=0x400100), 2, initial_state=state, input_strategy='zero-call-v1')

    def test_invalid_state_and_heap_reuse_refuse(self):
        self.fixture('c3', 'c3')
        state, _, _ = self.build()
        for patch_state in (dict(registers={'esp': 0}), dict(heap=1), dict(pages={1: bytes(4096)})):
            with self.assertRaises(ValueError):
                c.validate_state({**state, **patch_state})
        # A modeled allocator cannot reuse the setup's allocated addresses.
        target = next(a for a, name in h.PLATFORM.items() if name == 'mt_alloc')
        code = b'\xe8'+struct.pack('<i', target-(0x400100+5))+b'\xc3'
        self.fixture('c3', code.hex())
        state, _, _ = self.build()
        with self.assertRaisesRegex(h.Rejected, 'heap-cursor replay'):
            self.run_body(dict(addr=0x400100), 2, initial_state={**state, 'heap': h.HEAP_BASE+16},
                          input_strategy='zero-call-v1')

    def test_legacy_seed_is_identified_but_all_recorded_observations_must_match(self):
        self.fixture('c7410409000000 c3', 'c3')
        f = dict(addr=0x400000)
        seed = (f['addr'] << 8) ^ 3
        case = dict(addr=f['addr'], body_key='test', **self.run_body(f, seed))
        # Legacy records had neither the seed nor later x87 observations.
        case.pop('x87_control')
        case.pop('st0_ext')
        state, recipe = c.construct(f, case, {'ecx': ['regs_in', 'ecx']}, self.run_body, 'test')
        self.assertEqual(recipe['seed'], seed)
        self.assertEqual(state['registers']['ecx'], case['regs_in']['ecx'])
        case['writes'] = []
        with self.assertRaisesRegex(h.Rejected, 'replay differs'):
            c.construct(f, case, {}, self.run_body, 'test')

    def test_setup_cannot_supply_modified_code_or_partial_failed_state(self):
        self.fixture('c7050001400009000000 c3', 'c3')
        capture = {}
        with self.assertRaisesRegex(h.Rejected, 'writes executable memory'):
            self.run_body(dict(addr=0x400000), 1, capture_state=capture,
                          input_strategy='zero-call-v1')
        self.assertEqual(capture, {})
        self.fixture('ebfe', 'c3')
        with self.assertRaisesRegex(h.Rejected, 'runaway'):
            self.run_body(dict(addr=0x400000), 1, capture_state=capture,
                          max_insn=10, input_strategy='zero-call-v1')
        self.assertEqual(capture, {})


if __name__ == '__main__':
    unittest.main()
