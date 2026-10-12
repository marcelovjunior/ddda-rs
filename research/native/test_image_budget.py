"""Image-only control flow bounds, including deliberate proof refusals."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from image_budget import image_bound
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


if __name__ == '__main__':
    unittest.main()
