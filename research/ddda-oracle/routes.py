"""Acceptance routes: scripted actions recorded in the original (docs/ACCEPTANCE.md).

    python routes.py record [ROUTE ...] [--out DIR]   # default all routes, DIR E:\\ddda-re\\notes\\acceptance
    python routes.py list

Each route: from the start pose (where the player stands when recording
begins), hold keys by frame index while `trace` samples the player after
every game frame. The keys are camera-relative, and the camera cannot be
turned from the keyboard, so the recorder picks, among the 8 keyboard
directions, the one whose world direction (from the camera yaw) has the
longest walkable run in the stage collision (`ddda-ledges LEDGES_AT`).
After a route, the player runs back the other way and `safe_teleport`s
(<= 5 m, same cell) onto the start, so every route starts from the same pose.

The CSV traces are recordings of the original: private, never committed.
Back up the save before recording (the player moves).
"""
import math
import os
import struct
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from client import Oracle, OracleError  # noqa: E402
import recipes  # noqa: E402

W, A, S, D, SHIFT, CTRL, SPACE = 0x57, 0x41, 0x53, 0x44, 0xA0, 0xA2, 0x20
KEYBIT = {W: 1, A: 2, S: 4, D: 8, SHIFT: 16, CTRL: 32, SPACE: 64}
# stick (x right, y forward) of each keyboard direction
DIRS = {(W,): (0, 1), (S,): (0, -1), (A,): (-1, 0), (D,): (1, 0),
        (W, A): (-1, 1), (W, D): (1, 1), (S, A): (-1, -1), (S, D): (1, -1)}
COLUMNS = 'keys,dt_main,dt_unit,x,y,z,vx,vy,vz,yaw,action,motion,mframe,stamina,cam_yaw'


def seconds(o, s):
    """Frames for `s` seconds at the current frame rate (sMain mActualFps)."""
    fps = o.f32(o.u32(0x018D0B28) + 0x44)
    return max(1, round(s * fps))


# route -> builder(o, dirkeys) -> (frames, script, window, length_cm)
def r_run(o, d):
    a, b = 5, 5 + seconds(o, 3)
    return b + seconds(o, 1), [(a, list(d)), (b, [])], (a + seconds(o, 1), b), 15 * 30 * 3


def r_sprint_stop(o, d):
    a, b = 5, 5 + seconds(o, 4)
    return b + seconds(o, 2), [(a, list(d) + [SHIFT]), (b, [])], (a + seconds(o, 1), b), 23 * 30 * 4


def r_standing_jump(o, d):
    return 5 + seconds(o, 2.5), [(5, [SPACE]), (8, [])], None, 0


def r_running_jump(o, d):
    a = 5
    j = a + seconds(o, 1)
    end = j + seconds(o, 1.5)
    return end + seconds(o, 0.7), [(a, list(d)), (j, list(d) + [SPACE]), (j + 3, list(d)), (end, [])], (a + seconds(o, 0.5), j), 15 * 30 * 3


def r_drop(o, d):
    """Walk (Ctrl) off a shelf; started on the shelf by `record_drop`."""
    a, b = 5, 5 + seconds(o, 3)
    return b + seconds(o, 1.5), [(a, list(d) + [CTRL]), (b, [])], None, 0


ROUTES = {
    'run': r_run,
    'sprint_stop': r_sprint_stop,
    'standing_jump': r_standing_jump,
    'running_jump': r_running_jump,
}
# routes with their own start, recorded by name only
SPECIAL = {'drop': r_drop}


def nearest_keys(o, cam, target_yaw):
    """The keyboard direction whose world yaw is closest to target_yaw."""
    cy = cam_yaw(o, cam)
    best = None
    for keys, st in DIRS.items():
        n = math.hypot(*st)
        yaw = world_yaw(cy, (st[0] / n, st[1] / n))
        err = abs((yaw - target_yaw + math.pi) % (2 * math.pi) - math.pi)
        if best is None or err < best[0]:
            best = (err, keys, yaw)
    return best


def hop_to(o, x, z, step=450.0, tries=8):
    """safe_teleport hops (<= 4.5 m each) toward local (x, z); stops on any
    anomaly and after `tries` hops (a spot the character slides off)."""
    for _ in range(tries):
        px, py, pz = recipes.pos(o, recipes.player(o))
        d = math.hypot(x - px, z - pz)
        if d < 5:
            return
        k = min(1.0, step / d)
        _, anomalies = recipes.safe_teleport(o, px + (x - px) * k, pz + (z - pz) * k)
        if anomalies:
            raise OracleError(f'anomaly during hop: {anomalies}')
    raise OracleError(f'could not settle at ({x}, {z}) in {tries} hops')


def camera(o):
    for a in o.find('uCameraGame', limit=32):
        q = o.f32s(a + 0x40, 3)
        if all(math.isfinite(v) and abs(v) < 1e6 for v in q) and any(abs(v) > 1 for v in q):
            return a
    raise OracleError('no uCameraGame')


def cam_yaw(o, cam):
    c, t = o.f32s(cam + 0x40, 3), o.f32s(cam + 0x60, 3)
    return math.atan2(t[0] - c[0], t[2] - c[2])


def world_yaw(cyaw, stick):
    # ddsim's convention: stick x is screen-right; right is a negative turn about +Y
    return cyaw + math.atan2(-stick[0], stick[1])


def pick_direction(o, cam, length):
    """The keyboard direction with the longest walkable run (stage collision)."""
    p = recipes.player(o)
    x, y, z = recipes.pos(o, p)
    a, b = (int(v) for v in recipes.stage(o)['origin_cell'][6:-1].split('m'))
    cy = cam_yaw(o, cam)
    yaws = {}
    for keys, st in DIRS.items():
        n = math.hypot(*st)
        yaws[keys] = world_yaw(cy, (st[0] / n, st[1] / n))
    env = dict(os.environ, LEDGES_LINES=' '.join([str(length + 300)] + [repr(v) for v in yaws.values()]))
    r = subprocess.run([recipes.LEDGES, recipes.NATIVE_PC, str(a), str(b), str(x), str(y), str(z), str(length + 1200)],
                       capture_output=True, text=True, env=env)
    dists = [float(line.split()[1]) for line in r.stdout.strip().splitlines()]
    best = max(zip(dists, yaws.keys(), yaws.values()))
    return best


def record(o, name, out_dir, start, before=None):
    cam = camera(o)
    p = recipes.player(o)
    rec = o.u32(p + recipes.CHR_RECORD)
    sm = o.u32(0x018D0B28)
    wo = o.u32(recipes.SWORLDOFFSET_PTR)
    builder = ROUTES.get(name) or SPECIAL[name]
    frames, script, window, length = builder(o, (W,))
    keys = (W,)
    if name == 'drop':
        err, keys, yaw = nearest_keys(o, cam, DROP_YAW)
        print(f'  drop: direction {[hex(k) for k in keys]} world yaw {math.degrees(yaw):.0f} deg (want {math.degrees(DROP_YAW):.0f})')
        frames, script, window, length = builder(o, keys)
    elif length:
        clear, keys, yaw = pick_direction(o, cam, length)
        print(f'  {name}: direction {[hex(k) for k in keys]} world yaw {math.degrees(yaw):.0f} deg, clear {clear:.0f} cm (need {length})')
        frames, script, window, length = builder(o, keys)
    st = recipes.stage(o)
    cell = st['origin_cell'][6:-1].split('m')
    fields = [(p + 0x40, 12), (p + 0x2530, 12), (p + 0x257C, 4), (p + 0x2DD4, 4), (p + 0xF40, 4), (p + 0x3CC, 4),
              (p + 0x14, 4), (rec + recipes.REC_STAMINA, 4), (sm + 0x70, 4), (cam + 0x40, 12), (cam + 0x60, 12),
              (wo + recipes.WORLD_ABS_OFFSET, 12), (rec + recipes.REC_HP, 4)]
    if before:
        before()
    rows = o.trace(fields, frames, script)
    # keys held per frame index, from the script
    held, k = [], []
    sched = dict((f, ks) for f, ks in script)
    for i in range(frames):
        if i in sched:
            k = sched[i]
        held.append(sum(KEYBIT[v] for v in k))
    path = os.path.join(out_dir, f'{name}.csv')
    hp0 = None
    with open(path, 'w', newline='\n') as f:
        f.write(f'# route={name}\n# stage=100\n# cell={cell[0]} {cell[1]}\n')
        if window:
            f.write(f'# window={window[0]} {window[1]}\n')
        f.write(f'# recorded={time.strftime("%Y-%m-%d %H:%M:%S")} start_local={tuple(round(v, 1) for v in start)}\n')
        f.write(COLUMNS + '\n')
        for i, r in enumerate(rows):
            x, y, z, vx, vy, vz, yaw = struct.unpack_from('<7f', r, 0)
            act, mot = struct.unpack_from('<II', r, 28)
            mfr, dtu, sta, dtm = struct.unpack_from('<4f', r, 36)
            c = struct.unpack_from('<3f', r, 52)
            t = struct.unpack_from('<3f', r, 64)
            wx, wy, wz = struct.unpack_from('<3f', r, 76)
            hp = struct.unpack_from('<f', r, 88)[0]
            hp0 = hp if hp0 is None else hp0
            if hp < hp0:
                print(f'  WARNING: HP {hp0} -> {hp} at frame {i}')
            cy = math.atan2(t[0] - c[0], t[2] - c[2])
            f.write(f'{held[i]},{dtm:.7g},{dtu:.7g},{x - wx:.4f},{y - wy:.4f},{z - wz:.4f},{vx:.5g},{vy:.5g},{vz:.5g},'
                    f'{yaw:.6g},{act},{mot},{mfr:.5g},{sta:.6g},{cy:.6g}\n')
    first, last = rows[0], rows[-1]
    print(f'  wrote {path}: {frames} frames; actions {sorted(set(struct.unpack_from("<I", r, 28)[0] for r in rows))}')
    return keys


def go_back(o, keys, start, frames_back):
    """Run back the opposite way, then safe_teleport onto the start."""
    opp = tuple({W: S, S: W, A: D, D: A}[k] for k in keys)
    if frames_back:
        o.trace([(recipes.player(o) + 0x40, 12)], frames_back, [(0, list(opp)), (frames_back - 1, [])])
        o.wait(20)
    x, y, z = recipes.pos(o, recipes.player(o))
    if math.hypot(x - start[0], z - start[2]) > recipes.MAX_TELEPORT:
        raise OracleError(f'back at ({x:.0f}, {z:.0f}), {math.hypot(x - start[0], z - start[2]):.0f} cm from the start: '
                          'too far for a safe teleport; stopping')
    _, anomalies = recipes.safe_teleport(o, start[0], start[2])
    if anomalies:
        raise OracleError(f'anomaly after teleport: {anomalies}')
    o.wait(30)


# The drop (cell st100_58m49n): a narrow raised strip at local (3475, 2210),
# y ~40254, about 2.85 m above the ground at (3402.7, 39969.5, 2356.6). The
# character cannot stand on it (it slides off), so the route places the
# player on it and walks off toward the lower ground at once.
DROP_CELL = 'st100_58m49n'
DROP_RIDGE = (3475.0, 2210.0)
DROP_BELOW = (3402.7, 2356.6)
DROP_YAW = math.atan2(DROP_BELOW[0] - DROP_RIDGE[0], DROP_BELOW[1] - DROP_RIDGE[1])


def record_drop(o, out):
    if recipes.stage(o)['origin_cell'] != DROP_CELL:
        raise OracleError('not in the drop cell')
    p = recipes.player(o)
    x0, y0, z0 = recipes.pos(o, p)
    if math.hypot(DROP_RIDGE[0] - x0, DROP_RIDGE[1] - z0) > recipes.MAX_TELEPORT:
        raise OracleError('too far from the ridge for a safe teleport')
    ry = recipes.ground_y(o, DROP_RIDGE[0], DROP_RIDGE[1], y0 + 285.0)
    print(f'  ridge ground y {ry:.1f} ({ry - y0:+.1f} above the player)')
    put = lambda: recipes.set_pos(o, p, DROP_RIDGE[0], ry + 5.0, DROP_RIDGE[1])
    record(o, 'drop', out, (DROP_RIDGE[0], ry, DROP_RIDGE[1]), before=put)
    print('  after the drop at', recipes.pos(o, p))


def main(argv):
    out = r'E:\ddda-re\notes\acceptance'
    if '--out' in argv:
        i = argv.index('--out')
        out = argv[i + 1]
        argv = argv[:i] + argv[i + 2:]
    if argv and argv[0] == 'list':
        print('\n'.join(ROUTES))
        return 0
    names = argv[1:] if len(argv) > 1 else list(ROUTES) + list(SPECIAL)
    os.makedirs(out, exist_ok=True)
    o = Oracle(timeout=120)
    start = recipes.pos(o, recipes.player(o))
    print('start (local)', start, recipes.stage(o)['origin_cell'])
    for name in names:
        if name == 'drop':
            record_drop(o, out)
            continue
        keys = record(o, name, out, start)
        # time spent moving ~ the route's hold; run back as long
        moved = math.dist(start, recipes.pos(o, recipes.player(o)))
        back = 0 if moved < 300 else seconds(o, moved / (10.2 * 30))
        go_back(o, keys, start, back)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
