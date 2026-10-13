"""Compile/test the allocator diagnostic alone; never builds or runs ddplay."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class AllocLogTests(unittest.TestCase):
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
            for switch in ('0', '1'):
                env = dict(os.environ, DDPLAY_ALLOC_LOG=switch)
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
            result = subprocess.run([os.environ.get('RUSTC', 'rustc'), '--edition=2024',
                                     str(source), '-o', str(executable)],
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for switch in ('0', '1'):
                result = subprocess.run([str(executable)], env=dict(os.environ, DDPLAY_ALLOC_LOG=switch),
                                        capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
