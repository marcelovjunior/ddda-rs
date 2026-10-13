"""Target description for record_generic.py: 0x00B72360 (ESI = player
object, returns AL, plain RET). Callees (all thiscall): 0x00DE5CC0 and
0x00DE5BD0 (construct/destroy a local query object), 0x00DE45C0 (ten stack
arguments, ret 0x28), 0x00B85F70, 0x00DE4710 (four arguments, ret 0x10),
0x00DEA870 (the world query: four arguments, ret 0x10, writes its result
into the caller's frame) and 0x004038F0."""
import struct

ENTRY, LENGTH = 0x00B72360, 0x9C9  # through RET at 0x00B72D28
OBJ = 0x20000000
OBJECTS = [(OBJ, 0x5000)]
STUBS = {
    0x00DE5CC0: (0, False, [], []),
    0x00DE45C0: (10, False, [], []),
    0x00B85F70: (0, False, [], []),
    0x00DE4710: (4, False, [], []),
    # arguments 0 and 2 point to the 16-byte vectors (start, end) the query reads
    0x00DEA870: (4, False, [], [], [(0, 48), (2, 48)]),
    0x004038F0: (0, False, [], []),
    0x00DE5BD0: (0, False, [], []),
}


def rand_float(rng):
    r = rng.random()
    if r < 0.12:
        return 0.0
    if r < 0.2:
        return rng.choice((1.0, -1.0, 0.5, 2.0, 100.0))
    if r < 0.3:
        return rng.uniform(-0.05, 0.05)
    return rng.uniform(-6, 6)


def stub(o, address, rng):
    out = {'eax': rng.choice((0, 1, 1, rng.randrange(1 << 32)))}
    if address == 0x00B85F70:
        out['eax'] = struct.unpack('<I', struct.pack('<f', rng.choice((0.9, 0.8, 0.5, rand_float(rng)))))[0]
    if address == 0x00DEA870:
        # the query writes its result (hit position / normal) into the caller's frame
        frame = ((o.entry_esp - 4) & ~15) - 0x1C8 - 8
        data = b''.join(struct.pack('<f', rand_float(rng)) for _ in range(20))
        out['writes'] = [(frame + 0xC0, data)]
    return out


def fill(o, rng):
    uc = o.uc
    sparsity = rng.choice((0, 0, 0.3, 0.6, 0.85, 0.95))
    for off in range(0, 0x4A40, 4):
        v = 0.0 if rng.random() < sparsity else rand_float(rng)
        uc.mem_write(OBJ + off, struct.pack('<f', v))
    uc.mem_write(OBJ + 0x206D, bytes([rng.choice((0, 2, 4, 8, 6, rng.randrange(256)))]))
    uc.mem_write(OBJ + 0x2F84, bytes([rng.choice((0, 1))]))
    return {6: OBJ}, []
