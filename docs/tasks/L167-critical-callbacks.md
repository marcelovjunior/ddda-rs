# L167: critical callback ranking and exact shadow state save

Owner: codex-7. Branch `task/L167-critical-callbacks`, from FPS line with
L158 `394d8393` and L162 `de93eea8` merged. Assigned by head/user on
2026-10-13. No game, GPU, shard generation or shard build by this agent.

## Results: which callback delays which join

The L162 validated per-batch outputs cover h0 frames 6600..6749 and h1
6819..6968, 150 frames each, **10,350 actual main joins each**, with zero
rejected selected batches. `scripts/critical_callbacks.py` retains every
join's frame, batch, join ordinal and interval, last callback target,
candidate virtual target, vtable/class, thread, inclusive duration, and
solo interval intersecting that join. Four regression tests reject loss,
missing frames, duplicate batches, absent provenance and inconsistent
overlaps. It does not infer CPU self time from callback duration.

| Callback / candidate virtual target | DTI class | h0 mean final callback ms | h0 solo ms/frame | h1 solo ms/frame |
|---|---|---:|---:|---:|
| DB7620 / EE2C90 | uSkyStableShadow | 13.420 | 3.905 | 3.651 |
| 4B9FF0 / C70790 | uSoundGenerator | 0.555 | 0.466 | 0.418 |
| 4B9FF0 / BAF4F0 | uNpc | 2.817 | 0.460 | 0.800 |
| FED7E0 / none | cDraw | 0.298 | 0.247 | 0.217 |
| 4B9FF0 / B53650 | uCmc | 1.827 | 0.240 | 0.098 |
| 4B9FF0 / 5D12B0 | uPawnIntel | 1.291 | 0.195 | 0.134 |
| 1090F70 / AA9FA0 | uEm8300 | 1.008 | 0.190 | 0.173 |
| 1090F70 / 678210 | uGUILifeGauge | 0.562 | 0.189 | 0.212 |

Shadow is last in 150/150 frames in both windows, with a positive solo
overlap in 123/121. A zero solo overlap may mean main finishes its own
callback later; it does not mean the shadow callback was cheap. These
are older hash0/hash1 captures, **not** an A/B of the new state-save port.
Scene counts differ; shared-host load and instrumentation remain caveats.

Largest h0 joins, all DB7620 / EE2C90, vtable `0160DF98`, 297 queued jobs:

| Frame | Batch (join ordinal 0) | Callback ms | Solo overlap ms |
|---:|---:|---:|---:|
| 6631 | 298319 | 18.0920 | 7.7299 |
| 6663 | 301007 | 18.5104 | 7.5595 |
| 6727 | 306383 | 14.8048 | 7.4366 |
| 6745 | 307895 | 14.9559 | 7.3433 |
| 6732 | 306803 | 15.3923 | 7.0841 |

All 20,700 per-join rows are private in `native-L167/h0-callbacks.json`
and `h1-callbacks.json` under `E:/ddda-re/notes`. Inputs are L162's
`h0-6600-6749.json` / `h1-6819-6968.json`; their hashes are
`1f9d1c8e28ef0fd6af3893806716d17f131434f491ea3be7c8fc326fccc6a0af` /
`d5ade21c9c1323e822fe2e75bcaedb7e1cb5c36c403d650dff09c6730c59bc2e`.
Original decompressed trace hashes remain those in the L162 brief.

## Source path and selected optimization

Read-only Ghidra plus DTI identifies the possible path:
DB7620 -> EE2C90 (`uShadow::vf11`) -> vtable+60 EFEF80 -> EFDC70 ->
EFD240 -> sRender virtual+40. DB7620's other branch BFC7D0 returns
immediately. Candidate virtual identity alone still cannot prove which
branch ran for each captured invocation.

Independent c26vk stacks containing the shadow routines have nearest
visible functions FEE320 14, EA5B50 9, E12A20 8, **FED480 8**, and
10658B0/1066590 7 each. These counts are supporting path evidence, not
per-join samples or percentages. Seven FED480 samples explicitly pass
through FFE5C0 -> 886880 -> C52D70 and shadow dispatch. Larger material
functions remain candidates; no shortcut in shadow traversal is justified.

`mtcore::job_shadow::save_state` ports the **whole FED480 function**.
The partial-save branch performs 81 sequential 32-bit copies through
existing checked `Pages`, replacing the generic virtual `Mem` copy loop.
DF, per-word atomic width, overlap, guest spills, register outputs, final
flags, and reloads between six selected state words remain intact. The six
keys at `01447B44..01447B58` are read from the player's loaded image, not
embedded. The full-save branch calls existing memcpy at `012EFEA0` with
the same guest stack/return site; no memcpy port is included. Scheduler,
callback order and work partitioning are unchanged.

Independent switch **`DDRUNTIME_JOB_SHADOW_SAVE=1`**, default off, adds
only FED480 to ddports. It composes with JOB_HASH and JOB_LOOKUP; no shard
regeneration is needed. No whole-callback or FPS gain is claimed yet.

## Isolated proof and timings

- **432 original-instruction returns** match port and isolated lifted body
  separately. Both branches, all byte truth values represented by 0/1/255,
  both DF directions, four alignments including a page-crossing word,
  self-copy, source/stack aliasing, wrapping multiplication and signed INC
  overflow indices, randomized registers/keys/data, four MXCSR modes.
- The unowned memcpy boundary has explicit synthetic byte-copy effects,
  EAX=destination and otherwise preserved registers/flags. Call-entry
  registers and defined IMUL flags are checked, as are all final written
  bytes. This proves the caller's seam, not original memcpy implementation.
- **68,200 paired injected byte-access positions** compare port/lifted
  fault/completion, ordered accesses, writes, registers and modeled flags.
  Intermediate dead flags follow the existing verified lifted contract;
  final defined flags are checked against original instructions.
- Both bodies also replay all vectors against production `GuestMemory`
  in serial and atomic modes. Its module is compiled privately unchanged
  except an inactive preemption seam that panics if reached. Four isolated
  tests pass; no shards enter this build.
- Partial-save benchmark: lifted body opt1, port/base/production memory
  opt3, atomic accesses, 10,000 calls/sample, alternating order, seven
  pairs per setting. **56/56 pairs faster; median 1.47–1.87x** across eight
  alignment/direction settings. CPU reset and index reset occur equally
  inside both timings. Full-copy branch is verified, not benchmarked.

| DF / alignment | Lifted median ns | Port median ns | Median paired ratio |
|---|---:|---:|---:|
| forward / aligned | 439.25 | 237.26 | 1.81x |
| forward / +1 | 589.78 | 346.77 | 1.61x |
| forward / page edge | 544.33 | 376.11 | 1.47x |
| reverse / aligned | 419.63 | 222.50 | 1.87x |

Private vector `native-L167/job_shadow.json` SHA-256:
`8e8e61660a79c3a8ce6baf7bf800e76fd491a14aef51bd4a57c6bb2cc2254c49`.
Original FED480 code-range SHA-256:
`0ad6337881b298da612b7d7287dbbdc8a9fca72e48e0f231b3ee8095e7533e6b`.
Isolated log `E:/ddda-re/notes/L167-isolated-final.log` SHA-256:
`c75f3da247dc7689c30385ca412e8d0154c6d06a35bc5fd8af6cee8d8a5729b0`.

Reproduce with research Python `research/native/record_job_shadow.py`,
then `python scripts/check_job_shadow.py --bench`. The script pins its
private build target to `C:/ddda-targets/L167`, jobs=4, incremental=0.
Ranking: `python scripts/critical_callbacks.py PRIVATE-L162.json
--dti E:/ddda-re/notes/dti-registry.json --out PRIVATE-output.json`.

## Train acceptance remaining

Final local verification: `cargo test -p mtcore -p ddports` with required
vectors and SHADOW_SAVE=1 reports **20 passed / nine ignored**. Explicit
registration 1/0 rechecks each pass. `cargo test -p ddruntime
--no-default-features --no-fail-fast` reports **251 passed / zero failed /
seven ignored**, exit 0; jobs=4, incremental=0, target L167. Private light
log SHA-256:
`e2f06b95ca0dc0ab6d0e61160fa8062ff6fded70d42447c235eca30cd5ec84bd`.
Inventory credit/counts await the head's combined gate; no verification
of the entire shadow callback is implied by the FED480 corpus.

Head's one combined native gate must include JOB_SHADOW_SAVE=1 alongside
the other accepted candidates. Then head captures shadow-save 0/1 with
same route, hash/lookup/participants/sync switches and binary, recording
per-join shadow duration/solo overlap plus counts. Separately use profile
off for FPS/image acceptance. No measured callback speedup, image parity
or safe callback splitting is established by this isolated leaf proof.
