"""Private L140 Adler oracle: original instructions, no game/shard build.

Reads the player's ARC files read-only; writes only under --out (private).
Includes empty/null, arbitrary seeds, 16/5552-byte boundaries and real data.
"""
import argparse
import hashlib
import random
import struct
import zlib
from pathlib import Path
import vectors_inflate as common

core = common.core
core.SCRATCH_LEN = 0x24000
original_oracle = core.Uc


def oracle(*args, **kwargs):
    uc = original_oracle(*args, **kwargs)
    start = uc.emu_start
    def run(*args, **kwargs):
        # 128 KiB unrolled checksum exceeds the shared small-leaf 200k cap.
        # Still bounded, with the original return/call guards unchanged.
        kwargs['count'] = 2_000_000
        return start(*args, **kwargs)
    uc.emu_start = run
    return uc


core.Uc = oracle


def archive_payloads(root, limit=8):
    found = 0
    for path in sorted(root.rglob('*.arc')):
        with path.open('rb') as f:
            header = f.read(8)
            if len(header) != 8 or header[:4] != b'ARC\0':
                continue
            records = f.read(struct.unpack_from('<H', header, 6)[0] * 80)
            for offset in range(0, len(records), 80):
                compressed, size, location = struct.unpack_from('<III', records, offset + 68)
                size &= (1 << 29) - 1
                if not 5552 <= size <= 131072:
                    continue
                f.seek(location)
                data = zlib.decompress(f.read(compressed))
                assert len(data) == size
                yield data, f'{path.name}:{offset // 80}'
                found += 1
                if found == limit:
                    return
    assert found, 'no suitable installed archive payloads'


def case(data, seed, null=False):
    rnd = random.Random(seed)
    scratch = core.Scratch(rnd)
    address = scratch.alloc(len(data) + 1, 1)
    scratch.bytes(address, data)
    regs = {name: rnd.getrandbits(32) for name in core.GPR}
    regs['esp'] = core.ESP
    xmm = [[rnd.getrandbits(32) for _ in range(4)] for _ in range(8)]
    initial = rnd.getrandbits(32)
    result = core.run(0x00cf34d0, regs, xmm, scratch, [initial, 0 if null else address, len(data)], [])
    assert not result['calls']
    assert result['regs_out']['eax'] == (1 if null else zlib.adler32(data, initial))
    result['flags'] = common.final_flags
    return result


def write(path, cases):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('wb') as f:
        f.write(b'L140AD01' + struct.pack('<I', len(cases)))
        for case in cases:
            for side in ('in', 'out'):
                f.write(struct.pack('<8I', *(case['regs_' + side][name] for name in core.GPR)))
                windows = case['mem_' + side]
                f.write(struct.pack('<I', len(windows)))
                for address, text in windows:
                    data = bytes.fromhex(text)
                    f.write(struct.pack('<II', address, len(data)))
                    f.write(data)
            f.write(struct.pack('<I', case['flags']))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, default=Path('E:/ddda-re/notes/native-L140/adler.bin'))
    parser.add_argument('--install', type=Path, default=Path('E:/SteamLibrary/steamapps/common/DDDA/nativePC'))
    args = parser.parse_args()
    cases = []
    for count in (0, 1, 2, 15, 16, 17, 255, 4095, 4096, 5551, 5552, 5553, 11103, 11104, 11105, 65535, 131072):
        for pattern in (0, 1):
            data = bytes([255]) * count if pattern else random.Random(count).randbytes(count)
            cases.append(case(data, len(cases)))
    cases.extend(case(bytes(count), count + 1000, null=True) for count in (0, 1, 5552))
    for data, source in archive_payloads(args.install):
        cases.append(case(data, len(cases)))
        print(f'archive={source} size={len(data)} sha256={hashlib.sha256(data).hexdigest()}', flush=True)
    write(args.out, cases)
    print(f'{len(cases)} original Adler cases -> {args.out}; sha256={hashlib.sha256(args.out.read_bytes()).hexdigest()}')


if __name__ == '__main__':
    main()
