"""Find where one lifted function first diverges from the original.

    PY trace_one.py 0xADDR [seed]

Records one vector for the function (ext mode if it is ext), runs the
original in Unicorn printing the registers at every basic-block start, then
builds a throwaway test (crates/ddnative/tests/zz_trace.rs, deleted after)
holding the function's Rust with a block trace and replays the same vector.
Compiling it only links the already-built ddnative, so it takes seconds.
Prints the first block where the two traces differ.

Looks the function up in patterns/lift.json, then patterns/lift_mismatch.json.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
import harness as h  # noqa: E402
import lift  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EBX,  # noqa: E402
                               UC_X86_REG_ESP, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI)

REGS = [("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX), ("ebx", UC_X86_REG_EBX),
        ("esp", UC_X86_REG_ESP), ("ebp", UC_X86_REG_EBP), ("esi", UC_X86_REG_ESI), ("edi", UC_X86_REG_EDI)]
TEST = os.path.join(lift.R, "crates/ddnative/tests/zz_trace.rs")


def line(pc, regs):
    return f"trace {pc:#x} " + " ".join(f"{n}={regs[n]:#x}" for n, _ in REGS)


def main():
    addr = int(sys.argv[1], 16)
    seed_arg = int(sys.argv[2]) if len(sys.argv) > 2 else None
    lists = [json.load(open(f"{lift.N}/patterns/{n}.json")) for n in ("lift", "lift_mismatch")
             if os.path.exists(f"{lift.N}/patterns/{n}.json")]
    f = next(x for lst in lists for x in lst if x["addr"] == addr)
    closed = frozenset(x["addr"] for x in lists[0] if not x.get("ext"))
    leaders = {int(m, 16) for m in re.findall(r"^\s*0x([0-9A-F]+) =>", "\n".join(f["arms"]), re.M)}

    # 1. the original, with block-start registers
    orig, trace = h.Uc.hook_add, []

    def hook_add(self, kind, cb, *a, **k):
        if kind == UC_HOOK_CODE and cb.__name__ == "on_code":
            def wrap(uc, pc, size, ud):
                if pc in leaders:
                    trace.append(line(pc, {n: uc.reg_read(r) for n, r in REGS}))
                return cb(uc, pc, size, ud)
            return orig(self, kind, wrap, *a, **k)
        return orig(self, kind, cb, *a, **k)
    h.Uc.hook_add = hook_add
    case = None
    for s in ([seed_arg] if seed_arg else range(1, 25)):
        trace.clear()
        try:
            kw = {"end": addr + f["size"], "lifted": closed} if f.get("ext") else {"size": f["size"]}
            case = {"addr": addr, **h.run_sparse(addr, (addr << 8) ^ s, **kw)}
            print("seed", s)
            break
        except h.Rejected as e:
            print("seed", s, "rejected:", e)
    h.Uc.hook_add = orig
    if case is None:
        sys.exit("no usable seed")
    original = list(trace)

    # 2. the Rust, same vector, block trace
    os.environ["LIFT_TRACE"] = hex(addr)
    rust = lift.rust_fn(addr, f["name"], f["arms"])
    rust = rust.replace("super::lifted::", "ddnative::generated::lifted::").replace("crate::", "ddnative::")
    vec_path = os.path.join(lift.N, "vectors", "zz_trace.json")
    json.dump([case], open(vec_path, "w"))
    gen_test = open(os.path.join(lift.R, "crates/ddnative/tests/lifted.rs")).read()
    helpers = gen_test[gen_test.index("fn vectors("):gen_test.index("// verifies")]
    body = gen_test[gen_test.index("    for c in &cases {"):gen_test.index("    failures.sort();")]
    src = ("#![allow(warnings)]\nuse ddnative::mem::{Mem, Sparse};\nuse ddnative::x86::{self, Cpu, Entry, Flags};\n"
           "use ddnative::generated::lifted;\nuse serde_json::Value;\nuse std::cell::RefCell;\n"
           "use std::collections::VecDeque;\nuse std::rc::Rc;\n\n" + rust + "\n\n" + helpers +
           "#[test]\nfn trace() {\n    let Some(cases) = vectors(\"zz_trace\") else { panic!(\"no vectors\") };\n"
           f"    let table = Rc::new(lifted::dispatch_table());\n    let debug = Some(0x{addr:08X}u32);\n"
           "    let mut failures: Vec<String> = Vec::new();\n    let mut seen = std::collections::HashSet::new();\n"
           f"    let all: Vec<(u32, fn(&mut dyn Mem, &mut Cpu))> = vec![(0x{addr:08X}, f_{addr:08x})];\n"
           + body + "    eprintln!(\"failures {failures:?}\");\n}\n")
    open(TEST, "w").write(src)
    try:
        r = subprocess.run(["cargo", "test", "-q", "-p", "ddnative", "--test", "zz_trace", "--", "--nocapture"],
                           cwd=lift.R, capture_output=True, text=True)
    finally:
        os.remove(TEST)
        os.remove(vec_path)
    out = r.stdout + r.stderr
    ported = [l.strip() for l in out.splitlines() if l.strip().startswith("trace 0x")]
    if not ported:
        print(out[-3000:])
        sys.exit("the Rust side produced no trace (build error above?)")
    for k, (a, b) in enumerate(zip(original, ported)):
        if a != b:
            print(f"first divergence at block {k}:\n  original {a}\n  rust     {b}")
            print("previous block:", original[k - 1] if k else "(entry)")
            break
    else:
        print(f"traces agree for {min(len(original), len(ported))} blocks "
              f"(original {len(original)}, rust {len(ported)})")
    keep = [l for l in out.splitlines() if "failures" in l or "panicked" in l or l.startswith("  [")
            or l.startswith("0x") or "want regs" in l]
    print("\n".join(keep[:20]))


if __name__ == "__main__":
    main()
