"""Select a reproducible L55 sample without changing the shared native data.

The output is a private address/class manifest, never a copy of case data.
Classes describe static lifted bodies; execution is checked by the Rust test.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import re
import sqlite3


def array_items(path):
    """Stream a JSON array using bounded input buffering (stdlib only)."""
    decoder = json.JSONDecoder()
    with Path(path).open(encoding='utf-8') as stream:
        buf, pos, eof = '', 0, False
        while True:
            while pos < len(buf) and buf[pos] in ' \r\n\t[,':
                pos += 1
            if pos < len(buf) and buf[pos] == ']':
                return
            try:
                value, end = decoder.raw_decode(buf, pos)
            except json.JSONDecodeError:
                if eof:
                    raise
                buf = buf[pos:] + stream.read(1024 * 1024)
                pos = 0
                eof = stream.tell() == Path(path).stat().st_size
                continue
            yield value
            pos = end


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root', type=Path, default=Path('E:/ddda-re/notes/native'))
    p.add_argument('--inventory', default='E:/ddda-re/notes/inventory/inventory.sqlite')
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--per-class', type=int, default=200)
    p.add_argument('--random', type=int, default=2000)
    args = p.parse_args()
    repo = Path(__file__).resolve().parents[2]
    args.out = args.out.resolve()
    if args.out.is_relative_to(repo) or args.out.is_relative_to(args.root.resolve()):
        p.error('output must be outside the repo and the input data directory')
    if args.out == Path(args.inventory).resolve():
        p.error('output must not replace the inventory')
    with sqlite3.connect(f'file:{args.inventory}?mode=ro', uri=True) as db:
        verified = {a for a, in db.execute("SELECT address FROM functions WHERE status='verified'")}
    classes = {}
    for f in array_items(args.root/'patterns/lift.json'):
        if f['addr'] not in verified:
            continue
        text = str(f['arms'])
        labels = []
        if 'fs_base' in text:
            labels.append('fs')
        if 'call_ext' in text and 'fl.' in text:
            labels.append('flags_calls')
        if 'st.' in text:
            labels.append('x87')
        if re.search(r'xmm[0-7]\[', text) or 'mxcsr' in text:
            labels.append('sse')
        classes[f['addr']] = labels
    available = set()
    for c in array_items(args.root/'vectors/lift.json'):
        if c['addr'] in classes:
            available.add(c['addr'])
    rng = random.Random(55)
    chosen = {}
    counts = {}
    for label in ('fs', 'flags_calls', 'x87', 'sse', 'random'):
        pool = sorted(a for a in available - chosen.keys()
                      if label == 'random' or label in classes[a])
        n = min(len(pool), args.random if label == 'random' else args.per_class)
        for a in rng.sample(pool, n):
            chosen[a] = label
        counts[label] = dict(available=len(pool), selected=n)
    hashes = {}
    for name in ('patterns/lift.json', 'vectors/lift.json'):
        with (args.root/name).open('rb') as f:
            hashes[name] = hashlib.file_digest(f, 'sha256').hexdigest()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(dict(seed=55, root=str(args.root), hashes=hashes,
        counts=counts, functions=[dict(addr=a, cls=chosen[a], labels=classes[a]) for a in sorted(chosen)]), indent=2))
    print(json.dumps(counts), flush=True)


if __name__ == '__main__':
    main()
