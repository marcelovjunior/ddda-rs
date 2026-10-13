"""Flat emission stays optional and runs after the existing lean transforms."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import lift
import rust_base


class FlatMemoryTests(unittest.TestCase):
    def test_disabled_and_literals(self):
        text = 'pub fn f_00001000(m: &mut dyn Mem, cpu: &mut Cpu) {\n' \
               'let pt = m.access_context(); m.fast_u32(pt, a); ' \
               'panic!("m.fast_u32(pt, a)");\n}\n'
        with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": "0"}):
            self.assertEqual(lift.flat_memory(text), text)
        with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": "1"}):
            got = lift.flat_memory(text)
        self.assertIn('let flat = m.flat_base();', got)
        self.assertIn('m.flat_u32(flat, pt, a);', got)
        self.assertIn('panic!("m.fast_u32(pt, a)")', got)

    def test_six_accessors_and_part_entry(self):
        calls = ' '.join(f'm.fast_{name}(pt, a);' for name in
                         ('u8', 'u16', 'u32', 'set_u8', 'set_u16', 'set_u32'))
        text = 'fn f_00001000_p0(m: &mut dyn Mem, cpu: &mut Cpu, pt: Option<X>, mut pc: u32) -> Option<u32> {\n' + calls + '\n}\n'
        with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": "1"}):
            got = lift.flat_memory(text)
        self.assertEqual(got.count('let flat = m.flat_base();'), 1)
        self.assertEqual(got.count('(flat, pt, a)'), 6)

    def test_generated_entry_keeps_scheduling_separate_from_table(self):
        with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": "1"}):
            got = lift.rust_fn(0x1000, 'synthetic', ['0x1000 => { eax = m.u32(0x2000); return; }'])
        self.assertIn('let pt = m.access_context();', got)
        self.assertIn('let flat = m.flat_base();', got)
        self.assertIn('m.flat_u32(flat, pt, 0x2000)', got)

    def test_outlined_mode_keeps_same_operands(self):
        with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": "outline"}):
            got = lift.rust_fn(0x1000, 'synthetic', ['0x1000 => { eax = m.u32(0x2000); return; }'])
        self.assertIn('m.flat_o_u32(flat, 0x2000)', got)
        self.assertIn('let pt = m.access_context();', got)

    def test_emitted_modes_compile_and_preserve_fixture_accesses(self):
        root = Path(__file__).resolve().parents[2]
        for mode in ('1', 'outline'):
            with patch.dict(os.environ, {"LIFT_FLAT_MEMORY": mode}):
                generated = lift.rust_fn(0x1000, 'synthetic', [
                    '0x1000 => { eax = m.u32(1); m.set_u16(7, eax as u16); return; }'])
            source = rust_base.modules(root) + '\nuse mem::Mem; use x86::Cpu;\n' + generated + '''
fn main() {
    let mut m = mem::Image { base: 0, bytes: vec![0; 16] };
    let mut cpu = Cpu::default();
    m.set_u32(1, 0x12345678);
    f_00001000(&mut m, &mut cpu);
    assert_eq!(cpu.eax, 0x12345678);
    assert_eq!(m.u16(7), 0x5678);
}
'''
            with tempfile.TemporaryDirectory(prefix='L142-fixture-') as directory:
                path = Path(directory)
                (path / 'fixture.rs').write_text(source)
                for opt in ('0', '1'):
                    exe = path / f'fixture{opt}.exe'
                    subprocess.run(['rustc', '--edition=2021', '-Awarnings', '-C', f'opt-level={opt}',
                                    str(path / 'fixture.rs'), '-o', str(exe)], check=True, timeout=60,
                                   capture_output=True, text=True)
                    subprocess.run([str(exe)], check=True, timeout=10, capture_output=True)


if __name__ == '__main__':
    unittest.main()
