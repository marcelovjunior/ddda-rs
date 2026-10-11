# L09: vectors for the functions random inputs cannot drive

Owner: Codex (same session), after R01. Branch `task/L09-structured-inputs`
from the head's `main`, worktree `E:\ddda-rs-wt\L09`.

## Why

36,793 of 37,728 non-platform functions lift, but only about 31,600 are
verified: roughly 3,200 have no usable vector (`patterns/lift_novector.json`
plus the per-task `unverified-*.json` lists). The typical reasons, all
rejections in `harness.run_sparse`:

- a loop whose count comes from random memory, over random pointers or a
  virtual call per element: "touches over 256 pages", "runaway", minutes
  per seed (the 20-second budget now cuts them off);
- a mocked allocator or callee returns a random pointer that is then used
  as an array base with a random length;
- execution reaches code outside the image through a random function
  pointer that is not a call (stray returns are already rejected).

Every case is still a sound test when it is *some* valid input; the inputs
only have to be plausible enough to finish.

## Task

1. Classify the no-vector functions by rejection reason (record a few
   seeds per function and keep the reasons).
2. Add input strategies to the recorder, each opt-in and recorded in the
   vector (like `zero_inputs`), for example: small integers instead of full
   32-bit random values for words that are used as counts or indices
   (decide per load, e.g. a seeded mix of small and large values); pointer
   fields that point into a fresh zeroed or small-valued object region;
   mocked returns that are small counts or valid pointers. Keep everything
   deterministic per seed. A vector is valid whatever the inputs were; the
   replay checks the translation, not the inputs.
3. Re-record the no-vector functions with the new strategies (time budget
   per function, worker limits as before), adopt what passes, and report
   per strategy how many functions it unlocked.

## Rules

As in L08 (`docs/tasks/L08-lifter-integer-sse.md`, "Rules"). Note the
recorder pitfalls fixed in main (docs/NATIVE_PIPELINE.md: the code-hook
size sentinel, the TCG buffer cap, worker recycling, the per-function time
budget).

## Results

Implemented three deterministic, opt-in recorder distributions:
`small-v1`, `zero-objects-v1`, and `objects-v1`. They supply small aligned
register/return values, small words, and bounded synthetic object pointers
with zero or mixed small/pointer fields. Each successful vector records its
strategy. These are fixture distributions, not inferred game state or game
constants. Pages are filled once before access; original image bytes and
instructions, write tracking, return checks, flag/x87 checks and all existing
rejection limits remain intact. A wall-clock deadline now also rejects at
instruction boundaries, without resuming after a fault.

`research/native/structured_inputs.py` prepares missing candidates against
the existing closed-callee set, records per-seed rejection reasons, resumes
strategy-specific JSONL logs, and provisionally adopts successes. It checks
that the prepared closed boundary has not changed, preserves accepted
records, accumulates adoption history, and prunes accepted addresses from
the historical no-vector cache. It refuses the shared native output path.
The L10 closed-contract restoration helper is shared rather than duplicated;
every decoded body is released immediately, including on rejection.

The prepared historical no-vector list contained **4,605** addresses, not
the older estimate of 3,200. Scanning the private copy of the expanded
inventory (including its 6,094 supplementary entries) found **5,244** missing
liftable candidates against the existing closed set. Another **971** entries
reject during lifting, five platform model addresses are excluded, and four
known mismatches remain quarantined. This is a missing-entry scan preserving
the old selection, not a complete new closed-call fixed point.

Three random seeds per candidate produced **15,732 attempts**. Of the 5,244
functions, 1,317 already produced cases with the current ext call boundary.
For the 3,927 without cases, their most frequent rejection was:

| Primary rejection per function | Functions |
|---|---:|
| Execution outside the image | 1,581 |
| Over 256 pages | 1,153 |
| Runaway | 672 |
| NaN/Inf input | 364 |
| Oversized memcpy / memset model operation | 124 |
| Two NaN operands | 14 |
| Undefined integer flags | 8 |
| Return address overwritten | 4 |
| x87 stack fault / noreturn / CPU exception | 7 |

All five passes used three seeds and a 20-second budget per function. Each
later pass tried only the remainder, so these gains are disjoint and depend
on the stated order. **Every addition passed Rust replay.**

| Strategy, in order | Functions tried | Newly passing functions | New cases |
|---|---:|---:|---:|
| Current random inputs | 5,244 | 1,317 | 3,781 |
| Existing zero-input mode | 3,927 | 2,435 | 7,305 |
| New small words | 1,492 | 85 | 152 |
| New zero-filled objects | 1,407 | 13 | 39 |
| New mixed pointer/count objects | 1,394 | 21 | 31 |
| **Total** | | **3,871** | **11,308** |

The new distributions account for **119** additions beyond random/zero;
the other 3,752 come from retrying the complete missing selection with the
current call boundary and existing input modes. They are not attributed to
the new distributions. Six newly lifted functions were already verified by
hand ports, so the inventory gains **3,865**, not 3,871, verified entries.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted functions | 37,641 | 41,512 |
| Passing lifted cases | 112,266 | 123,574 |
| Inventory verified functions | 37,651 | 41,516 |
| Verified share of 56,572 non-thunk entries | 66.6% | 73.4% |
| Verified summed code bytes | 11,379,753 | 13,306,640 |
| Verified share of summed code-size estimates | 73.4% | 85.8% |
| Missing liftable candidates without cases | 5,244 | 1,373 |
| Historical no-vector list | 4,605 | 1,370 |

All 37,641 baseline function records and 112,266 baseline cases are retained
unchanged. The old no-vector list is not exhaustive: 1,365 of its remaining
addresses are in the final 1,373-candidate remainder, five are outside that
remainder, and eight remaining candidates were not in the historical list.
Use `structured-unverified.json` and the scan rejection file for follow-up.

Notable new sampled replay coverage includes `aGameMain::vf06`
(`0x004FB3E0`, random retry), `uCnsEdit::vf24` (`0x00860AC0`), and
`cCamImpNml::vf11` (`0x007C8880`). This verifies sampled paths with external
calls mocked as recorded; it does not demonstrate a working frame or game
boot. `sBbsRpgMain::vf06` at `0x0041DD60` and initializer `0x0137E490` still
lack usable fixtures. In the last strategy pass, primary reasons among the
1,373 remaining candidates were outside-image execution (1,118), runaway
(134), NaN/Inf input (78), and other guarded failures (43).

**Tests and resources:** eight new Python regressions, all 32 existing lifter
regressions and seven L10 tooling regressions pass (**47 total**). Synthetic
tests cover repeatability, bounded distributions, counted loops, overlapping
loads/writes, unchanged image inputs, recorded mock returns, invalid option
combinations, stray-return rejection, deadlines, failure provenance, stale
boundary rejection, adoption idempotence and failure-cache cleanup.

The addition/dependency subset passed **53,677 cases / 18,057 functions**
and seven unit tests (1m19s compilation, 17.28s replay). One final full
`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative` passed **32 Rust tests**,
including all **123,574 lifted cases** and seven unit tests. Legacy hand-port
tests keep their existing fixture-dependent behavior; required lifted replay
uses the private data and cannot skip. No live-game comparison was performed.

The final full build used four jobs, incremental off: **4m25s compilation**,
**55.64s lifted replay**, **328.06s total**, **14.94 GiB minimum observed free
RAM**. Six vector workers, recycled after eight functions, stayed above
22.59 GiB free; the missing scan took 36s with bodies released immediately.
No build/vector overlap, memory-threshold crossing, gate invocation, shared
inventory/native writes, Ghidra restart, or push occurred.

**Handoff:** the final matching `patterns/lift.json` and `vectors/lift.json`,
baseline backups, candidates, scan rejections, quarantine list, per-strategy
attempt logs/adoption lists, classification summary, remaining-candidate
report, private rebuilt inventory and resource logs are under
`E:/ddda-re/notes/native-L09`. Head can adopt that pair and regenerate on
main, then rebuild the shared inventory in its normal gate. Only source,
tests and docs are committed; generated worktree files remain uncommitted.
See `docs/NATIVE_PIPELINE.md` for repeatable prepare/record/adopt commands.

**Open:** investigate the 1,373 recorder failures without relaxing guards,
and the 971 lift rejections separately. The existing quarantine remains
`0x00C1D790`, `0x00D9EF20`, `0x00FB9A10` (bad-return fixtures) and
`0x00F276C0` (known Unicorn/CPU large-argument FSIN difference). No previously
quarantined case was reintroduced. Next action: review/adopt this verified
pair, then trace representative remaining outside-image failures.
