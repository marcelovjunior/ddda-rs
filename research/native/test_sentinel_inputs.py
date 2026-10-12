"""Generated sentinel records and finite trace proofs against synthetic code."""
import struct
import unittest
from unittest.mock import patch

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
from sentinel_inputs import SentinelInputs
from structured_inputs import _inputs


class SentinelInputTests(unittest.TestCase):
    LAYOUT = dict(stride=72, first={'register': 'edx'}, sentinel_offset=0,
                  sentinel=0xffffffff)

    def test_records_cross_pages_and_terminate_at_exact_count(self):
        for marker in (0, 0xffffffff, 19):
            for seed, count in enumerate(SentinelInputs.COUNTS, 1):
                layout = dict(self.LAYOUT, sentinel=marker)
                inputs = SentinelInputs(layout, seed)
                self.assertEqual(inputs.count, count)
                for index in range(count + 1):
                    address = inputs.first + index * layout['stride']
                    page = inputs.page(seed, address & ~4095)
                    value, = struct.unpack_from('<I', page, address & 4095)
                    self.assertEqual(value == marker, index == count)
                self.assertEqual(inputs.data, SentinelInputs(layout, seed).data)
                self.assertEqual(inputs.page(seed, 0x20000000), bytes(4096))

    def test_invalid_layouts_refused(self):
        for change in ({'stride': 0}, {'stride': 4097}, {'stride': True},
                       {'sentinel_offset': 72}, {'sentinel_offset': 1},
                       {'sentinel': -1}, {'sentinel': 1 << 32},
                       {'first': {'stack': 0}}, {'first': {'register': 'esp'}},
                       {'first': {}}, {'extra': 0}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                SentinelInputs(dict(self.LAYOUT, **change), 1)

    def test_finite_trace_uses_generated_entry_state_and_replays_layout(self):
        for binding in ({'register': 'edx'}, {'stack': 4}):
            prefix = bytes.fromhex('8b542404') if 'stack' in binding else b''
            code = prefix + bytes.fromhex('31c9 8b02 83f8ff 7406 41 83c248 ebf3 c3')
            md = Cs(CS_ARCH_X86, CS_MODE_32)
            body = [i.address for i in md.disasm(code, 0x400000)]
            page = code + bytes(4096-len(code))
            def read(address, size):
                return page[address-0x400000:address-0x400000+size]
            layout = dict(self.LAYOUT, first=binding)
            with patch.object(h, 'image', return_value=(0x400000, [(0x400000, page)])), \
                    patch.object(h, 'code', side_effect=read):
                for seed, count in enumerate(SentinelInputs.COUNTS, 1):
                    case = h.run_sparse(0x400000, seed, input_strategy='sentinel-records-v1',
                        input_layout=layout, max_insn=3, body=body, end=0x400000+len(code))
                    self.assertEqual(case['regs_out']['ecx'], count)
                    self.assertEqual(case['regs_out']['esp'], case['regs_in']['esp']+4)
                    self.assertEqual(case['image_budget']['instructions'], 6*count+5+bool(prefix))
                    self.assertEqual(_inputs([case]), dict(input_strategy='sentinel-records-v1',
                                                         input_layout=layout))


if __name__ == '__main__':
    unittest.main()
