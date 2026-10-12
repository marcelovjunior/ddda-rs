"""Lazy fills retain byte-exact observations and resource rejection guards."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from bulk_inputs import BulkFills


class BulkTests(unittest.TestCase):
    def record(self, size, suffix='c3', strategy='bulk-call-v1', immediate=True, **kw):
        target = next(a for a,n in h.PLATFORM.items() if n == 'memset')
        code = b'\x68'+struct.pack('<I',size) if immediate else b'\xb8'+struct.pack('<I',size)+b'\x50'
        code += bytes.fromhex('6a5a 6800000020')
        code += b'\xe8'+struct.pack('<i',target-(0x400000+len(code)+5))
        code += bytes.fromhex('83c40c '+suffix)
        data = code.ljust(4096,b'\xcc')
        body = [i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,0x400000)]
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,data)])), \
             patch.object(h,'code',side_effect=lambda a,n: data[a-0x400000:a-0x400000+n]):
            return h.run_sparse(0x400000,1,body=body,end=0x400000+len(code),
                                input_strategy=strategy,**kw)

    def test_large_fill_exact_range_and_later_access(self):
        c=self.record(1<<20,'c605030000207f a100000020 c3')
        self.assertEqual(c['regs_out']['eax'],0x7f5a5a5a)
        filled=next(bytes.fromhex(v) for a,v in c['writes'] if a==0x20000000)
        self.assertEqual(filled,b'Z'*3+b'\x7f'+b'Z'*((1<<20)-4))
        self.assertFalse(any(0x20000000 <= a < 0x20100000 for a,_ in c['reads']))

    def test_small_fill_matches_eager_observations(self):
        lazy=self.record(33,'a11f000020 c3')
        eager=self.record(33,'a11f000020 c3',strategy='guarded-objects-v1')
        lazy.pop('input_strategy');eager.pop('input_strategy')
        self.assertEqual(lazy,eager)

    def test_size_and_page_guards_remain(self):
        for size,immediate in ((h.CONST_MODEL_BYTES+1,True),(0x10001,False)):
            with self.assertRaisesRegex(h.Rejected,'memset of'):
                self.record(size,immediate=immediate)
        with patch.object(h,'MAX_PAGES',2):
            with self.assertRaisesRegex(h.Rejected,'pages'):
                self.record(1<<20,'a100000020 a100100020 a100200020 c3')
        with self.assertRaisesRegex(h.Rejected,'capture'):
            self.record(1<<20,capture_state={})

    def test_overlapping_fill_and_materialized_override(self):
        b=BulkFills();b.add(4094,6,1);b.add(4096,2,2)
        self.assertEqual(b.overlay(4092,bytes(10)),bytes([0,0,1,1,2,2,1,1,0,0]))
        pages={4096}
        output=b.output({4099,4100},pages,lambda a,n: b'\x03'*n)
        self.assertEqual(output,[[4094,'01010303030303']])
        with self.assertRaises(ValueError):b.add(0xffffffff,2,0)


if __name__ == '__main__':
    unittest.main()
