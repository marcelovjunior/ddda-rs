# L165 worker-code follow-up

Owner: codex-6; branch `task/L165-load-next`. Head board assignment #13500
supersedes loading work with three exact, independently opt-in worker ports.
No game runs or shard builds by this agent. Loading/autoplay results remain
in `L165-load-next.md` and their separate commits.

## Rank and scope

Six c26vk worker threads 0x1070..0x1084 have 4,014 samples: 2,334 recognized
OS waits and 1,680 other samples; 1,071 remain outside the excluded
DB7620/EA3130/EA5320/FF1920/FEE320/E12A20/E0E5E0 chains. Unknown frames
remain in the denominator. DBDC90's 211 nearest-lifted samples include
129 host waits and 21 unresolved ntdll frames; this is scheduling rather
than a new arithmetic port. Outside excluded chains, c26 nearest/inclusive:
1054700 39/124, FEDD60 29/29, D7CF70 21/47, 10A35D0 20/131.

Latest c31def+c31p8 nearest-lifted counts rank the chosen candidates:

| Function | c31def | c31p8 | Sum | Work |
|---|---:|---:|---:|---|
| FEDD60 | 17 | 40 | 57 | stable pair-key merge sort |
| 1054700 | 18 | 18 | 36 | joint-index dispatcher |
| 10A35D0 | 13 | 22 | 35 | four-way bounds traversal |

These text profiles cannot exclude whole caller chains; c26 raw stacks can.
D7CF70 has only 18 nearest samples in the latest pair. FEFB90 is omitted:
all its c26 nearest samples occur under excluded chains. Wrapper inclusive
counts are not added to CPU time. In particular 1054700 contains no SSE
math: its evaluator callbacks remain original, and unnamed helper time
must not be credited as dispatch savings.

Private input SHA-256:
- stacks_c26vk.json: `745cf2d17b508d8d771da9ac87131d53eec6560101ab1dd2aaf0f57c25b62036`
- profile_c31def.txt: `9555e7f829dd031705960067b2a1cc16716fbaa8563ef7ade569014a93bdfd9c`
- profile_c31p8.txt: `4af1ecff0e82a301f1ca160ff7ec0665b02df27a6ce22295037890015170fa14`

## Results: 1054700

`DDRUNTIME_JOB_JOINTS=1` replaces only the dispatcher. Structured loops
preserve original callbacks, return PCs, guest stack spills, rereads after
callbacks, register results and defined flags. No game policy changes.

Original-instruction Unicorn oracle: 840 returns / 5,092 callback entry
and return boundaries, covering five counts, seven model-flag choices,
four input modes, pending initialization and disabled/filter patterns.
Evaluators are explicitly synthetic ABI callees that clobber volatile
registers/flags and mutate later joint memory; no evaluator equivalence
claim is made. Return GPR/XMM/MXCSR, defined flags and all written bytes
match. Rust checks preserved x87 state. Both original lifted body and port
pass sparse Mem and atomic page-backed replay (four tests).

Isolated cost uses the original lifted body at opt-level 1 versus the port
at opt-level 3, matching integration compilation policy, atomic pages,
five counts 1..128, two modes, skipped/enabled joints, seven alternating
pairs each. All 20 median ratios exceed 1.5: **1.545–2.476x**; individual
pairs range **1.180–2.751x** on the shared machine. Every invocation resets
the mode argument which the original overwrites. This synthetic dispatch
benchmark excludes actual evaluator/callback cost and proves no FPS gain.
Opt-in registry uniqueness/mod precedence crate tests pass.

Reproduce without shards:
```
E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_job_workers.py
python scripts/check_job_workers.py --bench
```
Artifacts stay private. Corpus `E:/ddda-re/notes/native-L165/job_joints.json`
SHA-256 `cb44b2b92f4818b14ea23620655d838911e479f63cf99ac7b5498185401a73c6`.
Final log `C:/ddda-targets/L165/joints-gate.log` SHA-256
`72cfa01ad673c74d4b04f958744f6ab7f05582365a56ad690a2e8a83b6b81ea3`.

Not yet checked: live concurrent mutation, fault-site machine states,
combined native gate or gameplay image/per-worker speed. No default change.
Next: exact FEDD60 sort port and 10A35D0 traversal; then head integration
train with each opt-in separately and a matched route/camera.
