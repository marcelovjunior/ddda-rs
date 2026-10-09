"""Lit reference captures (claude-3 m1-lighting #711, head #714): read-only.

    python refs_lit.py once OUT_BASE          # OUT_BASE.png + OUT_BASE.json
    python refs_lit.py series DIR [--every 300] [--count N]

Each capture holds:
- the frame (PNG);
- the clock (sWeatherManager) and the fog manager bytes;
- every uCameraGame, sCamera and sRender raw;
- the player position;
- raw dumps of every light and fog unit instance;
- the Gamma and HDREmphasis getters of sRender.

Safety for the series: before each capture, enemies (every MtDTI class
deriving from uEnemy) within 40 m of the player, or a drop in the Arisen's
HP, stop the series. Nothing is written to the game; no input.
"""
import json
import math
import os
import struct
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from client import Oracle, OracleError  # noqa: E402
import recipes  # noqa: E402

LIGHTS = ['uSkyInfiniteLight', 'uSkyHemiSphereLight', 'uInfiniteLight', 'uHemiSphereLight', 'uAmbientLight',
          'uSkyInfiniteEffectLight', 'uSkyHemiSphereEffectLight']
FOGS = ['uSkyColorFog', 'uColorFog', 'uFog', 'uLightScatteringFog']
UCAMERAGAME_VT = 0x0159AE00


def descendants(o, root):
    reg = o.registry()
    by_dti = {c['dti']: n for n, c in reg.items()}
    out = []
    for n, c in reg.items():
        p, seen = c.get('parent'), 0
        while p and seen < 40:
            pn = by_dti.get(p)
            if pn == root:
                out.append(n)
                break
            p = reg[pn].get('parent') if pn else None
            seen += 1
    return out


def objects(o, classes, vt_set=None):
    """Plausible live objects per class: scan hits whose neighbouring words
    are not themselves vtables (tables of vtables are not objects)."""
    vts = {}
    for c in classes:
        v = o.registry().get(c, {}).get('vtable')
        if v:
            vts[int(v, 16)] = c
    if not vts:
        return {}
    allvt = vt_set or {int(c['vtable'], 16) for c in o.registry().values() if c.get('vtable')}
    hits = o.scan(list(vts), limit=256)
    out = {}
    for vt, addrs in hits.items():
        keep = []
        for a in addrs:
            try:
                around = o.u32s(a - 4, 3)
            except OracleError:
                continue
            if around[0] in allvt or around[2] in allvt:
                continue
            keep.append(a)
        out[vts[vt]] = keep
    return out


def enemies_near(o, radius=4000.0):
    p = recipes.pos(o, recipes.player(o))
    classes = descendants(o, 'uEnemy') + ['uEnemy']
    near = []
    for cls, addrs in objects(o, classes).items():
        for a in addrs:
            q = o.f32s(a + 0x40, 3)
            if all(math.isfinite(v) and abs(v) < 1e6 for v in q) and any(abs(v) > 1 for v in q):
                d = math.dist(p, q)
                if d <= radius:
                    near.append({'class': cls, 'obj': hex(a), 'pos': q, 'dist': round(d)})
    return near


ONE = 0x3F800000
EIGHTH = 0x3E000000


def cb_ambient(o, cap=50):
    """CPU copies of the shader constant buffer CBAmbient (claude-3 #792):
    hits h of 0.125 with f32 1.0 at h-0x10 and h-0x0c (else at h-0x10
    only); 0x90 bytes from h-0x1c each. Neighbours are checked from page
    reads, not one read per hit."""
    hits = o.scan([EIGHTH], limit=400000)[EIGHTH]
    pages = {}
    for h in hits:
        pages.setdefault(h & ~0xFFF, []).append(h)
    strict, loose = [], []
    for page, hs in pages.items():
        try:
            b = o.read(page - 0x20, 0x1040)
        except OracleError:
            continue
        for h in hs:
            i = h - (page - 0x20)
            w10 = struct.unpack_from('<I', b, i - 0x10)[0]
            w0c = struct.unpack_from('<I', b, i - 0x0C)[0]
            if w10 == ONE and w0c == ONE:
                strict.append(h)
            elif w10 == ONE:
                loose.append(h)
    chosen = strict if strict else loose
    out = []
    for h in chosen[:cap]:
        try:
            out.append({'addr': hex(h - 0x1C), 'hex': o.read(h - 0x1C, 0x90).hex()})
        except OracleError as e:
            out.append({'addr': hex(h - 0x1C), 'error': str(e)})
    return {'hits_0.125': len(hits), 'strict': len(strict), 'loose': len(loose),
            'mode': 'strict' if strict else 'loose', 'copies': out}


def snapshot(o, base):
    t0 = time.time()
    rep = {'time': time.strftime('%Y-%m-%d %H:%M:%S')}
    cap = o.capture(base + '.png')
    rep['capture'] = cap
    wm = o.u32(recipes.SWEATHERMANAGER_PTR)
    rep['sWeatherManager'] = hex(wm)
    rep['clock'] = {'+0x4c ms': o.u32(wm + 0x4c), '+0x50 h': o.u32(wm + 0x50), '+0x54 m': o.u32(wm + 0x54),
                    '+0x58 s': o.u32(wm + 0x58), '+0x268': o.u32(wm + 0x268), '+0x26c': o.u32(wm + 0x26c),
                    '+0x270 f32': o.f32(wm + 0x270)}
    rep['sWeatherManager+0x900..0xa00'] = o.read(wm + 0x900, 0x100).hex()
    cams = []
    for a in o.scan([UCAMERAGAME_VT], limit=64)[UCAMERAGAME_VT]:
        try:
            fov = o.f32(a + 0x3C)
        except OracleError:
            continue
        if 1.0 <= fov <= 179.0:
            cams.append({'obj': hex(a), 'raw_0x0_0x300': o.read(a, 0x300).hex()})
    rep['uCameraGame'] = cams
    sc = o.u32(0x018D0B6C)
    rep['sCamera'] = {'obj': hex(sc), 'raw_0x0_0xd00': o.read(sc, 0xD00).hex()}
    # viewport 0's camera (claude-3 #796): *(sCamera+0x34)
    vc = o.u32(sc + 0x34)
    by_vt = {int(c['vtable'], 16): n for n, c in o.registry().items() if c.get('vtable')}
    try:
        vt = o.u32(vc)
        rep['viewport0_camera'] = {'obj': hex(vc), 'vtable': hex(vt), 'class': by_vt.get(vt, '?'),
                                   'raw_0x0_0x300': o.read(vc, 0x300).hex()}
    except OracleError as e:
        rep['viewport0_camera'] = {'obj': hex(vc), 'error': str(e)}
    sr = o.u32(recipes.SRENDER_PTR)
    rep['sRender'] = {'obj': hex(sr), 'raw_0x0_0x140': o.read(sr, 0x140).hex()}
    rep['uPlayer_pos'] = recipes.pos(o, recipes.player(o))
    rep['stage'] = recipes.stage(o)
    allvt = {int(c['vtable'], 16) for c in o.registry().values() if c.get('vtable')}
    for group, classes, size in (('lights', LIGHTS, 0x350), ('fogs', FOGS, 0x2F0)):
        g = {}
        for cls, addrs in objects(o, classes, allvt).items():
            items = []
            for a in addrs:
                try:
                    items.append({'obj': hex(a), 'vtable': hex(o.u32(a)), 'raw': o.read(a, size).hex()})
                except OracleError as e:
                    items.append({'obj': hex(a), 'error': str(e)})
            g[cls] = items
        rep[group] = g
    rep['CBAmbient'] = cb_ambient(o)
    rep['getters'] = {
        'Gamma 0x00da69b0': o.call(0x00DA69B0, this=sr, float=True)['st0'],
        'HDREmphasis 0x00da6a00': o.call(0x00DA6A00, this=sr, float=True)['st0'],
    }
    rep['elapsed_s'] = round(time.time() - t0, 2)
    with open(base + '.json', 'w') as f:
        json.dump(rep, f, indent=1, default=str)
    return rep


# Enemy classes that are passive animals (head #760: uEm8300 = oxen). Other
# e8000-e8602 animals are counted as threats until their ids are checked
# (giant bats and rats do attack).
PASSIVE = {'uEm8300'}
HP_PAUSE = 325.0  # head #741: below 60 % of 542, pause the game (Esc) and stop


_last_pos = {}


def dead(o, g):
    """All RegionStatus HP at 0 (count +0x1D0C, pointers +0x1D18, HP R+0x08;
    layout from claude-8 / docs/COMBAT.md)."""
    try:
        n, arr = o.u32(g + 0x1D0C), o.u32(g + 0x1D18)
        if not arr or not 0 < n < 64:
            return False
        return all(o.f32(o.u32(arr + 4 * i) + 8) <= 0.0 for i in range(n))
    except OracleError:
        return False


def threats(o):
    """Head #798: an enemy within 30 m, or one (within 40 m) that moved more
    than 2 m since the previous poll. Passive animals excluded."""
    out = []
    for e in enemies_near(o):
        if e['class'] in PASSIVE or dead(o, int(e['obj'], 16)):
            continue
        prev = _last_pos.get(e['obj'])
        _last_pos[e['obj']] = e['pos']
        moved = prev is not None and math.dist(prev, e['pos']) > 200.0
        if e['dist'] <= 3000 or moved:
            out.append(e)
    return out


def game_hours(o):
    g = o.u32(recipes.SGAMESYS_PTR)
    return o.u32(g + recipes.GAME_TIME) / 3600000.0


def series(o, argv):
    """Head #714/#741: a capture every `every` s while no threat is within
    40 m for 60 s; pause the game (Esc) and stop if HP < HP_PAUSE; stop
    after one game day or `count` captures."""
    out = argv[1]
    every = int(argv[argv.index('--every') + 1]) if '--every' in argv else 300
    count = int(argv[argv.index('--count') + 1]) if '--count' in argv else 10 ** 6
    os.makedirs(out, exist_ok=True)
    rec = o.u32(recipes.player(o) + recipes.CHR_RECORD)
    h0, last_hours, elapsed_h = game_hours(o), game_hours(o), 0.0
    clear_since, last_shot, shots = None, 0.0, 0
    while shots < count and elapsed_h < 24.0:
        hp = o.f32(rec + recipes.REC_HP)
        if hp < HP_PAUSE:
            o.cmd('tap', vk=0x1B, frames=4)
            print(f'{time.strftime("%H:%M:%S")} PAUSED (Esc): Arisen HP {hp:.1f} < {HP_PAUSE}; stopping', flush=True)
            return 3
        t = threats(o)
        now = time.time()
        if t:
            clear_since = None
            print(f'{time.strftime("%H:%M:%S")} hold: {[(e["class"], e["dist"]) for e in t]} HP {hp:.1f}', flush=True)
        else:
            clear_since = clear_since or now
            if now - clear_since >= 60 and now - last_shot >= every:
                base = os.path.join(out, time.strftime('ref-%Y%m%d-%H%M%S'))
                rep = snapshot(o, base)
                last_shot, shots = now, shots + 1
                print(f'{time.strftime("%H:%M:%S")} {base} clock {rep["clock"]["+0x50 h"]}:{rep["clock"]["+0x54 m"]:02d} HP {hp:.1f}', flush=True)
        # first poll of an enemy cannot show movement: poll again soon
        hrs = game_hours(o)
        elapsed_h += (hrs - last_hours) % 24.0
        last_hours = hrs
        time.sleep(20)
    print(f'done: {shots} captures over {elapsed_h:.1f} game hours', flush=True)
    return 0


def main(argv):
    o = Oracle(timeout=120)
    if argv[0] == 'once':
        rep = snapshot(o, argv[1])
        print(json.dumps({k: rep[k] for k in ('time', 'clock', 'uPlayer_pos', 'getters', 'elapsed_s')}, default=str))
        print('lights', {k: len(v) for k, v in rep['lights'].items()}, 'fogs', {k: len(v) for k, v in rep['fogs'].items()},
              'cameras', len(rep['uCameraGame']))
        return 0
    if argv[0] == 'series':
        return series(o, argv)
    print(__doc__)
    return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
