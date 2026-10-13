"""Offline game-code corpus: rank real profiles and validate real call captures.

Never attaches to or runs the game. All snapshots/generated bodies stay private.
Nearest ownership includes host helpers; inclusive scores overlap and are not
CPU seconds. Text reports are truncated lower bounds; raw stacks take priority.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
WAITS = ('NtWait','ZwWait','NtDelayExecution','NtRemoveIoCompletion',
         'NtUserMsgWaitForMultipleObjects','NtUserGetMessage','NtAlertThreadByThreadId',
         'NtWaitForAlertByThreadId','ZwRemoveIoCompletion','NtSignalAndWaitForSingleObject','NtUserWaitMessage')


def address(symbol):
    match=re.search(r'(?:10|::|^)f_([0-9a-fA-F]{8})(?:_p\d+)?(?:E|::h[0-9a-fA-F]+)?(?:[.+][^ ]*|\s*\+\s*0x[0-9a-fA-F]+)?$',symbol)
    return '0x'+match[1].lower() if match and ('ddlift' in symbol or 'lifted_' in symbol or symbol.startswith('f_')) else None


def game_thread(name):
    return name=='guest main thread' or name in {f'guest thread 0x{x:x}' for x in range(0x1070,0x1085,4)}


def raw_counts(capture):
    counts={k:Counter() for k in ('self','nearest','inclusive')};busy=wait=empty=0
    for thread in capture['threads']:
        if not game_thread(thread['name']):continue
        for stack in thread['stacks']:
            names=[capture['symbols'].get(str(pc),'(unresolved)') for pc in stack]
            if not names:empty+=1;continue
            if any(w in names[0] for w in WAITS):wait+=1;continue
            busy+=1
            owners=[a for name in names if (a:=address(name))]
            if owners:
                counts['nearest'][owners[0]]+=1;counts['inclusive'].update(set(owners))
            if a:=address(names[0]):counts['self'][a]+=1
    return counts,dict(busy=busy,wait=wait,empty=empty,truncated=False)


def text_counts(text):
    counts={k:Counter() for k in ('self','nearest','inclusive')};wanted=False;section=None;busy=wait=empty=0
    for line in text.splitlines():
        header=re.match(r"== thread \d+ '([^']+)':.*busy (\d+)/\d+ samples, wait (\d+), empty (\d+)",line)
        if header:
            wanted=game_thread(header[1]);section=None
            if wanted:busy+=int(header[2]);wait+=int(header[3]);empty+=int(header[4])
            continue
        if not wanted:continue
        if line.startswith('  -- '):
            section={'  -- self (host leaf)':'self','  -- nearest lifted function (self + non-lifted callees)':'nearest',
                     '  -- inclusive lifted functions':'inclusive'}.get(line)
            continue
        row=re.match(r'\s*[\d.]+%\s+(\d+)\s+(.+)',line)
        if section and row:
            a=address(row[2]) if section=='self' else row[2] if re.fullmatch(r'0x[0-9a-fA-F]{8}',row[2]) else None
            if a:counts[section][a.lower()]+=int(row[1])
    return counts,dict(busy=busy,wait=wait,empty=empty,truncated=True)


def rank(paths,limit,metric='self-inclusive'):
    total={k:Counter() for k in ('self','nearest','inclusive')};sources=[]
    for path in paths:
        data=path.read_bytes();counts,meta=raw_counts(json.loads(data)) if path.suffix=='.json' else text_counts(data.decode('utf-8'))
        sources.append(dict(path=str(path),sha256=hashlib.sha256(data).hexdigest(),**meta))
        for kind in total:total[kind].update(counts[kind])
    addresses=set().union(*total.values())
    rows=[dict(entry=a,**{k:total[k][a] for k in total},score=total['nearest'][a] if metric=='nearest' else total['self'][a]+total['inclusive'][a]) for a in addresses]
    rows.sort(key=lambda r:(-r['score'],-r['nearest'],r['entry']))
    if len(rows)<limit:raise ValueError(f'only {len(rows)} ranked functions, requested {limit}')
    return dict(version=1,scope='main + six job threads; recognized OS waits excluded; unknown leaves retained',
                metric=metric+' sample counts (ranking score, not CPU time)',sources=sources,functions=rows[:limit])


def cpu(v):
    if not isinstance(v,list) or len(v)!=84 or any(type(x)is not int or not 0<=x<=0xffffffff for x in v):
        raise ValueError('CPU must contain 84 u32 words')


def materialize(call):
    """Create memory preimages and opaque seams; reject incoherent snapshots.

    This is deliberately stricter than transcript playback: changing reads
    without an observed write cannot be represented by a fixed page snapshot.
    Never manufacture a plausible input for concurrent/out-of-band effects.
    """
    if call.get('overflow') or call.get('panicked'):raise ValueError('incomplete call')
    cpu(call['input']);cpu(call['output'])
    initial={};current={};written=set();seams=[];depth=[];seam=None;reads=set()
    for event in call['events']:
        if not isinstance(event,list) or len(event)<2:raise ValueError('invalid event')
        kind,a,*rest=event
        if type(a)is not int or not 0<=a<=0xffffffff:raise ValueError('invalid event address')
        if kind in ('r','w'):
            if len(rest)!=1 or not isinstance(rest[0],list) or not rest[0] or any(type(b)is not int or not 0<=b<=255 for b in rest[0]):raise ValueError('invalid bytes')
            if a+len(rest[0])>0x100000000:raise ValueError('wrapped access')
            for offset,b in enumerate(rest[0]):
                at=a+offset
                if kind=='r':
                    reads.add(at)
                    if at in current and current[at]!=b:raise ValueError(f'incoherent read at {at:08x}; concurrent or unobserved write')
                    if at not in current:initial[at]=b;current[at]=b
                else:
                    current[at]=b;written.add(at)
            if depth and kind=='w':seam['writes'].append([a,rest[0]])
        elif kind in ('enter','leave'):
            if len(rest)!=1:raise ValueError('invalid call event')
            cpu(rest[0])
            if kind=='enter':
                if not depth:seam=dict(target=a,input=rest[0],memory=sorted(current.items()),writes=[])
                depth.append(a)
            else:
                if not depth or depth.pop()!=a:raise ValueError('unbalanced call')
                if not depth:seam['output']=rest[0];seams.append(seam);seam=None
        elif kind in ('alloc','probe'):
            if not depth:raise ValueError(f'caller {kind} requires explicit runtime contract; not page-only replayable')
        else:raise ValueError(f'unknown event {kind}')
    if depth:raise ValueError('unterminated call')
    if len({a//4096 for a in current})>16384:raise ValueError('call exceeds 64 MiB replay-page budget')
    # Write-only preimages are unknown. Poison is diagnostic padding, never a
    # claim about captured bytes. Guarded replay rejects reads before writes.
    return dict(input=call['input'],output=call['output'],initial=sorted(initial.items()),
                final=sorted((a,current[a]) for a in written),seams=seams,
                read_addresses=sorted(reads),write_addresses=sorted(written))


def private(path):
    if any((p/'.git').exists() for p in (path.resolve(),*path.resolve().parents)):raise ValueError('raw artifacts must remain outside any git worktree')
    path.parent.mkdir(parents=True,exist_ok=True)
    return path


def opaque_calls(body,a):
    # Replay requires real matching enter/leave events for each seam.
    body=re.sub(r'x86::call_local\(m, cpu, (0x[0-9a-fA-F]+), (?:super::lifted::f_[0-9a-f]+|crate::platform::\w+)\)',r'x86::call_ext(m, cpu, \1)',body)
    body=body.replace('x86::call_lifted(','x86::call_ext(')
    if 'super::lifted' in body:raise ValueError(f'unhandled direct call in {a}')
    return body


def extract(root,addresses,ref=None):
    pattern='pub fn f_('+'|'.join(a[2:] for a in addresses)+')'
    command=['git','grep','-l','-E',pattern]+([ref] if ref else [])+['--','crates/ddlift']
    paths=subprocess.check_output(command,cwd=root,text=True).splitlines();bodies={}
    for path in paths:
        text=subprocess.check_output(['git','show',path],cwd=root,text=True) if ref else (root/path).read_text(encoding='utf-8')
        for a in addresses:
            start=text.find(f'pub fn f_{a[2:]}(')
            if start<0:continue
            end=text.find('// Translated from',start);body=text[start:end if end>=0 else len(text)]
            bodies[a]=opaque_calls(body,a)
    if set(bodies)!=set(addresses):raise ValueError(f'missing bodies: {set(addresses)-set(bodies)}')
    return bodies


def prepare(out,addresses,baseline_ref,candidate_root,candidate_bodies=None):
    private(out/'Cargo.toml');out.mkdir(parents=True,exist_ok=True)
    candidate=extract(candidate_root,addresses) if candidate_bodies is None else {
        a:opaque_calls((candidate_bodies/f'f_{a[2:]}.rs').read_text(encoding='utf-8'),a) for a in addresses}
    sources={'baseline':extract(ROOT,addresses,baseline_ref),'candidate':candidate}
    hashes={}
    for name,bodies in sources.items():
        directory=out/name;directory.mkdir(exist_ok=True)
        text='#![allow(warnings)]\npub use ddnative_base::{mem,x86,platform};use mem::Mem;use x86::{Cpu,Flags};use ddnative_base::emit;\n'+'\n'.join(bodies.values())
        text+='\npub fn lookup(a:u32)->Option<x86::LiftFn>{match a{'+','.join(f'{a}=>Some(f_{a[2:]})'for a in addresses)+',_=>None}}\n'
        (directory/'lib.rs').write_text(text,encoding='utf-8')
        (directory/'Cargo.toml').write_text(f'''[package]
name="{name}"
version="0.0.0"
edition="2021"
[lib]
path="lib.rs"
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
''',encoding='utf-8')
        hashes[name]={a:hashlib.sha256(b.encode()).hexdigest() for a,b in bodies.items()}
    (out/'Cargo.toml').write_text(f'''[package]
name="l165-codegen-corpus"
version="0.0.0"
edition="2021"
[workspace]
[dependencies]
baseline={{path="baseline"}}
candidate={{path="candidate"}}
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{ROOT.as_posix()}/crates/ddnative-host"}}
serde={{version="1",features=["derive"]}}
serde_json="1"
[[bin]]
name="l165-codegen-corpus"
path="main.rs"
[profile.release]
opt-level=3
lto=false
[profile.release.package.baseline]
opt-level=1
[profile.release.package.candidate]
opt-level=1
''',encoding='utf-8')
    harness=(ROOT/'crates/mtcore/tests/codegen_corpus.rs').read_text(encoding='utf-8').replace('#![cfg(any())]','')
    (out/'main.rs').write_text(harness,encoding='utf-8')
    (out/'sources.json').write_text(json.dumps(dict(baseline_ref=baseline_ref,candidate_root=str(candidate_root),candidate_bodies=str(candidate_bodies) if candidate_bodies else None,body_sha256=hashes),indent=2),encoding='utf-8')
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L165',CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='4')
    subprocess.run(['cargo','build','--release','--manifest-path',str(out/'Cargo.toml')],env=env,check=True)
    return Path(env['CARGO_TARGET_DIR'])/'release/l165-codegen-corpus.exe'


def capture_cases(directory,addresses):
    cases={a:[] for a in addresses};rejected={a:[] for a in addresses}
    for path in sorted(directory.glob('*.json')):
        sample=json.loads(path.read_text(encoding='utf-8'))
        if 'entry' not in sample:continue
        a=f"0x{int(sample['entry']):08x}"
        if a not in cases:continue
        if sample.get('origin')!='runtime-lift' or sample.get('capture')is not True:
            rejected[a].append(f'{path.name}: not a real runtime capture');continue
        if sample.get('version')!=1 or sample.get('panicked') or sample.get('overflow'):
            rejected[a].append(f'{path.name}: wrong version or incomplete');continue
        for index,call in enumerate(sample['records']):
            try:case=materialize(call)
            except ValueError as error:rejected[a].append(f'{path.name}:{index}: {error}');continue
            case['provenance']=dict(path=str(path),call=index,sha256=hashlib.sha256(path.read_bytes()).hexdigest())
            cases[a].append(case)
    return cases,rejected


def run(args):
    manifest=json.loads(args.rank.read_text(encoding='utf-8'));addresses=[r['entry'] for r in manifest['functions']]
    cases,rejected=capture_cases(args.captures,addresses)
    exe=prepare(args.out,addresses,args.baseline_ref,args.candidate_root,args.candidate_bodies)
    subprocess.run([str(exe),'--self-test'],check=True)
    report=[]
    print('entry        cases  result       baseline_ns candidate_ns ratio   (callee-excluded median)',flush=True)
    for a in addresses:
        row=dict(entry=a,cases=len(cases[a]),rejected=rejected[a]);report.append(row)
        if not cases[a]:
            row['status']='REJECTED' if rejected[a] else 'MISSING';print(f"{a} {0:5}  {row['status']}",flush=True);continue
        corpus=args.out/f'{a}.json';corpus.write_text(json.dumps(dict(entry=int(a,16),cases=cases[a])),encoding='utf-8')
        result=subprocess.run([str(exe),str(corpus),str(args.iterations)],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        (args.out/f'{a}.log').write_text(result.stdout,encoding='utf-8')
        if result.returncode:
            row['status']='FAIL';print(f'{a} {len(cases[a]):5}  FAIL (see private log)',flush=True);continue
        times={}
        for entry,index,round_,port,iters,total,exclusive in re.findall(r'CORPUS (\d+) (\d+) (\d+) (\d+) (\d+) (\d+) (\d+)',result.stdout):
            assert int(entry)==int(a,16)
            times.setdefault((int(index),int(round_)),{})[int(port)]=(int(total)/int(iters),int(exclusive)/int(iters))
        assert len(times)==len(cases[a])*7 and all(set(t)=={0,1} for t in times.values())
        ratios=[t[0][1]/t[1][1]for t in times.values() if t[1][1]>0]
        row.update(status='PASS',baseline_ns=statistics.median(t[0][1]for t in times.values()),candidate_ns=statistics.median(t[1][1]for t in times.values()),median_ratio=statistics.median(ratios),pairs=len(times),timing=times_to_json(times))
        print(f"{a} {len(cases[a]):5}  PASS       {row['baseline_ns']:11.1f} {row['candidate_ns']:12.1f} {row['median_ratio']:5.3f}x",flush=True)
    (args.out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('Both bodies opt1; common base/host opt3. Opaque callees excluded; no FPS claim. Missing inputs are never synthesized.',flush=True)
    return 0 if all(r['status']=='PASS' for r in report) else 2


def times_to_json(times):return [dict(case=i,round=r,baseline=list(v[0]),candidate=list(v[1])) for(i,r),v in sorted(times.items())]

def main():
    ap=argparse.ArgumentParser(description=__doc__);sub=ap.add_subparsers(dest='command',required=True)
    p=sub.add_parser('rank');p.add_argument('profiles',nargs='+',type=Path);p.add_argument('--count',type=int,default=50);p.add_argument('--out',type=Path,required=True)
    p.add_argument('--metric',choices=['self-inclusive','nearest'],default='self-inclusive')
    p=sub.add_parser('run');p.add_argument('--rank',type=Path,required=True);p.add_argument('--captures',type=Path,required=True)
    p.add_argument('--baseline-ref',default='4ae43a7c');p.add_argument('--candidate-root',type=Path,default=ROOT)
    p.add_argument('--candidate-bodies',type=Path,help='private f_XXXXXXXX.rs bodies from a generator experiment, instead of candidate-root')
    p.add_argument('--out',type=Path,default=Path('C:/ddda-targets/L165/codegen-corpus'));p.add_argument('--iterations',type=int,default=100)
    args=ap.parse_args()
    if args.command=='rank':
        result=rank(args.profiles,args.count,args.metric);private(args.out).write_text(json.dumps(result,indent=2),encoding='utf-8')
        args.out.with_suffix('.entries.txt').write_text('\n'.join(r['entry'] for r in result['functions'])+'\n',encoding='utf-8')
        print('rank  entry        self nearest inclusive score')
        for i,r in enumerate(result['functions'],1):print(f"{i:4}  {r['entry']} {r['self']:5} {r['nearest']:7} {r['inclusive']:9} {r['score']:5}")
        print('Sample scores overlap; text-only observations are truncated lower bounds.')
    elif args.command=='run':raise SystemExit(run(args))


if __name__=='__main__':main()
