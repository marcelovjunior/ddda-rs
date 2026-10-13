"""Offline wait/wake ranking from sample_profile --save-stacks JSON.

Host PCs alone do not contain wait handles or guest registers. Reports preserve
nearest lifted host return PCs so a matching binary can establish guest sites.
No attachment, game run, symbol downloads, or source modification occurs.
"""
import argparse
import collections
import hashlib
import json
from pathlib import Path
from sample_profile import WAITS, lifted_address


def state(leaf):
    if any(name in leaf for name in ('NtAlertThreadByThreadId', 'ZwAlertThreadByThreadId')):
        return 'wake'
    if any(name in leaf for name in WAITS):
        return 'wait'
    if '<unresolved' in leaf or leaf.startswith('?'):
        return 'unknown'
    return 'nonwait'


def mechanism(names, thread):
    def has(text):
        return any(text in name for name in names)
    # The nearest application frame wins: a guest-lock check nested inside
    # wait_event is not itself the event's condition-variable wait.
    for name in names:
        if 'ddruntime::threads::GuestLock::' in name or 'ddruntime::threads::holds_lock' in name:
            return 'guest execution lock / ownership check'
        if 'ddruntime::threads::wait_event::' in name:
            return 'guest event condition variable'
        if 'ddruntime::threads::wait::' in name:
            return 'guest multiple-object wait'
        if 'ddruntime::threads::sleep_for' in name:
            return 'guest sleep'
        if 'ddruntime::threads::contend::' in name:
            return 'guest critical-section contention'
        if 'ddruntime::threads::block_on::' in name:
            return 'runtime block_on condition variable'
        if 'ddruntime::threads::signal_key' in name:
            return 'guest notification / wait-cell lock'
        if 'ddruntime::d3d9dev::D3dCell::' in name:
            return 'D3D object lock'
        if 'ddruntime::d3d9draw::graphics' in name:
            return 'graphics mutex during query GetData' if has('query_get_data') else 'graphics mutex'
    if has('ddgpu::WgpuBackend::wait_finisher'):
        index = next(i for i, name in enumerate(names) if 'ddgpu::WgpuBackend::wait_finisher' in name)
        caller = next((name for name in names[index + 1:] if name.startswith('ddgpu::')), 'unknown caller')
        return 'backend waiting for finisher completion: ' + caller
    if has('ddgpu::encode::Pool::finish'):
        return 'finisher waiting for encode results'
    if has('d3d9cmd::deferred::DeferredBackend::command'):
        if has('::recv<'):
            return 'producer waiting for Present retirement'
        return 'backend command queue send'
    if thread == 'd3d9-backend' and has('::recv<'):
        return 'backend command queue receive'
    if thread == 'wgpu-finish' and (has('::recv<') or has('::next<')):
        return 'finisher batch queue receive'
    if thread.startswith('wgpu-encode-') and (has('::recv<') or has('::next<')):
        return 'encode work queue receive'
    return next((name for name in names if name.startswith(('ddruntime::', 'ddgpu::', 'd3d9cmd::', 'ddplay::'))), 'unattributed')


def summarize(capture):
    symbols = capture['symbols']
    result = []
    for thread in capture['threads']:
        classes = collections.Counter()
        groups = collections.Counter()
        for pcs in thread['stacks']:
            if not pcs:
                classes['empty'] += 1
                continue
            names = [symbols.get(str(pc), f'?0x{pc:x}') for pc in pcs]
            category = state(names[0])
            classes[category] += 1
            if category not in ('wait', 'wake'):
                continue
            guest = next(((lifted_address(name), f'0x{pc:x}') for pc, name in zip(pcs, names)
                          if lifted_address(name)), (None, None))
            groups[(category, mechanism(names, thread['name']), names[0], *guest)] += 1
        samples = len(thread['stacks'])
        cpu = thread.get('cpu')
        result.append({'tid': thread['tid'], 'name': thread['name'], 'samples': samples,
                       'cpu_seconds': cpu,
                       'cpu_percent': 100 * cpu / capture['wall'] if cpu is not None and capture['wall'] else None,
                       'classes': dict(classes),
                       'groups': [dict(zip(('state', 'mechanism', 'leaf', 'guest_function', 'guest_host_pc'), key),
                                       samples=count, percent=100 * count / samples if samples else 0,
                                       wait_handle=None, guest_return=None)
                                  for key, count in groups.most_common()]})
    return sorted(result, key=lambda row: -(row['cpu_seconds'] or 0))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    parser.add_argument('--json', type=Path)
    parser.add_argument('--threads', default='', help='comma-separated exact descriptions; default all')
    parser.add_argument('--top', type=int, default=8)
    args = parser.parse_args()
    if args.top < 1:
        parser.error('--top must be positive')
    raw = args.capture.read_bytes()
    capture = json.loads(raw)
    capture_hash = hashlib.sha256(raw).hexdigest()
    rows = summarize(capture)
    selected = {name.strip() for name in args.threads.split(',') if name.strip()}
    rows = [row for row in rows if not selected or row['name'] in selected]
    if args.json:
        args.json.write_text(json.dumps({'capture_pid': capture.get('pid'), 'capture_sha256': capture_hash, 'wall': capture['wall'], 'threads': rows}, indent=2))
    print(f'capture pid={capture.get("pid")} SHA256={capture_hash}')
    print('PC-only samples: handles and guest return addresses are NOT captured. Host PCs are retained for matching-binary analysis.')
    print('Wait/wake counts are sampled states, not durations. CPU percent uses independent thread CPU-time deltas.')
    for row in rows:
        cpu = '?' if row['cpu_percent'] is None else f"{row['cpu_percent']:.1f}%"
        print(f"\n{row['name']!r} tid={row['tid']} samples={row['samples']} CPU={cpu} classes={row['classes']}")
        for group in row['groups'][:args.top]:
            print(f"  {group['samples']:5} {group['percent']:5.1f}% {group['state']}: {group['mechanism']}; "
                  f"guest_fn={group['guest_function']} host_pc={group['guest_host_pc']} leaf={group['leaf']}")


if __name__ == '__main__':
    main()
