import os
import unittest
from unittest.mock import patch

import inline_flags


class InlineFlagsTests(unittest.TestCase):
    def test_default_is_identical(self):
        source = 'cpu.fl.alu_sub(a, b, 32)'
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(inline_flags.retarget(source), source)

    def test_only_exact_enabled_value(self):
        source = 'cpu.fl.alu_sub(a, b, 32)'
        for value in ('0', '', 'true', '2'):
            with patch.dict(os.environ, LIFT_INLINE_FLAGS=value):
                self.assertEqual(inline_flags.retarget(source), source)

    def test_preserves_operands_and_other_helpers(self):
        source = 'cpu.fl.alu_add(m.fast_u32(pt, cpu.eax), cpu.ebx, 8); cpu.fl.inc_dec(x86::INC, a, r, 16);'
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='1'):
            self.assertEqual(inline_flags.retarget(source), source.replace('alu_add(', 'alu_add_inline('))

    def test_all_five_and_idempotence(self):
        source = '\n'.join(f'cpu.fl.alu_{op}(a, b, 32)' for op in ('add', 'sub', 'and', 'or', 'xor'))
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='1'):
            out = inline_flags.retarget(source)
            self.assertEqual(out.count('_inline('), 5)
            self.assertEqual(inline_flags.retarget(out), out)

    def test_comments_and_strings_are_untouched(self):
        source = '// cpu.fl.alu_sub(a,b,32)\n"cpu.fl.alu_add(a,b,32)"'
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='1'):
            self.assertEqual(inline_flags.retarget(source), source)

    def test_register_local_flags_receiver(self):
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='1'):
            self.assertEqual(inline_flags.retarget('l161_fl.alu_sub(a, b, 16)'),
                             'l161_fl.alu_sub_inline(a, b, 16)')

    def test_shard_hook_preserves_default(self):
        import lift
        function = dict(addr=0x400000, name='synthetic', arms=[
            '0x400000 => { cpu.fl.alu_sub(cpu.eax, cpu.ebx, 32); return; }'])
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='0'):
            baseline = lift.shard_fn(function)
        with patch.dict(os.environ, LIFT_INLINE_FLAGS='1'):
            candidate = lift.shard_fn(function)
        self.assertIn('alu_sub_inline(', candidate)
        self.assertEqual(candidate.replace('alu_sub_inline(', 'alu_sub('), baseline)


if __name__ == '__main__':
    unittest.main()
