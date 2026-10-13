"""Synthetic PE fixtures: no installed game, GPU, process attachment or network."""
import io
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

from profile_symbols import PEImage, fetch_pdb


def fixture():
    data = bytearray(0x1200)
    data[:2] = b'MZ'
    struct.pack_into('<I', data, 0x3c, 0x80)
    data[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<HHI', data, 0x84, 0x8664, 1, 123)
    struct.pack_into('<H', data, 0x94, 240)
    optional = 0x98
    struct.pack_into('<H', data, optional, 0x20b)
    struct.pack_into('<I', data, optional + 56, 0x2000)
    struct.pack_into('<I', data, optional + 108, 16)
    struct.pack_into('<IIII', data, optional + 240 + 8, 0x1000, 0x1000, 0x1000, 0x200)
    struct.pack_into('<I', data, optional + 240 + 36, 0x60000020)

    def put(rva, content):
        data[rva - 0x1000 + 0x200:rva - 0x1000 + 0x200 + len(content)] = content

    def directory(index, rva, size):
        struct.pack_into('<II', data, optional + 112 + 8 * index, rva, size)

    directory(3, 0x1800, 24)
    put(0x1800, struct.pack('<IIIIII', 0x1100, 0x1140, 0x1900, 0x1200, 0x1240, 0x1900))
    directory(0, 0x1500, 0x200)
    put(0x1500, struct.pack('<IIHHIIIIIII', 0, 0, 0, 0, 0, 1, 3, 3, 0x1540, 0x1550, 0x1560))
    put(0x1540, struct.pack('<III', 0x1000, 0x1300, 0x1600))
    put(0x1550, struct.pack('<III', 0x1570, 0x1580, 0x1590))
    put(0x1560, struct.pack('<HHH', 0, 1, 2))
    put(0x1570, b'one_byte_ret\0')
    put(0x1580, b'copy_export\0')
    put(0x1590, b'forwarded\0')
    put(0x1600, b'other.copy\0')
    put(0x1000, b'\xc3\xcc\xcc\xcc\xcc')
    put(0x1300, b'\xe9' + struct.pack('<i', 0x1200 - 0x1305))
    directory(6, 0x1700, 28)
    codeview = b'RSDS' + bytes(range(16)) + struct.pack('<I', 12) + b'C:\\build\\fixture.pdb\0'
    put(0x1700, struct.pack('<IIHHIIII', 0, 0, 0, 0, 2, len(codeview), 0x1740, 0))
    put(0x1740, codeview)
    return data


class SymbolTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'fixture.dll'
        self.path.write_bytes(fixture())
        self.image = PEImage(self.path)

    def test_identity(self):
        self.assertEqual(self.image.timestamp, 123)
        self.assertEqual(self.image.size, 0x2000)
        self.assertEqual(self.image.pdb, ('fixture.pdb', '030201000504070608090A0B0C0D0E0FC'))

    def test_no_nearest_export_bleed_into_helper(self):
        previous = dict(name='one_byte_ret', rva=0x1000, size=0)
        self.assertEqual(self.image.bounded_name(0x1000, previous), ('one_byte_ret', 'export-instruction'))
        self.assertIn('unresolved', self.image.bounded_name(0x1001, previous)[0])
        self.assertEqual(self.image.bounded_name(0x1105, previous),
                         ('<unresolved function RVA 0x1100>', 'unwind-only'))

    def test_pdb_size_and_exclusive_end(self):
        symbol = dict(name='private_helper', rva=0x1100, size=0x30)
        self.assertEqual(self.image.bounded_name(0x112f, symbol), ('private_helper', 'pdb-size'))
        self.assertNotEqual(self.image.bounded_name(0x1130, symbol)[0], 'private_helper')
        self.assertNotEqual(self.image.bounded_name(0x10ff, symbol)[0], 'private_helper')

    def test_dbghelp_synthetic_export_size_is_not_a_function_size(self):
        export = dict(name='one_byte_ret', rva=0x1000, size=0x300, flags=0x200)
        self.assertEqual(self.image.bounded_name(0x1000, export), ('one_byte_ret', 'export-instruction'))
        self.assertEqual(self.image.bounded_name(0x1105, export)[1], 'unwind-only')

    def test_zero_size_unwind_range(self):
        symbol = dict(name='private_helper', rva=0x1100, size=0)
        self.assertEqual(self.image.bounded_name(0x113f, symbol), ('private_helper', 'unwind-range'))
        self.assertIsNone(self.image.function_range(0x1140))
        self.assertIsNone(self.image.function_range(0xfff))

    def test_export_thunk_target(self):
        self.assertEqual(self.image.bounded_name(0x1210, {}), ('copy_export', 'export-thunk/unwind-range'))
        self.assertNotIn(0x1600, self.image.exports)
        self.assertEqual(self.image.bounded_name(0x1304, dict(name='copy_export', rva=0x1300, size=0)),
                         ('copy_export', 'export-instruction'))
        self.assertIn('unresolved', self.image.bounded_name(0x1305, {})[0])

    def test_exact_unknown_size_symbol_does_not_claim_next_byte(self):
        symbol = dict(name='leaf', rva=0x1400, size=0)
        self.assertEqual(self.image.bounded_name(0x1400, symbol), ('leaf', 'exact-address'))
        self.assertEqual(self.image.bounded_name(0x1401, symbol)[1], 'unresolved')

    def test_download_exact_identity_and_reuse(self):
        with patch('urllib.request.urlopen', return_value=io.BytesIO(b'synthetic pdb')) as fetch:
            destination = fetch_pdb(self.image, self.temp.name)
            self.assertEqual(destination.read_bytes(), b'synthetic pdb')
            self.assertEqual(fetch_pdb(self.image, self.temp.name), destination)
            self.assertEqual(fetch.call_count, 1)
            self.assertIn('/030201000504070608090A0B0C0D0E0FC/', fetch.call_args.args[0])

    def test_reject_other_architecture(self):
        data = fixture()
        struct.pack_into('<H', data, 0x84, 0x14c)
        self.path.write_bytes(data)
        with self.assertRaises(ValueError):
            PEImage(self.path)


if __name__ == '__main__':
    unittest.main()
