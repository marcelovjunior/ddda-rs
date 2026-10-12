"""The gen() wiring of the readable math ports (L24, docs/MATH.md)."""
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, os.path.dirname(__file__))
import lift  # noqa: E402


class WiringTests(unittest.TestCase):
    def test_ports_shadow_their_lifted_bodies_and_keep_them(self):
        selected = [{"addr": a} for a in (0x00D00010, 0x00D00690, 0x00D006C0)]
        text = lift.wired_lines(selected, {0x00D006C0, 0x00D00690}, chunk=2)
        self.assertEqual(text, "pub use mtmath::abi::f_00d00690;\n"
                               "pub use mtmath::abi::f_00d006c0;\n"
                               "/// The lifted bodies of the functions mtmath replaces.\n"
                               "pub mod replaced {\n"
                               "    pub use super::lifted_000::f_00d00690;\n"
                               "    pub use super::lifted_001::f_00d006c0;\n"
                               "}\n")

    def test_a_shard_without_ports_is_unchanged(self):
        self.assertEqual(lift.wired_lines([{"addr": 0x00401000}], {0x00D00690}, chunk=400), "")

    def test_ports_come_from_the_ports_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "crates/mtmath/src"))
            with open(os.path.join(tmp, "crates/mtmath/src/abi.rs"), "w") as f:
                f.write("pub fn f_00cfc770() {}\n"  # an adapter alone is not wired
                        "pub const PORTS: &[(u32, LiftFn, u8)] = &[\n"
                        "    (0x00D0_0690, f_00d00690, 0b1111_1111),\n"
                        "    (0x007A_D870, f_007ad870, 0b0001_1111),\n];\n")
            with patch.object(lift, "R", tmp):
                self.assertEqual(lift.wired_ports(), {0x00D00690, 0x007AD870})
            with patch.object(lift, "R", os.path.join(tmp, "absent")):
                self.assertEqual(lift.wired_ports(), set())


if __name__ == "__main__":
    unittest.main()
