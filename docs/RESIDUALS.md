# Residual functions

Inventory snapshot: **train 15, main `17add4c7`**, 59,391 / 59,440
non-thunk functions verified; **49 remain (0.082%)**.

One row per non-verified inventory address, sorted by address. Supplementary
entries count only when admitted to inventory; thunks are excluded. Private
cases remain here until a head integration train credits them. Guard reasons
describe attempted inputs, not a unique cause or invalid game code.

Sources: `scripts/inventory.py summary` and read-only query
`SELECT address,status FROM functions WHERE NOT thunk AND status != 'verified'`;
`native/patterns/verification_gaps.json`; train-15 native gate;
[L28 Results](tasks/L28-crt-platform-class.md#results),
[L49 Results](tasks/L49.md#results), L50 owner messages #9784/#9823
(a395f418), and the
[L21 cross-entry census](tasks/L21-boot-bring-up.md).
Private row evidence: `native-L28c/L28c/residual-census-train15.json`.
0x00786D69 is a stale-body input task assigned to Codex but is not an admitted
inventory address; it remains outside this denominator.

L50 also flags currently verified `0x007128B0`, `0x00712D90` and
`0x00A68400` for possible input refresh after its x87 contract change;
these are outside the table until inventory marks them non-verified.

Ownership: **3 claude-6 (gap)**, **12 claude-6 (lifter)**, **2 claude-6 (lifter/oracle)**, **32 codex (recorder)**.
**23 rows have private cases awaiting replay.** Boot stops take priority.
**9 recorder rows are set aside** with evidence; two inherited-frame rows moved to the lifter owner (#9859).
Set-aside rows retain their blocker and the evidence needed to resume.

| Address | Class | Blocker | Owner |
|---|---|---|---|
| 0x00410000 | Recorder: replay pending | 3/3 private one-call-v1 cases; ordinary mock EAX=1 inputs; original instruction limit retained. Head replay pending. | codex (recorder) |
| 0x004644D0 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00493AA0 | Recorder: set aside / setup mismatch | Set aside: Both setup orders change accepted observations: manager-first changes destination reads/writes/MXCSR; destination-first changes manager reads/writes/calls and ECX/EDX. Requires a compatible producer sequence. | codex (recorder) |
| 0x004A0C00 | Recorder: set aside / object/manager state | Set aside: Checked 437BE0 + 489F80 setup still leaves linked-object facts unknown at branch 4027E9 after 92 proof instructions (0/3). Requires evidence for the linked object/callee state. | codex (recorder) |
| 0x004EE720 | Recorder: set aside / instruction bound | Set aside: Finite proof stops at first indirect CALL EDX, 4EE736: outgoing argument count is not established before the first root call. EBP-frame/scalar-one trials each hit the instruction limit (3/3). Requires a checked dispatch/argument contract and a complete finite trace. | codex (recorder) |
| 0x005652E0 | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x00565308 | Lifter: cross-entry boundary | L50 decodes this inherited EDI/ESI frame inside 0x005652E0, now recorded 3/3. Owner #9859 confirms inventory credit through the parent; no standalone fixture needed. Existing 565308 record remains unchanged for head disposition. | claude-6 (lifter) |
| 0x0056532A | Lifter: cross-entry boundary | L50 decodes this inherited EDI/ESI frame inside 0x005652E0, now recorded 3/3. Owner #9859 confirms inventory credit through the parent; no standalone fixture needed. Existing 565308 record remains unchanged for head disposition. | claude-6 (lifter) |
| 0x00689960 | Recorder: set aside / floating comparison | Set aside: Finite proof stops at COMISS XMM0,[EAX+4] (next PC 68D945). EBP-frame/scalar-one trials each hit the instruction limit (3/3). Requires justified floating comparison facts; no numeric branch is guessed. | codex (recorder) |
| 0x00691AE0 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00691E50 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00692A40 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x006F3242 | Lifter: cross-entry boundary | L50 refreshed owner 0x006F3230 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x007126C0 | Recorder: replay pending | 3/3 private tail-frame-v1 cases; matching fixed local cleanup before every return or external tail jump. Head replay pending. | codex (recorder) |
| 0x00736370 | Inventory admission | L22 identifies an address-taken ASCII tag, not code; resolve inventory admission (current decode rejects BOUND). | claude-6 (lifter) |
| 0x007BEA60 | Recorder: replay pending | 3/3 private ebp-call-v1 cases; aligned EBP locals with stable frame pointer and matching return restores. Head replay pending. | codex (recorder) |
| 0x00815C60 | Recorder: set aside / floating comparison | Set aside: Checked constructor proof stops at COMISS 816522 after 9,319 instructions; zero-state proof also stops at that comparison. EBP-frame/scalar-one trials hit the instruction limit (3/3 each). Requires justified floating comparison facts. | codex (recorder) |
| 0x00947493 | Lifter: cross-entry boundary | L50 refreshed owner 0x009472C0 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x009897C0 | Lifter/oracle: x87 state | x87 stack fault before fild at 0x009899d0 | claude-6 (lifter/oracle) |
| 0x009921E0 | Verification gap | unproven table at 0x00992713 | claude-6 (gap) |
| 0x009D0E70 | Recorder: replay pending | 3/3 private ebp-call-v1 cases; aligned EBP locals with stable frame pointer and matching return restores. Head replay pending. | codex (recorder) |
| 0x00A195D0 | Recorder: replay pending | 3/3 private ebp-call-v1 cases; aligned EBP locals with stable frame pointer and matching return restores. Head replay pending. | codex (recorder) |
| 0x00A2F3B0 | Verification gap | unproven table at 0x00A2F4A5 | claude-6 (gap) |
| 0x00B75EC0 | Recorder: replay pending | 3/3 private outgoing-call-v1 cases; entry outgoing allocation consumed by a final indirect call on every finite path. Head replay pending. | codex (recorder) |
| 0x00BD45C0 | Recorder: replay pending | 3/3 private ebp-call-v1 cases; aligned EBP locals with stable frame pointer and matching return restores. Head replay pending. | codex (recorder) |
| 0x00C1D790 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x00CED240 | Recorder: replay pending | 3/3 private null-call-v1 cases; generated memory selects a decoded null branch that clears EAX and returns with checked cleanup. Head replay pending. | codex (recorder) |
| 0x00CEEBC0 | Recorder: replay pending | 3/3 private exception-frame-v1 cases; inline exception-frame pushes excluded after matching entry links and every return unlink. Head replay pending. | codex (recorder) |
| 0x00D29AF0 | Recorder: set aside / allocator state | Set aside: Nine exact caller replays pass; all target runs hit the instruction limit, as do three finite-proof trials from captured state. The earlier manager recipe lacks a usable allocator at D29B4C. Requires a complete allocation/callee bound; no replacement fixture adopted. | codex (recorder) |
| 0x00D9EF20 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x00DA46C0 | Recorder: replay pending | 3/3 private constructed-v1 cases; checked original setup replay supplies method state; every accepted setup observation reproduced. Head replay pending. | codex (recorder) |
| 0x00E0D750 | Recorder: replay pending | 3/3 private one-call-v1 cases; ordinary mock EAX=1 inputs; original instruction limit retained. Head replay pending. | codex (recorder) |
| 0x00F83C00 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x00FB9A10 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x01137DF3 | Lifter: cross-entry boundary | L50 refreshed owner 0x01137DD0 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x01146420 | Lifter: cross-entry boundary | L50 refreshed owner 0x01146410 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x012A5B30 | Recorder: set aside / floating comparison | Set aside: Opaque shuffle rule advances the proof to COMISS 12A47C0 in closed callee 12A4320 after 105 steps; all three guarded-values trials hit the ordinary instruction limit. Requires justified floating comparison facts. | codex (recorder) |
| 0x012E7CE0 | Verification gap | unproven table at 0x012E7D24; unproven table at 0x012E7E06 | claude-6 (gap) |
| 0x012EFA1E | Recorder: set aside / continuation state | Set aside: Restores ESP from [argument 2 - 4], EBP from argument 2 + 12, unlinks FS:[0], then jumps to argument 1. Requires an evidenced exception continuation frame and destination; ordinary independent-entry inputs cannot supply that contract. | codex (recorder) |
| 0x012F693F | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x012F6948 | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x012F6A6F | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x012F6A78 | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x012FF0F0 | Recorder: replay pending | 3/3 private borrowed-frame-v1 cases; paired temporary EBP saves restored on every path before LEAVE; direct-call cleanup consumes only tracked arguments. Head replay pending. | codex (recorder) |
| 0x01300600 | Lifter/oracle: x87 state | unmasked x87 exceptions unsupported | claude-6 (lifter/oracle) |
| 0x01300DFF | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
| 0x01314A90 | Recorder: set aside / floating comparison | Set aside: Opaque exchange rule advances the proof to FCOMP ST(3), 1314B7B, after 108 steps; all three guarded-values trials hit the ordinary instruction limit. Requires justified x87 comparison facts for the nested numeric loops. | codex (recorder) |
| 0x013156B0 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x01329888 | Recorder: replay pending | 3/3 private zero-call-v1 cases on the L50 refreshed body; actual continuation and frame restoration execute. Inferred x87 entry values supplied where required. Head replay pending. | codex (recorder) |
