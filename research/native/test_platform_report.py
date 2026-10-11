"""Platform reporting keeps selection, verification and residuals distinct."""
from contextlib import closing, redirect_stdout
import io
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch
import platform_report as p


class ReportTests(unittest.TestCase):
    def test_rule_counts_and_body_key_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'patterns').mkdir()
            with closing(sqlite3.connect(root/'inventory.sqlite')) as db, db:
                db.execute('CREATE TABLE functions(address,name,size,thunk,status,platform_rule)')
                db.executemany('INSERT INTO functions VALUES(?,?,?,?,?,?)',[
                    (1,'selected',1,0,'verified','CRT'),(2,'thunk',1,1,'platform','CRT'),
                    (3,'model',1,0,'translated','MT'),(4,'reject',1,0,'platform','CRT')])
            with closing(sqlite3.connect(root/'inventory-baseline.sqlite')) as db, db:
                db.execute('CREATE TABLE functions(address,status)')
                db.executemany('INSERT INTO functions VALUES(?,?)',[(a,'platform') for a in (1,2,3,4)])
            for name,data in [('lift',[{'addr':1}]),('lift-baseline',[]),
                              ('structured_candidates',[{'addr':1},{'addr':2}]),
                              ('structured_rejections',{'4':'segment override'})]:
                (root/'patterns'/f'{name}.json').write_text(json.dumps(data))
            trial=dict(addr=2,body_key='stale',strategy='zero',attempts=[dict(reason='runaway',seed=1)])
            path=root/'structured-zero.jsonl';path.write_text(json.dumps(trial)+'\n')
            with patch.object(p.lift,'DB',str(root/'inventory.sqlite')),patch.object(p.lift,'vector_key',return_value='current'),patch.object(p.h,'PLATFORM',{3:'allocator'}),redirect_stdout(io.StringIO()):
                with self.assertRaisesRegex(SystemExit,'lack evidence'):p.report(root)
                trial.update(body_key='current', cases=[{}]);path.write_text(json.dumps(trial)+'\n')
                with self.assertRaisesRegex(SystemExit,'unadopted'):p.report(root)
                trial['cases']=[];path.write_text(json.dumps(trial)+'\n')
                report=p.report(root)
                trial['cases']=[{}];path.write_text(json.dumps(trial)+'\n')
                (root/'quarantine_evidence.json').write_text(json.dumps({'2':'observed replay mismatch'}))
                quarantined=p.report(root)
                self.assertEqual(quarantined['rules']['CRT']['replay-mismatch'],1)
            counts=report['rules']['CRT']
            self.assertEqual((counts['total'],counts['thunks'],counts['liftable'],counts['verified']), (3,1,2,1))
            self.assertEqual(counts['verified_before'],0)
            self.assertEqual(counts['selected_before'],0)
            self.assertEqual(counts['added'],1)
            self.assertEqual({r['address']:r['category'] for r in report['entries']},
                             {1:'selected',2:'recorder-rejection',3:'test-model',4:'lift-rejection'})


if __name__=='__main__':unittest.main()
