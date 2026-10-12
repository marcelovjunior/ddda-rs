"""Lazy modeled fill effects; serialize ordinary exact byte ranges for replay."""


class BulkFills:
    def __init__(self):
        self.fills = []

    def add(self, address, size, value):
        if size < 0 or address < 0 or address + size > 1 << 32:
            raise ValueError('bulk fill wraps address space')
        if size:
            self.fills.append((address, address + size, value & 255))

    def contains(self, address):
        return any(start <= address < end for start, end, _ in self.fills)

    def overlay(self, address, data):
        data = bytearray(data)
        for start, end, value in self.fills:
            lo, hi = max(address, start), min(address + len(data), end)
            if lo < hi:
                data[lo-address:hi-address] = bytes([value]) * (hi-lo)
        return bytes(data)

    def output(self, ordinary, mapped, read):
        """Coalesce all writes, using live bytes on every materialized page.

        Unmaterialized pages contain only modeled fills. Later ordinary writes
        and overlapping fills already reside in live mapped pages.
        """
        intervals = [(a, b) for a, b, _ in self.fills]
        for address in sorted(ordinary):
            if intervals and intervals[-1][1] == address:
                intervals[-1] = (intervals[-1][0], address+1)
            else:
                intervals.append((address, address+1))
        merged = []
        for start, end in sorted(intervals):
            if merged and start <= merged[-1][1]:
                merged[-1] = (merged[-1][0], max(end, merged[-1][1]))
            else:
                merged.append((start, end))
        output = []
        for start, end in merged:
            data = bytearray(self.overlay(start, bytes(end-start)))
            for page in range(start & ~4095, end, 4096):
                if page in mapped:
                    lo, hi = max(start,page), min(end,page+4096)
                    data[lo-start:hi-start] = read(lo,hi-lo)
            output.append([start, data.hex()])
        return output
