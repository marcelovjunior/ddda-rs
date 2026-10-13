"""L129: conservative CFG flag liveness and direct predicates."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import lean_emission as le
from test_lean_emission import FIXTURE

CMP = '{ let r = cpu.fl.alu_sub(cpu.eax, cpu.ecx, 32); }'
ADD = '{ let r = cpu.fl.alu_add(cpu.eax, 1u32, 32); cpu.eax = r; }'


class Flow(unittest.TestCase):
    def test_successor_kill(self):
        arms = [f'0x1000 => {{ {ADD} pc = 0x1010; continue; }}',
                f'0x1010 => {{ {CMP} return; }}']
        out = le.function_flags(arms)
        self.assertNotIn('alu_add', out[0])
        self.assertEqual(arms[1], out[1])

    def test_transitive_and_cyclic_edges(self):
        neutral = '0x1010 => { pc = 0x1020; continue; }'
        kill = f'0x1020 => {{ {CMP} return; }}'
        self.assertEqual(le.entry_kills([neutral, kill]), {0x1010, 0x1020})
        self.assertEqual(le.entry_kills([neutral, '0x1020 => { pc = 0x1010; continue; }']), set())

    def test_boundaries_and_partial_writes(self):
        for next_body in ('return;', 'x86::call_ext(m, cpu, 42); ' + CMP,
                          'cpu.edx = cpu.fl.lahf() as u32; ' + CMP,
                          'cpu.fl.inc_dec(x86::INC, a, r, 32); ' + CMP,
                          'if n != 0 { ' + CMP + ' } return;',
                          'pc = cpu.edx; continue;'):
            arms = [f'0x1000 => {{ {ADD} pc = 0x1010; continue; }}',
                    f'0x1010 => {{ {next_body} }}']
            self.assertEqual(le.function_flags(arms)[0], arms[0], next_body)

    def test_fuse_branch_only_when_both_successors_kill(self):
        branch = f'0x1000 => {{ {CMP} pc = if cpu.fl.cond("b") {{ 0x1010 }} else {{ 0x1020 }}; continue; }}'
        arms = [branch, f'0x1010 => {{ {ADD} return; }}', f'0x1020 => {{ {ADD} return; }}']
        out = le.function_flags(arms)[0]
        self.assertIn('(cpu.eax) < (cpu.ecx)', out)
        self.assertNotIn('cpu.fl', out)
        arms[2] = '0x1020 => { return; }'
        self.assertIn(CMP, le.function_flags(arms)[0]); self.assertIn("(cpu.fl.a < cpu.fl.b)", le.function_flags(arms)[0])
        arms[2] = '0x1020 => { pc = 0x1020; continue; }'
        self.assertIn(CMP, le.function_flags(arms)[0]); self.assertIn("(cpu.fl.a < cpu.fl.b)", le.function_flags(arms)[0])

    def test_setcc_cmovcc_and_memory_operands(self):
        compare = '{ let r = cpu.fl.alu_sub(m.fast_u32(pt, cpu.eax), cpu.ecx, 32); }'
        for read in ('cpu.edx = cpu.fl.cond("le") as u32;',
                     '{ let v = m.fast_u32(pt, cpu.esi); if cpu.fl.cond("le") { cpu.edx = v; } }'):
            arm = f'0x1000 => {{ {compare} {read} {ADD} return; }}'
            out = le.function_flags([arm])[0]
            self.assertIn('((m.fast_u32(pt, cpu.eax)) as i32) <= ((cpu.ecx) as i32)', out)
            self.assertEqual(out.count('m.fast_u32(pt, cpu.eax)'), 1)
            live = arm.replace(ADD, 'cpu.ebx = cpu.fl.lahf() as u32;')
            self.assertIn(compare, le.function_flags([live])[0]); self.assertIn("(cpu.fl.a as i32) <= (cpu.fl.b as i32)", le.function_flags([live])[0])

    def test_predicate_forms_and_unknown_cc(self):
        for cc in le.PREDICATE:
            arm = f'0x1000 => {{ {CMP} cpu.edx = cpu.fl.cond("{cc}") as u32; {ADD} return; }}'
            direct = le.direct_compare('sub', '32', cc)
            for form in (arm, le.predicates(arm)):
                out = le.function_flags([form])[0]
                self.assertEqual('l129_condition' in out, direct is not None, (cc, out))
        self.assertEqual(le.predicates('cpu.fl.cond("unknown")'), 'cpu.fl.cond("unknown")')

    def test_no_intervening_instruction_or_multiple_read(self):
        for gap in ('cpu.eax = 4;', 'cpu.edx = cpu.fl.cf() as u32;', 'x86::call_ext(m, cpu, 42);'):
            arm = f'0x1000 => {{ {CMP} {gap} cpu.edx = cpu.fl.cond("e") as u32; {ADD} return; }}'
            self.assertNotIn('l129_condition', le.function_flags([arm])[0])

    def test_native_predicates_match_real_flag_helpers(self):
        root = Path(__file__).resolve().parents[2]
        base = root / 'crates/ddnative-base/src'
        cases = []
        for bits in ('8', '16', '32'):
            for op in ('sub', 'and'):
                for cc in le.PREDICATE:
                    expr = le.direct_compare(op, bits, cc)
                    if expr is not None:
                        cases.append(f'{{ let mask = u32::MAX >> (32 - {bits}); let (a,b) = (x & mask,y & mask); '
                                     f'let mut f = Flags::default(); f.alu_{op}(x,y,{bits}); '
                                     f'assert_eq!({expr}, f.cond("{cc}"), "{op}/{bits}/{cc}: {{x:x}} {{y:x}}"); }}')
        arms = [
            f'0x1000 => {{ {ADD} pc = 0x1004; continue; }}',
            f'0x1004 => {{ {CMP} pc = if cpu.fl.cond("l") {{ 0x1010 }} else {{ 0x1020 }}; continue; }}',
            f'0x1010 => {{ {CMP} cpu.edx = cpu.fl.cond("b") as u32; {ADD} pc = 0x1030; continue; }}',
            '0x1020 => { { let r = cpu.fl.alu_and(m.fast_u32(pt, cpu.esp), cpu.ecx, 16); } '
            '{ let v = m.fast_u32(pt, cpu.esi); if cpu.fl.cond("ne") { cpu.edx = v; } } '
            + ' cpu.ebx = cpu.fl.lahf() as u32; pc = 0x1030; continue; }',
            '0x1030 => { cpu.fl.alu_sub(cpu.edx, cpu.eax, 32); m.fast_set_u32(pt, cpu.esp, cpu.edx); '
            'x86::call_ext(m, cpu, 42); return; }',
        ]
        optimized = le.function_flags([le.lean(a) for a in arms])
        self.assertEqual(sum(a.count('let l129_condition') for a in optimized), 2)
        source = '\n'.join(f'#[path="{(base / (m + ".rs")).as_posix()}"] mod {m};' for m in ('mem', 'x86', 'x87', 'emit', 'hooks'))
        source += '\nuse mem::Mem; use x86::{Cpu,Flags};'
        for module, body in [('plain', arms), ('lean', optimized), ('split', optimized)]:
            name = 'g_00001000' if module == 'split' else 'f_00001000'
            source += (f'mod {module} {{ use super::*; pub fn {name}(m: &mut dyn Mem, cpu: &mut Cpu) {{ '
                       'let pt = m.page_table(); let mut pc = 0x1000; loop { match pc {'
                       + '\n'.join(body) + '_ => unreachable!(), } } } }')
        source += FIXTURE.replace('#[test] fn same_state_memory_and_event_order()', 'fn same_state_memory_and_event_order()')
        source += '\nfn main() { same_state_memory_and_event_order(); let values = [0u32,1,2,0x7f,0x80,0xff,0x100,0x7fff,0x8000,0xffff,0x10000,0x7fffffff,0x80000000,0xfffffffe,0xffffffff]; for x in values { for y in values {'
        source += '\n'.join(cases) + '} } }'
        target = Path(os.environ.get('CARGO_TARGET_DIR', 'C:/ddda-targets/L129'))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='flags-', dir=target) as tmp:
            path = Path(tmp) / 'check.rs'
            exe = Path(tmp) / 'check.exe'
            path.write_text(source)
            for opt in (0, 1):
                for command in (['rustc', '--edition=2021', '-Awarnings', f'-Copt-level={opt}', str(path), '-o', str(exe)], [str(exe)]):
                    run = subprocess.run(command, capture_output=True, text=True, timeout=60)
                    self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == '__main__':
    unittest.main()
