"""Read-only emitter eligibility probe, NOT execution or verification.

Consumes measure.py's private report and the existing private selection caches.
Calls lift(..., ext=True) with all callees external; this deliberately does not
claim the result has the production closed-callee x87 contract. No scan, gen,
vector recording, inventory rebuild, or Ghidra mutation is performed.
"""
import argparse
import json
import os
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement', type=Path, required=True)
    parser.add_argument('--native', type=Path, default=Path('E:/ddda-re/notes/native'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.out.resolve()
    if output.is_relative_to(root) or output.is_relative_to(args.native.resolve()):
        parser.error('output must be outside the repository and input native directory')
    os.environ['LIFT_DATA'] = str(output.parent)
    sys.path.insert(0, str(root/'research/native'))
    import lift
    data = json.loads(args.measurement.read_text())
    def addresses(file):
        return {x['addr'] if isinstance(x,dict) else x for x in json.loads(file.read_text())}
    have = addresses(args.native/'patterns/lift.json')
    novector = addresses(args.native/'patterns/lift_novector.json')
    mismatch = addresses(args.native/'patterns/lift_mismatch.json')
    roots = set(data['roots']['0x4079e0']['missing_entries']) | {
        0x41dd60,0x41d9c0,0x41eaf0,0x4980f0,0x4fb3e0,0x72a5f0,
        0x41fe50,0xdb9f40,0xdb1480,0xdce0e0,0xdce600,0xcfe6b0,
        0x41ffe0,0xdac090,0xdba9a0,0xdbdef0,0xdbdf10,0x11a45a0,
        0x11a45b0,0xdbfa90,0xdb6740,0x419910,
    }
    result = []
    for address in sorted(roots):
        row = data['rows'].get(str(address))
        if not row or row['status']=='platform': continue
        selection = ('verified lifted' if address in have else
                     'no usable vector (cache)' if address in novector else
                     'mismatch cache' if address in mismatch else 'not selected')
        try:
            lift.lift(address,row['size'],ext=True)
            probe = 'emits with all calls external'
        except lift.Unsupported as error:
            probe = str(error)
        finally:
            lift.release_body(address,row['size'])
        result.append(dict(addr=address,**row,selection=selection,probe=probe))
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(result,indent=2))
    for row in result:
        print(f"{row['addr']:#010x}: {row['selection']}; {row['probe']}")


if __name__ == '__main__': main()
