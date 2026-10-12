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

    def test_mtmath_ports_and_tests_are_declarations(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            for d in ('crates/ddnative/src', 'crates/ddnative/tests', 'crates/mtmath/src', 'crates/mtmath/tests'):
                (repo/d).mkdir(parents=True)
            (repo/'crates/mtmath/src/random.rs').write_text('// Translated from 0x00D00690 (DDDA.exe)\n')
            (repo/'crates/mtmath/tests/differential.rs').write_text('// verifies 0x00D00690\n')
            self.assertEqual(i.native_declarations(repo), ({0xD00690}, {0xD00690}))

    def test_legacy_metadata_is_explicitly_unknown(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.closing(sqlite3.connect(':memory:')) as db:
            root=Path(tmp);(root/'platform.json').write_text('[4194304]')
            db.executescript("CREATE TABLE functions(address,status,platform_rule); CREATE TABLE supplementary(address); INSERT INTO functions VALUES(4194304,'untouched',NULL);")
            i.classify_platform(db,root)
            self.assertEqual(db.execute('SELECT platform_rule FROM functions').fetchone()[0], 'legacy shape (rule unavailable)')


if __name__ == '__main__': unittest.main()
