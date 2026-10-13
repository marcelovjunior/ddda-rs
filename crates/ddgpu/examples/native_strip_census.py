"""Count native strip eligibility in an existing DDFRAME v1 capture.

Read-only, CPU-only. Skip compressed blobs entirely; emit aggregate metadata,
never game payloads. Encoding source: d3d9frame/src/lib.rs Writer::draw.
This checks the declared D3D9 range, not the contents of index buffers.
"""
import argparse
import collections
import json
import os
import struct


def summarize(path):
    counts = collections.Counter()
    with open(path, "rb") as stream:
        end = os.fstat(stream.fileno()).st_size
        if stream.read(12) != b"DDFRAME\0\x01\0\0\0":
            raise ValueError("requires DDFRAME version 1")
        while header := stream.read(5):
            if len(header) != 5:
                counts["truncated_tail_records"] += 1
                break
            tag, size = struct.unpack("<BI", header)
            # A capture can end mid-record when its process is stopped. Report
            # this explicitly and census only the complete command prefix.
            if size > end - stream.tell():
                counts["truncated_tail_records"] += 1
                break
            if tag == 1:
                stream.seek(size, 1)
                continue
            if tag != 2:
                raise ValueError(f"unknown record tag {tag}")
            # Host diagnostic bound; commands contain blob IDs, not payloads.
            if size > 16 * 1024 * 1024:
                raise ValueError("unexpectedly large command metadata")
            payload = stream.read(size)
            if len(payload) != size or not payload:
                raise ValueError("incomplete command metadata")
            if payload[0] == 4:
                counts["presents"] += 1
            if payload[0] != 2:
                continue
            primitive, _, _, _, minimum, vertices = struct.unpack_from("<6I", payload, 1)
            counts["draws"] += 1
            offset = 26
            if payload[25] == 0:  # stream count then five words per stream
                streams = struct.unpack_from("<I", payload, offset)[0]
                offset += 4 + 20 * streams
            elif payload[25] == 1:  # inline blob and stride
                offset += 8
            else:
                raise ValueError("unknown vertex encoding")
            indexed = payload[offset]
            if not indexed or primitive not in (3, 5):
                continue
            if indexed not in (1, 2):
                raise ValueError("unknown index encoding")
            _, index_format = struct.unpack_from("<II", payload, offset + 1)
            restart = {101: 0xFFFF, 102: 0xFFFFFFFF}.get(index_format)
            name = f"{'triangle' if primitive == 5 else 'line'}_strip_format_{index_format}"
            counts[name] += 1
            if restart is None:
                counts[name + "_unknown_format"] += 1
            elif vertices == 0:
                counts[name + "_empty_range"] += 1
            elif minimum + vertices > restart:
                counts[name + "_unproven_range"] += 1
            else:
                counts[name + "_restart_excluded"] += 1
    return dict(sorted(counts.items()))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture")
    print(json.dumps(summarize(parser.parse_args().capture), indent=2))
