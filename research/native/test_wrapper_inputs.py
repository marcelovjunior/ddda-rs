"""Synthetic wrapper strategies: preserve the caller's real stack contract."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from wrapper_inputs import virtual_tail_wrapper, fixed_frame_allocations, aligned_frame_saves, ebp_frame_allocation, initial_outgoing_allocation, null_leaf_cleanup, exception_frame_pushes, borrowed_frame_pushes


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

    def run_caller(self, suffix, strategy, prefix='', callee='8b01 8b4058 ffe0'):
        caller = bytes.fromhex(prefix+' 6a01 6a02')
        caller += b'\xe8' + struct.pack('<i', 0x400100-(0x400000+len(caller)+5))
        caller += bytes.fromhex(suffix)
        data = bytearray(b'\xcc'*4096)
        data[:len(caller)] = caller
        callee_bytes = bytes.fromhex(callee)
        data[256:256+len(callee_bytes)] = callee_bytes
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

    def test_scalar_one_mock_can_complete_a_wait_loop(self):
        # CALL; CMP EAX,1; JNE back to CALL; RET.
        caller = bytes.fromhex('e8fb000000 83f801 75f6 c3')
        data = caller + b'\xcc'*(256-len(caller)) + b'\xc3'
        body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(caller, 0x400000)]
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000,data)])), \
             patch.object(h, 'code', side_effect=lambda a,n: data[a-0x400000:a-0x400000+n]):
            with self.assertRaisesRegex(h.Rejected, 'runaway'):
                h.run_sparse(0x400000, 1, body=body, end=0x400000+len(caller),
                             input_strategy='tail-frame-v1', max_insn=100)
            result = h.run_sparse(0x400000, 1, body=body, end=0x400000+len(caller),
                                  input_strategy='one-call-v1', max_insn=100)
        self.assertEqual(len(result['calls']), 1)
        self.assertEqual(result['calls'][0]['out']['eax'], 1)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)
        for name in ('ecx', 'edx'):
            self.assertEqual(result['calls'][0]['out'][name], 0)

    def test_null_memory_fast_return_supplies_checked_cleanup(self):
        callee = '83b9ec00000000 7505 31c0 c20800 8b89ec000000 8b01 8b4018 ffe0'
        with self.assertRaises(h.Rejected):
            self.run_caller('c3', 'tail-frame-v1', callee=callee)
        result = self.run_caller('c3', 'null-call-v1', callee=callee)
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def test_null_cleanup_refuses_other_values_and_unchecked_paths(self):
        def cleanup(code, value=0):
            data = bytes.fromhex(code)
            return null_leaf_cleanup(0, lambda a,n:data[a:a+n], {'ecx': 0x1000},
                                     lambda a,n: value.to_bytes(4, 'little'))
        code = '83b90400000000 7505 31c0 c20800'
        self.assertEqual(cleanup(code), 8)
        self.assertIsNone(cleanup(code, 1))
        for bad in (code.replace('7505', '7f05'), code.replace('31c0', '31c9'),
                    code.replace('31c0', '31c0 40'), code.replace('c20800', '66c20800'),
                    code.replace('83b90400000000', '80b90400000000')):
            self.assertIsNone(cleanup(bad))
        self.assertEqual(cleanup('83b90400000000 7401 cc 31c0 c20800'), 8)

    def test_inline_exception_frame_is_not_counted_as_arguments(self):
        prefix = ('6aff 6800034000 64a100000000 50 51 '
                  'a100024000 33c4 50 8d442408 64a300000000 ')
        suffix = '85c0 7400 8b4c2408 64890d00000000 59 83c410 c3'
        with self.assertRaises(h.Rejected):
            self.run_caller(suffix, 'tail-frame-v1', prefix)
        result = self.run_caller(suffix, 'exception-frame-v1', prefix)
        self.assertEqual(result['calls'][0]['out']['pop'], 8)
        self.assertEqual(result['regs_out']['esp'], result['regs_in']['esp']+4)

    def test_exception_frame_requires_matching_links_and_all_returns(self):
        def sites(code):
            data = bytes.fromhex(code)
            body = [i.address for i in Cs(CS_ARCH_X86, CS_MODE_32).disasm(data, 0)]
            return exception_frame_pushes(0, body, lambda a,n:data[a:a+n])
        prefix = ('6aff 6800034000 64a100000000 50 51 '
                  'a100024000 33c4 50 8d442408 64a300000000 ')
        suffix = '8b4c2408 64890d00000000 59 83c410 c3'
        self.assertEqual(sites(prefix+'85c0 7400 '+suffix), {0,2,13,14,22})
        for code in (prefix+suffix+' c3', prefix+suffix.replace('83c410','83c40c'),
                     prefix.replace('8d442408','8d442404')+suffix,
                     prefix+suffix.replace('8b4c2408','8b4c2404'),
                     prefix+suffix.replace('64890d00000000','64890d04000000'),
                     prefix.replace('64a100000000','65a100000000')+suffix):
            self.assertEqual(sites(code), set())

    def borrowed_caller(self):
        caller = bytearray(bytes.fromhex('55 8bec 83ec04 53 51 8b4508 55 6a00 8be9'))
        def call():
            caller.extend(b'\xe8'+struct.pack('<i',0x400100-(0x400000+len(caller)+5)))
        call()
        caller.extend(bytes.fromhex('56 57 31c0 ffd0 5f 5e 5d 55 8be9 6a01'))
        call()
        caller.extend(bytes.fromhex('5d 59 5b c9 c3'))
        return bytes(caller)

    def test_borrowed_frame_saves_preserve_return_and_registers(self):
        caller = self.borrowed_caller()
        data = caller+b'\xcc'*(256-len(caller))+bytes.fromhex('c20400')
        body = [i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(caller,0x400000)]
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,data)])), \
             patch.object(h,'code',side_effect=lambda a,n:data[a-0x400000:a-0x400000+n]):
            with self.assertRaises(h.Rejected):
                h.run_sparse(0x400000,1,body=body,end=0x400000+len(caller),input_strategy='tail-frame-v1')
            result = h.run_sparse(0x400000,1,body=body,end=0x400000+len(caller),input_strategy='borrowed-frame-v1')
        self.assertEqual([c['out']['pop'] for c in result['calls']], [4,0,4])
        self.assertEqual(result['regs_out']['esp'],result['regs_in']['esp']+4)
        for name in ('ebp','ebx','esi','edi'):
            self.assertEqual(result['regs_out'][name],result['regs_in'][name])

    def test_borrowed_frame_rejects_unmatched_saves_and_argument_cleanup(self):
        def sites(caller, pop=4):
            ds=list(Cs(CS_ARCH_X86,CS_MODE_32).disasm(caller,0))
            saved={i.address for i in ds if i.mnemonic=='push' and i.op_str in ('esi','edi')}
            return borrowed_frame_pushes(0,[i.address for i in ds],lambda a,n:caller[a:a+n],lambda a:pop,saved)
        caller=self.borrowed_caller()
        self.assertEqual(len(sites(caller)),2)
        self.assertEqual(sites(caller,8),set())
        for bad in (caller.replace(bytes.fromhex('5d 59 5b c9'),bytes.fromhex('90 59 5b c9')),
                    caller.replace(bytes.fromhex('5f 5e 5d'),bytes.fromhex('5d 5e 5d')),
                    caller.replace(bytes.fromhex('55 6a00'),bytes.fromhex('55 83c404 6a00')),
                    caller.replace(bytes.fromhex('c9 c3'),bytes.fromhex('90 c3'))):
            self.assertEqual(sites(bad),set())

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
