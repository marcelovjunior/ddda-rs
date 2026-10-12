"""Time one build or replay command and retain its inputs and exit status.

Run this inside heavy.py for any command that compiles shards, e.g.:
  heavy.py codex -- python scripts/build_speed.py --label shard05-opt1 \
    --output E:/ddda-re/notes/L66 -- cargo rustc --profile play -p ddlift-05 --lib
Receipts contain hashes and timings only; compiler output stays beside them.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def source_digest(root):
    digest = hashlib.sha256()
    count = 0
    paths = [root / "Cargo.toml", root / "Cargo.lock"]
    for folder in ("crates/ddlift", "crates/ddnative-base"):
        paths.extend((root / folder).rglob("*.rs"))
        paths.extend((root / folder).rglob("Cargo.toml"))
    for path in sorted(paths):
        if path.is_file():
            digest.update(path.relative_to(root).as_posix().encode())
            digest.update(b"\0")
            digest.update(path.read_bytes())
            count += 1
    return {"sha256": digest.hexdigest(), "files": count}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a command is required")
    if Path(args.label).name != args.label or any(c in args.label for c in '/\\:'):
        parser.error("label must be a file name")
    root = Path(__file__).resolve().parents[1]
    args.output.mkdir(parents=True, exist_ok=True)
    receipt = {
        "command": command, "sources_before": source_digest(root),
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "environment": {key: os.environ.get(key) for key in (
            "CARGO_TARGET_DIR", "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS", "RUSTFLAGS",
            "RUSTC_BOOTSTRAP", "RUST_MIN_STACK", "LIFT_DATA")},
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
    }
    start = time.perf_counter()
    with (args.output / (args.label + ".log")).open("w", encoding="utf-8") as log:
        process = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True, encoding="utf-8", errors="replace")
        for line in process.stdout:
            log.write(line)
            log.flush()
            print(line, end="", flush=True)
        receipt["exit_code"] = process.wait()
    receipt["seconds"] = time.perf_counter() - start
    receipt["sources_after"] = source_digest(root)
    receipt["sources_unchanged"] = receipt["sources_before"] == receipt["sources_after"]
    (args.output / (args.label + ".json")).write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(f"{args.label}: {receipt['seconds']:.3f} s, exit {receipt['exit_code']}, unchanged={receipt['sources_unchanged']}")
    return receipt["exit_code"]


if __name__ == "__main__":
    raise SystemExit(main())
