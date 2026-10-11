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
