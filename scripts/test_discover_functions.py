"""Synthetic evidence/merge regressions; no game files or Ghidra required."""
import contextlib
import io
import json
from pathlib import Path
import sqlite3
import struct
import tempfile
import unittest
from unittest.mock import patch

import discover_functions as d
import inventory


class FakeImage(d.Image):
    def __init__(self):
        self.data = bytearray(0x10000)
        self.sections = [dict(name='.text', va=0x1000, virtual_size=0x1000, raw_size=0x1000, raw=0x1000, flags=0x60000020),
                         dict(name='.rdata', va=0x3000, virtual_size=0x1000, raw_size=0x1000, raw=0x3000, flags=0x40000040),
                         dict(name='.data', va=0x5000, virtual_size=0x3000, raw_size=0x1000, raw=0x5000, flags=0xC0000040)]
    def offset(self, a):
        if not self.section(a):
            raise ValueError(a)
        return a
    def put(self, a, data): self.data[a:a+len(data)] = data
    def word_at(self, a, value): self.put(a, struct.pack('<I', value))


class DiscoveryTests(unittest.TestCase):
    def test_entry_needs_control_transfer_and_executable_section(self):
        p = FakeImage(); m = d.decoder()
        p.put(0x1000, b'\x31\xc0\xc3'); p.put(0x3000, b'\xc3')
        self.assertTrue(d.code_entry(p, m, 0x1000))
        self.assertFalse(d.code_entry(p, m, 0x3000))
        p.put(0x1000, b'\xcc')
        self.assertFalse(d.code_entry(p, m, 0x1000))
        # L49: a case table (three nearby code addresses) is not an entry,
        # although its bytes decode to instructions ending in a transfer
        p.put(0x1100, struct.pack('<III', 0x1180, 0x1190, 0x11a0) + b'\xc3')
        self.assertFalse(d.code_entry(p, m, 0x1100))

    def test_address_taken_entry_needs_a_function_boundary(self):
        # L45: immediates naming code mid-instruction are constants, not entries
        p = FakeImage(); m = d.decoder()
        cases = {
            0x1100: (b'\x90\x90\x31\xc0\xcc\xcc\xcc\xcc', True),   # int3 padding
            0x1200: (b'\x90\x90\x90\x90\x90\x31\xc0\xc3', True),   # after ret
            0x1300: (struct.pack('<II', 0x12F0, 0x12E8), True),     # after a jump table
            0x1400: (bytes((0, 1, 1, 1, 1, 1, 0, 0)), True),       # after a byte index table
            0x1500: (b'\x90\x90\x90\x90\xb0\x0b\x83\xc0', False),  # inside `add eax, 4`
            0x1600: (b'\x61\x6f\x72\x00\x65\x64\x70\x8b', False),  # ASCII data then code
        }
        for a, (before, want) in cases.items():
            p.put(a - 8, before); p.put(a, b'\x04\x50\xc3')
            self.assertEqual(d.function_boundary(p, m, a), want, hex(a))

    def test_mt_registration_bss_and_table_anchor(self):
        p = FakeImage(); m = d.decoder()
        # One-argument registration form exists as well as six-argument form.
        p.put(0x1100, b'\x68'+struct.pack('<I', 0x3500)+b'\xb9'+struct.pack('<I', 0x7000)+
              b'\xe8'+struct.pack('<i', 0xCF5780-0x110F)+b'\xc3')
        p.put(0x3500, b'Example\0')
        p.put(0x1200, b'\xb8'+struct.pack('<I', 0x7000)+b'\xc3')
        for a in (0x1300, 0x1310, 0x1320, 0x1330, 0x1340): p.put(a, b'\xc3')
        for k,a in enumerate((0x1300,0x1310,0x1320,0x1330,0x1200,0x1340)):
            p.word_at(0x3100+4*k, a)
        # Arbitrary pointer run has no RTTI evidence and is not admitted.
        for k in range(8): p.word_at(0x3300+4*k, 0x1300)
        self.assertEqual(list(d.vtables(p,m,{0x1100})), [])
        r = list(d.vtables(p,m,{0x1100},stores={0x3100: {0x1400}}))
        self.assertEqual([a for a,_ in r], [0x1300,0x1310,0x1320,0x1330,0x1200,0x1340])
        self.assertEqual(r[-1][1]['registration'], 0x110A)
        self.assertEqual(r[-1][1]['descriptor'], 0x7000)

    def test_rttiless_table_needs_store_and_known_slot(self):
        # L22: e.g. a COM interface table stored by its constructor.
        p = FakeImage(); m = d.decoder()
        for a in (0x1300, 0x1310, 0x1320): p.put(a, bytes([0xC3]))  # ret
        for k, a in enumerate((0x1300, 0x1310, 0x1320, 0x5000)):
            p.word_at(0x3100+4*k, a)
        self.assertEqual(list(d.vtables(p, m, set(), known={0x1300})), [])          # no store
        self.assertEqual(list(d.vtables(p, m, set(), stores={0x3100: {0x1400}})), [])  # no known slot
        r = list(d.vtables(p, m, set(), known={0x1310}, stores={0x3100: {0x1400}}))
        self.assertEqual([a for a, _ in r], [0x1300, 0x1310, 0x1320])               # cut at data
        # L45: a slot naming code mid-instruction (text after the table) ends it
        p.put(0x1328, b'\x90\x90\x90\x90\xb0\x0b\x83\xc0\x04\x50\xc3')
        p.word_at(0x3100+4*3, 0x1330)
        r = list(d.vtables(p, m, set(), known={0x1310}, stores={0x3100: {0x1400}}))
        self.assertEqual([a for a, _ in r], [0x1300, 0x1310, 0x1320])
        p.word_at(0x3100+4*3, 0x5000)
        self.assertEqual(r[0][1]['rtti'], 'none')
        self.assertEqual(r[0][1]['stores'], [0x1400])
        # A secondary vptr store (object + 4) anchors as well; the MT rule's
        # `stores` stay offset 0.
        r = list(d.vtables(p, m, set(), known={0x1310}, stores={}, vptr_stores={0x3100: {0x1404}}))
        self.assertEqual([a for a, _ in r], [0x1300, 0x1310, 0x1320])
        # A store into .data is not a read-only table.
        p.word_at(0x5100, 0x1300)
        self.assertEqual(list(d.vtables(p, m, set(), known={0x1300}, stores={0x5100: {0x1400}})), [])

    def test_l54_callback_table_and_runtime_evidence(self):
        # known 0x1100; 0x1200 and 0x1300 after int3 padding; a callback table
        # [0x1100, 0x1200, 0, 0x1300] at 0x3100 (a NULL slot is an unused callback)
        p = FakeImage(); m = d.decoder()
        p.put(0x1100, b'\xc3')
        for a in (0x1200, 0x1300, 0x1400):
            p.put(a - 8, b'\xcc' * 8); p.put(a, b'\x31\xc0\xc3')
        for k, v in enumerate((0x1100, 0x1200, 0, 0x1300)):
            p.word_at(0x3100 + 4 * k, v)
        p.word_at(0x3110, 0x7777)  # data ends the run
        found = lambda: {v: ev['table'] for v, ev in d.callback_tables(p, m, {0x1100})}
        self.assertEqual(found(), {})  # nothing refers to the table
        p.word_at(0x5000, 0x3100)
        self.assertEqual(found(), {0x1200: 0x3100, 0x1300: 0x3100})
        # a member that is no function boundary: the run is not a table
        p.put(0x12F8, b'\x90\x90\x90\x90\xb0\x0b\x83\xc0'); p.put(0x1300, b'\x04\x50\xc3')
        self.assertEqual(found(), {})
        # a table in writable data counts too, and says so
        p.word_at(0x5100, 0x1100); p.word_at(0x5104, 0x1400); p.word_at(0x3200, 0x5100)
        self.assertEqual([(v, ev.get('writable')) for v, ev in d.callback_tables(p, m, {0x1100})], [(0x1400, True)])
        self.assertTrue(d.text_like(0x00706873))  # "shp\0"
        self.assertFalse(d.text_like(0x01325580))
        # runtime evidence: an indirect-call target the emulator saw
        with patch.object(d, 'initializers', return_value=[]):
            rows, _ = d.discover(p, {0x1100}, [], runtime=[(0x1400, dict(source='runtime', caller=0x1100,
                                                                             instruction=0x1100))])
        self.assertEqual([(r['address'], r['sources']) for r in rows], [('0x00001400', 'callback_table,runtime')])

    def test_l58_record_table_with_a_stride(self):
        # {size, getter, handler} records at 0x3300 (0x01441CF0's shape):
        # [8, 0x1200, 0x1100], [4, 0x1300, 0x1100], then data
        p = FakeImage(); m = d.decoder()
        p.put(0x1100, b'\xc3')
        for a in (0x1200, 0x1300):
            p.put(a - 8, b'\xcc' * 8); p.put(a, b'\x31\xc0\xc3')
        for k, v in enumerate((8, 0x1200, 0x1100, 4, 0x1300, 0x1100, 0x77777)):
            p.word_at(0x3300 + 4 * k, v)
        found = lambda: {v: (ev['table'], ev['stride'], ev['records']) for v, ev in d.record_tables(p, m, {0x1100})}
        self.assertEqual(found(), {})  # nothing refers to the records
        p.word_at(0x5000, 0x3300)
        self.assertEqual(found(), {0x1200: (0x3300, 12, 2), 0x1300: (0x3300, 12, 2)})
        # without an integer field it is a plain pointer run (callback_tables)
        p.word_at(0x3300, 0); p.word_at(0x330C, 0)
        self.assertEqual(found(), {})

    def test_msvc_requires_matching_hierarchy(self):
        p = FakeImage();m=d.decoder();p.put(0x1200,b'\xc3')
        for a,v in [(0x3100,0x3200),(0x3104,0x1200),(0x320C,0x3500),(0x3210,0x3300),
                    (0x3308,1),(0x330C,0x3400),(0x3400,0x3600),(0x3600,0x3500)]: p.word_at(a,v)
        p.put(0x3508,b'.?AVExample@@\0')
        r=list(d.vtables(p,m,set()));self.assertEqual([a for a,_ in r],[0x1200])
        p.word_at(0x3600,0x3504)
        self.assertEqual(list(d.vtables(p,m,set())),[])

    def test_initializer_image_guard(self):
        p=FakeImage()
        with self.assertRaises((KeyError,ValueError)):
            list(d.initializers(p,d.decoder()))

    def test_call_evidence_and_deterministic_sizes(self):
        p=FakeImage();p.put(0x1100,b'\xe8'+struct.pack('<i',0x1200-0x1105)+b'\xc3')
        p.put(0x1200,b'\x31\xc0\xc3')
        with patch.object(d,'initializers',return_value=[]):
            rows,rejected=d.discover(p,{0x1100},[dict(addr=0x1100,body=[0x1100,0x1105])])
        self.assertEqual(rejected,[]);self.assertEqual(len(rows),1)
        self.assertEqual(rows[0]['size'],3)
        self.assertEqual(json.loads(rows[0]['evidence']),[dict(source='call',caller=0x1100,instruction=0x1100)])
        p.put(0x1100,b'\xe8\0\0\0\0\x58\xc3')
        with patch.object(d,'initializers',return_value=[]):
            rows,_=d.discover(p,{0x1100},[dict(addr=0x1100,body=[0x1100,0x1105,0x1106])])
        self.assertEqual(rows,[])

    def test_incremental_scan_keeps_old_boundary_and_releases_bodies(self):
        import lift
        with tempfile.TemporaryDirectory() as td:
            root=Path(td);(root/'patterns').mkdir();dbpath=root/'inventory.sqlite'
            old=[dict(addr=0x1000,size=1,arms=['return;'],calls=[],body=[0x1000])]
            (root/'patterns/lift.json').write_text(json.dumps(old))
            with contextlib.closing(sqlite3.connect(dbpath)) as db:
                db.executescript('CREATE TABLE supplementary(address INTEGER); CREATE TABLE functions(address INTEGER,name TEXT,size INTEGER); INSERT INTO supplementary VALUES(8192); INSERT INTO functions VALUES(8192,"new",1);')
                db.commit()
            with patch.multiple(lift,N=str(root),DB=str(dbpath),X87_NET={},BODY_INFO={0x2000:dict(body=[0x2000])}), \
                 patch.object(lift,'lift',return_value=['return;']) as translate, \
                 patch.object(lift,'release_body') as release, contextlib.redirect_stdout(io.StringIO()):
                lift.scan_extra()
            after=json.loads((root/'patterns/lift.json').read_text())
            self.assertEqual(after[0],old[0]);self.assertTrue(after[1]['ext'])
            translate.assert_called_once_with(0x2000,1,{0x1000},ext=True)
            self.assertEqual(release.call_count,2)

    def test_supplement_merge_keeps_export_and_high_address_candidates(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td); export=root/'export';export.mkdir()
            (export/'functions.tsv').write_text('address\tname\tnamespace\tsize\tthunk\tcallers\tcallees\n0x00401000\tOriginal\tGame\t3\tfalse\t0\t0\n0x012E1000\tCRT\tGlobal\t5\tfalse\t0\t0\n')
            (export/'calls.tsv').write_text('caller\tcallee\n')
            extra=root/'extra.tsv'
            extra.write_text('address\tsize\tsize_kind\tsources\tevidence\n0x00401000\t1\thint\tcall\t[]\n'
                             '0x0136DDF0\t37\treachable\tcpp_initializer,call\t'
                             '[{"source":"call","caller":4198400},{"source":"call","caller":4198400}]\n')
            dbpath=root/'private.sqlite'
            with patch.multiple(inventory,EXPORT=export,EXTRA=extra,DB=dbpath,REPO=root), \
                 patch.object(inventory,'native_declarations',return_value=(set(),set())), contextlib.redirect_stdout(io.StringIO()):
                inventory.build();inventory.build()
            with contextlib.closing(sqlite3.connect(dbpath)) as db:
                self.assertEqual(db.execute('select name,size from functions where address=0x401000').fetchone(),('Original',3))
                self.assertEqual(db.execute('select status from functions where address=0x136DDF0').fetchone(),('untouched',))
                self.assertEqual(db.execute('select status from functions where address=0x12E1000').fetchone(),('platform',))
                self.assertEqual(db.execute('select count(*) from supplementary').fetchone(),(2,))
                self.assertEqual(db.execute('select caller,callee from calls').fetchall(),[(0x401000,0x136DDF0)])


if __name__ == '__main__': unittest.main()
