# Residual functions

Inventory snapshot: **train 14, main `1ed8c4aa`**, 59,391 / 59,440
non-thunk functions verified; **49 remain (0.082%)**.

One row per non-verified inventory address, sorted by address. Supplementary
entries count only when admitted to inventory; thunks are excluded. Private
cases remain here until a head integration train credits them. Guard reasons
describe attempted inputs, not a unique cause or invalid game code.

Sources: `scripts/inventory.py summary` and read-only query
`SELECT address,status FROM functions WHERE NOT thunk AND status != 'verified'`;
`native/patterns/verification_gaps.json`; train-14 native gate;
[L28 Results](tasks/L28-crt-platform-class.md#results),
[L49 Results](tasks/L49.md#results), L50 owner messages #9784/#9823
(a395f418), and the
[L21 cross-entry census](tasks/L21-boot-bring-up.md).
Private row evidence: `native-L28c/L28c/residual-census-train14.json`.
0x00786D69 is a stale-body input task assigned to Codex but is not an admitted
inventory address; it remains outside this denominator.

L50 also flags currently verified `0x007128B0`, `0x00712D90` and
`0x00A68400` for possible input refresh after its x87 contract change;
these are outside the table until inventory marks them non-verified.

Ownership: **3 claude-6 (gap)**, **10 claude-6 (lifter)**, **2 claude-6 (lifter/oracle)**, **34 codex (recorder)**.
**16 rows have private cases awaiting replay.** Boot stops take priority;
0x00493AA0 remains set aside.

| Address | Class | Blocker | Owner |
|---|---|---|---|
| 0x00410000 | Recorder: replay pending | 3/3 private one-call-v1 cases; ordinary mock EAX=1 inputs; original instruction limit retained. Head replay pending. | codex (recorder) |
| 0x004644D0 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00493AA0 | Recorder: setup mismatch | Set aside: manager-first changes destination reads/writes/MXCSR; destination-first changes manager reads/writes/calls and ECX/EDX. Neither setup order passes exact replay. | codex (recorder) |
| 0x004A0C00 | Recorder: object/manager state | Checked 0x00437BE0 + 0x00489F80 setup still stops at branch 0x004027E9 after 92 instructions; linked-object input/proof facts unresolved. | codex (recorder) |
| 0x004EE720 | Recorder: instruction bound | EBP-frame and scalar-one trials each reach the instruction limit in 3/3 attempts; generated state still needs a finite trip-count proof. | codex (recorder) |
| 0x005652E0 | Recorder: dispatch/return | Selected body branches into the continuing saved-register frame at 0x00565308; boundary refresh must precede valid frame inputs. | codex (recorder) |
| 0x00565308 | Recorder: stale-body inputs | Stale body omits 13 instructions from 0x0056532A; refreshed-body recording leaves the image. Resolve inputs before refreshing accepted cases. | codex (recorder) |
| 0x0056532A | Recorder: dispatch/return | Continuation of the 0x00565308 body with inherited saved registers; ordinary independent-entry inputs do not restore the caller frame. | codex (recorder) |
| 0x00689960 | Recorder: instruction bound | EBP-frame and scalar-one trials each reach the instruction limit in 3/3 attempts; generated state still needs a finite trip-count proof. | codex (recorder) |
| 0x00691AE0 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00691E50 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x00692A40 | Recorder: replay pending | 3/3 private prefix-call-v1 cases; fixed local frame with interleaved register arithmetic and matching cleanup at every return. Head replay pending. | codex (recorder) |
| 0x006F3242 | Lifter: cross-entry boundary | L50 refreshed owner 0x006F3230 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x007126C0 | Recorder: replay pending | 3/3 private tail-frame-v1 cases; matching fixed local cleanup before every return or external tail jump. Head replay pending. | codex (recorder) |
| 0x00736370 | Inventory admission | L22 identifies an address-taken ASCII tag, not code; resolve inventory admission (current decode rejects BOUND). | claude-6 (lifter) |
| 0x007BEA60 | Recorder: replay pending | 3/3 private ebp-call-v1 cases; aligned EBP locals with stable frame pointer and matching return restores. Head replay pending. | codex (recorder) |
| 0x00815C60 | Recorder: floating comparison | Floating comparison prevents the existing finite proof; EBP-frame and scalar-one trials each reach the instruction limit in 3/3 attempts. | codex (recorder) |
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
| 0x00D29AF0 | Recorder: allocator state | Zero-object inputs divide by zero; three checked-caller attempts from C9DDF0, C9DF30 and C9EDD0 reach the instruction limit. No replacement fixture adopted. | codex (recorder) |
| 0x00D9EF20 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x00DA46C0 | Recorder: replay pending | 3/3 private constructed-v1 cases; checked original setup replay supplies method state; every accepted setup observation reproduced. Head replay pending. | codex (recorder) |
| 0x00E0D750 | Recorder: replay pending | 3/3 private one-call-v1 cases; ordinary mock EAX=1 inputs; original instruction limit retained. Head replay pending. | codex (recorder) |
| 0x00F83C00 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x00FB9A10 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x01137DF3 | Lifter: cross-entry boundary | L50 refreshed owner 0x01137DD0 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x01146420 | Lifter: cross-entry boundary | L50 refreshed owner 0x01146410 with cases executing this continuation; head integration and inventory disposition remain pending. | claude-6 (lifter) |
| 0x012A5B30 | Recorder: instruction bound | Scalar-one trials reach the instruction limit in 3/3 attempts; closed setup/loop callees need valid state and a finite bound. | codex (recorder) |
| 0x012E7CE0 | Verification gap | unproven table at 0x012E7D24; unproven table at 0x012E7E06 | claude-6 (gap) |
| 0x012EFA1E | Recorder: continuation state | Restores ESP from [stack argument 2 - 4], sets EBP from that argument + 12, unlinks FS:[0], and jumps to stack argument 1; requires a valid exception continuation frame. | codex (recorder) |
| 0x012F693F | Recorder: shared-tail inputs | L50 a395f418 includes 0x0130070B in this body; prepare with that rule and record the actual frame restoration. Owner confirmation #9823. | codex (recorder) |
| 0x012F6948 | Recorder: shared-tail inputs | L50 a395f418 includes 0x0130070B in this body; prepare with that rule and record the actual frame restoration. Owner confirmation #9823. | codex (recorder) |
| 0x012F6A6F | Recorder: shared-tail inputs | L50 a395f418 includes 0x0130070B in this body; prepare with that rule and record the actual frame restoration. Owner confirmation #9823. | codex (recorder) |
| 0x012F6A78 | Recorder: shared-tail inputs | Current body mocks the 0x0130070B continuation and returns through saved x87 control data; check the L50 prepared body before recording. | codex (recorder) |
| 0x012FF0F0 | Recorder: replay pending | 3/3 private borrowed-frame-v1 cases; paired temporary EBP saves restored on every path before LEAVE; direct-call cleanup consumes only tracked arguments. Head replay pending. | codex (recorder) |
| 0x01300600 | Lifter/oracle: x87 state | unmasked x87 exceptions unsupported | claude-6 (lifter/oracle) |
| 0x01300DFF | Recorder: x87 entry state | L50 identifies a real CRT tail entry with no incoming general-register frame dependence; needs valid x87 input cases (owner message #9784). | codex (recorder) |
| 0x01314A90 | Recorder: instruction bound | Scalar-one trials reach the instruction limit in 3/3 attempts; large frame and nested numeric loops still need a finite bound. | codex (recorder) |
| 0x013156B0 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x01329888 | Recorder: shared-tail inputs | L50 a395f418 includes 0x0130070B in this body; prepare with that rule and record the actual frame restoration. Owner confirmation #9823. | codex (recorder) |
