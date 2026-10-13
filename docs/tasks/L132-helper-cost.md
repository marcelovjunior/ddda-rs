# L132: hot lifted-code helper cost

Assigned to codex-11 on `task/L132-helper-cost`, from `task/fps-line`.
Scope: ddnative-base memory/Flags helpers, a criterion-free timing example,
and regressions. Do not inline public helpers into the lifted shards. No game
runs by this agent. Follow AGENT-RULES.md and NATIVE_PIPELINE.md.

## Results

Implemented: private Flags construction directly inside ALU helpers removes
the nested out-of-line constructor, returned temporary and copy. Public
`Flags::new`, ALU and flag getters remain `#[inline(never)]`. Private flag
evaluators compose inside the shared helpers; undefined-flag panics move to
a cold function. Only private carry/overflow evaluators force inlining,
preventing an extra tail jump from their public wrappers. No generated call
site expands these bodies. Every operand/result field, flag rule, condition
alias, short-circuit order and undefined-flag trap is preserved.

`Flags` remains 20 bytes. Its public `bits: u32` and operands are used by
literal construction and external code; shrinking or omitting these stores
would change that API/representation. The retained optimization removes
temporary stores rather than public fields. Direct `Flags::new` has no
claimed improvement.

Memory investigation: play-profile assembly already has one permission-byte
load and one page-pointer load, with no table bounds check. Both loads and
their acquire ordering are retained. An aligned-word specialization removed
the straddle/mode checks, but repeated measurements did not establish a
consistent win. A cold unaligned helper also penalized nonparallel unaligned
accesses. These experiments were reverted; **mem.rs is unchanged from the
base**. In particular, checkpoint `e9403828` must not be taken alone.

### Verification

- **28 ddnative-base tests pass** in debug and play profiles. New regressions
  compare **390,090 ALU inputs** to host x86 instructions: exhaustive byte
  pairs and 79 boundary/pseudorandom values per operand for 16/32 bits, across
  ADD/SUB/AND/OR/XOR. Check all defined flags, public stored operands and every
  condition alias. Logic AF remains the previous model's value, not a claim
  about architecturally undefined hardware AF. Additional cases cover raw
  flags, COMISS and partial-flag short-circuit/traps.
- Existing memory tests cover aligned, unaligned, crossing, wrapping,
  permissions, unbacked pages and both atomic modes. No guard was weakened.
- Runtime light suite with `--no-fail-fast -- --test-threads=1`:
  **185 pass / 1 inherited gamma-reset failure / 3 ignored**. An initial
  parallel run hit the render-tail scheduling assertion; serial runs pass
  it. The full runtime receipt preceded the final memory revert/private
  evaluator inline adjustment; the final base tests cover those changes.
- **Lifted vectors/native gate pending combined integration**, as head
  explicitly approved on board #11483: fps-line + L129 + L132 + L130 + L109c,
  one gate through codex-7/HEAVY_BIG. L129's separate green gate does not
  verify L132. No shards, game, GPU tests or original-game comparison run
  here. No gameplay/FPS improvement claimed.

### Reproduce measurements

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_TARGET_DIR='C:/ddda-targets/L132'
$env:CARGO_BUILD_JOBS='4'
cargo run -p ddnative-base --example helper_cost --profile play -- 5000000
```

The example reports median/min/max of seven samples after warmup. Inputs are
synthetic object/stack-style word addresses over 1 MiB, aligned and unaligned,
page straddles, trait fallback, 8/16/32-bit ALU inputs, and SUB-derived
conditions. They are not a captured gameplay distribution. Iteration and
black-box overhead is included; combined SUB+condition is ns per pair.
Windows x86-64, play profile, public calls remain out of line. For the final
comparison, baseline/updated executables alternate in three pairs (reverse
order in the middle pair), each with seven samples of five million calls,
inherited process affinity to logical CPU 10. This is a shared machine,
not a reserved core. Report the median of the three per-process medians;
small differences and unchanged controls are noise/layout-sensitive.

Baseline source: `ee21acb3` (benchmark only). Private binaries/logs remain in
`C:/ddda-targets/L132/`, including `helper_cost-before.exe`,
`helper_cost-final.exe`, and `retained-{0,1,2}-{before,final}.txt`.

### Final ns/call

| Mode | Helper | Baseline | Retained |
|---|---|---:|---:|
| atomic=false | `page_table` | 1.415 | 1.322 |
| atomic=false | `fast_u32/aligned` | 2.068 | 2.014 |
| atomic=false | `fast_set_u32/aligned` | 2.224 | 2.489 |
| atomic=false | `fast_u32/unaligned` | 2.070 | 2.175 |
| atomic=false | `fast_set_u32/unaligned` | 2.267 | 2.490 |
| atomic=false | `fast_u32/straddle` | 14.195 | 14.202 |
| atomic=false | `fast_u32/trait` | 13.693 | 13.715 |
| atomic=true | `page_table` | 1.452 | 1.392 |
| atomic=true | `fast_u32/aligned` | 2.074 | 2.231 |
| atomic=true | `fast_set_u32/aligned` | 2.253 | 2.353 |
| atomic=true | `fast_u32/unaligned` | 2.587 | 2.387 |
| atomic=true | `fast_set_u32/unaligned` | 2.529 | 2.521 |
| atomic=true | `fast_u32/straddle` | 14.771 | 14.269 |
| atomic=true | `fast_u32/trait` | 13.472 | 14.195 |
| flags | `Flags::new` | 3.078 | 3.078 |
| flags | `alu_sub/32` | 7.714 | 2.074 |
| flags | `alu_add/32` | 7.884 | 2.059 |
| flags | `alu_and/32` | 7.674 | 2.055 |
| flags | `alu_sub/8` | 7.517 | 2.067 |
| flags | `alu_add/8` | 7.827 | 2.063 |
| flags | `alu_and/8` | 7.772 | 1.954 |
| flags | `alu_sub/16` | 7.593 | 2.076 |
| flags | `alu_add/16` | 7.650 | 2.064 |
| flags | `alu_and/16` | 7.663 | 2.062 |
| flags | `inc_dec` | 11.406 | 3.281 |
| flags | `zf/sub` | 2.267 | 2.182 |
| flags | `cf/sub` | 2.608 | 2.819 |
| flags | `cond/le/sub` | 9.470 | 7.123 |
| flags | `sub+cond/be` | 14.095 | 6.622 |

Memory and `page_table` are unchanged controls, not claimed optimizations.

Next action: head/codex-7 combined native gate, then head same-scene profile
and FPS comparison. Milestone handoff goes through head because codex-7 owns
`docs/MILESTONES.md` (board claim checked); no inventory/runtime-system row
changes.
