"""Public synthetic proofs of cache boundaries; no game data required."""
import unittest

import register_cache as rc


class RegisterCacheTests(unittest.TestCase):
    def test_leaf_and_exit(self):
        out = rc.cache_body(' cpu.eax = m.fast_u32(pt, cpu.esp); return; ')
        self.assertIn('let mut l161_eax = cpu.eax;', out)
        self.assertIn('l161_eax = m.fast_u32(pt, l161_esp);', out)
        self.assertIn('cpu.eax = l161_eax; cpu.esp = l161_esp; return }', out)

    def test_call_flushes_arguments_and_reloads_outputs(self):
        out = rc.cache_body(' cpu.eax = 4; x86::call_ext(m, cpu, cpu.eax); cpu.edx = cpu.eax; return; ')
        self.assertIn('cpu.eax = l161_eax; cpu.edx = l161_edx; x86::call_ext(m, cpu, cpu.eax); '
                      'l161_eax = cpu.eax; l161_edx = cpu.edx;', out)
        self.assertIn('l161_edx = l161_eax;', out)

    def test_more_than_ten_boundaries(self):
        out = rc.cache_body(' cpu.eax = 1; ' + 'x86::call_ext(m, cpu, 1); ' * 12 + 'return;')
        self.assertNotIn('L161_BOUNDARY', out)
        self.assertEqual(out.count('x86::call_ext(m, cpu, 1)'), 12)

    def test_unknown_cpu_observer_rejects(self):
        for text in (' cpu.eax = 1; unknown(cpu); return;',
                     ' cpu.eax = 1; let v = x86::call_ext(m, cpu, 1); return;',
                     ' cpu.eax = 1; foo(x86::call_ext(m, cpu, 1)); return;'):
            self.assertEqual(rc.cache_body(text), text)

    def test_unknown_return_rejects(self):
        text = ' cpu.eax = 1; return Some(next);'
        self.assertEqual(rc.cache_body(text), text)

    def test_strings_are_not_state(self):
        out = rc.cache_body(' cpu.eax = 1; let s = "cpu unknown(cpu) return;"; return;')
        self.assertIn('"cpu unknown(cpu) return;"', out)

    def test_push_pop_esp_order(self):
        out = rc.cache_body(' emit::push(cpu.esp, m, pt, cpu); cpu.esp = emit::pop(m, pt, cpu); return;')
        self.assertIn('let l161_push = l161_esp; l161_esp = l161_esp.wrapping_sub(4);', out)
        self.assertIn('l161_esp = { let l161_pop = m.fast_u32(pt, l161_esp); '
                      'l161_esp = l161_esp.wrapping_add(4); l161_pop }', out)

    def test_split_part_exits(self):
        source = ('fn f_00001000_p0(m: &mut dyn Mem, cpu: &mut Cpu) -> Option<u32> {\n'
                  ' cpu.eax = 1; if cpu.edx == 0 { return None; } return Some(pc);\n}\n')
        out = rc.cache_functions(source)
        self.assertEqual(out.count('cpu.eax = l161_eax;'), 2)
        self.assertIn('let mut l161_eax', out)

    def test_match_arm_return_with_comma(self):
        out = rc.cache_body(' cpu.eax = 1; match pc { _ => return Some(pc), }')
        self.assertIn('_ => { cpu.eax = l161_eax; return Some(pc) },', out)


if __name__ == '__main__':
    unittest.main()
