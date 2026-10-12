"""Read-only x86 EH census. Output is private: tables are executable data.

Run with the research Python; no Ghidra writes or vector workers.
Recognises MSVC EH4 prolog callers and EH3 handler thunks, retaining
unresolved candidates rather than guessing a table from arbitrary words.
"""
import argparse
import csv
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "native"))
import harness
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86_const import X86_OP_IMM


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inventory", default="E:/ddda-re/notes/inventory")
    ap.add_argument("--output", required=True)
    args = ap.parse_args()
    rows = {}
    for name in ("functions.tsv", "extra_functions.tsv"):
        path = Path(args.inventory) / name
        if path.exists():
            for r in csv.DictReader(path.open(encoding="utf-8"), delimiter="\t"):
                rows[int(r["address"], 0)] = r
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.detail = True
    prolog, handler, throw, cxx = 0x12F71E0, 0x12F7240, 0x12F530B, 0x12EFAAB
    out = {"seh4": [], "cxx_frames": [], "throws": [], "imports": [], "unresolved": []}
    # Scan whole inventoried extents; raw instruction references also catch
    # handler thunks not yet identified as functions by Ghidra.
    refs = {}
    immediates = {}
    for addr, row in sorted(rows.items()):
        try:
            ins = list(md.disasm(harness.code(addr, int(row["size"])), addr))
        except (KeyError, ValueError):
            continue
        for i, x in enumerate(ins):
            for operand in x.operands:
                if operand.type == X86_OP_IMM:
                    immediates.setdefault(operand.imm & 0xffffffff, set()).add((addr,x.address))
            if x.mnemonic in ("call", "jmp") and x.operands and x.operands[0].type == X86_OP_IMM:
                dest = x.operands[0].imm & 0xffffffff
                refs.setdefault(dest, []).append([addr, x.address])
                if dest == throw:
                    out["throws"].append({"function": addr, "site": x.address})
                if dest == prolog:
                    pushes = [p.operands[0].imm & 0xffffffff for p in ins[max(0, i-5):i]
                              if p.mnemonic == "push" and p.operands[0].type == X86_OP_IMM]
                    tables = [p for p in pushes if p > 0x400000 and not harness.executable(p)]
                    out["seh4"].append({"function": addr, "site": x.address, "tables": tables})
                if dest == cxx:
                    prev = ins[i-1] if i else None
                    if prev and prev.mnemonic == "mov" and prev.op_str.startswith("eax,") and prev.operands[1].type == X86_OP_IMM:
                        table = prev.operands[1].imm & 0xffffffff
                        magic = struct.unpack("<I", harness.code(table, 4))[0]
                        out["cxx_frames"].append({"thunk": addr, "load_site": prev.address, "table": table, "magic": magic})
                    else:
                        out["unresolved"].append({"function": addr, "site": x.address, "kind": "cxx-frame"})
        # Inline EH4 frames use push handler and push scope table, rather
        # than __SEH_prolog4. Record candidates separately, without conflating.
        for i, x in enumerate(ins):
            if x.mnemonic == "push" and x.operands[0].type == X86_OP_IMM and x.operands[0].imm == handler:
                if addr == prolog:
                    continue
                prev=ins[i-1] if i else None
                if prev and prev.mnemonic=="push" and prev.operands[0].type==X86_OP_IMM:
                    out["seh4"].append({"function":addr,"site":x.address,"tables":[prev.operands[0].imm & 0xffffffff]})
                else:
                    out["unresolved"].append({"function": addr, "site": x.address, "kind": "inline-eh4"})
    # Inventory extents omit funclets after nonreturning calls and two EH
    # thunks. Cross-check every executable-section E8/E9 reference, then
    # validate recovered thunks by their instructions and FuncInfo magic.
    def relative_sites(opcode, target):
        hits=[]
        for va,b in harness.image()[1]:
            if not harness.executable(va): continue
            pos=0
            while True:
                pos=b.find(bytes([opcode]),pos)
                if pos<0 or pos+5>len(b): break
                if (va+pos+5+struct.unpack_from("<i",b,pos+1)[0])&0xffffffff==target:
                    hits.append(va+pos)
                pos+=1
        return hits
    out["raw_crosscheck"]={"prolog_calls":relative_sites(0xe8,prolog),"throw_calls":relative_sites(0xe8,throw),"cxx_jumps":relative_sites(0xe9,cxx)}
    for site in out["raw_crosscheck"]["cxx_jumps"]:
        if any(x["load_site"]+5==site for x in out["cxx_frames"]): continue
        decoded=list(md.disasm(harness.code(site-22,27),site-22))
        if (len(decoded)==7 and decoded[0].mnemonic=="mov" and decoded[0].op_str=="edx, dword ptr [esp + 8]"
                and decoded[-2].mnemonic=="mov" and decoded[-2].op_str.startswith("eax,")
                and decoded[-2].operands[1].type==X86_OP_IMM and decoded[-1].address==site):
            table=decoded[-2].operands[1].imm & 0xffffffff
            magic=struct.unpack("<I",harness.code(table,4))[0]
            if magic==0x19930522:
                out["cxx_frames"].append({"thunk":site-22,"load_site":site-5,"table":table,"magic":magic,"origin":"raw-reference"})
                continue
        out["unresolved"].append({"site":site,"kind":"raw-cxx-jump"})
    for item in out["cxx_frames"]:
        item["references"] = sorted(immediates.get(item["thunk"], []))
        fields=struct.unpack("<9I",harness.code(item["table"],36))
        item["max_state"]=fields[1]
        item["unwind_map"]=fields[2]
        item["try_count"]=fields[3]
        item["try_map"]=fields[4]
        item["catches"]=[]
        for k in range(fields[3]):
            low,high,catch_high,n,catches=struct.unpack("<5I",harness.code(fields[4]+20*k,20))
            for j in range(n):
                adjectives,type_,offset,entry=struct.unpack("<4I",harness.code(catches+16*j,16))
                item["catches"].append({"entry":entry,"type":type_,"adjectives":adjectives})
                for insn in md.disasm(harness.code(entry,64),entry):
                    if insn.address in out["raw_crosscheck"]["throw_calls"] and not any(x["site"]==insn.address for x in out["throws"]):
                        out["throws"].append({"function":entry,"site":insn.address,"origin":"catch-map"})
                    if insn.mnemonic in ("ret","int3","jmp"):break
    for site in out["raw_crosscheck"]["throw_calls"]:
        if not any(x["site"]==site for x in out["throws"]):out["unresolved"].append({"site":site,"kind":"raw-throw-call"})
    for item in out["seh4"]:
        item["scopes"]=[]
        for table in item["tables"]:
            # EH4 scope entries: enclosing level, filter, handler. Stop at
            # the first structurally invalid entry; report this as a bound,
            # not proof of reachability of every state.
            for level in range(256):
                enclosing,filter_,body=struct.unpack("<3I",harness.code(table+16+level*12,12))
                if enclosing!=0xfffffffe and enclosing>=level:
                    break
                if filter_ and not harness.executable(filter_):
                    break
                if not harness.executable(body):
                    break
                item["scopes"].append({"level":level,"enclosing":enclosing,"filter":filter_,"handler":body})
    # Use exported Ghidra call edges for reachability, not linear decoding
    # (which may cross non-code gaps in a noncontiguous function extent).
    # Indirect/virtual edges are deliberately not invented.
    edges={}
    with (Path(args.inventory)/"calls.tsv").open(encoding="utf-8") as call_file:
        for row in csv.DictReader(call_file,delimiter="\t"):
            edges.setdefault(int(row["caller"],0),set()).add(int(row["callee"],0))
    for name,root in (("startup",0x12efa14),("winmain",0x4079e0)):
        seen=set(); pending=[root]
        while pending:
            a=pending.pop()
            if a in seen: continue
            seen.add(a); pending.extend(edges.get(a,()))
        for kind in ("seh4","throws"):
            for item in out[kind]: item[name+"_direct"]=item["function"] in seen
    # PE import names/IAT addresses from pefile, read only.
    names = {"RaiseException", "RtlUnwind", "SetUnhandledExceptionFilter", "UnhandledExceptionFilter", "AddVectoredExceptionHandler", "RemoveVectoredExceptionHandler", "IsDebuggerPresent"}
    raw = Path(harness.EXE).read_bytes()
    opt = struct.unpack_from("<I", raw, 0x3c)[0] + 24
    base = struct.unpack_from("<I", raw, opt + 28)[0]
    load_rva,load_directory_size=struct.unpack_from("<II",raw,opt+96+10*8)
    if load_rva:
        load=base+load_rva
        load_size=struct.unpack("<I",harness.code(load,4))[0]
        # This image's directory says 64, but its own structure size is 72.
        # The SafeSEH fields are present at +64/+68; don't silently drop them.
        if load_size>=72:
            table,count=struct.unpack("<II",harness.code(load+64,8))
            entries=struct.unpack("<%dI"%count,harness.code(table,count*4)) if count else ()
            out["safe_seh"]={"load_config":load,"directory_size":load_directory_size,"structure_size":load_size,
                             "table":table,"handlers":[base+x for x in entries]}
    desc = base + struct.unpack_from("<I", raw, opt + 104)[0]
    while True:
        lookup, _, _, dll, iat = struct.unpack("<5I", harness.code(desc, 20))
        if not dll:
            break
        k = 0
        while True:
            ptr = struct.unpack("<I", harness.code(base + (lookup or iat) + k * 4, 4))[0]
            if not ptr:
                break
            name = "" if ptr & 0x80000000 else harness.code(base + ptr + 2, 100).split(b"\0")[0].decode()
            if name in names:
                out["imports"].append({"name": name, "iat": base + iat + k * 4})
            k += 1
        desc += 20
    out["direct_references"] = {hex(a): refs.get(a, []) for a in (prolog, handler, throw, cxx)}
    Path(args.output).write_text(json.dumps(out, indent=2), encoding="utf-8")
    print({k:len(out[k]) for k in ("seh4","cxx_frames","throws","imports","unresolved")},
          "SafeSEH handlers:",len(out.get("safe_seh",{}).get("handlers",[])))


if __name__ == "__main__":
    main()
