"""Bounded PE/PDB attribution helpers. No process execution or third-party modules.

Public/export symbols are points, not function ranges. In particular a CRT
export may precede unrelated private functions. Never extend its name across
that gap. Read x64 unwind ranges and export jump thunks from the actual image;
optionally fetch the exact CodeView-identified PDB from Microsoft's server.
"""
from bisect import bisect_right
import mmap
from pathlib import Path
import struct
import urllib.request
import uuid


class PEImage:
    def __init__(self, path):
        self.path = Path(path)
        with self.path.open('rb') as stream:
            with mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
                self._read(data)

    def _read(self, data):
        pe = struct.unpack_from('<I', data, 0x3c)[0]
        if data[:2] != b'MZ' or data[pe:pe + 4] != b'PE\0\0':
            raise ValueError('not a PE image')
        machine, count, self.timestamp = struct.unpack_from('<HHI', data, pe + 4)
        optional = pe + 24
        if machine != 0x8664 or struct.unpack_from('<H', data, optional)[0] != 0x20b:
            raise ValueError('requires an AMD64 PE32+ image')
        self.size = struct.unpack_from('<I', data, optional + 56)[0]
        section_table = optional + struct.unpack_from('<H', data, pe + 20)[0]
        self.sections = []
        self.executable = []
        for index in range(count):
            offset = section_table + 40 * index
            virtual_size, rva, raw_size, raw = struct.unpack_from('<IIII', data, offset + 8)
            self.sections.append((rva, virtual_size, raw, raw_size))
            if struct.unpack_from('<I', data, offset + 36)[0] & 0x20000000:
                self.executable.append((rva, rva + raw_size))

        def read(rva, size):
            for start, _, raw, raw_size in self.sections:
                if start <= rva and rva + size <= start + raw_size:
                    offset = raw + rva - start
                    result = data[offset:offset + size]
                    if len(result) == size:
                        return result
            raise ValueError(f'unbacked RVA {rva:#x}+{size:#x}')

        def string(rva):
            result = bytearray()
            for offset in range(4096):
                ch = read(rva + offset, 1)
                if ch == b'\0':
                    return result.decode('utf-8', errors='replace')
                result.extend(ch)
            raise ValueError('unterminated PE string')

        directories = struct.unpack_from('<I', data, optional + 108)[0]

        def directory(index):
            return struct.unpack_from('<II', data, optional + 112 + 8 * index) if index < directories else (0, 0)

        self.ranges = []
        rva, size = directory(3)  # IMAGE_DIRECTORY_ENTRY_EXCEPTION
        for offset in range(0, size - 11, 12):
            start, end, _ = struct.unpack('<III', read(rva + offset, 12))
            if start < end:
                self.ranges.append((start, end))
        self.ranges.sort()
        self.starts = [start for start, _ in self.ranges]
        self.exports = {}
        self.thunks = {}
        self.tiny_ranges = {}
        rva, size = directory(0)
        if rva and size:
            fields = struct.unpack('<IIHHIIIIIII', read(rva, 40))
            functions, names, ordinals = fields[8:11]
            for index in range(fields[7]):
                name = string(struct.unpack('<I', read(names + index * 4, 4))[0])
                ordinal = struct.unpack('<H', read(ordinals + index * 2, 2))[0]
                target = struct.unpack('<I', read(functions + ordinal * 4, 4))[0]
                if not target or rva <= target < rva + size:  # forwarded export
                    continue
                if not any(start <= target and target + 5 <= end for start, end in self.executable):
                    continue  # exported data/BSS is not executable code
                self.exports.setdefault(target, []).append(name)
                code = read(target, 5)
                if code[0] == 0xc3:  # exact one-byte RET, not the following helper
                    self.tiny_ranges[target] = target + 1
                elif code[0] == 0xe9:
                    destination = target + 5 + struct.unpack_from('<i', code, 1)[0]
                    self.thunks.setdefault(destination, []).append(name)
                    self.tiny_ranges[target] = target + 5

        self.pdb = None
        rva, size = directory(6)  # IMAGE_DIRECTORY_ENTRY_DEBUG
        for offset in range(0, size - 27, 28):
            fields = struct.unpack('<IIHHIIII', read(rva + offset, 28))
            if fields[4] != 2 or fields[5] < 25:
                continue
            cv = read(fields[6], fields[5])
            if cv[:4] == b'RSDS':
                name = cv[24:].split(b'\0')[0].decode('utf-8').replace('\\', '/').split('/')[-1]
                guid = uuid.UUID(bytes_le=cv[4:20]).hex.upper()
                age = struct.unpack_from('<I', cv, 20)[0]
                self.pdb = (name, guid + format(age, 'X'))

    def function_range(self, rva):
        index = bisect_right(self.starts, rva) - 1
        if index >= 0 and rva < self.ranges[index][1]:
            return self.ranges[index]
        return None

    def bounded_name(self, rva, symbol):
        """Return (label, evidence); no guessed range between adjacent exports."""
        span = self.function_range(rva)
        start = symbol.get('rva', -1)
        # DbgHelp manufactures Size for export-only symbols as the distance
        # to the next export (SYMFLAG_EXPORT=0x200). It is NOT a PDB extent.
        size = 0 if symbol.get('flags', 0) & 0x200 else symbol.get('size', 0)
        name = symbol.get('name', '')
        # PDB function sizes are stronger evidence than unwind ranges (which
        # can cover only a funclet). Do not accept out-of-range nearest names.
        if name and size and start <= rva < start + size:
            return name, 'pdb-size'
        if name and not size:
            if start in self.tiny_ranges and start <= rva < self.tiny_ranges[start]:
                return name, 'export-instruction'
            if span and span[0] <= start <= rva < span[1]:
                return name, 'unwind-range'
            if rva == start:
                return name, 'exact-address'
        if span:
            aliases = self.exports.get(span[0], []) + self.thunks.get(span[0], [])
            if aliases:
                return '/'.join(sorted(set(aliases))), 'export-thunk/unwind-range'
            return f'<unresolved function RVA {span[0]:#x}>', 'unwind-only'
        return f'<unresolved RVA {rva:#x}>', 'unresolved'


def fetch_pdb(image, cache):
    """Fetch only the GUID/age-matched public PDB, never execute the image."""
    if image.pdb is None:
        raise ValueError(f'{image.path}: no RSDS identity')
    name, key = image.pdb
    destination = Path(cache) / name / key / name
    if not destination.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        # A failed/interrupted transfer must not become a cached PDB.
        temporary = destination.with_suffix('.download')
        try:
            with urllib.request.urlopen(f'https://msdl.microsoft.com/download/symbols/{name}/{key}/{name}', timeout=30) as response:
                with temporary.open('wb') as output:
                    while chunk := response.read(1024 * 1024):
                        output.write(chunk)
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)
    return destination
