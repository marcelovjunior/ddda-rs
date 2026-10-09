"""Recipes on top of the oracle (see docs/ORACLE.md for the evidence).

    python recipes.py player            # uPlayer address, position, cell
    python recipes.py teleport DX DY DZ # move the player by (DX, DY, DZ) cm
    python recipes.py stage             # stage class, floating-origin cell
    python recipes.py time              # in-game clock
    python recipes.py party             # Arisen + pawns: object, name, level, position
    python recipes.py npcs              # NPCs by distance
    python recipes.py talk              # press Converse (E) if an NPC is near (unverified)
    python recipes.py menu              # Esc: opens / closes the Pause Menu
    python recipes.py tap KEY [FRAMES]  # scripted key press (VK name or code)

Units: centimetres (100 = 1 m), y up. Positions are in the floating-origin
frame of the current cell (docs/WORLD.md).
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from client import Oracle, OracleError  # noqa: E402

# ---------------------------------------------------------------- addresses
SGAMESYS_PTR = 0x018FA4BC    # sGameSys* (set in its ctor 0x00437be0)
GAME_TIME = 0xB8768          # sGameSys mGameTime (getter 0x004372a0)
GAME_HOUR = 0xB876C          # sGameSys GameHour (getter 0x00435680)
GAME_MINUTE = 0xB8770        # sGameSys GameMinute (getter 0x004372b0)

SPLAYERMANAGER_PTR = 0x018FA4EC  # sPlayerManager* (found live: the only image pointer to it)
PARTY = 0x99C                # sPlayerManager: 4 x uPlayerBase* [Arisen, pawn, pawn, pawn]
CHR_RECORD = 0x3DEC          # uPlayerBase -> character record inside sGameSys
REC_LEVEL = 0x700            # record: level (u32)
REC_NAME = 0x70C             # record: name (C string)
SWORLDOFFSET_PTR = 0x018FA528
WORLD_ABS_OFFSET = 0x50      # sWorldOffset mWorldAbsoluteOffset (vector3)
SAREA_PTR = 0x018D099C       # sAreaExt*
SAREA_MPAREA = 0x382C        # sArea mpArea[i] (getter 0x00412f50); [2] = current aStageNNN

UCOORD_POS = 0x40            # uCoord mPos (vector3)
UCOORD_QUAT = 0x50           # uCoord mQuat

VK = {
    'esc': 0x1B, 'tab': 0x09, 'enter': 0x0D, 'space': 0x20, 'shift': 0xA0, 'ctrl': 0xA2, 'alt': 0xA4,
    'left': 0x25, 'up': 0x26, 'right': 0x27, 'down': 0x28, 'backspace': 0x08,
    **{chr(c).lower(): c for c in range(0x41, 0x5B)},
    **{str(d): 0x30 + d for d in range(10)},
    **{f'f{i}': 0x6F + i for i in range(1, 13)},
}


def vk(name):
    if isinstance(name, int):
        return name
    n = name.lower()
    return VK[n] if n in VK else int(n, 0)


_cache = {}


def singleton(o, cls):
    """The one live object of an MtDTI class (first scan hit, cached)."""
    a = _cache.get(cls)
    if a and o.u32(a) == o.vtable_of(cls):
        return a
    hits = o.find(cls, limit=8)
    if not hits:
        raise OracleError(f'no live {cls}')
    _cache[cls] = hits[0]
    return hits[0]


def party(o):
    """[(slot, object, class, name, level)] from sPlayerManager's party array."""
    pm = o.u32(SPLAYERMANAGER_PTR)
    by_vt = {int(c['vtable'], 16): n for n, c in o.registry().items() if c.get('vtable')}
    out = []
    for k, a in enumerate(o.u32s(pm + PARTY, 4)):
        if not a:
            continue
        rec = o.u32(a + CHR_RECORD)
        out.append({'slot': k, 'obj': a, 'class': by_vt.get(o.u32(a), '?'),
                    'name': o.cstr(rec + REC_NAME, 32) if rec else None,
                    'level': o.u32(rec + REC_LEVEL) if rec else None,
                    'pos': pos(o, a)})
    return out


def player(o):
    """uPlayer: slot 0 of the party array (no heap scan)."""
    p = o.u32(o.u32(SPLAYERMANAGER_PTR) + PARTY)
    if not p or o.u32(p) != o.vtable_of('uPlayer'):
        p = singleton(o, 'uPlayer')
    return p


def pos(o, obj):
    return o.f32s(obj + UCOORD_POS, 3)


def set_pos(o, obj, x, y, z):
    """Writes uCoord.mPos on the main thread between two frames."""
    return o.write(obj + UCOORD_POS, struct.pack('<3f', x, y, z), main=True)


def teleport(o, dx, dy, dz, settle_frames=10):
    p = player(o)
    x, y, z = pos(o, p)
    set_pos(o, p, x + dx, y + dy, z + dz)
    o.wait(settle_frames)
    return (x, y, z), pos(o, p)


def stage(o):
    """Current stage class and, for stage 100, the floating-origin cell.

    mWorldAbsoluteOffset is minus the origin cell's position in the fixed
    layout frame (origin at cell 50m50n, docs/WORLD.md), so the cell is
    st100_<50 - z/10000>m<50 - x/10000>n. Checked live against the
    uStageSplitMdl at local (0, 0, 0) (st100_63m52n, offset (-20000, 0, -130000)).
    """
    area = o.u32(SAREA_PTR)
    by_vt = {int(c['vtable'], 16): n for n, c in o.registry().items() if c.get('vtable')}
    areas = []
    for i in range(4):
        a = o.u32(area + SAREA_MPAREA + 4 * i)
        if a:
            areas.append({'index': i, 'obj': a, 'class': by_vt.get(o.u32(a), '?')})
    w = o.u32(SWORLDOFFSET_PTR)
    x, y, z = o.f32s(w + WORLD_ABS_OFFSET, 3)
    a_, b_ = 50 - round(z / 10000), 50 - round(x / 10000)
    return {'areas': areas, 'world_abs_offset': (x, y, z), 'origin_cell': f'st100_{a_}m{b_}n'}


def npcs(o, max_dist=None):
    """Live uNpc / uMultiNpc objects sorted by distance to the player.
    Scan hits are filtered by a sane position (stray vtable-valued words
    exist in tables)."""
    import math
    p = pos(o, player(o))
    out = []
    for cls in ('uNpc', 'uMultiNpc'):
        for a in o.find(cls, limit=512):
            q = pos(o, a)
            if all(math.isfinite(v) and abs(v) < 1e6 for v in q) and any(abs(v) > 1 for v in q):
                d = math.dist(p, q)
                if max_dist is None or d <= max_dist:
                    out.append({'dist': d, 'obj': a, 'class': cls, 'pos': q})
    return sorted(out, key=lambda n: n['dist'])


def talk(o, max_dist=300.0):
    """Taps the Converse key (E, PC default) if an NPC is within max_dist cm.
    UNVERIFIED: no NPC was near the player when this was written."""
    near = npcs(o, max_dist)
    if not near:
        return {'talked': False, 'reason': f'no NPC within {max_dist} cm'}
    o.cmd('tap', vk=VK['e'], frames=4)
    o.wait(30)
    return {'talked': True, 'npc': near[0]}


def clock(o):
    g = o.u32(SGAMESYS_PTR)
    return {'sGameSys': g, 'time': o.u32(g + GAME_TIME), 'hour': o.u32(g + GAME_HOUR), 'minute': o.u32(g + GAME_MINUTE)}


def main(argv):
    o = Oracle()
    c = argv[0] if argv else 'player'
    if c == 'player':
        p = player(o)
        print(json.dumps({'uPlayer': hex(p), 'pos': pos(o, p)}))
    elif c == 'teleport':
        d = [float(v) for v in argv[1:4]]
        before, after = teleport(o, *d)
        print(json.dumps({'before': before, 'after': after}))
    elif c == 'stage':
        print(json.dumps(stage(o)))
    elif c == 'time':
        print(json.dumps(clock(o)))
    elif c == 'party':
        for m in party(o):
            m['obj'] = hex(m['obj'])
            print(json.dumps(m))
    elif c == 'npcs':
        for n in npcs(o)[:20]:
            n['obj'] = hex(n['obj'])
            print(json.dumps(n))
    elif c == 'talk':
        print(json.dumps(talk(o), default=str))
    elif c == 'menu':
        print(json.dumps(o.cmd('tap', vk=VK['esc'], frames=4)))
    elif c == 'tap':
        frames = int(argv[2]) if len(argv) > 2 else 3
        print(json.dumps(o.cmd('tap', vk=vk(argv[1]), frames=frames)))
    else:
        print(__doc__)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
