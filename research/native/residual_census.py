"""Exhaustive private census of inventory entries without verification.

Run after rebuilding the private inventory from the replayed generated set.
Includes thunks (coverage summaries traditionally exclude them). Reads every
trial log for evidence; never treats a recorded case as a passed Rust replay.
"""
import argparse
from collections import Counter
from contextlib import closing
import hashlib
import json
import os
import re
from pathlib import Path
import sqlite3

import harness as h
import lift
from structured_inputs import read, records, write

# Evidence, not exceptions to acceptance: NATIVE_PIPELINE.md "call_ext" and
# "Debugging a failing lifted function", plus L09 Results.
QUARANTINE_REASONS = {
    0x00C1D790: 'Only invalid own-return fixtures; return does not reach its caller',
    0x00D9EF20: 'Only invalid own-return fixtures; return does not reach its caller',
    0x00FB9A10: 'Only invalid own-return fixtures; return does not reach its caller',
    0x00F276C0: 'Large-argument FSIN differs: Unicorn 0x181c7560, host CPU 0x181c74e9',
}


def classify(row, selected, candidates, rejections, quarantine, trials, platform):
    a = row['address']
    if a in selected:
        return 'selected-uncredited', 'Selected entry has no inventory verification credit; rebuild after Rust replay', []
    if a in platform:
        return 'platform-model-overlap', f'Explicit harness.PLATFORM model {platform[a]} excluded from instruction lifting', []
    if a in quarantine:
        return 'quarantine', QUARANTINE_REASONS.get(a,'Existing quarantine; not readmitted by a scan'), []
    if str(a) in rejections:
        return 'lift-rejection', rejections[str(a)], []
    if a in candidates:
        evidence = trials.get(a, [])
        if not evidence:
            return 'unrecorded', 'Liftable entry without a fresh trial log', []
        if any(t['case_count'] for t in evidence):
            return 'unadopted', 'Recorded cases require adoption and Rust replay', evidence
        reasons = Counter(attempt['reason'] for t in evidence for attempt in t['attempts'])
        return 'recorder-rejection', '; '.join(f'{r} ({n})' for r,n in sorted(reasons.items())), evidence
    if row['size'] < 1:
        return 'size-filter', 'Inventory size is below the scan minimum of one byte', []
    return 'unaccounted', 'No selection, model, quarantine, rejection or candidate evidence', []


def census(root, require_evidence=False):
    selected = {f['addr'] for f in read(root/'patterns/lift.json')}
    candidates = {f['addr']:lift.vector_key(f) for f in read(root/'patterns/structured_candidates.json')}
    rejections = read(root/'patterns/structured_rejections.json')
    quarantine = set(read(root/'structured_quarantine.json'))
    trials = {}
    for path in sorted(root.glob('structured-*.jsonl')):
        for trial in records(path):
            if trial['body_key'] != candidates.get(trial['addr']):
                continue  # A different body/call boundary is not evidence.
            trials.setdefault(trial['addr'], []).append(dict(
                file=path.name, strategy=trial['strategy'], body_key=trial['body_key'],
                attempts=trial['attempts'], case_count=len(trial['cases'])))
    with closing(sqlite3.connect(f'file:{lift.DB}?mode=ro', uri=True)) as db:
        db.row_factory = sqlite3.Row
        inventory = {r['address']:dict(r) for r in db.execute('SELECT * FROM functions')}
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    md = Cs(CS_ARCH_X86, CS_MODE_32); md.detail = True
    result = []
    for a,row in sorted(inventory.items()):
        if row['status'] in ('platform','verified'):
            continue
        category,reason,evidence = classify(row,selected,candidates,rejections,quarantine,trials,h.PLATFORM)
        entry = dict(row, category=category, reason=reason, trials=evidence)
        if category == 'lift-rejection':
            entry['evidence_source'] = 'patterns/structured_rejections.json'
            try:
                instructions,_,_,_ = lift.cached_body(a,row['size'],True)
                at = re.search(r'at (0x[0-9a-f]+)',reason)
                mnemonic = reason.split()[1] if reason.startswith(('x87-unsupported','platform-dependent')) else reason.split()[0]
                entry['diagnostic_instructions'] = [dict(address=i.address,mnemonic=i.mnemonic,operands=i.op_str)
                    for i in instructions if (at and i.address==int(at[1],16)) or i.mnemonic==mnemonic
                    or (mnemonic=='indirect' and i.mnemonic=='jmp' and i.operands[0].type!=2)
                    or (mnemonic=='segment' and any(o.type==3 and o.mem.segment for o in i.operands))]
            except lift.Unsupported as error:
                entry['decode_reason'] = str(error)
            finally:
                lift.release_body(a,row['size'])
        elif category == 'quarantine':
            entry['evidence_source'] = 'structured_quarantine.json; docs/tasks/L09-structured-inputs.md; docs/NATIVE_PIPELINE.md'
        elif category == 'platform-model-overlap':
            entry['evidence_source'] = 'research/native/harness.py: PLATFORM; crates/ddnative/src/platform.rs'
        try:
            first = next(md.disasm(h.code(a,15),a,count=1))
            entry['entry_instruction'] = dict(address=a,mnemonic=first.mnemonic,operands=first.op_str)
            if first.mnemonic == 'jmp' and first.operands[0].type == 2:
                target = first.operands[0].imm & 0xffffffff
                entry['tail_target'] = dict(address=target, status=inventory.get(target,{}).get('status','not-in-inventory'))
        except (KeyError,StopIteration):
            entry['entry_instruction'] = dict(address=a,reason='no decodable entry bytes')
        result.append(entry)
    summary = dict(total=len(result),nonthunks=sum(not r['thunk'] for r in result),
                   categories=dict(Counter(r['category'] for r in result)),
                   statuses=dict(Counter(r['status'] for r in result)))
    inputs = [Path(lift.DB),root/'patterns/lift.json',root/'vectors/lift.json',
              root/'patterns/structured_candidates.json',root/'patterns/structured_rejections.json',
              root/'structured_quarantine.json']
    hashes = {}
    for path in inputs:
        if path.exists():
            with path.open('rb') as stream:hashes[str(path)]=hashlib.file_digest(stream,'sha256').hexdigest()
    report = dict(inventory=str(lift.DB), input_sha256=hashes, selection_count=len(selected), summary=summary, entries=result)
    write(root/'residual-census.json',report)
    print(json.dumps(summary,indent=2))
    if require_evidence and any(r['category'] in ('unaccounted','unrecorded','unadopted','selected-uncredited') for r in result):
        raise SystemExit('Incomplete census: inspect residual-census.json')
    return report


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-evidence',action='store_true')
    args=parser.parse_args(); root=Path(lift.N)
    if not os.environ.get('LIFT_DATA') or root.resolve()==Path('E:/ddda-re/notes/native').resolve():
        parser.error('set LIFT_DATA to a private directory')
    census(root,args.require_evidence)
