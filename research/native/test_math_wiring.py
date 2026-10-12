"""The gen() wiring of the readable ports (L24 mtmath, L33 mtcore;
docs/MATH.md, docs/CORE.md)."""
import os
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, os.path.dirname(__file__))
import lift  # noqa: E402


def write_ports(root, crate, rows):
    os.makedirs(os.path.join(root, f"crates/{crate}/src"))
    with open(os.path.join(root, f"crates/{crate}/src/abi.rs"), "w") as f:
        f.write("pub fn f_00cfc770() {}\n"  # an adapter alone is not wired
                "pub const PORTS: &[(u32, LiftFn, u8)] = &[\n" + rows + "];\n")


class WiringTests(unittest.TestCase):
    def test_ports_shadow_their_lifted_bodies_and_keep_them(self):
        selected = [{"addr": a} for a in (0x00D00010, 0x00D00690, 0x00D006C0, 0x00D01B60)]
        ported = {0x00D006C0: "mtmath", 0x00D00690: "mtmath", 0x00D01B60: "mtcore"}
        text = lift.wired_lines(selected, ported, chunk=2)
        self.assertEqual(text, "pub use mtmath::abi::f_00d00690;\n"
                               "pub use mtmath::abi::f_00d006c0;\n"
                               "pub use mtcore::abi::f_00d01b60;\n"
                               "/// The lifted bodies of the functions the readable ports replace.\n"
                               "pub mod replaced {\n"
                               "    pub use super::lifted_000::f_00d00690;\n"
                               "    pub use super::lifted_001::f_00d006c0;\n"
                               "    pub use super::lifted_001::f_00d01b60;\n"
                               "}\n")

    def test_a_shard_without_ports_is_unchanged(self):
        self.assertEqual(lift.wired_lines([{"addr": 0x00401000}], {0x00D00690: "mtmath"}, chunk=400), "")

    def test_ports_come_from_the_ports_tables(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_ports(tmp, "mtmath", "    (0x00D0_0690, f_00d00690, 0b1111_1111),\n"
                                       "    (0x007A_D870, f_007ad870, 0b0001_1111),\n")
            with patch.object(lift, "R", tmp):
                self.assertEqual(lift.wired_ports(), {0x00D00690: "mtmath", 0x007AD870: "mtmath"})
            write_ports(tmp, "mtcore", "    (0x00CF_5E20, f_00cf5e20, 0b1111_1111),\n")
            with patch.object(lift, "R", tmp):
                self.assertEqual(lift.wired_ports(), {0x00D00690: "mtmath", 0x007AD870: "mtmath",
                                                      0x00CF5E20: "mtcore"})
            with patch.object(lift, "R", os.path.join(tmp, "absent")):
                self.assertEqual(lift.wired_ports(), {})

    def test_an_address_ported_twice_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_ports(tmp, "mtmath", "    (0x00D0_0690, f_00d00690, 0b1111_1111),\n")
            write_ports(tmp, "mtcore", "    (0x00D0_0690, f_00d00690, 0b1111_1111),\n")
            with patch.object(lift, "R", tmp), self.assertRaisesRegex(ValueError, "both mtmath and mtcore"):
                lift.wired_ports()


if __name__ == "__main__":
    unittest.main()
