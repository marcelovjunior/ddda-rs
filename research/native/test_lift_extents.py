"""Synthetic recursive bodies and recorder ownership; no game bytes."""
import struct
import unittest
from unittest.mock import patch

import harness as h
import lift


class ExtentTests(unittest.TestCase):
    def fixture(self, blocks):
        def read(a, n):
            for base, data in blocks.items():
                if base <= a < base + len(data):
                    return data[a-base:a-base+n]
            raise KeyError(a)
        def executable(a, n=1):
            return any(base <= a and a+n <= base+len(data) for base, data in blocks.items())
        return read, executable

    def decode(self, blocks, addr=0x400000, size=1, entries=frozenset(), noreturn=frozenset()):
        read, executable = self.fixture(blocks)
        return lift.decode_body(addr, size, entries, noreturn, read, executable)

    def test_disjoint_body_ignores_hint_and_embedded_data(self):
        blocks = {0x400000: bytes.fromhex('eb06') + b'\x0f'*6 + bytes.fromhex('40c3')}
        for size in (1, 2, 9, 1000):
            ins, _, _, ranges = self.decode(blocks, size=size)
            self.assertEqual([i.address for i in ins], [0x400000, 0x400008, 0x400009])
            self.assertEqual(ranges, [[0x400000, 0x400002], [0x400008, 0x40000a]])
        read, executable = self.fixture(blocks)
        with patch.object(h, 'code', read), patch.object(h, 'executable', executable):
            self.assertIn('eax =', ' '.join(lift.lift(0x400000, 1)))

    def test_tail_entry_and_shared_interior(self):
        # Both entries jump backwards into the same epilogue interior.
        blocks = {0x400000: b'\x90\xc3',
                  0x400010: b'\xe9'+struct.pack('<i',0x400001-0x400015),
                  0x400020: b'\xe9'+struct.pack('<i',0x400001-0x400025)}
        entries = frozenset(blocks)
        for addr in (0x400010, 0x400020):
            ins, _, _, _ = self.decode(blocks, addr, entries=entries)
            self.assertEqual({i.address for i in ins}, {addr, 0x400001})
        blocks[0x400020] = b'\xe9'+struct.pack('<i',0x400010-0x400025)
        ins, _, _, _ = self.decode(blocks, 0x400020, entries=entries)
        self.assertEqual([i.address for i in ins], [0x400020])

    def test_overlap_bad_decode_and_executable_guard(self):
        # jz enters the middle of the following mov immediate.
        for data in (bytes.fromhex('7401b801000000c3'), b'\x0f', bytes.fromhex('eb7f')):
            with self.assertRaises(lift.Unsupported):
                self.decode({0x400000: data})

    def test_noreturn_and_bounded_remote_switch(self):
        ins, _, stops, _ = self.decode({0x400000: bytes.fromhex('e8fb0000000f')}, noreturn={0x400100})
        self.assertEqual(len(ins), 1)
        self.assertEqual(stops, {0x400000})
        # cmp eax,0; ja default; jmp [eax*4+402000]; default: ret
        blocks = {0x400000: bytes.fromhex('83f8007707ff248500204000c3'),
                  0x401010: b'\xc3', 0x402000: struct.pack('<II',0x401010,0)}
        ins, switches, _, _ = self.decode(blocks, entries={0x400000,0x401000})
        self.assertEqual(switches[0x400005], [0x401010])
        self.assertIn(0x401010, {i.address for i in ins})

    def record(self, code, body, **kw):
        data = code.ljust(0x1000, b'\x90')
        with patch.object(h, 'image', return_value=(0x400000, [(0x400000,data)])):
            return h.run_sparse(0x400000, 17, body=body, zero_inputs=True, **kw)

    def test_recorder_return_beyond_hint_and_shared_callee(self):
        # call the shared ret; jmp to that ret as our own final epilogue.
        code = bytes.fromhex('e80b000000eb09') + b'\xcc'*9 + b'\xc3'
        body = [0x400000,0x400005,0x400010]
        c = self.record(code, body, size=1)
        self.assertEqual(c['regs_out']['esp'], c['regs_in']['esp']+4)
        # Shared nested ret must not be mistaken for the entry's own return.
        c = self.record(code, body, end=0x400001, lifted={0x400010})
        self.assertEqual(c['calls'], [])
        # A closed return followed immediately by an external call must
        # restore root ownership before the external recorder's hook.
        code = bytes.fromhex('e80b000000e8f6000000eb04') + b'\xcc'*4 + b'\xc3'
        c = self.record(code, [0x400000,0x400005,0x40000a,0x400010],
                        end=0x400001, lifted={0x400010})
        self.assertEqual([x['target'] for x in c['calls']], [0x400100])
        # Jump over a gap then corrupt the return slot beyond the old hint.
        code = bytes.fromhex('eb06')+b'\xcc'*6+bytes.fromhex('c7042400000000c3')
        with self.assertRaisesRegex(h.Rejected, 'instead of the caller'):
            self.record(code, [0x400000,0x400008,0x40000f], size=1)

    def test_recorder_ext_call_from_remote_island_and_noreturn(self):
        code = bytes.fromhex('eb06')+b'\xcc'*6+bytes.fromhex('e8f3000000c3')
        c = self.record(code, [0x400000,0x400008,0x40000d], end=0x400002)
        self.assertEqual([x['target'] for x in c['calls']], [0x400100])
        with self.assertRaisesRegex(h.Rejected, 'noreturn call'):
            self.record(code, [0x400000,0x400008], end=0x400002, noreturn_sites={0x400008})

    def test_vector_boundary_key(self):
        f = {'body':[1,3], 'calls':[], 'x87_push':[], 'noreturn_sites':[]}
        self.assertEqual(lift.vector_key(f), lift.vector_key(dict(f)))
        for change in ({'body':[1,2,3]}, {'calls':[4]}, {'ext':True}, {'x87_push':[2]}, {'noreturn_sites':[3]}):
            self.assertNotEqual(lift.vector_key(f), lift.vector_key({**f, **change}))


if __name__ == '__main__':
    unittest.main()
