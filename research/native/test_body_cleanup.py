"""Complete-body mock cleanup inputs, including unknown-edge refusals."""
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from body_cleanup import BodyCleanup, uniform_cleanup
from structured_inputs import _inputs


class BodyCleanupTests(unittest.TestCase):
    def fixture(self, callee=None):
        # A checked table path and an independent constant-index path join.
        callee = callee or bytes.fromhex(
            '85c9 7407 83f801 770d eb04 31c0 eb00 ff248500024000 c20400 c20400')
        image = bytearray(b'\xcc' * 4096)
        caller = bytes.fromhex('6a01 e8f9000000 c3')
        image[:len(caller)] = caller
        image[0x100:0x100+len(callee)] = callee
        struct.pack_into('<II', image, 0x200, 0x400116, 0x400119)
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        body = [i.address for i in md.disasm(callee, 0x400100)]
        def read(address, size):
            if not 0x400000 <= address < 0x401000:
                raise KeyError(address)
            return bytes(image[address-0x400000:address-0x400000+size])
        layout = {'bodies': {'0x400100': body}}
        return image, caller, body, layout, read

    def test_joined_switch_bound_provides_uniform_cleanup(self):
        _, _, body, layout, read = self.fixture()
        self.assertEqual(uniform_cleanup(0x400100, body, read), 4)
        inputs = BodyCleanup(layout, read)
        self.assertEqual(inputs.pop(0x400100), 4)
        self.assertIsNone(inputs.pop(0x400300))

    def test_unknown_edges_incomplete_body_and_conflicting_returns_refuse(self):
        code = bytes.fromhex('85c9 7407 83f801 770d eb04 31c0 eb00 ff248500024000 c20400 c20400')
        variants = [code.replace(bytes.fromhex('31c0'), bytes.fromhex('89d0')),
                    code[:-3]+bytes.fromhex('c20800'),
                    bytes.fromhex('ffe0 c20400'), bytes.fromhex('90'),
                    bytes.fromhex('eb02 c20400'), bytes.fromhex('c20400 cc')]
        for callee in variants:
            _, _, body, _, read = self.fixture(callee)
            self.assertIsNone(uniform_cleanup(0x400100, body, read))
        _, _, body, _, read = self.fixture()
        self.assertIsNone(uniform_cleanup(0x400100, body[:-1], read))
        def missing_table(address, size):
            return b'' if address >= 0x400200 else read(address, size)
        self.assertIsNone(uniform_cleanup(0x400100, body, missing_table))

    def test_layout_validation(self):
        for layout in ({}, {'bodies': {}}, {'bodies': {'bad': [0]}},
                       {'bodies': {'0x100': [0x101]}},
                       {'bodies': {'0x100': [0x100, 0x100]}},
                       {'bodies': {'0x100': [True]}},
                       {'bodies': {'0x100': [0x100], '256': [0x100]}}):
            with self.subTest(layout=layout), self.assertRaises(ValueError):
                BodyCleanup(layout, lambda a,n:b'')

    def test_original_caller_preserves_return_address_and_replay_layout(self):
        image, caller, _, layout, read = self.fixture()
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        body = [i.address for i in md.disasm(caller, 0x400000)]
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000, bytes(image))])), \
                patch.object(h, 'code', side_effect=read):
            with self.assertRaises(h.Rejected):
                h.run_sparse(0x400000, 1, body=body, end=0x400000+len(caller),
                             input_strategy='frame-call-v1')
            case = h.run_sparse(0x400000, 1, body=body, end=0x400000+len(caller),
                               input_strategy='body-call-v1', input_layout=layout)
        self.assertEqual(case['calls'][0]['out']['pop'], 4)
        self.assertEqual(case['regs_out']['esp'], case['regs_in']['esp']+4)
        self.assertEqual(_inputs([case]), dict(input_strategy='body-call-v1', input_layout=layout))


if __name__ == '__main__':
    unittest.main()
