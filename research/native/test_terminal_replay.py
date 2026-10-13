"""Terminal fixtures end to end: the recorder's terminal-v1 case replayed by the
generated lifted.rs template against the real emitter's Rust for that body.

Builds a throwaway crate on ddnative-base and serde_json only (cargo --offline,
the workspace lock), not the lifted shards. Skips without cargo.
"""
import gc
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
import lift

ROOT = Path(__file__).resolve().parents[2]
# push ebp; mov ebp,esp; push 1; call 0x400100 (a noreturn entry)
CODE = bytes.fromhex('558bec6a01e8f6000000')
BASE, NORETURN = 0x400000, 0x400100


class TerminalReplay(unittest.TestCase):
    def tearDown(self):
        gc.collect()

    def synthetic(self):
        image = CODE.ljust(0x1000, bytes([0xcc]))
        return (patch.object(h, 'image', return_value=(BASE, [(BASE, image)])),
                patch.object(h, 'code', side_effect=lambda a, n: image[a - BASE:a - BASE + n]),
                patch.object(lift, 'noreturn_entries', return_value=frozenset({NORETURN})),
                patch.object(lift, 'function_entries', return_value=frozenset({BASE, NORETURN})),
                patch.object(h, 'executable', side_effect=lambda a, n=1: BASE <= a < BASE + 0x1000))

    def record_and_lift(self):
        p = self.synthetic()
        with p[0], p[1], p[2], p[3], p[4], patch.object(h, 'static_pop', return_value=0):
            arms = lift.lift(BASE, len(CODE), frozenset(), ext=True)
            info = lift.BODY_INFO[BASE]
            lift.release_body(BASE, len(CODE))
            case = h.run_sparse(BASE, 9, body=info['body'], end=BASE + len(CODE),
                                noreturn_sites=info['noreturn_sites'], noreturn_targets={NORETURN}, terminal=True)
        return lift.rust_fn(BASE, 'synthetic', arms), info, case

    def test_recorded_terminal_case_replays_and_a_wrong_one_fails(self):
        cargo = shutil.which('cargo')
        if cargo is None:
            self.skipTest('cargo not available')
        function, info, case = self.record_and_lift()
        self.assertEqual(info['noreturn_sites'], [BASE + 5])
        self.assertTrue(case['terminal'])
        source = lift.lifted_test_source('// verifies 0x00400000')
        stub = ('mod lifted {\n    use ddnative_base::mem::Mem;\n    use ddnative_base::x86::{self, Cpu, Flags};\n'
                 '    use ddnative_base::emit;\n'
                + function + '\n'
                '    pub fn all() -> Vec<(u32, fn(&mut dyn Mem, &mut Cpu))> { vec![(0x400000, f_00400000)] }\n'
                '    pub fn dispatch_table() -> std::collections::HashMap<u32, fn(&mut dyn Mem, &mut Cpu)> { Default::default() }\n'
                '}\n')
        self.assertIn('use ddnative::generated::lifted;\n', source)
        source = '#![allow(warnings)]\n' + source.replace('use ddnative::generated::lifted;\n', stub).replace('ddnative::', 'ddnative_base::')
        wrong = json.loads(json.dumps(case))
        wrong['regs_out']['esp'] ^= 4
        target = ROOT / 'target'
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='terminal-test-', dir=target) as tmp:
            tmp = Path(tmp)
            (tmp / 'tests').mkdir()
            (tmp / 'Cargo.toml').write_text(
                '[package]\nname = "terminal_check"\nversion = "0.0.0"\nedition = "2021"\n[workspace]\n'
                f'[dependencies]\nddnative-base = {{ path = "{(ROOT / "crates/ddnative-base").as_posix()}" }}\n'
                'serde_json = "1"\n')
            (tmp / 'src').mkdir()
            (tmp / 'src/lib.rs').write_text('')
            shutil.copy(ROOT / 'Cargo.lock', tmp / 'Cargo.lock')
            (tmp / 'tests/lifted.rs').write_text(source)
            data = tmp / 'data'
            (data / 'vectors').mkdir(parents=True)
            env = dict(os.environ, LIFT_DATA=str(data), DDNATIVE_REQUIRE_VECTORS='1',
                       CARGO_TARGET_DIR=str(target / 'terminal-check'), CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0')
            run = lambda: subprocess.run([cargo, 'test', '--offline', '-q', '--test', 'lifted'], cwd=tmp, env=env,
                                         capture_output=True, text=True)
            (data / 'vectors/lift.json').write_text(json.dumps([dict(addr=BASE, **case)]))
            good = run()
            self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
            (data / 'vectors/lift.json').write_text(json.dumps([dict(addr=BASE, **wrong)]))
            bad = run()
            self.assertNotEqual(bad.returncode, 0, bad.stdout + bad.stderr)
            self.assertIn('0x00400000', bad.stdout + bad.stderr)


if __name__ == '__main__':
    unittest.main()
