import tempfile
import unittest
from pathlib import Path
from hot_shards import Layout, read_hot_list, configure_packages, route_calls


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
        addresses = [0x400000, 0x400010, 0x400020, 0x400030]
        layout = Layout(addresses, addresses[:3], capacity=2)
        calls = " ".join(f"super::lifted::f_{a:08x}(m, cpu);" for a in addresses)
        for caller in addresses:
            emitted = route_calls(calls, caller, layout)
            for target in addresses:
                direct = f"super::lifted::f_{target:08x}("
                dispatched = f"x86::call_lifted(m, cpu, 0x{target:08X})"
                self.assertEqual(direct in emitted, layout.owner(caller) == layout.owner(target))
                self.assertEqual(dispatched in emitted, layout.owner(caller) != layout.owner(target))

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
