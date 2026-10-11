# L01: lifter: switch-table indirect jumps

Owner: Codex (one agent, no subagents). Head: Claude. Branch `task/L01-switch-tables`,
worktree `E:\ddda-rs-wt\L01`. Never push, never touch `main`.

## Direction (read first)

The project no longer hand-builds features toward a demo. Every function of
DDDA.exe is translated 1:1 into Rust by `research/native/lift.py` (x86 to
literal Rust over `ddnative::mem::Mem`, one statement group per instruction)
and verified against the original exe by automatic differential vectors
(Unicorn, `research/native/harness.py: run_sparse`). Read
`docs/NATIVE_PIPELINE.md` ("The lifter", "Platform models", "Pitfalls") and
`AGENTS.md` before changing anything. 12,308 functions are verified today.

## Task

`lift()` rejects every indirect `jmp` ("indirect", about 2,100 functions).
Most are switch tables: `jmp dword ptr [reg*4 + TABLE]` with TABLE in the
image, often preceded by a bounds check and sometimes a byte index table
(`movzx reg, byte ptr [reg + INDEX]`). Make these lift:

1. Recognise switch-table jumps strictly: read the jump table from the image
   (`harness.code`), find its entries (stop at the first entry outside the
   function; check the bounds-check constant when present), require every
   entry to be an instruction start inside the function, and add them as
   basic-block leaders. Anything else stays rejected.
2. Emit Rust that reads the target through `Mem` exactly like the CPU
   (`pc = m.u32(address); continue;`), so the vectors record the table read.
   The `match pc` arms already dispatch; an unexpected target must panic,
   not fall through.
3. Count what changed: rejection reasons before/after.

## Rules

- Exact semantics only; the vectors and tests judge. No special cases
  without a vector proving them.
- Your private data: `set LIFT_DATA=E:/ddda-re/notes/native-L01` (create it;
  copy `E:/ddda-re/notes/native/patterns/lift_novector.json` there if you
  want). Never write to `E:/ddda-re/notes/native` itself: the head's
  16k-function run is using it. The inventory DB is read-only for you.
- Do NOT run `scripts/native_gate.sh` (it rebuilds the shared inventory).
  Verify with: `lift.py scan`, `lift.py vectors`, `lift.py gen`, then
  `DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative --test lifted` with
  `LIFT_DATA` set. A smaller list is fine for iteration (e.g. only the
  newly lifted functions plus their callees); the final check should cover
  every function your change newly lifts.
- CPU: the head runs heavy jobs. Use `LIFT_WORKERS=2` and
  `CARGO_BUILD_JOBS=4`. Disk is tight (E: about 25 GB free): one target
  dir, clean up anything large you create outside it.
- Commit only source and docs (`research/native/*.py`,
  `crates/ddnative/src/x86.rs` if needed, docs). Do NOT commit
  `crates/ddnative/src/generated/*` or `tests/lifted.rs`: the head
  regenerates them after merging. Commit with
  `git -c user.name=janne -c user.email=marcelovjunior55@gmail.com commit`,
  no AI co-author trailer.
- Document the new rule in `docs/NATIVE_PIPELINE.md` (lifter section and
  "Extending it"), including anything a future agent would trip over.

## Results

(Codex fills this in: what lifts now, rejection counts before/after, test
result, commits.)
