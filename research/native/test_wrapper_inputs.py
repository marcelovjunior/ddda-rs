"""Synthetic wrapper strategies: preserve the caller's real stack contract."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from wrapper_inputs import virtual_tail_wrapper


class WrapperTests(unittest.TestCase):
    def test_recognizer_accepts_only_frameless_load_chains(self):
        for code in ('8b01 8b4058 ffe0', '8b89800d0000 8b01 8b406c ffe0'):
            data = bytes.fromhex(code)
            self.assertTrue(virtual_tail_wrapper(0, lambda a,n: data[a:a+n]))
        for code in ('50 8b01 ffe0', '894104 8b01 ffe0', '8b0424 ffe0',
                     '8b31 ffe6', '8b01 c3', 'ffe0', '8b01 7500 ffe0',
                     '648b01 ffe0', '8b01 e800000000 ffe0'):
            data = bytes.fromhex(code)
            self.assertFalse(virtual_tail_wrapper(0, lambda a,n: data[a:a+n]))

    def run_caller(self, suffix, strategy):
        caller = bytes.fromhex('6a01 6a02')
        caller += b'\xe8' + struct.pack('<i', 0x400100-(0x400000+len(caller)+5))
        caller += bytes.fromhex(suffix)
        data = bytearray(b'\xcc'*4096)
        data[:len(caller)] = caller
        data[256:263] = bytes.fromhex('8b01 8b4058 ffe0')
        data = bytes(data)
        body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(caller, 0x400000)]
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000,data)])), \
             patch.object(h, 'code', side_effect=lambda a,n: data[a-0x400000:a-0x400000+n]):
            return h.run_sparse(0x400000, 1, body=body, end=0x400000+len(caller),
                                input_strategy=strategy)

    def test_opt_in_wrapper_uses_outgoing_arguments(self):
        with self.assertRaises(h.Rejected):
            self.run_caller('c3', 'zero-call-v1')
        result = self.run_caller('c3', 'wrapper-call-v1')
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def test_caller_cleanup_is_not_applied_twice(self):
        result = self.run_caller('83c408 c3', 'wrapper-call-v1')
        self.assertEqual(result['calls'][0]['out']['pop'], 0)


if __name__ == '__main__':
    unittest.main()
