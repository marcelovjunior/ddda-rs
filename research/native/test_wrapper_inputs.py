"""Synthetic wrapper strategies: preserve the caller's real stack contract."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from wrapper_inputs import virtual_tail_wrapper, fixed_frame_allocations, aligned_frame_saves, ebp_frame_allocation, initial_outgoing_allocation


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

    def run_caller(self, suffix, strategy, prefix=''):
        caller = bytes.fromhex(prefix+' 6a01 6a02')
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

    def test_fixed_frame_requires_matching_all_return_epilogues(self):
        def sites(code):
            data=bytes.fromhex(code)
            body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(data,0)]
            return fixed_frame_allocations(0,body,lambda a,n:data[a:a+n])
        self.assertEqual(sites('8b01 83ec20 85c0 7400 83c420 c3'),{2})
        for code in ('8b01 83ec20 85c0 7400 83c410 c3',
                     '8b01 83ec20 85c0 7400 83c420 c3 c3',
                     '8b01 83ec20 83ec20 85c0 7400 83c420 c3'):
            self.assertEqual(sites(code),set())

    def test_fixed_frame_is_not_counted_as_wrapper_arguments(self):
        result=self.run_caller('83c420 c3','wrapper-call-v1',
                               '8b01 83ec20 85c0 7400 89442404')
        self.assertEqual(result['calls'][0]['out']['pop'],8)
        self.assertEqual(result['regs_out']['esp'],result['regs_in']['esp']+4)

    def test_arithmetic_prefix_keeps_local_frame_out_of_mock_arguments(self):
        prefix = '8b01 83ec20 53 83c001 85c0 7400 89442404 '
        suffix = '5b 83c420 c3'
        with self.assertRaises(h.Rejected):
            self.run_caller(suffix, 'frame-call-v1', prefix)
        result = self.run_caller(suffix, 'prefix-call-v1', prefix)
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def test_arithmetic_prefix_requires_matching_frame_and_no_other_stack_writes(self):
        def sites(prefix, suffix='83c420 c3'):
            data = bytes.fromhex(prefix+' 85c0 7400 '+suffix)
            body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0)]
            return fixed_frame_allocations(0, body, lambda a,n:data[a:a+n],
                                           arithmetic_prefix=True)
        self.assertEqual(sites('83c001 83ec20 f30f10c1 f30f2ac8 0fb7c8'), {3})
        for prefix in ('83ec20 83c401', '83ec20 6683c401',
                       '83ec20 83ec20', '83ec20 8901', '83ec20 58'):
            self.assertEqual(sites(prefix), set())
        self.assertEqual(sites('83ec20 83c001', '83c410 c3'), set())
        self.assertEqual(sites('83ec20 83c001', '83c420 c3 c3'), set())

    def test_ebp_local_frame_preserves_return_and_saved_registers(self):
        prefix = '55 8bec 83e4f0 8b01 83ec20 53 57 '
        suffix = '5f 5b 8be5 5d c3'
        old = self.run_caller(suffix, 'prefix-call-v1', prefix)
        self.assertEqual(old['calls'][0]['out']['pop'], 40)
        result = self.run_caller(suffix, 'ebp-call-v1', prefix)
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)
        for register in ('ebp', 'ebx', 'esi', 'edi'):
            self.assertEqual(result['regs_out'][register], result['regs_in'][register])

    def test_ebp_local_frame_requires_stable_frame_and_matching_restores(self):
        def sites(code):
            data = bytes.fromhex(code)
            body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0)]
            return ebp_frame_allocation(0, body, lambda a,n:data[a:a+n])
        prefix = '55 8bec 83e4f0 8b01 83ec20 53 57 85c0 7400 '
        suffix = '5f 5b 8be5 5d c3'
        self.assertEqual(sites(prefix+suffix), {8})
        self.assertEqual(sites(prefix+suffix.replace('5f 5b', '5f f30f11460c 5b')), {8})
        self.assertEqual(sites(prefix.replace('53 57 ', '')+suffix.replace('5f 5b ', '')), {8})
        for code in (prefix+suffix+' c3', prefix+suffix.replace('5f 5b','5b 5f'),
                     prefix+suffix.replace('8be5','8be3'),
                     prefix.replace('83e4f0','83e4f9')+suffix,
                     prefix.replace('83ec20','83ec20 83ec20')+suffix,
                     prefix.replace('83ec20','53 83ec20')+suffix,
                     prefix+'8beb '+suffix, prefix+'6689dd '+suffix,
                     prefix+suffix.replace('5f 5b', '5f 83c404 5b')):
            self.assertEqual(sites(code), set())

    def test_entry_outgoing_area_is_consumed_by_final_indirect_call(self):
        data = bytes.fromhex('83ec20 31c0 89442404 6a01 6a02 ffd0 c3')
        body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0x400000)]
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000,data)])), \
             patch.object(h, 'code', side_effect=lambda a,n: data[a-0x400000:a-0x400000+n]):
            with self.assertRaises(h.Rejected):
                h.run_sparse(0x400000, 1, body=body, end=0x400000+len(data),
                             input_strategy='ebp-call-v1')
            result = h.run_sparse(0x400000, 1, body=body, end=0x400000+len(data),
                                  input_strategy='outgoing-call-v1')
        self.assertEqual(result['calls'][0]['out']['pop'], 40)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def test_entry_outgoing_area_requires_finite_final_calls_on_every_path(self):
        def sites(code):
            data = bytes.fromhex(code)
            body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0)]
            return initial_outgoing_allocation(0, body, lambda a,n:data[a:a+n])
        self.assertEqual(sites('83ec20 85c0 7405 6a01 ffd0 c3 6a02 ffd2 c3'), {0})
        for code in ('83ec20 85c0 74fc ffd0 c3', '83ec20 ffd0 83c420 c3',
                     '83ec20 83c404 ffd0 c3', '83ec20 6683c404 ffd0 c3',
                     '83ec20 666a01 ffd0 c3', '83ec20 85c0 7403 ffd0 c3 c3',
                     '83ec20 ffe0', '83ec20 e800000000 c3'):
            self.assertEqual(sites(code), set())

    def test_fixed_frame_can_restore_before_a_tail_call(self):
        prefix = '8b01 83ec20 53 '
        suffix = '5b 83c420 ffe0'
        with self.assertRaises(h.Rejected):
            self.run_caller(suffix, 'outgoing-call-v1', prefix)
        result = self.run_caller(suffix, 'tail-frame-v1', prefix)
        self.assertEqual([c['out']['pop'] for c in result['calls']], [8, 0])
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)
        self.assertEqual(result['regs_out']['ebx'], result['regs_in']['ebx'])

    def test_tail_frame_requires_matching_cleanup_at_all_exits(self):
        def sites(code):
            data = bytes.fromhex(code)
            body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0)]
            return fixed_frame_allocations(0, body, lambda a,n:data[a:a+n],
                                           arithmetic_prefix=True, tail_exits=True)
        self.assertEqual(sites('8b01 83ec20 85c0 7405 83c420 ffe0 83c420 c3'), {2})
        for code in ('8b01 83ec20 85c0 7405 83c420 ffe0 c3',
                     '8b01 83ec20 85c0 747f 83c420 ffe0',
                     '8b01 83ec20 85c0 7405 83c410 ffe0 83c420 c3'):
            self.assertEqual(sites(code), set())

    def test_aligned_frame_pointer_save_is_not_an_outgoing_argument(self):
        prefix = '55 8bec 83e4f8 83ec18 8b01 53 55 56 57 '
        suffix = '5f 5e 5d 5b 8be5 5d c3'
        with self.assertRaises(h.Rejected):
            self.run_caller(suffix, 'wrapper-call-v1', prefix)
        result = self.run_caller(suffix, 'frame-call-v1', prefix)
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)
        for register in ('ebp', 'ebx', 'esi', 'edi'):
            self.assertEqual(result['regs_out'][register], result['regs_in'][register])

    def test_aligned_frame_requires_matching_save_and_restore_order(self):
        def sites(code):
            data = bytes.fromhex(code)
            body = [i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(data,0)]
            return aligned_frame_saves(0, body, lambda a,n:data[a:a+n])
        prefix = '55 8bec 83e4f8 53 55 56 57 85c0 7400 '
        suffix = '5f 5e 5d 5b 8be5 5d c3'
        self.assertEqual(sites(prefix+suffix), {7})
        for code in (prefix+suffix+' c3', prefix+suffix.replace('5d 5b','5b 5d'),
                     prefix+suffix.replace('8be5','8be3'),
                     prefix.replace('83e4f8','83e4f9')+suffix,
                     prefix.replace('53 55','8beb 53 55')+suffix):
            self.assertEqual(sites(code), set())


if __name__ == '__main__':
    unittest.main()
