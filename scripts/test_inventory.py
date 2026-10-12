"""Platform provenance survives citations and verified generated declarations."""
import contextlib
import io
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
import inventory as i


class PlatformTests(unittest.TestCase):
    def test_build_preserves_rule_and_verified_precedence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); export = root/'inventory'; export.mkdir()
            native = root/'native'; native.mkdir()
            repo = root/'repo'; (repo/'crates/ddnative/src').mkdir(parents=True)
            (repo/'crates/ddnative/tests').mkdir(parents=True)
            (repo/'crates/ddnative/src/lib.rs').write_text('// Translated from 0x012E0010\n')
            (repo/'crates/ddnative/tests/fixture.rs').write_text('// verifies 0x012E0010\n')
            (export/'functions.tsv').write_text('address\tname\tnamespace\tsize\tthunk\tcallers\tcallees\n'
                '00400000\twrapper\tGlobal\t21\tfalse\t0\t0\n'
                '012E0010\tcrt\tGlobal\t9\tfalse\t0\t0\n')
            (export/'calls.tsv').write_text('caller\tcallee\n')
            (export/'extra_functions.tsv').write_text('address\tsize\tsources\tevidence\tsize_kind\n'
                '0136DDE0\t5\t[]\t[]\tdiscovered\n')
            (native/'platform.json').write_text(json.dumps([0x400000,0x12e0010]))
            (native/'platform_rules.json').write_text(json.dumps({str(0x400000):'construct-if-memory wrapper',str(0x12e0010):'shape'}))
            db = root/'inventory.sqlite'
            with patch.multiple(i,EXPORT=export,EXTRA=export/'extra_functions.tsv',DB=db,REPO=repo), \
                 patch.dict(i.os.environ,{'LIFT_DATA':str(native)}), contextlib.redirect_stdout(io.StringIO()):
                i.build()
            with contextlib.closing(sqlite3.connect(db)) as conn:
                rows = list(conn.execute('SELECT address,status,platform_rule FROM functions ORDER BY address'))
            self.assertEqual(rows,[(0x400000,'platform','construct-if-memory wrapper'),
                                   (0x12e0010,'verified','CRT/library range'),(0x136dde0,'untouched',None)])

    def test_fragments_are_credited_through_verified_owners(self):
        # L49: 0x012E0020 is code of the verified 0x012E0010 (fragments.json);
        # 0x012E0030's owner 0x012E0040 is not verified, so it stays as it is
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); export = root/'inventory'; export.mkdir()
            native = root/'native'; (native/'patterns').mkdir(parents=True)
            repo = root/'repo'; (repo/'crates/ddnative/src').mkdir(parents=True)
            (repo/'crates/ddnative/tests').mkdir(parents=True)
            (repo/'crates/ddnative/src/lib.rs').write_text('// Translated from 0x012E0010\n')
            (repo/'crates/ddnative/tests/fixture.rs').write_text('// verifies 0x012E0010\n')
            (export/'functions.tsv').write_text('address\tname\tnamespace\tsize\tthunk\tcallers\tcallees\n'
                '012E0010\towner\tGlobal\t9\tfalse\t0\t0\n012E0020\tcaseD_1\tGlobal\t4\tfalse\t0\t0\n'
                '012E0030\ttail\tGlobal\t4\tfalse\t1\t0\n012E0040\tother\tGlobal\t9\tfalse\t0\t0\n')
            (export/'calls.tsv').write_text('caller\tcallee\n')
            (export/'extra_functions.tsv').write_text('address\tsize\tsources\tevidence\tsize_kind\n')
            (native/'platform.json').write_text('[]'); (native/'platform_rules.json').write_text('{}')
            (native/'patterns/fragments.json').write_text(json.dumps({
                '0x012E0020': {'owners': ['0x012E0010'], 'evidence': 'no callers'},
                '0x012E0030': {'owners': ['0x012E0040'], 'evidence': 'frame tail'}}))
            db = root/'inventory.sqlite'
            with patch.multiple(i,EXPORT=export,EXTRA=export/'extra_functions.tsv',DB=db,REPO=repo), \
                 patch.dict(i.os.environ,{'LIFT_DATA':str(native)}), contextlib.redirect_stdout(io.StringIO()):
                i.build()
            with contextlib.closing(sqlite3.connect(db)) as conn:
                status = dict(conn.execute('SELECT address,status FROM functions'))
            self.assertEqual(status[0x12e0020], 'verified')
            self.assertNotEqual(status[0x12e0030], 'verified')

    def test_readable_ports_and_tests_are_declarations(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            for d in ('crates/ddnative/src', 'crates/ddnative/tests', 'crates/mtmath/src', 'crates/mtmath/tests',
                      'crates/mtcore/src', 'crates/mtcore/tests'):
                (repo/d).mkdir(parents=True)
            (repo/'crates/mtmath/src/random.rs').write_text('// Translated from 0x00D00690 (DDDA.exe)\n')
            (repo/'crates/mtmath/tests/differential.rs').write_text('// verifies 0x00D00690\n')
            (repo/'crates/mtcore/src/dti.rs').write_text('// Translated from 0x00CF5E20 (DDDA.exe)\n')
            (repo/'crates/mtcore/tests/differential.rs').write_text('// verifies 0x00CF5E20\n')
            self.assertEqual(i.native_declarations(repo), ({0xD00690, 0xCF5E20}, {0xD00690, 0xCF5E20}))

    def test_legacy_metadata_is_explicitly_unknown(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.closing(sqlite3.connect(':memory:')) as db:
            root=Path(tmp);(root/'platform.json').write_text('[4194304]')
            db.executescript("CREATE TABLE functions(address,status,platform_rule); CREATE TABLE supplementary(address); INSERT INTO functions VALUES(4194304,'untouched',NULL);")
            i.classify_platform(db,root)
            self.assertEqual(db.execute('SELECT platform_rule FROM functions').fetchone()[0], 'legacy shape (rule unavailable)')


if __name__ == '__main__': unittest.main()
