"""Synthetic multi-crate calls and regeneration, without private game data."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import lift


class ShardTests(unittest.TestCase):
    def test_crate_graph_calls_and_stable_writes(self):
        root = Path(__file__).resolve().parents[2]
        target = Path(os.environ.get('CARGO_TARGET_DIR', 'C:/ddda-targets/L27b'))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target, prefix="shards-test-") as tmp:
            repo = Path(tmp)
            (repo / "Cargo.toml").write_text('[workspace]\nresolver="2"\nmembers=["crates/*", "crates/ddlift/lift_*"]\nexclude=["crates/ddlift"]\n[workspace.package]\nedition="2021"\nlicense="MIT"\npublish=false\n')
            shutil.copytree(root / "crates/ddnative-base", repo / "crates/ddnative-base")
            # Shards depend on mtmath (L24), mtcore (L33) and mtframe (L39);
            # with no PORTS table, gen wires nothing.
            for crate in ("mtmath", "mtcore", "mtframe"):
                (repo / f"crates/{crate}/src").mkdir(parents=True)
                (repo / f"crates/{crate}/Cargo.toml").write_text(f'[package]\nname="{crate}"\nversion="0.1.0"\nedition.workspace=true\n')
                (repo / f"crates/{crate}/src/lib.rs").write_text('')
            for i in range(lift.SHARDS):
                dest = repo / f"crates/ddlift/lift_{i:02}"
                (dest / "src").mkdir(parents=True)
                for name in ("Cargo.toml", "src/lib.rs"):
                    shutil.copyfile(root / f"crates/ddlift/lift_{i:02}" / name, dest / name)
            native = repo / "crates/ddnative"
            (native / "src/generated").mkdir(parents=True)
            (native / "tests").mkdir()
            shutil.copyfile(root / "crates/ddnative/Cargo.toml", native / "Cargo.toml")
            (native / "src/lib.rs").write_text('pub use ddnative_base::{mem, x86, x87, platform}; pub mod generated;')
            (native / "src/generated/mod.rs").write_text('pub mod lifted;\n')
            (repo / "patterns").mkdir()
            def function(addr, body):
                return dict(addr=addr, name="synthetic", arms=[f"0x{addr:X} => {{ {body} return; }}"])
            functions = [
                function(0x400000, 'super::lifted::f_00500000(m, cpu); eax += 1;'),
                function(0x500000, 'super::lifted::f_00500010(m, cpu); super::lifted::f_00600000(m, cpu);'),
                function(0x500010, 'eax += 2;'),
                function(0x600000, 'x86::call_ext(m, cpu, 0x900000);'),
            ]
            functions[-1]['ext'] = True
            patterns = repo / "patterns/lift.json"
            patterns.write_text(json.dumps(functions))
            with patch.object(lift, "R", str(repo)), patch.object(lift, "N", str(repo)):
                lift.gen(instructions=False)
                files = list(repo.glob("crates/**/*.rs"))
                stamps = {p: p.stat().st_mtime_ns for p in files}
                lift.gen(instructions=False)
                self.assertEqual(stamps, {p: p.stat().st_mtime_ns for p in files})
                functions[2]["arms"][0] = functions[2]["arms"][0].replace('+= 2', '+= 3')
                patterns.write_text(json.dumps(functions))
                lift.gen(instructions=False)
                changed = [p.relative_to(repo).as_posix() for p in files if p.stat().st_mtime_ns != stamps[p]]
                self.assertEqual(changed, ['crates/ddlift/lift_05/src/generated/lifted_000.rs'])
                # Removal of an entire shard leaves a valid empty registry.
                patterns.write_text(json.dumps(functions[:3]))
                with self.assertRaisesRegex(ValueError, 'missing lifted callees'):
                    lift.gen(instructions=False)
                patterns.write_text(json.dumps([functions[2]]))
                lift.gen(instructions=False)
                self.assertFalse((repo / 'crates/ddlift/lift_06/src/generated/lifted_000.rs').exists())
                patterns.write_text(json.dumps(functions))
                lift.gen(instructions=False)
            (native / 'tests/shards.rs').write_text(r'''
use ddnative::{generated::lifted, mem::Sparse, x86::{self, Cpu}};
#[test] fn nested_direct_calls_and_external_boundary() {
    let mut m = Sparse::default(); let mut c = Cpu::default();
    x86::set_ext_calls(Some(Box::new(|_, c, t| { assert_eq!(t, 0x900000); c.eax *= 7; })));
    // Public entry initializes itself, without requiring all() first.
    lifted::f_00400000(&mut m, &mut c);
    assert_eq!(c.eax, 22);
    let all = lifted::all(); assert_eq!(all.len(), 4);
    let external = lifted::dispatch_table();
    assert!(external.contains_key(&0x400000));
    assert!(!external.contains_key(&0x600000));
    // The immutable resolver works on another thread and after a panic.
    std::thread::spawn(|| {
        let mut m = Sparse::default(); let mut c = Cpu::default();
        let bad = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x86::call_lifted(&mut m, &mut c, 0xdeadbeef)));
        assert!(bad.is_err()); x86::call_lifted(&mut m, &mut c, 0x500010);
        assert_eq!(c.eax, 3);
    }).join().unwrap();
    x86::set_ext_calls(None);
}
''')
            env = dict(os.environ, CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
                       CARGO_TARGET_DIR=str(repo / 'target'))
            # Each synthetic workspace is isolated; never compile the real batch here.
            run = subprocess.run(['cargo', 'test', '-p', 'ddnative', '--test', 'shards', '--offline'], cwd=repo, env=env, capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_inventory_scans_shards_and_ignores_obsolete_modules(self):
        root = Path(__file__).resolve().parents[2]
        spec = importlib.util.spec_from_file_location('inventory', root / 'scripts/inventory.py')
        inventory = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(inventory)
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            samples = {
                'crates/ddnative/src/port.rs': '// Translated from 0x00400000',
                'crates/ddnative/src/generated/lifted_000.rs': '// Translated from 0x00400010',
                'crates/ddlift/lift_05/src/generated/lifted_000.rs': '// Translated from 0x00500000',
                'crates/ddnative/tests/lifted.rs': '// verifies 0x00400000 0x00500000',
            }
            for name, text in samples.items():
                p = repo / name; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(text)
            ported, tested = inventory.native_declarations(repo)
            self.assertEqual(ported, {0x400000, 0x500000})
            self.assertEqual(ported, tested)


if __name__ == '__main__':
    unittest.main()
