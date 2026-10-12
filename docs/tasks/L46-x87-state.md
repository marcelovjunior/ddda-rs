# L46: the x87 save/restore state (FNSAVE/FRSTOR)

- **Owner:** claude-6
- **Branch:** `task/L46-x87-state` (worktree `E:\ddda-rs-wt\L46`), from
  `task/L27b-integrate`
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

L45's boot triage left 0x012F1C9D open. It is the CRT `pow` interior
reached from `_CIpow` 0x012F1C40 on the sBbsRpgMain-constructor route (5
calls deep). Its error path saves the whole x87 state with `fnsave`,
calls a C helper (0x01301C61) and restores the state with `frstor`.
Neither the emitter nor `ddnative-base` modelled these instructions. Its
x87 values also come from three small call-free helpers: 0x01300675
(`fldcw`), `__fload_withFB` 0x013006A5 (pushes one value) and 0x01300660
(2^x on st0). As mocked ext callees they cannot supply those values.

## Task

1. Model `fnsave`/`frstor` (and `fnstenv`/`fldenv` if the same paths use
   them) in the emitter and `ddnative-base`, bit-exact with the 108-byte
   (and 28-byte) 32-bit protected-mode layouts. Test against the real FPU
   with host-oracle tests, like L22's SSE oracle.
2. Promote the three call-free helpers closed with Codex's promote path.
3. Re-record 0x012F1C9D and anything else the closure reaches.

Data: a copy of `E:/ddda-re/notes/native-train4b` (train 3 + L45 + L45b;
59,545 / 177,261) as `native-L46`. The frozen base is
`E:/ddda-re/notes/native-train4b-base`.

## Results

(claude-6, in progress)
