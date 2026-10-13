"""L172: original FED3B0 relocation oracle; private synthetic memory fixtures.

Runs the installed exe's merger and memcpy in Unicorn, with no mocked calls.
Does not run the game. Invoke through the shared heavy.py recording slot.
"""
import hashlib
import json
from pathlib import Path
import struct
import harness as h

OUT = Path("E:/ddda-re/notes/native-L172/lanes")


def record():
    OUT.mkdir(parents=True, exist_ok=True)
    cases = []
    for lane_count in range(4):
        for nested in (False, True):
            data = bytearray(0xF0000)

            def words(offset, *values):
                struct.pack_into("<" + "I" * len(values), data, offset, *values)

            parent, lanes, dest = 0, 0x40000, 0xE0000
            words(parent + 0x44, h.SCRATCH + dest, 0, 512)
            roots = []
            for lane in range(lane_count):
                context = lanes + lane * 0x33C84
                root = 0xE4000 + lane * 0x100
                record = 0xE8000 + lane * 0x100
                roots.append([h.SCRATCH + root, 2])
                words(context + 0x44, h.SCRATCH + root, 2)
                words(root, 7 + lane, h.SCRATCH + record, 11 + lane, h.SCRATCH + record + 32)
                words(record + 32, 0x70000, 1, 0x600000 + lane * 4)
                if nested:
                    child = root + 32
                    words(record, 0x60000, lane + 100, h.SCRATCH + child, 1)
                    words(child, 17 + lane, h.SCRATCH + record + 64)
                    words(record + 64, 0x70000, 0, 0x600000 + lane * 4)
                    words(context + 0x30878, h.SCRATCH + record)
                    words(context + 0x30C78, 1)
                else:
                    words(record, 0x70000, 0, 0x600000 + lane * 4)
            result = h.run(0x00FED3B0, ecx=h.SCRATCH + parent,
                           args=(h.SCRATCH + lanes, lane_count), scratch=bytes(data),
                           allow=(0x012EFEA0,), max_insn=100000)
            assert result["esp_delta"] == 8
            after = result["scratch"]
            assert struct.unpack_from("<I", after, 0x48)[0] == lane_count * 2
            stem = f"lanes-{lane_count}-nested-{int(nested)}"
            paths = []
            hashes = []
            for suffix, contents in (("before", data), ("after", after)):
                path = OUT / f"{stem}.{suffix}.bin"
                path.write_bytes(contents)
                paths.append(path.name)
                hashes.append(hashlib.sha256(contents).hexdigest())
            cases.append(dict(before=paths[0], after=paths[1], sha256=hashes,
                              roots=roots, context=h.SCRATCH + parent,
                              insns=result["insns"]))
    manifest = dict(function="0x00FED3B0", base=h.SCRATCH,
                    exe_sha256=hashlib.sha256(Path(h.EXE).read_bytes()).hexdigest(), cases=cases)
    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"{len(cases)} original FED3B0 returns: {OUT}/manifest.json")


if __name__ == "__main__":
    record()
