"""Count L109c fusion sites in the private lift cache, without regenerating it."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'research/native'))
import lift


def functions(path):
    """Stream the cache's top-level JSON array to bound shared-machine RAM."""
    decoder = json.JSONDecoder()
    with path.open(encoding='utf-8') as stream:
        buffer, eof = '', False
        while True:
            buffer = buffer.lstrip(' \r\n\t[,')
            if buffer.startswith(']'):
                return
            try:
                value, end = decoder.raw_decode(buffer)
            except json.JSONDecodeError:
                if eof:
                    raise ValueError('Incomplete lift cache')
                chunk = stream.read(1 << 20)
                eof = not chunk
                buffer += chunk
                continue
            yield value
            buffer = buffer[end:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cache', type=Path, nargs='?', default=Path(lift.N) / 'patterns/lift.json')
    args = parser.parse_args()
    sites, functions_hit, geometry = Counter(), 0, 0
    for function in functions(args.cache):
        count = 0
        for arm in function['arms']:
            # Count the legacy template structurally. Actual generation uses
            # exact_compares to recover COMISS vs UCOMISS from original bytes.
            arm = arm.replace('x86::comiss(', 'x86::comiss_hw::<false>(')
            for match in lift.FLOAT_TEST_BRANCH.finditer(lift.compact_alu(arm)):
                sites[(match[3], match[4])] += 1
                count += 1
        functions_hit += bool(count)
        if function['addr'] == 0xFE9580:
            geometry = count
    print(json.dumps(dict(sites=sum(sites.values()), functions=functions_hit,
                          geometry_sites=geometry,
                          predicates={f'{mask}/{cc}': n for (mask, cc), n in sorted(sites.items())}), indent=2))


if __name__ == '__main__':
    main()
