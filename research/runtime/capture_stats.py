"""Counts in a frame-oracle capture (docs/FRAME_ORACLE.md, "File format"):
commands by kind per frame (a frame ends at a Present), and the bytes
uploaded per resource. L21: how far a boot got (did it present?) and
what its draws cost.

    python capture_stats.py FILE [TOP]

Reads only the record headers, the command kind and the upload commands'
first words; blobs are skipped, not inflated (their raw length is in their
header). The file holds game data: keep it out of the repository.
"""
import collections
import struct
import sys

KINDS = ['upload image', 'upload buffer', 'draw', 'clear', 'present', 'copy', 'fill', 'read back',
         'query begin', 'query end', 'query result', 'gamma', 'release']


def main():
    path = sys.argv[1]
    top = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    blob_len = []
    frames = [collections.Counter()]
    uploads = [collections.Counter()]  # per frame: resource -> raw bytes
    counts = [collections.Counter()]  # per frame: resource -> uploads
    with open(path, 'rb') as f:
        if f.read(8) != b'DDFRAME\0':
            sys.exit('not a capture')
        (version,) = struct.unpack('<I', f.read(4))
        while True:
            head = f.read(5)
            if len(head) < 5:
                break
            tag, n = head[0], struct.unpack_from('<I', head, 1)[0]
            payload = f.read(n)
            if len(payload) < n:
                break  # cut short: the stream ends at the last complete record
            if tag == 1:
                _, raw = struct.unpack_from('<II', payload)
                blob_len.append(raw)
            elif tag == 2:
                kind = payload[0]
                frames[-1][KINDS[kind] if kind < len(KINDS) else f'kind {kind}'] += 1
                if kind == 0:
                    v = struct.unpack_from('<9I', payload, 1)
                    uploads[-1][('image', v[0])] += blob_len[v[8]]
                    counts[-1][('image', v[0])] += 1
                elif kind == 1:
                    res, blob = struct.unpack_from('<II', payload, 1)
                    uploads[-1][('buffer', res)] += blob_len[blob]
                    counts[-1][('buffer', res)] += 1
                elif kind == 4:
                    frames.append(collections.Counter())
                    uploads.append(collections.Counter())
                    counts.append(collections.Counter())
    presents = len(frames) - 1
    print(f'version {version}; {presents} Presents; {len(blob_len)} blobs, {sum(blob_len) / 2**20:.1f} MiB raw')
    for k, (c, u) in enumerate(zip(frames, uploads)):
        if k >= top and k != len(frames) - 1:
            continue
        label = f'frame {k}' if k < presents else 'after the last Present' if presents else 'no Present yet'
        print(f'{label}: {dict(c)}; uploaded {sum(u.values()) / 2**20:.1f} MiB')
        for key, b in u.most_common(5):
            print(f'    {key[0]} {key[1]:#x}: {counts[k][key]} uploads, {b / 2**20:.2f} MiB')


if __name__ == '__main__':
    main()
