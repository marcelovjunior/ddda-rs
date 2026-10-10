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
    # platform layer (Rust replaces it, no 1:1 port): deleting destructors,
    # DTI allocation, construct wrappers, the MT allocators; classified by
    # shape family in E:/ddda-re/notes/native/platform.json
    pf = EXPORT.parent / "native" / "platform.json"
    if pf.exists():
        import json
        db.executemany("UPDATE functions SET status='platform' WHERE address=?",
                       [(a,) for a in json.loads(pf.read_text())])
    # the C runtime / library block at the top of .text
    db.execute("UPDATE functions SET status='platform' WHERE address >= 0x012E0000")
    db.execute("UPDATE functions SET status='documented' WHERE address IN (SELECT address FROM cites WHERE kind='doc')")
    db.execute("UPDATE functions SET status='translated' WHERE address IN (SELECT address FROM cites WHERE kind='code')")
    # crates/ddnative holds whole 1:1 ports, each with a differential test
    # against the original (docs/METHODOLOGY.md, "The 1:1 pipeline")
    # only the "Translated from" line of a port counts (callees named in
    # doc comments are not verified by it)
    ported = set()
    for path in REPO.glob("crates/ddnative/src/**/*.rs"):
        for line in path.read_text(encoding="utf-8").splitlines():
            if "Translated from" in line:
                ported.update(int(m.group(1), 16) for m in ADDR.finditer(line))
    # families: same body modulo one constant, each member replayed by the
    # family's differential test (E:/ddda-re/notes/native/families/*.json)
    import json
    for fam in (EXPORT.parent / "native" / "families").glob("*.json"):
        f = json.loads(fam.read_text())
        if f["representative"] in ported:
            ported.update(f["members"])
    db.executemany("UPDATE functions SET status='verified' WHERE address=?", [(a,) for a in ported])
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


GAP = re.compile(r"not yet|unsupported|stand-?in|placeholder|not traced|untraced|not modell?ed|"
                 r"unimplemented|OPEN|TODO|approximat|guess", re.I)
CRT = re.compile(r"^(_|__|FUN_012[EF]|operator|std::|Catch|Unwind)")


def partial():
    """Translated functions that cannot yet behave as the original:
    (a) a gap marker within 12 lines of the citation in Rust code;
    (b) non-thunk callees that are not translated (the call graph closure)."""
    db = connect()
    starts = {a for (a,) in db.execute("SELECT address FROM functions")}
    flagged = {}
    for path in REPO.glob("crates/**/*.rs"):
        lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
        gaps = [i for i, l in enumerate(lines) if GAP.search(l)]
        if not gaps:
            continue
        for i, l in enumerate(lines):
            for m in ADDR.finditer(l):
                a = int(m.group(1), 16)
                if a in starts and any(abs(g - i) <= 12 for g in gaps):
                    flagged.setdefault(a, str(path.relative_to(REPO)))
    db.execute("DROP TABLE IF EXISTS partial")
    db.execute("CREATE TABLE partial(address INTEGER PRIMARY KEY, marker INTEGER, missing_callees INTEGER, file TEXT)")
    q = """SELECT t.address, COUNT(f.address) FROM functions t
           LEFT JOIN calls c ON c.caller=t.address
           LEFT JOIN functions f ON f.address=c.callee AND f.status!='translated' AND NOT f.thunk
           WHERE t.status='translated' GROUP BY t.address"""
    rows = [(a, a in flagged, miss, flagged.get(a)) for a, miss in db.execute(q)]
    db.executemany("INSERT INTO partial VALUES (?,?,?,?)", rows)
    db.commit()
    n = len(rows)
    closed = sum(1 for r in rows if r[2] == 0)
    marked = sum(1 for r in rows if r[1])
    clean = sum(1 for r in rows if r[2] == 0 and not r[1])
    print(f"translated: {n}")
    print(f"  all callees translated (closed): {closed} ({100*closed/n:.1f}%)")
    print(f"  gap marker near the citation:    {marked} ({100*marked/n:.1f}%)")
    print(f"  closed AND no marker (complete): {clean} ({100*clean/n:.1f}%)")
    print("most-missing translated functions:")
    for a, name, ns, miss in db.execute(
            "SELECT p.address, f.name, f.namespace, p.missing_callees FROM partial p JOIN functions f USING(address) "
            "ORDER BY p.missing_callees DESC LIMIT 12"):
        print(f"  0x{a:08X} {miss:3} missing  {ns}::{name}")


def classes(n=40):
    db = connect()
    rows = db.execute("""SELECT namespace, COUNT(*), SUM(size),
            SUM(status='translated'), SUM(CASE WHEN status='translated' THEN size ELSE 0 END)
            FROM functions WHERE NOT thunk GROUP BY namespace ORDER BY SUM(size) DESC""").fetchall()
    named = [r for r in rows if r[0] not in ("Global", "")]
    glob = [r for r in rows if r[0] in ("Global", "")]
    print(f"{len(named)} named namespaces (DTI classes) + Global")
    for ns, cnt, size, tr, trs in glob + named[:n]:
        print(f"{(ns or '-')[:34]:34} {cnt:6} fn {size:9} B  {100*(trs or 0)/size:5.1f}% translated")


def split():
    db = connect()
    crt = gn = 0
    cs = gs = 0
    for name, ns, size in db.execute("SELECT name, namespace, size FROM functions WHERE NOT thunk"):
        if ns in ("Global", "") and CRT.search(name) or 0x012E0000 <= 0:
            crt += 1; cs += size
        else:
            gn += 1; gs += size
    # The CRT/runtime block sits at the top of .text (0x012E0000+); count it by range as well.
    rng_n, rng_s = db.execute("SELECT COUNT(*), SUM(size) FROM functions WHERE NOT thunk AND address >= 0x012E0000").fetchone()
    print(f"named runtime/CRT: {crt} fn {cs} B; address block >= 0x012E0000: {rng_n} fn {rng_s} B")
    print(f"game + engine (rest): {gn} fn {gs} B")


def attribute():
    """Infer a class (namespace) for unlabelled Global functions.
    MSVC links one object file's functions contiguously, so a Global run
    bracketed by the same class on both sides is assigned that class
    ('neighbours'); otherwise >= 70% of callers in one class ('callers').
    Inference only: column 'inferred', with its method; labels unchanged."""
    db = connect()
    cols = [r[1] for r in db.execute("PRAGMA table_info(functions)")]
    if "inferred" not in cols:
        db.execute("ALTER TABLE functions ADD COLUMN inferred TEXT")
        db.execute("ALTER TABLE functions ADD COLUMN method TEXT")
    db.execute("UPDATE functions SET inferred=NULL, method=NULL")
    rows = db.execute("SELECT address, namespace FROM functions ORDER BY address").fetchall()
    is_g = [ns in ("Global", "") for _, ns in rows]
    out = {}
    i = 0
    while i < len(rows):
        if not is_g[i]:
            i += 1
            continue
        j = i
        while j < len(rows) and is_g[j]:
            j += 1
        left = rows[i - 1][1] if i > 0 else None
        right = rows[j][1] if j < len(rows) else None
        if left and left == right and (j - i) <= 40:
            for k in range(i, j):
                out[rows[k][0]] = (left, "neighbours")
        i = j
    callers = {}
    for callee, ns in db.execute("""SELECT c.callee, f.namespace FROM calls c
            JOIN functions f ON f.address=c.caller WHERE f.namespace NOT IN ('Global','')"""):
        callers.setdefault(callee, []).append(ns)
    for a, g in zip((r[0] for r in rows), is_g):
        if g and a not in out and a in callers:
            nss = callers[a]
            top = max(set(nss), key=nss.count)
            if nss.count(top) / len(nss) >= 0.7:
                out[a] = (top, "callers")
    db.executemany("UPDATE functions SET inferred=?, method=? WHERE address=?",
                   [(ns, m, a) for a, (ns, m) in out.items()])
    db.commit()
    total_g, gs = db.execute("SELECT COUNT(*), SUM(size) FROM functions WHERE namespace IN ('Global','') AND NOT thunk").fetchone()
    for m, n, sz in db.execute("SELECT method, COUNT(*), SUM(size) FROM functions WHERE method IS NOT NULL AND NOT thunk GROUP BY method"):
        print(f"{m:10} {n:6} fn ({100*n/total_g:4.1f}% of Global), {sz} B ({100*sz/gs:4.1f}%)")
    print(f"Global total {total_g} fn {gs} B")


FAMILIES = [  # (family, regex on the effective class name); first match wins
    ("player actions", r"^cPlAct|^cPlAction|^nPlayer"), ("player/pawn units", r"^uPlayer|^uCmc|^uActorModelPl|^cPlayer|^cCmc"),
    ("enemies", r"^uEm|^cEm|^uEnemy|^cEnemy|^uEmWolf"), ("NPCs", r"^uNpc|^cNpc"),
    ("AI", r"^cAI|^sAI|^cAI.*|Think|^cFSM"), ("GUI", r"GUI|^uGUI|^cGUI|^sGUI"),
    ("effects/shells", r"^uEffect|^uShl|^cShl|^rEffect|Effect"), ("camera", r"^cCam|^uCam|^sCam"),
    ("animation/constraints", r"^uCns|^cCns|^rMotion|Motion|^uSimSoftBody|Chain"),
    ("collision/physics", r"Collision|^sCollision|^cScr|^rScr|Phys|Havok|^hk"),
    ("rendering", r"^sPrimitive|^sRender|^uWater|^uSky|Shadow|Filter|^sShader|^rShader|^rTexture|^rModel|^uModel|^uObjModel|Light"),
    ("sound", r"Sound|^sSnd|^rSnd|^cSnd|Voice|Bgm"), ("quests/story/events", r"Quest|^sEvent|^uEvent|^rEvent|Scenario|Story"),
    ("items/shops/economy", r"Item|Shop|Equip|Craft"), ("save/system", r"^sSave|Save|^sGameSys|^sApp|^sMain|Option"),
    ("network/online/rift", r"Net|Online|Rift|Pawn(?!Ai)|Steam"), ("resources/archives", r"^r[A-Z]|^sResource|Archive|^cResource"),
    ("framework (MT)", r"^Mt|^cMt|^sMt|^s[A-Z]"), ("units/other", r"^u[A-Z]"), ("components/other", r"^c[A-Z]|^n[A-Z]"),
]


def systems():
    db = connect()
    pats = [(f, re.compile(r)) for f, r in FAMILIES]
    agg = {}
    for ns, inf, size, status in db.execute(
            "SELECT namespace, inferred, size, status FROM functions WHERE NOT thunk AND address < 0x012E0000"):
        eff = ns if ns not in ("Global", "") else (inf or "")
        fam = next((f for f, p in pats if eff and p.search(eff)), "unattributed" if not eff else "other")
        a = agg.setdefault(fam, [0, 0, 0, 0])
        a[0] += 1; a[1] += size
        if status == "translated":
            a[2] += 1; a[3] += size
    print(f"{'system':24} {'functions':>9} {'bytes':>10} {'translated':>11}")
    for fam, (n, sz, tn, ts) in sorted(agg.items(), key=lambda kv: -kv[1][1]):
        print(f"{fam:24} {n:9} {sz:10} {100*ts/sz:10.1f}%")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "summary"
    {"build": build, "summary": summary, "partial": partial, "split": split, "attribute": attribute, "systems": systems,
     "classes": lambda: classes(int(sys.argv[2]) if len(sys.argv) > 2 else 40),
     "next": lambda: next_up(int(sys.argv[2]) if len(sys.argv) > 2 else 25)}[cmd]()
