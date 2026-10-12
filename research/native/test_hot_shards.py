import tempfile
import unittest
from pathlib import Path
from hot_shards import Layout, read_hot_list, configure_packages, shared_instruction_routes


class HotShardsTests(unittest.TestCase):
    def test_default_preserves_address_ownership_and_optimization(self):
        addresses = [0x400000, 0x480000, 0xC00000]
        layout = Layout(addresses)
        self.assertEqual([layout.owner(a) for a in addresses], ["08", "09", "24"])
        self.assertEqual(len(layout.shards), 32)
        self.assertTrue(all(layout.opt_level(s) == 1 for s in layout.shards))

    def test_explicit_empty_list_opts_into_cold_build(self):
        layout = Layout([0x400000], [])
        self.assertEqual(layout.owner(0x400000), "08")
        self.assertTrue(all(layout.opt_level(s) == 0 for s in layout.shards))

    def test_hot_ownership_is_deterministic_and_bounded(self):
        addresses = [0x400000, 0x400010, 0x400020, 0x480000]
        layout = Layout(addresses, reversed(addresses[:3]), capacity=2)
        self.assertEqual([layout.owner(a) for a in addresses], ["hot_00", "hot_00", "hot_01", "09"])
        self.assertEqual(layout.opt_level("hot_00"), 1)
        self.assertEqual(layout.opt_level("09"), 0)
        self.assertEqual(layout.owners, Layout(reversed(addresses), addresses[:3], capacity=2).owners)

    def test_package_configuration_and_return_to_default(self):
        import tomllib
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def write(path, text):
                path = Path(path)
                path.parent.mkdir(parents=True, exist_ok=True)
                if not path.exists() or path.read_text() != text:
                    path.write_text(text)
            write(root / "crates/ddlift/lift_00/Cargo.toml", '[package]\nname = "ddlift-00"\n')
            write(root / "crates/ddlift/lift_00/src/lib.rs", '//! Fixed address shard\nmod generated;\n')
            write(root / "crates/ddnative/Cargo.toml", '[package]\nname="ddnative"\n[dependencies]\n')
            write(root / "Cargo.toml", '[profile.play]\ninherits="release"\n[profile.play.package.ddlift-00]\nopt-level = 1\n')
            split = Layout([0x400000], [0x400000])
            configure_packages(root, split, write)
            native = tomllib.loads((root / "crates/ddnative/Cargo.toml").read_text())
            self.assertEqual(native['dependencies']['ddlift-hot-00']['path'], '../ddlift/lift_hot_00')
            profiles = tomllib.loads((root / "Cargo.toml").read_text())['profile']['play']['package']
            self.assertEqual(profiles['ddlift-00']['opt-level'], 0)
            self.assertEqual(profiles['ddlift-hot-00']['opt-level'], 1)
            all_profiles = tomllib.loads((root / 'Cargo.toml').read_text())['profile']
            self.assertEqual(all_profiles['dev']['package']['ddlift-hot-00']['opt-level'], 0)
            self.assertEqual(all_profiles['release']['package']['ddlift-hot-00']['opt-level'], 1)
            files = list(root.rglob('*.*'))
            stamps = {p: p.stat().st_mtime_ns for p in files}
            configure_packages(root, split, write)
            self.assertEqual(stamps, {p: p.stat().st_mtime_ns for p in files})
            configure_packages(root, Layout([0x400000]), write)
            native = tomllib.loads((root / "crates/ddnative/Cargo.toml").read_text())
            self.assertNotIn('ddlift-hot-00', native['dependencies'])
            profiles = tomllib.loads((root / "Cargo.toml").read_text())['profile']['play']['package']
            self.assertEqual(profiles['ddlift-00']['opt-level'], 1)
            self.assertNotIn('mod generated', (root / 'crates/ddlift/lift_hot_00/src/lib.rs').read_text())
            configure_packages(root, split, write)
            self.assertIn('mod generated', (root / 'crates/ddlift/lift_hot_00/src/lib.rs').read_text())

    def test_calls_cross_each_hot_cold_boundary(self):
        import lift
        addresses = [0x400000, 0x400010, 0x400020, 0x400030]
        layout = Layout(addresses, addresses[:3], capacity=2)
        calls = " ".join(f"super::lifted::f_{a:08x}(m, cpu);" for a in addresses)
        for caller in addresses:
            function = dict(addr=caller, name="synthetic", arms=[f"0x{caller:X} => {{ {calls} return; }}"])
            emitted = lift.shard_fn(function, layout.owner)
            for target in addresses:
                # Local function references also cover L60 call_local wrappers.
                direct = f"super::lifted::f_{target:08x}"
                dispatched = f"x86::call_lifted(m, cpu, 0x{target:08X})"
                self.assertEqual(direct in emitted, layout.owner(caller) == layout.owner(target))
                self.assertEqual(dispatched in emitted, layout.owner(caller) != layout.owner(target))

    def test_shared_instruction_keeps_old_address_band_precedence(self):
        low, high = 0x400000, 0x1000000  # old bands 08 and 00
        owners = {0x400001: {low, high}, 0x400002: {low}}
        self.assertEqual(shared_instruction_routes(owners, Layout([low, high])), [])
        for hot in [{high}, {low, high}]:
            layout = Layout([low, high], hot)
            self.assertEqual(shared_instruction_routes(owners, layout), [(0x400001, 'hot_00', high)])
        layout = Layout([low, high], {low})
        self.assertEqual(shared_instruction_routes(owners, layout), [(0x400001, '00', high)])

    def test_generator_routes_steps_calls_and_restores_default(self):
        import json
        import shutil
        from unittest.mock import patch
        import lift
        source = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            metadata = ['Cargo.toml', 'crates/ddnative/Cargo.toml']
            metadata += [f'crates/ddlift/lift_{i:02}/{name}' for i in range(lift.SHARDS)
                         for name in ['Cargo.toml', 'src/lib.rs']]
            for name in metadata:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source / name, target)
            (root / 'crates/ddnative/src/generated').mkdir(parents=True)
            (root / 'crates/ddnative/src/generated/mod.rs').write_text('pub mod lifted;\n')
            (root / 'patterns').mkdir()
            functions = [dict(addr=a, name='synthetic', arms=[f'0x{a:X} => {{ {body} return; }}'])
                         for a, body in [(0x400000, 'super::lifted::f_00400010(m, cpu);'),
                                         (0x400010, 'super::lifted::f_01000000(m, cpu);'),
                                         (0x1000000, 'eax += 1;')]]
            (root / 'patterns/lift.json').write_text(json.dumps(functions))
            def step(function, addresses):
                address = function['addr']
                return (f'pub fn s_{address:08x}(_: &mut dyn Mem, _: &mut Cpu, _: u32) -> Flow {{ Flow::Next(0x{address:X}) }}',
                        [address, 0x400001])
            with patch.object(lift, 'R', str(root)), patch.object(lift, 'N', str(root)), \
                 patch.object(lift, 'wired_ports', return_value={}), \
                 patch.object(lift, 'verification_gaps', return_value={}), \
                 patch.object(lift, 'fragments', return_value={}), \
                 patch.object(lift, 'step_source', side_effect=step):
                lift.gen(instructions='all')
                original = {p: p.read_text() for p in root.rglob('*.rs')}
                lift.gen(instructions='all', hot_functions={0x400000, 0x1000000})
                facade = (root / 'crates/ddnative/src/generated/lifted.rs').read_text()
                self.assertIn('0x00400001 => return Some(ddlift_hot_00::s_01000000)', facade)
                self.assertIn('ddlift_hot_00::f_00400000', facade)
                hot = (root / 'crates/ddlift/lift_hot_00/src/generated/lifted_000.rs').read_text()
                cold = (root / 'crates/ddlift/lift_08/src/generated/lifted_000.rs').read_text()
                self.assertIn('x86::call_lifted(m, cpu, 0x00400010)', hot)
                self.assertIn('x86::call_lifted(m, cpu, 0x01000000)', cold)
                stamps = {p: p.stat().st_mtime_ns for p in root.rglob('*.rs')}
                lift.gen(instructions='all', hot_functions={0x400000, 0x1000000})
                self.assertEqual(stamps, {p: p.stat().st_mtime_ns for p in root.rglob('*.rs')})
                lift.gen(instructions='all')
                self.assertEqual(original, {p: p.read_text() for p in original})
                self.assertFalse((root / 'crates/ddlift/lift_hot_00/src/generated/lifted_000.rs').exists())

    def test_unknown_hot_address_rejected(self):
        with self.assertRaisesRegex(ValueError, "no lifted body"):
            Layout([0x400000], [0x400010])

    def test_list_supports_hex_comments_and_duplicates(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory, "hot.txt")
            path.write_text("00400000\n0x400010 # sample\n00400000\n# comment\n")
            self.assertEqual(read_hot_list(path), {0x400000, 0x400010})
            for invalid in ["0x100000000", "-1", "00400000 00400010"]:
                path.write_text(invalid)
                with self.assertRaises(ValueError):
                    read_hot_list(path)


if __name__ == "__main__":
    unittest.main()
