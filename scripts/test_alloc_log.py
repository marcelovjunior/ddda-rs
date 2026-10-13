"""Compile/test the allocator diagnostic alone; never builds or runs ddplay."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class AllocLogTests(unittest.TestCase):
    def test_stack_ranking_uses_latest_counters_and_merges_duplicate_stacks(self):
        from alloc_sites import latest
        text = '\n'.join([
            'alloc-site id=3 bytes=200 calls=2 base=1000 pcs=1001,1002,0',
            'alloc-site id=3 bytes=900 calls=9 base=1000 pcs=1001,1002,0',
            'alloc-site id=4 bytes=100 calls=1 base=1000 pcs=1001,1002,0',
            'alloc-site id=5 bytes=950 calls=5 base=1000 pcs=1100,1200,0',
        ])
        self.assertEqual(latest(text), [
            (1000, 10, 0x1000, (0x1001, 0x1002)),
            (950, 5, 0x1000, (0x1100, 0x1200)),
        ])

    def test_system_allocator_forwarding_and_switch(self):
        root = Path(__file__).resolve().parents[1]
        target = Path(os.environ.get('CARGO_TARGET_DIR', root / 'target'))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='alloc-log-', dir=target) as temporary:
            binary = Path(temporary) / 'test.exe'
            command = [os.environ.get('RUSTC', 'rustc'), '--edition=2024', '--test',
                       str(root / 'crates/ddplay/src/alloc_log.rs'), '-o', str(binary)]
            result = subprocess.run(command, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for switch, stacks in (('0', '0'), ('1', '0'), ('1', '1')):
                env = dict(os.environ, DDPLAY_ALLOC_LOG=switch, DDPLAY_ALLOC_STACKS=stacks)
                result = subprocess.run([str(binary), '--test-threads=1'], env=env,
                                        capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            # Compile a normal console fixture as well: unlike --test this
            # installs the real global allocator, catching recursive startup.
            source = Path(temporary) / 'global.rs'
            source.write_text(f'#[path="{(root / "crates/ddplay/src/alloc_log.rs").as_posix()}"] mod alloc_log;\n' + r'''
fn main() {
    let before=alloc_log::snapshot();
    let mut data=Vec::with_capacity(1<<20); data.resize(1<<20,42u8);
    std::hint::black_box(&data);
    let allocated=alloc_log::snapshot();
    if let (Some(before),Some(allocated))=(before,allocated) {
        assert!(allocated.live_bytes>=before.live_bytes+(1<<20));
    }
    drop(data);
    if let (Some(allocated),Some(after))=(allocated,alloc_log::snapshot()) {
        assert!(after.live_bytes+(1<<20)<=allocated.live_bytes);
    }
}
''')
            executable = Path(temporary) / 'global.exe'
            result = subprocess.run([os.environ.get('RUSTC', 'rustc'), '--edition=2024', '-g',
                                     str(source), '-o', str(executable)],
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for switch, stacks in (('0', '0'), ('1', '0'), ('1', '1')):
                result = subprocess.run([str(executable)], env=dict(os.environ, DDPLAY_ALLOC_LOG=switch, DDPLAY_ALLOC_STACKS=stacks),
                                        capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                if stacks == '1':
                    self.assertIn('alloc-site id=', result.stderr)
                    self.assertIn('pcs=', result.stderr)
                    log = Path(temporary) / 'sites.log'
                    log.write_text(result.stderr)
                    ranked = subprocess.run([os.environ.get('PYTHON', 'python'), str(root / 'scripts/alloc_sites.py'),
                                             str(log), '--exe', str(executable)],
                                            capture_output=True, text=True, timeout=30)
                    self.assertEqual(ranked.returncode, 0, ranked.stdout + ranked.stderr)
                    self.assertIn('requested_bytes=', ranked.stdout)
                    self.assertIn('global::', ranked.stdout)


if __name__ == '__main__':
    unittest.main()
