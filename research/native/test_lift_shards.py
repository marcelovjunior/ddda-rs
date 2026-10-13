"""Synthetic multi-crate calls and regeneration, without private game data."""
import importlib.util
import json
import re
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
            # L74: the slow paths' policy (lifted code needs it installed)
            shutil.copytree(root / "crates/ddnative-host", repo / "crates/ddnative-host")
            # The facade (ddnative) still names mtmath; the shards depend on
            # no port crate (L60: ports are registered overrides).
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
                self.assertEqual(changed, [f'crates/ddlift/lift_{lift.shard(0x500010):02}/src/generated/lifted_000.rs'])
                # Removal of an entire shard leaves a valid empty registry.
                patterns.write_text(json.dumps(functions[:3]))
                with self.assertRaisesRegex(ValueError, 'missing lifted callees'):
                    lift.gen(instructions=False)
                patterns.write_text(json.dumps([functions[2]]))
                lift.gen(instructions=False)
                self.assertFalse((repo / f'crates/ddlift/lift_{lift.shard(0x600000):02}/src/generated/lifted_000.rs').exists())
                patterns.write_text(json.dumps(functions))
                lift.gen(instructions=False)
            (native / 'tests/shards.rs').write_text(r'''
use ddnative::{generated::lifted, mem::Sparse, x86::{self, Cpu}};
#[test] fn nested_direct_calls_and_external_boundary() {
    ddnative_host::install();
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
            # L60: an override registered by address replaces the lifted body
            # for an in-shard call (0x500000 -> 0x500010), a cross-shard
            # call and the facade's all(); its own process (the registration
            # is once per process)
            (native / 'tests/overrides.rs').write_text(r'''
use ddnative::{generated::lifted, mem::{Mem, Sparse}, x86::{self, Cpu, LiftFn}};
fn hundred(_: &mut dyn Mem, c: &mut Cpu) { c.eax += 100; }
#[test] fn an_override_replaces_the_body_for_every_caller() {
    ddnative_host::install();
    x86::set_overrides(&[(0x500010, hundred as LiftFn)]);
    x86::set_ext_calls(Some(Box::new(|_, c, t| { assert_eq!(t, 0x900000); c.eax *= 7; })));
    let mut m = Sparse::default(); let mut c = Cpu::default();
    lifted::f_00500000(&mut m, &mut c);
    assert_eq!(c.eax, 700);
    let mut c = Cpu::default();
    x86::call_lifted(&mut m, &mut c, 0x500010);
    assert_eq!(c.eax, 100);
    let all = lifted::all();
    let f = all.iter().find(|e| e.0 == 0x500010).unwrap().1;
    assert!(std::ptr::fn_addr_eq(f, hundred as LiftFn));
    let body = lifted::bodies().into_iter().find(|e| e.0 == 0x500010).unwrap().1;
    let mut c = Cpu::default(); body(&mut m, &mut c); assert_eq!(c.eax, 3); // the body edited above
    x86::set_ext_calls(None);
}
''')
            env = dict(os.environ, CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
                       CARGO_TARGET_DIR=str(repo / 'target'))
            # Each synthetic workspace is isolated; never compile the real batch here.
            run = subprocess.run(['cargo', 'test', '-p', 'ddnative', '--test', 'shards', '--test', 'overrides', '--offline'],
                                 cwd=repo, env=env, capture_output=True, text=True)
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


class ChunkTests(unittest.TestCase):
    def test_modules_split_by_bytes_and_count(self):
        # L100: a shard's functions go into modules of about `chunk_bytes`
        # (rustc gives each module its own codegen units), in address order.
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            (repo / "Cargo.toml").write_text('[workspace]\n')
            for i in range(lift.SHARDS):
                (repo / f"crates/ddlift/lift_{i:02}/src").mkdir(parents=True)
            for name in ("Cargo.toml", "src/lib.rs"):
                shutil.copyfile(root / "crates/ddlift/lift_00" / name, repo / "crates/ddlift/lift_00" / name)
            (repo / "crates/ddnative/src/generated").mkdir(parents=True)
            shutil.copyfile(root / "crates/ddnative/Cargo.toml", repo / "crates/ddnative/Cargo.toml")
            (repo / "crates/ddnative/src/generated/mod.rs").write_text('pub mod lifted;\n')
            (repo / "patterns").mkdir()
            addrs = [0x500000 + 0x10 * k for k in range(5)]
            (repo / "patterns/lift.json").write_text(json.dumps(
                [dict(addr=a, name="synthetic", arms=[f"0x{a:X} => {{ eax += 1; return; }}"]) for a in addrs]))
            generated = repo / f"crates/ddlift/lift_{lift.shard(0x500000):02}/src/generated"

            def modules():
                found = {}
                for p in sorted(generated.glob("lifted_*.rs")):
                    found[p.stem] = [int(a, 16) for a in re.findall(r'^pub fn f_([0-9a-f]{8})', p.read_text(), re.M)]
                return found

            with patch.object(lift, "R", str(repo)), patch.object(lift, "N", str(repo)):
                lift.gen(instructions=False)
                self.assertEqual(modules(), {'lifted_000': addrs})
                lift.gen(instructions=False, chunk=2)
                self.assertEqual(modules(), {'lifted_000': addrs[:2], 'lifted_001': addrs[2:4], 'lifted_002': addrs[4:]})
                lift.gen(instructions=False, chunk_bytes=1)
                self.assertEqual(modules(), {f'lifted_{k:03}': [a] for k, a in enumerate(addrs)})
                index = (generated / "mod.rs").read_text()
                for k in range(len(addrs)):
                    self.assertIn(f'mod lifted_{k:03}; pub use lifted_{k:03}::*;', index)
                # back to one module: the stale ones are removed
                lift.gen(instructions=False)
                self.assertEqual(modules(), {'lifted_000': addrs})


if __name__ == '__main__':
    unittest.main()
