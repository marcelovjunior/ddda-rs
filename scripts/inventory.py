"""Phase 0 rebuild tracker (docs/METHODOLOGY.md).

Builds a SQLite inventory of every function in DDDA.exe from the headless
Ghidra export (research/ghidra/ExportInventory.java) and marks the ones the
Rust code cites by address ("Translated from 0x..." and other 0x00XXXXXX
mentions). The database lives in the private research tree because it is
derived from the executable; only this script and the summary numbers are
committed.

    python scripts/inventory.py build      # (re)build from the export + repo scan
    python scripts/inventory.py summary    # coverage numbers
    python scripts/inventory.py next [N]   # untranslated functions most called by translated ones
"""
import re
import sqlite3
import sys
from pathlib import Path

EXPORT = Path("E:/ddda-re/notes/inventory")
DB = EXPORT / "inventory.sqlite"
REPO = Path(__file__).resolve().parent.parent
ADDR = re.compile(r"0x(00[4-9A-Fa-f][0-9A-Fa-f]{5}|01[0-9A-Fa-f]{6})\b")


def connect():
    return sqlite3.connect(DB)


def build():
    db = connect()
    db.executescript("""
        DROP TABLE IF EXISTS functions; DROP TABLE IF EXISTS calls; DROP TABLE IF EXISTS cites;
        CREATE TABLE functions(address INTEGER PRIMARY KEY, name TEXT, namespace TEXT,
            size INTEGER, thunk INTEGER, callers INTEGER, callees INTEGER,
            status TEXT NOT NULL DEFAULT 'untouched');
        CREATE TABLE calls(caller INTEGER, callee INTEGER);
        CREATE TABLE cites(address INTEGER, file TEXT, kind TEXT);
    """)
    with open(EXPORT / "functions.tsv", encoding="utf-8") as f:
        next(f)
        rows = []
        for line in f:
            a, name, ns, size, thunk, callers, callees = line.rstrip("\n").split("\t")
            rows.append((int(a, 16), name, ns, int(size), thunk == "true", int(callers), int(callees)))
    db.executemany("INSERT INTO functions(address,name,namespace,size,thunk,callers,callees) VALUES (?,?,?,?,?,?,?)", rows)
    with open(EXPORT / "calls.tsv", encoding="utf-8") as f:
        next(f)
        db.executemany("INSERT INTO calls VALUES (?,?)",
                       ((int(a, 16), int(b, 16)) for a, b in (l.rstrip("\n").split("\t") for l in f)))
    starts = set(r[0] for r in rows)
    cites = []
    for path in list(REPO.glob("crates/**/*.rs")) + list(REPO.glob("docs/**/*.md")):
        kind = "code" if path.suffix == ".rs" else "doc"
        rel = str(path.relative_to(REPO))
        for m in ADDR.finditer(path.read_text(encoding="utf-8", errors="ignore")):
            a = int(m.group(1), 16)
            if a in starts:
                cites.append((a, rel, kind))
    db.executemany("INSERT INTO cites VALUES (?,?,?)", cites)
    db.execute("UPDATE functions SET status='documented' WHERE address IN (SELECT address FROM cites WHERE kind='doc')")
    db.execute("UPDATE functions SET status='translated' WHERE address IN (SELECT address FROM cites WHERE kind='code')")
    db.execute("CREATE INDEX calls_caller ON calls(caller)")
    db.execute("CREATE INDEX calls_callee ON calls(callee)")
    db.commit()
    print(f"built {DB}: {len(rows)} functions, {len(cites)} citations")


def summary():
    db = connect()
    total, size = db.execute("SELECT COUNT(*), SUM(size) FROM functions WHERE NOT thunk").fetchone()
    for status, n, s in db.execute(
            "SELECT status, COUNT(*), SUM(size) FROM functions WHERE NOT thunk GROUP BY status ORDER BY status"):
        print(f"{status:11} {n:6} functions ({100*n/total:5.1f}%), {s:9} bytes ({100*s/size:5.1f}%)")
    print(f"{'total':11} {total:6} functions, {size} bytes")


def next_up(n=25):
    db = connect()
    q = """SELECT f.address, f.name, f.namespace, f.size, COUNT(*) AS uses
           FROM calls c JOIN functions t ON t.address=c.caller AND t.status='translated'
           JOIN functions f ON f.address=c.callee AND f.status!='translated' AND NOT f.thunk
           GROUP BY f.address ORDER BY uses DESC, f.size DESC LIMIT ?"""
    for a, name, ns, size, uses in db.execute(q, (n,)):
        print(f"0x{a:08X} {uses:3} uses {size:6} B  {ns}::{name}")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "summary"
    {"build": build, "summary": summary,
     "next": lambda: next_up(int(sys.argv[2]) if len(sys.argv) > 2 else 25)}[cmd]()
