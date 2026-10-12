"""Image-only control flow bounds, including deliberate proof refusals."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from image_budget import image_bound, WriteGuard
import harness as h


class ImageBudgetTests(unittest.TestCase):
    # Load an image-backed trip count, then increment EAX that many times.
    CODE = bytes.fromhex('8b0d00204000 31c0 40 49 75fc c3')

    def fixture(self, code=CODE, count=6000):
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.detail = True
        instructions = list(md.disasm(code, 0x400000))
        pages = {0x400000:code.ljust(4096,b'\xcc'),0x402000:struct.pack('<I',count).ljust(4096,b'\0')}
        def read(a,n):
            page=a & ~4095
            if page not in pages or (a & 4095)+n>4096:
                raise KeyError(a)
            return pages[page][a & 4095:(a & 4095)+n]
        return instructions,read,pages

    def test_known_image_count_and_exact_ceiling(self):
        ins,read,_=self.fixture()
        self.assertEqual(image_bound(ins,read,0xf008000),18003)
        self.assertIsNone(image_bound(ins,read,0xf008000,ceiling=18002))
        self.assertIsNone(image_bound(ins,read,0xf008000,deadline=0))

    def test_unknown_inputs_and_calls_never_establish_a_bound(self):
        for code in ('8b08 49 75fd c3','85c0 7401 90 c3','e800000000 c3','ffe0'):
            ins,read,_=self.fixture(bytes.fromhex(code))
            self.assertIsNone(image_bound(ins,read,0xf008000))

    def test_stores_invalidate_image_facts_and_preserve_unknowns(self):
        # Replacing the count must affect the proof; an unknown replacement
        # must not be replaced again with the executable's original bytes.
        for prefix,expected in [('c7050020400002000000',10),('a300204000',None)]:
            ins,read,_=self.fixture(bytes.fromhex(prefix)+self.CODE)
            self.assertEqual(image_bound(ins,read,0xf008000),expected)

    def test_stack_return_code_and_resource_guards(self):
        for code in ('c7042400000000c3','50c3','c6050000400090c3','ebfe'):
            ins,read,_=self.fixture(bytes.fromhex(code))
            self.assertIsNone(image_bound(ins,read,0xf008000,ceiling=20))
        ins,read,_=self.fixture(bytes.fromhex('56575f5ec3'))
        self.assertEqual(image_bound(ins,read,0xf008000),5)
        self.assertIsNone(image_bound(ins,read,0xf008000,max_bytes=4))

    def test_opt_in_original_recording_and_default_limit(self):
        ins,read,pages=self.fixture(count=8000)
        with patch.object(h,'image',return_value=(0x400000,list(pages.items()))), \
                patch.object(h,'code',side_effect=read):
            kw=dict(body=[i.address for i in ins],end=0x400000+len(self.CODE))
            with self.assertRaisesRegex(h.Rejected,'runaway'):
                h.run_sparse(0x400000,7,input_strategy='image-v1',**kw)
            case=h.run_sparse(0x400000,7,input_strategy='image-bounded-v1',**kw)
            self.assertEqual(case['regs_out']['eax'],8000)
            self.assertEqual(case['image_budget'],dict(rule='image-trace-v1',instructions=24003,limit=44003))

    def test_input_dependent_loop_still_runs_out_of_budget(self):
        # ECX is supplied by the input, even if this particular seed's loop
        # would eventually terminate. Never use concrete random registers.
        code=bytes.fromhex('40 49 75fc c3')
        ins,read,pages=self.fixture(code)
        with patch.object(h,'image',return_value=(0x400000,list(pages.items()))), \
                patch.object(h,'code',side_effect=read):
            with self.assertRaisesRegex(h.Rejected,'runaway'):
                h.run_sparse(0x400000,7,input_strategy='image-bounded-v1',
                             body=[i.address for i in ins],end=0x400000+len(code))

    def guarded_record(self, code):
        ins,read,pages=self.fixture(code)
        with patch.object(h,'image',return_value=(0x400000,list(pages.items()))), \
                patch.object(h,'code',side_effect=read):
            return h.run_sparse(0x400000,7,input_strategy='guarded-bounded-v1',
                                body=[i.address for i in ins],end=0x400000+len(code))

    def test_guarded_unknown_output_does_not_invent_pointer_values(self):
        # Unknown entry EDI, but a known trip count. Writes through EDI may
        # be ignored ONLY subject to a concrete runtime non-aliasing check.
        code=bytes.fromhex('b9401f0000 31d2 8917 42 49 75fa c3')
        ins,read,_=self.fixture(code)
        self.assertIsNone(image_bound(ins,read,0xf008000))
        certificate=image_bound(ins,read,0xf008000,guarded=True)
        self.assertEqual(certificate['unknown_writes'],[0x400007])
        guard=WriteGuard(certificate)
        self.assertFalse(guard.conflicts(0x400007,0x20000000,4))
        self.assertTrue(guard.conflicts(0x400007,0x400000,4))
        self.assertTrue(guard.conflicts(0x400007,0xf007fff,4))
        case=self.guarded_record(code)
        self.assertEqual(case['regs_out']['edx'],8000)
        self.assertEqual(case['image_budget']['rule'],'guarded-image-trace-v1')

    def test_alias_assumption_is_checked_during_original_execution(self):
        # The abstract return-PC load is unknown. The concrete caller token
        # minus this offset points at the code: the write guard must fire.
        prefix=bytes.fromhex('8b3c24 81ef')+struct.pack('<I',h.RET-0x400000)
        code=prefix+bytes.fromhex('b9401f0000 31d2 8917 42 49 75fa c3')
        with self.assertRaisesRegex(h.Rejected,'unknown output aliases a protected byte'):
            self.guarded_record(code)

    def test_closed_callee_save_restore_is_traced_exactly(self):
        caller=bytes.fromhex('bf02000000 e8f6000000 4f75f8c3')
        callee=bytes.fromhex('57 bf10270000 4f79fd 5fc3')
        md=Cs(CS_ARCH_X86,CS_MODE_32);md.detail=True
        instructions=list(md.disasm(caller,0x400000))
        other={i.address:i for i in md.disasm(callee,0x400100)}
        c=image_bound(instructions,lambda a,n: (_ for _ in ()).throw(KeyError(a)),0xf008000,
                      guarded=True,closed={0x400100},decode=other.get)
        self.assertIsNotNone(c)
        self.assertGreater(c['instructions'],40000)

    def test_unknown_writes_cannot_silently_modify_known_control(self):
        code=bytes.fromhex('8b0d00204000 8907 49 75fb c3')
        ins,read,_=self.fixture(code,count=3)
        c=image_bound(ins,read,0xf008000,guarded=True)
        self.assertIsNotNone(c)
        self.assertTrue(WriteGuard(c).conflicts(0x400006,0x402000,4))

    def test_platform_model_writes_also_enforce_the_guard(self):
        prefix=bytes.fromhex('8b3c24 81ef')+struct.pack('<I',h.RET-0x400000)
        prefix+=bytes.fromhex('6a04 6a00 57')
        call=0x400000+len(prefix)
        code=prefix+b'\xe8'+struct.pack('<i',0x400100-call-5)+bytes.fromhex('83c40c')
        code+=bytes.fromhex('b9401f0000 31d2 42 49 75fc c3')
        ins,_,pages=self.fixture(code)
        pages[0x400000]=pages[0x400000][:0x100]+b'\xc3'+pages[0x400000][0x101:]
        def read(a,n):
            if a & ~4095 not in pages: raise KeyError(a)
            return pages[a & ~4095][a & 4095:(a & 4095)+n]
        with patch.object(h,'image',return_value=(0x400000,list(pages.items()))), \
                patch.object(h,'code',side_effect=read),patch.object(h,'PLATFORM',{0x400100:'memset'}):
            with self.assertRaisesRegex(h.Rejected,'unknown output aliases a protected byte'):
                h.run_sparse(0x400000,7,input_strategy='guarded-bounded-v1',
                             body=[i.address for i in ins],end=0x400000+len(code))

    def test_guard_wrap_and_unknown_flags_after_opaque_values(self):
        guard=WriteGuard(dict(unknown_writes=[1],protected=[0,0x10,0xffffffff]))
        self.assertTrue(guard.conflicts(1,0xfffffffe,4))
        self.assertFalse(guard.conflicts(2,0xfffffffe,4))
        self.assertFalse(guard.conflicts(1,0x11,4))
        ins,read,_=self.fixture(bytes.fromhex('89c1 c1e917 85c9 7401 90 c3'))
        self.assertIsNone(image_bound(ins,read,0xf008000,guarded=True))

    def test_not_preserves_flags_and_unknown_values(self):
        ins,read,_=self.fixture(bytes.fromhex('31c0 f7d7 7502 f7d0 c3'))
        self.assertEqual(image_bound(ins,read,0xf008000,guarded=True)['instructions'],5)
        ins,read,_=self.fixture(bytes.fromhex('f7d7 85ff 7401 90 c3'))
        self.assertIsNone(image_bound(ins,read,0xf008000,guarded=True))

    def test_indirect_jump_requires_known_interior_target(self):
        ins,read,_=self.fixture(bytes.fromhex('b808004000 ffe0 90 c3'))
        self.assertEqual(image_bound(ins,read,0xf008000,guarded=True)['instructions'],3)
        for code in ('ffe0c3','b800014000ffe0c3'):
            ins,read,_=self.fixture(bytes.fromhex(code))
            self.assertIsNone(image_bound(ins,read,0xf008000,guarded=True))

    def test_rep_store_requires_bounded_count_and_guards_unknown_destination(self):
        ins,read,_=self.fixture(bytes.fromhex('b9401f0000 31c0 f3ab c3'))
        c=image_bound(ins,read,0xf008000,guarded=True)
        self.assertEqual(c['instructions'],8004)
        self.assertTrue(WriteGuard(c).conflicts(0x400007,0x400000,4))
        for code in ('f3abc3','bf00204000 b902000000 f3abc3'):
            ins,read,_=self.fixture(bytes.fromhex(code))
            self.assertIsNone(image_bound(ins,read,0xf008000,guarded=True))
        ins,read,_=self.fixture(bytes.fromhex('fc bf00204000 b902000000 31c0 f3ab c3'))
        self.assertEqual(image_bound(ins,read,0xf008000,guarded=True)['instructions'],8)


if __name__ == '__main__':
    unittest.main()
