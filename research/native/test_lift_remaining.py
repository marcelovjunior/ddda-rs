"""L12 synthetic switch bounds, delayed float returns and terminal traps."""
import gc
import struct
import unittest
from unittest.mock import patch
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
import harness as h
import lift

class RemainingTests(unittest.TestCase):
    def tearDown(self):gc.collect()

    def switch(self, code=None, remap=bytes((0,1,1,0)), targets=(0x401000,0x401010)):
        code=code or bytes.fromhex('83f803770e0fb68800304000ff248d00204000c3')
        sections=[(0x400000,code.ljust(4096,b'\xcc')),
                  (0x401000,(b'\xc3'+b'\xcc'*15+b'\xc3').ljust(4096,b'\xcc')),
                  (0x402000,struct.pack('<'+'I'*len(targets),*targets)+b'\0'*4),
                  (0x403000,remap)]
        return code,sections

    def test_cross_register_remap_with_original_reads(self):
        code,sections=self.switch()
        with patch.object(h,'image',return_value=(0x400000,sections)),patch.object(h,'executable',return_value=True),patch.object(lift,'function_entries',return_value=frozenset({0x400000})):
            arms=lift.lift(0x400000,len(code))
            body=lift.BODY_INFO[0x400000]
            self.assertIn(0x401010,body['body'])
            c=h.run_sparse(0x400000,17,zero_inputs=True,size=len(code),**{k:v for k,v in body.items() if k in ('body','noreturn_sites')})
            reads={a+k for a,raw in c['reads'] for k in range(len(bytes.fromhex(raw)))}
            self.assertIn(0x403000,reads);self.assertIn(0x402000,reads)
            self.assertIn('m.u8', ' '.join(arms));self.assertIn('m.u32', ' '.join(arms))

    def test_remap_requires_source_bound_and_valid_targets(self):
        for kw in ({'code':bytes.fromhex('83fa03770e0fb68800304000ff248d00204000c3')},
                   {'remap':bytes((0,1,2,0))}, {'targets':(0x400001,0x401010)},
                   {'remap':bytes((0,1))}):
            code,sections=self.switch(**kw)
            with patch.object(h,'image',return_value=(0x400000,sections)),patch.object(h,'executable',return_value=True),patch.object(lift,'function_entries',return_value=frozenset({0x400000})):
                with self.assertRaises(lift.Unsupported):
                    lift.lift(0x400000,len(code))
                # L22 ext open tables: an unproven remap uses table-only
                # discovery (unknown runtime targets leave the body); bad
                # targets or remap bytes still reject.
                if 'code' in kw or kw.get('remap')==bytes((0,1)):
                    lift.lift(0x400000,len(code),ext=True)
                    self.assertEqual(lift.cached_body(0x400000,len(code),True)[1][0x40000c],[0x401000,0x401010])
                else:
                    with self.assertRaises(lift.Unsupported):
                        lift.lift(0x400000,len(code),ext=True)
            lift._BODY_CACHE.clear()

    def program(self,text,record=False):
        code=bytes.fromhex(text);sections=[(0x400000,code.ljust(4096,b'\xcc'))]
        with patch.object(h,'image',return_value=(0x400000,sections)),patch.object(h,'executable',return_value=True),patch.object(lift,'function_entries',return_value=frozenset({0x400000})):
            arms=lift.lift(0x400000,len(code),ext=True)
            info=lift.BODY_INFO[0x400000];push=lift.X87_PUSH_SITES[0x400000]
            if record:
                case=h.run_sparse(0x400000,17,zero_inputs=True,end=0x400000+len(code),
                                  body=info['body'],noreturn_sites=info['noreturn_sites'],x87_push=push)
                return arms,push,case
            return arms,push,info

    def test_float_return_after_local_work_and_branch(self):
        arms,push,c=self.program('e8fb000000d9e885c07402d9e0dec9c3',True)
        self.assertEqual(push,[0x400005])
        self.assertEqual(c['x87_depth'],1)
        self.assertEqual(c['st0_out'],c['calls'][0]['out']['st_push'])

    def test_existing_value_needs_no_extra_return(self):
        _,push,c=self.program('d9e8e8f9000000d91d00200000c3',True)
        self.assertEqual(push,[]);self.assertEqual(c['x87_depth'],0)
        self.assertNotIn('st_push',c['calls'][0]['out'])

    def test_two_missing_values_reject(self):
        with self.assertRaises(lift.Unsupported):self.program('e8fb000000ded9c3')

    def test_entry_arguments_are_an_explicit_contract(self):
        # L22: x87 values read before any call are the caller's (MSVC float
        # intrinsics); a read after a call stays the mock's or rejects.
        _,push,info=self.program('d9e0c3')
        self.assertEqual(info['x87_entry'],1);self.assertEqual(push,[])
        _,_,info=self.program('d9e0dec9c3')
        self.assertEqual(info['x87_entry'],2)
        _,push,info=self.program('d9e0e8f7000000ded9c3')  # caller value + one mocked return
        self.assertEqual((info['x87_entry'],push),(1,[0x400007]))
        with self.assertRaises(lift.Unsupported):self.program('d9e0e8f7000000ded9ded9c3')

    def test_intervening_call_owns_the_return(self):
        _,push,_=self.program('e8fb000000e8f6000000d91d00200000c3')
        self.assertEqual(push,[0x40000a])

    def test_terminal_traps_reject_only_taken_paths(self):
        for trap in ('cc','0f0b'):
            # Zero inputs take JZ over the terminal trap.
            _,_,c=self.program('85c074'+f'{len(bytes.fromhex(trap)):02x}'+trap+'c3',True)
            self.assertEqual(c['regs_out']['esp'],c['regs_in']['esp']+4)
            arms,_,info=self.program(trap)
            self.assertIn('panic!', ' '.join(arms))
            self.assertEqual(info['noreturn_sites'],[0x400000])
            with self.assertRaisesRegex(h.Rejected,'noreturn'):
                self.program(trap,True)

    def test_xlatb_reads_ebx_plus_al(self):
        arms,_,c=self.program('d7c3',True)
        self.assertIn('m.u8(ebx.wrapping_add(eax & 0xFF))',' '.join(arms))
        self.assertEqual(c['regs_out']['eax'] & ~0xFF, c['regs_in']['eax'] & ~0xFF)
        with self.assertRaises(lift.Unsupported):self.program('64d7c3')  # fs: override

    def test_platform_state_has_explicit_contracts(self):
        # L22: CPUID/RDTSC answers are recorded machine events, segment
        # reads record the selectors, POPFD follows CPL3 or rejects.
        import machine_events
        arms,_,c=self.program('0fa2c3',True)
        self.assertIn('crate::platform::cpuid(cpu);',' '.join(arms))
        self.assertEqual(c['machine'],[['cpuid',0,0,*machine_events.host_cpuid(0,0)]])
        arms,_,c=self.program('0f31c3',True)
        self.assertEqual([e[0] for e in c['machine']],['rdtsc'])
        arms,_,c=self.program('9c9dc3',True)
        self.assertIn('x86::pushfd_image(cpu)',' '.join(arms));self.assertIn('x86::popfd(cpu, v)',' '.join(arms))
        _,_,c=self.program('668cd0c3',True)
        self.assertEqual(set(c['segments']),{'es','cs','ss','ds','fs','gs'})
        with self.assertRaisesRegex(h.Rejected,'popfd'):self.program('68000100009dc3',True)  # sets TF
        with self.assertRaisesRegex(lift.Unsupported,'push segment'):self.program('1ec3')

    def test_interleaved_local_allocation_is_not_a_mock_argument(self):
        _,_,c=self.program('8b018b501083ec0889f1ffd231c083c408c3',True)
        self.assertEqual(c['calls'][0]['out']['pop'],0)
        self.assertEqual(c['regs_out']['esp'],c['regs_in']['esp']+4)
        # A mismatched epilogue still rejects; no guard is bypassed.
        with self.assertRaises(h.Rejected):
            self.program('8b018b501083ec0889f1ffd231c083c404c3',True)

    def test_initialized_outgoing_space_is_not_a_local_hint(self):
        code=bytes.fromhex('9083ec08c7042401000000ffd083c408c3')
        body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,0x400000)]
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,code)])):
            self.assertEqual(h.local_alloc_sites(0x400000,body),set())

if __name__=='__main__':unittest.main()
