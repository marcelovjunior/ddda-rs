# Residual functions

Inventory snapshot: **train 12, main `d20706b7`**, 59,331 / 59,440
non-thunk functions verified; **109 remain (0.183%)**.

One row per non-verified inventory address, sorted by address. Supplementary
entries count only when admitted to inventory; thunks are excluded. Private
cases remain here until a head integration train credits them. Guard reasons
describe attempted inputs, not a unique cause or invalid game code.

Sources: `scripts/inventory.py summary` and read-only query
`SELECT address,status FROM functions WHERE NOT thunk AND status != 'verified'`;
`native/patterns/verification_gaps.json`; train-12 native gate;
[L28 Results](tasks/L28-crt-platform-class.md#results),
[L49 Results](tasks/L49.md#results), and the
[L21 cross-entry census](tasks/L21-boot-bring-up.md).
Private row evidence: `native-L28c/L28c/residual-census-train12.json`.
0x00786D69 is a stale-body input task assigned to Codex but is not an admitted
inventory address; it remains outside this denominator.

Ownership: **3 claude-6 (gap)**, **13 claude-6 (lifter)**, **2 claude-6 (lifter/oracle)**, **91 codex (recorder)**.
**21 rows have private cases awaiting replay.** Boot stops take priority;
0x00493AA0 remains set aside.

| Address | Class | Blocker | Owner |
|---|---|---|---|
| 0x00410000 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00416740 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x004644D0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00475DD0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00476D60 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00480BA0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00493AA0 | Recorder: setup mismatch | Set aside: manager-first changes destination reads/writes/MXCSR; destination-first changes manager reads/writes/calls and ECX/EDX. Neither setup order passes exact replay. | codex (recorder) |
| 0x004A0C00 | Recorder: object/manager state | Checked 0x00437BE0 + 0x00489F80 setup still stops at branch 0x004027E9 after 92 instructions; linked-object input/proof facts unresolved. | codex (recorder) |
| 0x004A0E20 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x004EE3E0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x004EE720 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x004F5200 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x004F53B0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00524A10 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00524BE0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00528550 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x005293E0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x005652E0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00565308 | Recorder: stale-body inputs | Stale body omits 13 instructions from 0x0056532A; refreshed-body recording leaves the image. Resolve inputs before refreshing accepted cases. | codex (recorder) |
| 0x0056532A | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x005F3450 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00606670 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x006076F0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00608AE0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00618880 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x006197E0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0061BC10 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0062E3A0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00633A70 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00633B70 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00646A30 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0064DD20 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00663040 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x006692F0 | Recorder: return address | Return address is changed; needs valid input layout or a supported callee stack contract. | codex (recorder) |
| 0x00669F40 | Recorder: return address | Return address is changed; needs valid input layout or a supported callee stack contract. | codex (recorder) |
| 0x0066B9E0 | Recorder: memory extent | Recording touches more than 256 pages; bounded valid object/buffer inputs needed. | codex (recorder) |
| 0x00677930 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00685030 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00685B40 | Recorder: processor flags | POPF(D) requests unsupported processor-flag changes; valid caller/stack state remains unresolved. | codex (recorder) |
| 0x00689960 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x0068A9F0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00691AE0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00691E50 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00692A40 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x006A1770 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x006E0EB0 | Recorder: memory extent | Recording touches more than 256 pages; bounded valid object/buffer inputs needed. | codex (recorder) |
| 0x006EDFE0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x006F1590 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x006F2350 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x006F3242 | Lifter: cross-entry boundary | L50: body continues from verified owner 0x006F3230; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x006F8730 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x006F8C00 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00705E00 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0070B210 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0070F3C0 | Recorder: memory extent | Recording touches more than 256 pages; bounded valid object/buffer inputs needed. | codex (recorder) |
| 0x007126C0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00712AC0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x0071C0F0 | Recorder: return address | Return address is changed; needs valid input layout or a supported callee stack contract. | codex (recorder) |
| 0x00736370 | Inventory admission | L22 identifies an address-taken ASCII tag, not code; resolve inventory admission (current decode rejects BOUND). | claude-6 (lifter) |
| 0x007BEA60 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x007F2020 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00811A90 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00815C60 | Recorder: floating comparison | Checked 0x00811780 constructor reaches COMISS at 0x00816522; exact floating-point facts needed for a finite trace. | codex (recorder) |
| 0x008282E0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x0083FFF0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00947493 | Lifter: cross-entry boundary | L50: body continues from verified owner 0x009472C0; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x009897C0 | Lifter/oracle: x87 state | x87 stack fault before fild at 0x009899d0 | claude-6 (lifter/oracle) |
| 0x009921E0 | Verification gap | unproven table at 0x00992713 | claude-6 (gap) |
| 0x009D0E70 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00A195D0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00A2F3B0 | Verification gap | unproven table at 0x00A2F4A5 | claude-6 (gap) |
| 0x00B75EC0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00B7FF30 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00B952C0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00B955C0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00B97220 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00BA31D0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00BA4160 | Recorder: dispatch/return | L49 fixed the INC/DEC flag guard; closed callees still execute at 0x00000CD7. Valid input/stack state needed. | codex (recorder) |
| 0x00BA5540 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00BD45C0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00C1D790 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x00C9BAA0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00C9E0F0 | Recorder: replay pending | 3/3 private cases; exact-register serializer strategy. | codex (recorder) |
| 0x00CED240 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00CED280 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x00CEEBC0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x00D29AF0 | Recorder: allocator state | Checked 0x00740590 + 0x00D29820 setup supplies 20-byte input but allocator dispatch 0x00D29B4C still has EAX=0; real allocator state unresolved. | codex (recorder) |
| 0x00D9EF20 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x00DA46C0 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00E0D750 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x00F83C00 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x00FB9A10 | Quarantine: return contract | Prior own-return fixture rejected; resolve and revalidate the function/return contract before recording. | claude-6 (lifter) |
| 0x011251F0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x01137DF3 | Lifter: cross-entry boundary | L50: body continues from verified owner 0x01137DD0; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x01146420 | Lifter: cross-entry boundary | L50: body continues from verified owner 0x01146410; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x011591C0 | Recorder: replay pending | 3/3 private cases with general wrapper-call-v1; caller-derived mock cleanup recorded and return checked. | codex (recorder) |
| 0x012A5B30 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x012E7CE0 | Verification gap | unproven table at 0x012E7D24; unproven table at 0x012E7E06 | claude-6 (gap) |
| 0x012EFA1E | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x012F693F | Lifter: cross-entry boundary | L50: body continues from verified owner 0x012F68B0 / 0x01304A9E; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x012F6948 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x012F6A6F | Lifter: cross-entry boundary | L50: body continues from verified owner 0x012F69E0 / 0x01304C6E; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x012F6A78 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x012FF0F0 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
| 0x01300600 | Lifter/oracle: x87 state | unmasked x87 exceptions unsupported | claude-6 (lifter/oracle) |
| 0x01300DFF | Lifter: cross-entry boundary | L50: body continues from verified owner 0x01300D70 / 0x013076EE; boundary fix first, then Codex fixture. | claude-6 (lifter) |
| 0x01314A90 | Recorder: instruction bound | Recording exceeds its instruction limit; requires valid input state and a proved finite trace. | codex (recorder) |
| 0x013156B0 | Lift rejection | x87 overflow at mocked callees that consume x87 values; per-call stack-consumption contract needed. | claude-6 (lifter) |
| 0x01329888 | Recorder: dispatch/return | Execution leaves the image; valid object/call-target or stack-cleanup evidence still needed. | codex (recorder) |
