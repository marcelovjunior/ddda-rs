#!/usr/bin/env python3
"""Shared coordination board for the agents working on ddda-rs.

One append-only event log in SQLite (default E:/ddda-board/board.sqlite;
the directory can be overridden with the DDDA_BOARD environment variable)
that every agent reads and writes through this script, whichever worktree
it runs from. SQLite's transactions make concurrent writers from several
processes safe, and a killed writer cannot leave a half-written event.
Python standard library only.

Agents: `head` (the coordinating Claude session) and its subordinates,
named `<kind>-<n>` (e.g. `claude-4`, `codex-2`). See docs/BOARD.md.

    python scripts/board.py show                       overview
    python scripts/board.py join codex-2 --kind codex --role "asset streaming"
    python scripts/board.py status codex-2 "reading the split loader" --task streaming
    python scripts/board.py claim codex-2 world/streaming --path crates/ddworld/** --note "cell streaming"
    python scripts/board.py check codex-2 crates/ddworld/src/lib.rs
    python scripts/board.py post codex-2 claude-4 "I need Terrain::load to take a cell list"
    python scripts/board.py inbox codex-2
    python scripts/board.py wait codex-2 --timeout 600
    python scripts/board.py release codex-2 world/streaming
"""
import argparse
import datetime as dt
import fnmatch
import json
import os
import sqlite3
import sys
import time

BOARD = os.environ.get('DDDA_BOARD', 'E:/ddda-board')
EVENTS = os.path.join(BOARD, 'board.sqlite')
HEAD = 'head'
STALE_HOURS = 2.0
STATES = ('working', 'blocked', 'waiting', 'review', 'idle', 'done')
KINDS = ('question', 'info', 'request', 'blocker', 'answer', 'assign', 'conflict')


# ---------------------------------------------------------------- storage

def db():
    os.makedirs(BOARD, exist_ok=True)
    con = sqlite3.connect(EVENTS, timeout=30, isolation_level=None)
    con.execute('PRAGMA journal_mode=WAL')
    con.execute('PRAGMA busy_timeout=30000')
    con.execute('CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY AUTOINCREMENT,'
                ' ts TEXT NOT NULL, body TEXT NOT NULL)')
    return con


def load():
    con = db()
    try:
        return [{'id': i, 'ts': ts, **json.loads(body)}
                for i, ts, body in con.execute('SELECT id, ts, body FROM events ORDER BY id')]
    finally:
        con.close()


def now():
    return dt.datetime.now().astimezone().isoformat(timespec='seconds')


def append(event):
    """Appends one event in its own transaction; returns it with its id."""
    con = db()
    try:
        ts = now()
        cur = con.execute('INSERT INTO events (ts, body) VALUES (?, ?)',
                          (ts, json.dumps(event, ensure_ascii=False)))
        return {'id': cur.lastrowid, 'ts': ts, **event}
    finally:
        con.close()


# ---------------------------------------------------------------- state

def state(events):
    """Replays the log: agents, open claims, messages, read marks."""
    agents, claims, msgs, read = {}, {}, [], {}
    for e in events:
        a = e.get('agent')
        if a:
            ag = agents.setdefault(a, {'kind': '?', 'role': '', 'state': 'idle', 'status': '', 'task': '',
                                       'seen': e['ts'], 'joined': e['ts'], 'left': False})
            ag['seen'] = e['ts']
        t = e['type']
        if t == 'join':
            ag.update(kind=e.get('kind', '?'), role=e.get('role', ''), joined=e['ts'], left=False)
        elif t == 'leave':
            ag['left'] = True
            ag['state'] = 'idle'
            for k in [k for k in claims if k[0] == a]:
                del claims[k]
        elif t == 'status':
            ag.update(status=e['text'], state=e.get('state') or ag['state'], task=e.get('task') or ag['task'])
        elif t == 'claim':
            claims[(e.get('owner', a), e['area'])] = e
        elif t == 'release':
            owner = e.get('owner', a)
            for k in [k for k in claims if k[0] == owner and (e.get('area') in (None, k[1]))]:
                del claims[k]
        elif t == 'msg':
            msgs.append(e)
        elif t == 'read':
            read[a] = max(read.get(a, 0), e['upto'])
    return agents, claims, msgs, read


def age_hours(ts):
    return (dt.datetime.now().astimezone() - dt.datetime.fromisoformat(ts)).total_seconds() / 3600


def ago(ts):
    s = age_hours(ts) * 3600
    return f'{s:.0f}s' if s < 120 else f'{s / 60:.0f}m' if s < 7200 else f'{s / 3600:.1f}h'


def to_me(m, agent):
    to = m['to']
    return m['agent'] != agent and (to == 'all' or agent in to.split(','))


def norm(p):
    return p.replace('\\', '/').lstrip('./')


def prefix(glob):
    """The literal part of a path glob, up to its first wildcard."""
    g = norm(glob)
    cut = min([i for i in (g.find('*'), g.find('?'), g.find('[')) if i >= 0], default=len(g))
    return g[:cut]


def paths_overlap(a, b):
    pa, pb = prefix(a), prefix(b)
    if pa == norm(a) and pb == norm(b):
        return pa == pb
    if pa == norm(a):  # a is a concrete path
        return fnmatch.fnmatch(pa, norm(b)) or pa.startswith(pb)
    if pb == norm(b):
        return fnmatch.fnmatch(pb, norm(a)) or pb.startswith(pa)
    return pa.startswith(pb) or pb.startswith(pa)


def areas_overlap(a, b):
    sa, sb = a.strip('/').split('/'), b.strip('/').split('/')
    n = min(len(sa), len(sb))
    return sa[:n] == sb[:n]


def conflicts(claims, agent, area=None, paths=()):
    """Other agents' claims that overlap `area` or any of `paths`."""
    out = []
    for (owner, carea), c in claims.items():
        if owner == agent:
            continue
        why = []
        if area and areas_overlap(area, carea):
            why.append(f'area {carea}')
        for p in paths:
            for q in c.get('paths', []):
                if paths_overlap(p, q):
                    why.append(f'{norm(p)} ~ {q}')
        if why:
            out.append((owner, carea, c, why))
    return out


# ---------------------------------------------------------------- output

def fmt_msg(m, read_upto=None):
    new = '' if read_upto is None or m['id'] <= read_upto else ' *new*'
    re = f' re #{m["re"]}' if m.get('re') else ''
    return (f'#{m["id"]} [{m.get("kind", "info")}] {m["agent"]} -> {m["to"]}{re} ({ago(m["ts"])} ago){new}\n'
            f'    ' + m['text'].replace('\n', '\n    '))


def show(events, n_msgs=15, agent=None):
    agents, claims, msgs, read = state(events)
    lines = [f'board {EVENTS}  ({len(events)} events)', '', 'AGENTS']
    for a, ag in sorted(agents.items(), key=lambda kv: (kv[0] != HEAD, kv[0])):
        if ag['left']:
            continue
        stale = '  STALE' if age_hours(ag['seen']) > STALE_HOURS else ''
        task = f' [{ag["task"]}]' if ag['task'] else ''
        lines.append(f'  {a:10} {ag["kind"]:7} {ag["state"]:8}{task} seen {ago(ag["seen"])} ago{stale}')
        if ag['role']:
            lines.append(f'  {"":10} role: {ag["role"]}')
        if ag['status']:
            lines.append(f'  {"":10} now:  {ag["status"]}')
    lines += ['', 'CLAIMS']
    for (owner, area), c in sorted(claims.items()):
        p = ', '.join(c.get('paths', [])) or '-'
        stale = '  STALE' if owner in agents and age_hours(agents[owner]['seen']) > STALE_HOURS else ''
        lines.append(f'  {area:28} {owner:10} paths: {p}{stale}')
        if c.get('note'):
            lines.append(f'  {"":28} {c["note"]}')
    if not claims:
        lines.append('  (none)')
    shown = [m for m in msgs if agent is None or to_me(m, agent) or m['agent'] == agent][-n_msgs:]
    lines += ['', f'MESSAGES (last {len(shown)})']
    for m in shown:
        lines.append('  ' + fmt_msg(m, read.get(agent) if agent else None).replace('\n', '\n  '))
    if not shown:
        lines.append('  (none)')
    return '\n'.join(lines)


# ---------------------------------------------------------------- commands

def need_known(events, agent):
    agents, *_ = state(events)
    if agent not in agents or agents[agent]['left']:
        sys.exit(f'board: {agent} has not joined; run: board.py join {agent} --kind <claude|codex|human> --role "..."')


def cmd_join(a):
    e = append({'type': 'join', 'agent': a.agent, 'kind': a.kind, 'role': a.role or ''})
    print(f'joined as {a.agent} (event #{e["id"]})\n')
    print(show(load(), agent=a.agent))


def cmd_leave(a):
    append({'type': 'leave', 'agent': a.agent})
    print(f'{a.agent} left; its claims are released')


def cmd_status(a):
    need_known(load(), a.agent)
    append({'type': 'status', 'agent': a.agent, 'text': a.text, 'state': a.state, 'task': a.task})
    unread = [m for m in state(load())[2] if to_me(m, a.agent) and m['id'] > state(load())[3].get(a.agent, 0)]
    print('status recorded' + (f'; {len(unread)} unread message(s): run inbox {a.agent}' if unread else ''))


def cmd_claim(a):
    events = load()
    need_known(events, a.agent)
    _, claims, _, _ = state(events)
    hits = conflicts(claims, a.agent, a.area, a.path or [])
    if hits and not a.force:
        print(f'CONFLICT: {a.area} overlaps claims held by other agents:')
        for owner, carea, c, why in hits:
            print(f'  {owner} holds {carea} ({c.get("note", "")}): {"; ".join(why)}')
        print('\nTalk to them first: board.py post', a.agent, ','.join(sorted({h[0] for h in hits})),
              '"..."  (or ask head). Use --force only if head agreed.')
        sys.exit(2)
    append({'type': 'claim', 'agent': a.agent, 'area': a.area, 'paths': [norm(p) for p in a.path or []],
            'note': a.note or ''})
    if hits:
        owners = ','.join(sorted({h[0] for h in hits} | {HEAD}))
        append({'type': 'msg', 'agent': a.agent, 'to': owners, 'kind': 'conflict',
                'text': f'I claimed {a.area} with --force although it overlaps your claim(s): '
                        + '; '.join(f'{h[1]} ({", ".join(h[3])})' for h in hits)})
        print(f'claimed {a.area} (forced; {owners} notified)')
    else:
        print(f'claimed {a.area}')


def cmd_release(a):
    events = load()
    _, claims, _, _ = state(events)
    owner = a.owner or a.agent
    held = [k for k in claims if k[0] == owner and (a.area is None or k[1] == a.area)]
    if not held:
        sys.exit(f'board: {owner} holds no claim {a.area or ""}'.rstrip())
    append({'type': 'release', 'agent': a.agent, 'owner': owner, 'area': a.area})
    print(f'released {", ".join(k[1] for k in held)} of {owner}')


def cmd_check(a):
    _, claims, _, _ = state(load())
    hits = conflicts(claims, a.agent, a.area, a.paths)
    if not hits:
        print('clear: no other agent claims these')
        return
    for owner, carea, c, why in hits:
        print(f'{owner} holds {carea} ({c.get("note", "")}): {"; ".join(why)}')
    sys.exit(1)


def cmd_post(a):
    events = load()
    need_known(events, a.agent)
    agents, *_ = state(events)
    for t in a.to.split(','):
        if t != 'all' and t not in agents:
            print(f'note: {t} has not joined yet; the message waits for it')
    text = a.text if a.text != '-' else sys.stdin.read()
    e = append({'type': 'msg', 'agent': a.agent, 'to': a.to, 'kind': a.kind, 'text': text, 're': a.re})
    print(f'posted #{e["id"]} to {a.to}')


def cmd_inbox(a):
    events = load()
    _, _, msgs, read = state(events)
    upto = read.get(a.agent, 0)
    mine = [m for m in msgs if to_me(m, a.agent) and (a.all or m['id'] > upto)]
    if not mine:
        print('no unread messages' if not a.all else 'no messages')
    for m in mine:
        print(fmt_msg(m, upto))
    if mine and not a.peek:
        append({'type': 'read', 'agent': a.agent, 'upto': max(m['id'] for m in mine)})


def cmd_wait(a):
    deadline = time.time() + a.timeout
    while True:
        _, _, msgs, read = state(load())
        upto = read.get(a.agent, 0)
        mine = [m for m in msgs if to_me(m, a.agent) and m['id'] > upto]
        if mine:
            for m in mine:
                print(fmt_msg(m, upto))
            append({'type': 'read', 'agent': a.agent, 'upto': max(m['id'] for m in mine)})
            return
        if time.time() > deadline:
            print('no messages (timeout)')
            sys.exit(3)
        time.sleep(a.poll)


def cmd_thread(a):
    _, _, msgs, _ = state(load())
    ids = {a.id}
    changed = True
    while changed:
        changed = False
        for m in msgs:
            if (m['id'] in ids or m.get('re') in ids) and not ({m['id'], m.get('re')} <= ids | {None}):
                ids |= {m['id'], m.get('re')} - {None}
                changed = True
    for m in msgs:
        if m['id'] in ids:
            print(fmt_msg(m))


def cmd_show(a):
    print(show(load(), a.n, a.agent))


def cmd_log(a):
    for e in load()[-a.n:]:
        print(json.dumps(e, ensure_ascii=False))


def cmd_render(a):
    out = a.out or os.path.join(BOARD, 'BOARD.md')
    with open(out, 'w', encoding='utf-8') as f:
        f.write(f'# Agent board (rendered {now()})\n\n```\n{show(load(), a.n)}\n```\n')
    print(f'wrote {out}')


def main():
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)

    p = sub.add_parser('join', help='register (or re-register) an agent')
    p.add_argument('agent')
    p.add_argument('--kind', default='claude', help='claude, codex or human')
    p.add_argument('--role', help='what this agent is for')
    p.set_defaults(f=cmd_join)

    p = sub.add_parser('leave', help='unregister; releases all claims')
    p.add_argument('agent')
    p.set_defaults(f=cmd_leave)

    p = sub.add_parser('status', help='report what you are doing now')
    p.add_argument('agent')
    p.add_argument('text')
    p.add_argument('--state', choices=STATES)
    p.add_argument('--task', help='brief id from docs/tasks')
    p.set_defaults(f=cmd_status)

    p = sub.add_parser('claim', help='claim an area (and paths) before working on it')
    p.add_argument('agent')
    p.add_argument('area', help='slash-separated system name, e.g. world/streaming')
    p.add_argument('--path', action='append', help='file or glob you will edit (repeatable)')
    p.add_argument('--note')
    p.add_argument('--force', action='store_true', help='claim despite overlaps (notifies the holders)')
    p.set_defaults(f=cmd_claim)

    p = sub.add_parser('release', help='release your claim (or all of them)')
    p.add_argument('agent')
    p.add_argument('area', nargs='?')
    p.add_argument('--owner', help='release another agent\'s claim (head only)')
    p.set_defaults(f=cmd_release)

    p = sub.add_parser('check', help='does anyone else claim these paths? exit 1 if so')
    p.add_argument('agent')
    p.add_argument('paths', nargs='*')
    p.add_argument('--area')
    p.set_defaults(f=cmd_check)

    p = sub.add_parser('post', help='send a message (to an agent, a comma list, head or all)')
    p.add_argument('agent', help='sender')
    p.add_argument('to')
    p.add_argument('text', help="message, or - to read it from stdin")
    p.add_argument('--kind', choices=KINDS, default='info')
    p.add_argument('--re', type=int, help='id of the message this answers')
    p.set_defaults(f=cmd_post)

    p = sub.add_parser('inbox', help='unread messages to you (marks them read)')
    p.add_argument('agent')
    p.add_argument('--all', action='store_true', help='include read ones')
    p.add_argument('--peek', action='store_true', help='do not mark read')
    p.set_defaults(f=cmd_inbox)

    p = sub.add_parser('wait', help='block until a message for you arrives')
    p.add_argument('agent')
    p.add_argument('--timeout', type=float, default=600)
    p.add_argument('--poll', type=float, default=5)
    p.set_defaults(f=cmd_wait)

    p = sub.add_parser('thread', help='a message and its replies')
    p.add_argument('id', type=int)
    p.set_defaults(f=cmd_thread)

    p = sub.add_parser('show', help='agents, claims, recent messages')
    p.add_argument('--n', type=int, default=15)
    p.add_argument('--agent', help='only messages to/from this agent; marks *new*')
    p.set_defaults(f=cmd_show)

    p = sub.add_parser('log', help='raw events')
    p.add_argument('--n', type=int, default=50)
    p.set_defaults(f=cmd_log)

    p = sub.add_parser('render', help='write BOARD.md next to the log')
    p.add_argument('--out')
    p.add_argument('--n', type=int, default=30)
    p.set_defaults(f=cmd_render)

    a = ap.parse_args()
    a.f(a)


if __name__ == '__main__':
    main()
