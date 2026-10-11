"""Census completeness: thunks, platform overlap and missing trial evidence."""
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
import residual_census as c
import structured_inputs as s


class CensusTests(unittest.TestCase):
    def test_prepare_includes_platform_and_thunks_but_not_verified_hand_ports(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'patterns').mkdir();(root/'patterns/lift.json').write_text('[]')
            db=root/'inventory.sqlite'
            with closing(sqlite3.connect(db)) as d, d:
                d.execute('CREATE TABLE functions(address INTEGER, namespace TEXT, name TEXT, size INTEGER, thunk INTEGER, status TEXT)')
                d.executemany('INSERT INTO functions VALUES(?,?,?,?,?,?)',[
                    (1,'','hand-port',5,0,'verified'),(2,'','model',5,0,'platform'),
                    (3,'','thunk',5,1,'untouched'),(0xDE0B80,'','explicit-model',5,0,'platform')])
            with patch.object(s.lift,'DB',str(db)),patch.object(s.lift,'seed_closed_contracts',return_value=set()),patch.object(s.lift,'lift',return_value=[]),patch.object(s.lift,'BODY_INFO',{2:dict(body=[2]),3:dict(body=[3]),0xDE0B80:dict(body=[0xDE0B80])}):
                s.prepare(root)
            self.assertEqual([r['addr'] for r in s.read(root/'patterns/structured_candidates.json')],[2,3,0xDE0B80])
            self.assertTrue(s.read(root/'patterns/structured_candidates.json')[-1]['original_entry'])
            # A future full scan must retain thunks too. Unlike the missing-
            # entry scan, it deliberately includes verified call dependencies.
            with patch.object(s.lift,'DB',str(db)),patch.object(s.lift,'N',str(root)),patch.object(s.lift,'lift',return_value=[]):
                s.lift.scan()
            self.assertEqual([r['addr'] for r in s.read(root/'patterns/lift.json')],[1,2,3,0xDE0B80])

    def test_all_unverified_rows_including_thunks_and_stale_trials(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'patterns').mkdir()
            db=root/'inventory.sqlite'
            with closing(sqlite3.connect(db)) as d, d:
                d.execute('CREATE TABLE functions(address INTEGER, name TEXT, size INTEGER, thunk INTEGER, status TEXT)')
                d.executemany('INSERT INTO functions VALUES(?,?,?,?,?)',[
                    (1,'verified',1,0,'verified'),(2,'platform',1,0,'platform'),
                    (3,'thunk',5,1,'untouched'),(4,'unsupported',1,0,'documented'),
                    (5,'model',1,0,'translated'),(6,'quarantine',1,0,'untouched'),
                    (7,'missing',1,0,'untouched')])
            for name,value in [('lift',[]),('structured_candidates',[{'addr':3}]),('structured_rejections',{'4':'unsupported test'})]:
                (root/'patterns'/f'{name}.json').write_text(json.dumps(value))
            (root/'structured_quarantine.json').write_text('[6]')
            trial=dict(addr=3,strategy='zero',body_key='old',attempts=[dict(reason='ok',seed=1)],cases=[{}])
            path=root/'structured-zero.jsonl';path.write_text(json.dumps(trial)+'\n')
            with patch.object(c.lift,'DB',str(db)),patch.object(c.lift,'vector_key',return_value='current'),patch.object(c.h,'PLATFORM',{5:'test-model'}),patch.object(c.h,'code',return_value=b'\xc3'),patch.object(c.lift,'cached_body',side_effect=c.lift.Unsupported('synthetic rejection')):
                report=c.census(root)
                self.assertEqual(report['summary']['total'],5)
                self.assertEqual(report['summary']['nonthunks'],4)
                cats={r['address']:r['category'] for r in report['entries']}
                self.assertEqual(cats,{3:'unrecorded',4:'lift-rejection',5:'platform-model-overlap',6:'quarantine',7:'unaccounted'})
                with self.assertRaises(SystemExit):c.census(root,True)
                with closing(sqlite3.connect(db)) as d, d:d.execute('DELETE FROM functions WHERE address=7')
                trial.update(body_key='current',attempts=[dict(reason='runaway',seed=1)],cases=[])
                path.write_text(json.dumps(trial)+'\n')
                report=c.census(root,True)
                thunk=next(r for r in report['entries'] if r['address']==3)
                self.assertEqual(thunk['category'],'recorder-rejection')
                self.assertEqual(thunk['trials'][0]['attempts'][0]['seed'],1)
                self.assertEqual(thunk['entry_instruction']['mnemonic'],'ret')


if __name__=='__main__':unittest.main()
