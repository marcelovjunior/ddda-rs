"""Rank final callback solo tails from validated job_wake.py per-batch JSON.

Reads offline private analysis only. Candidate virtual targets do not prove
dispatch; duration is inclusive wall time, never self CPU time. Every row
identifies its batch and join interval; only solo overlap is aggregated.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path


def rank(data, labels=None):
    if any(data['rejected'].values()):
        raise ValueError('selected analysis has rejected batches')
    if not data.get('source_sha256') or len(data['source_sha256']) != 64:
        raise ValueError('missing trace provenance')
    first, last = data['min_frame'], data['max_frame']
    if last is None or last < first:
        raise ValueError('bounded frame interval required')
    frames, seen, rows = set(), set(), []
    for batch in data['batches']:
        ident, frame = batch['batch'], batch['frame']
        if ident in seen or not first <= frame <= last:
            raise ValueError('duplicate batch or frame outside interval')
        seen.add(ident)
        frames.add(frame)
        job = batch['last_job']
        for ordinal, join in enumerate(batch['join_waits']):
            if job is None or job['panicked'] or join['result'] != 0:
                raise ValueError('invalid joined callback')
            duration = job['end'] - job['start']
            solo = batch['last_job_solo_tail_ns']
            if not 0 <= solo <= duration or not join['start'] <= join['end']:
                raise ValueError('invalid callback/join interval')
            overlap = max(0, min(join['end'], job['end']) -
                          max(join['start'], job['end'] - solo))
            if overlap != join['last_job_overlap_ns']:
                raise ValueError('solo overlap disagrees with interval')
            rows.append(dict(frame=frame, batch=ident, join_ordinal=ordinal,
                             queued=batch['queued'], participants=batch['participants'],
                             callback=f"{job['target']:08x}",
                             candidate_virtual=f"{job['resolved']:08x}",
                             vtable=f"{job['vtable']:08x}",
                             class_name=(labels or {}).get(job['vtable']),
                             thread=job['tid'], callback_start_ns=job['start'],
                             callback_end_ns=job['end'], duration_ns=duration,
                             join_start_ns=join['start'], join_end_ns=join['end'],
                             solo_overlap_ns=overlap))
    if frames != set(range(first,last+1)):
        raise ValueError('missing selected frames')
    groups = defaultdict(list)
    for row in rows:
        groups[(row['callback'],row['candidate_virtual'],row['vtable'])].append(row)
    families = []
    for key, group in groups.items():
        solo = sum(row['solo_overlap_ns'] for row in group)
        families.append(dict(callback=key[0],candidate_virtual=key[1],vtable=key[2],
                             class_name=group[0]['class_name'],last_count=len(group),
                             positive_solo_count=sum(row['solo_overlap_ns']>0 for row in group),
                             solo_overlap_ns=solo,solo_ms_per_frame=solo/len(frames)/1e6,
                             mean_final_duration_ns=sum(row['duration_ns'] for row in group)/len(group)))
    return dict(trace_sha256=data['source_sha256'],first=first,last=last,frames=len(frames),
                families=sorted(families,key=lambda r:-r['solo_overlap_ns']),
                joins=sorted(rows,key=lambda r:-r['solo_overlap_ns']))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('analysis',type=Path)
    parser.add_argument('--dti',type=Path)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    raw=args.analysis.read_bytes()
    labels={}
    if args.dti:
        for entry in json.loads(args.dti.read_text())['classes']:
            if entry.get('vtable'): labels[int(entry['vtable'],16)]=entry['name']
    result=rank(json.loads(raw),labels)
    result['analysis_sha256']=hashlib.sha256(raw).hexdigest()
    args.out.write_text(json.dumps(result,indent=2))
    for family in result['families'][:12]: print(json.dumps(family))


if __name__=='__main__': main()
