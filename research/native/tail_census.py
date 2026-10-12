"""L45 census: code past Ghidra's end of a function, and the verification
hole it opened (mocked transfers to addresses that are not entries).

    python tail_census.py <LIFT_DATA> <out.json>

Reads the private pair (patterns/lift.json, vectors/lift.json) and the
inventory (INVENTORY_DB). Ghidra's extent of a function is its exported
address + size (supplementary entries have no extent and are skipped for
section 1). Writes counts and examples; nothing from the exe is stored.
"""
import collections
import json
import sqlite3
import sys

from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_IMM, X86_OP_MEM

import harness as h
import lift


def main(root, out):
    db = sqlite3.connect(f"file:{lift.DB}?mode=ro", uri=True)
    extent = {a: (s, ns) for a, s, ns in db.execute("SELECT address, size, namespace FROM functions")}
    entries = lift.function_entries()
    records = json.load(open(f"{root}/patterns/lift.json"))
    owners = collections.defaultdict(set)
    for f in records:
        for pc in f.get("body", []):
            owners[pc].add(f["addr"])
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    # 1. instructions past Ghidra's end (or before the entry)
    tails = {}
    kinds = collections.Counter()
    for f in records:
        a = f["addr"]
        size, ns = extent.get(a, (0, ""))
        if ns == "Supplementary" or size <= 1:
            continue
        outside = [pc for pc in f.get("body", []) if not a <= pc < a + size]
        if not outside:
            continue
        shared = [pc for pc in outside if len(owners[pc]) > 1]
        kind = "shared tail" if shared else "own tail"
        kinds[kind] += 1
        tails[a] = dict(kind=kind, outside=len(outside), shared=len(shared), first=min(outside))
    # real tail calls: direct jumps out of the body to another entry
    tail_calls = 0
    for f in records:
        body = set(f.get("body", []))
        for pc in f.get("body", []):
            i = next(md.disasm(h.code(pc, 15), pc, count=1), None)
            if i and i.mnemonic.startswith("j") and i.operands and i.operands[0].type == X86_OP_IMM:
                t = i.operands[0].imm & 0xFFFFFFFF
                if t not in body and t in entries:
                    tail_calls += 1
                    break
    # 2. table jumps whose index is bounded on its block's edges
    edge_changes = []
    for f in records:
        body = f.get("body", [])
        jumps = [pc for pc in body
                 if (lambda i: i is not None and i.mnemonic == "jmp" and i.operands
                     and i.operands[0].type == X86_OP_MEM and i.operands[0].mem.index)(
                         next(md.disasm(h.code(pc, 15), pc, count=1), None))]
        if not jumps:
            continue
        try:
            ins, switches, _, _ = lift.decode_body(f["addr"], f["size"], ext=bool(f.get("ext")))
        except lift.Unsupported as e:
            edge_changes.append(dict(addr=f["addr"], change="now rejected", reason=str(e)))
            continue
        new = sorted({i.address for i in ins} - set(body))
        proof = lift.TABLE_PROOF.get(f["addr"], {})
        unproven = sorted(pc for pc, ok in proof.items() if not ok)
        if new or unproven:
            edge_changes.append(dict(addr=f["addr"], change="body grows" if new else "unproven table",
                                     added=len(new), first=new[0] if new else None,
                                     unproven=[f"{pc:#010x}" for pc in unproven]))
    del records
    # 3. recorded mocked transfers to non-entries
    imports = set(h.import_pops()[1])
    bound = entries | imports | set(h.PLATFORM)
    cases = json.load(open(f"{root}/vectors/lift.json"))
    hole = collections.defaultdict(set)
    for c in cases:
        for e in c.get("calls") or []:
            # Random pointers (a vtable call through random memory) are legal
            # mocks; a transfer into the image's code that is not an entry
            # is code the lifted body should have owned.
            if e["target"] not in bound and h.executable(e["target"]):
                hole[c["addr"]].add(e["target"])
    report = dict(
        tails=dict(functions=len(tails), kinds=dict(kinds),
                   examples={f"{a:#010x}": v for a, v in list(sorted(tails.items()))[:40]}),
        tail_calls=tail_calls,
        edge_rule=dict(functions=len(edge_changes),
                       kinds=dict(collections.Counter(c["change"] for c in edge_changes)), changes=edge_changes),
        hole=dict(functions=len(hole), cases_targets={f"{a:#010x}": sorted(f"{t:#010x}" for t in ts)[:8]
                                                       for a, ts in sorted(hole.items())}),
    )
    json.dump(report, open(out, "w"), indent=1)
    print(json.dumps({k: (v if k == "tail_calls" else {kk: vv for kk, vv in v.items() if kk in ("functions", "kinds")})
                      for k, v in report.items()}, indent=1))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
