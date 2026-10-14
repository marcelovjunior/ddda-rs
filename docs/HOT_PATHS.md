# Hot paths: where a frame's time goes, what is ported, what is next

Written 2026-10-14 (task L190) so nobody re-derives this. Sources: the
sampling profiles `C:/ddda-targets/L172/runs/profile_{base2,base4,base5,s2e}.txt`
(`scripts/sample_profile.py --waits`, 30 s, 40 Hz, inside the "heavy"
gameplay window), L162/L167 stack captures (`docs/tasks/L162-job-wake.md`,
`L167-critical-callbacks.md`), the L172/L184/L185 commit messages on
`task/train6` and `task/L172-parallel-shadow`, and Ghidra for roles.
Numbers are CPU-seconds summed over the 30 s sample, so 30 s = one core
fully busy. Profiles are noisy (up to 7 fps run to run; judge by two runs).

Related docs (not repeated here): [FRAME.md](FRAME.md) (the original's frame,
job lines, passes), [NATIVE_PIPELINE.md](NATIVE_PIPELINE.md) (lifter, port
conventions, vector format), [RUNTIME.md](RUNTIME.md) (threads, dispatch,
native device), [tasks/L172-parallel-shadow.md](tasks/L172-parallel-shadow.md),
[tasks/L180-render-worker.md](tasks/L180-render-worker.md),
[tasks/L25-presentation-performance.md](tasks/L25-presentation-performance.md),
[tasks/opening-perf.md](tasks/opening-perf.md),
[tasks/L162-job-wake.md](tasks/L162-job-wake.md) and
[tasks/L167-critical-callbacks.md](tasks/L167-critical-callbacks.md) (the
shadow call tree).

## 1. Where the time is (build base5, 2026-10-13 22:36, heavy window ~40 fps)

Threads (CPU s of 30 s):

| Thread | CPU s | What |
|---|---:|---|
| guest main thread (`0x004079E0` WinMain -> `0x00DF0D20` loop -> `0x0041DD60` frame) | 18.8 | the frame; waits 34% of samples inside `DBE560` for the job batch |
| render worker `guest thread 0x1034` (`0x00DAC090`) | 24.6 | replays the draw lists through the native sink (`ndraw_native`): `draw_call` 20% self, `Mirror::textures` 4%, `shadow_samplers` 3.4%, begin_frame/prune 6% (L180 owns this; 74% of it is the sink `command`) |
| job workers `guest thread 0x1070..0x1084` (loop `sMain::vf20` `0x00DBDC90` -> `0x00DBDEF0`) | 4 x ~10-14 (=71) | run the job batches; each is 45-55% waiting. `0x00DB7620` (the job callback that hosts the shadow and model draws) is 29 of the 66 CPU-s under the loop |
| guest thread 0x1038 | 6.7 | sound (mixer, x87 leaves `0x01320700`); off the critical path |
| `d3d9-backend`, `wgpu-encode-0`, `wgpu-finish` | 10.9 / 8.5 / 5.5 | GPU command execution, not guest code |

Self time is dominated by the memory and flag helpers of lifted code
(`fast_u32` 15%, `fast_set_u32` 6%, `X87::real_memory_dyn` 5%, flag helpers
~3%): there is no single hot leaf, the lifted bodies themselves are the cost.
A readable port removes those helpers (typically 2-10x on the ported body).

The critical path is the main thread's wait for the job batch, and in the
batch the **last job is the uSkyStableShadow callback** (`DB7620` / `EE2C90`,
vtable `0160DF98`): last in 150/150 sampled frames, mean callback 13.4 ms,
3.9 ms of it alone (no other job running) per frame (L167). The user
confirms it: fps drops from ~50 to high 30s when pawns walk into the sun.

## 2. Frame and job structure (one paragraph per level)

- `0x00DF0D20` loop -> `sBbsRpgMain::vf06` `0x0041DD60` (frame; steps in
  FRAME.md section 2). Steps 36-42 run the unit passes A/B/C
  (`sUnit::vf06/13/11` `0x00DB6740` / `0x00DB5700` / `0x00DB5690`), step 58
  `sShadow::vf06` `0x00E277A0`, 60 `sRender::vf06` `0x00DAF000`
  (-> `0x00DB0DE0`, viewport/camera `0x00DC4770`), 65 `0x004B9660` queues every
  unit's draw as jobs.
- Jobs: the scheduler global at `0x018D0B28` (per-thread view in the lifter
  since 8f7e50e7). `DBE560` runs a batch (`DBD8D0` participates on the
  caller, workers loop in `DBDC90`); the main thread waits in
  `wait_until` (92% of its waits are `from DBE560`).
- Job callbacks seen (L167, solo ms/frame): `DB7620`/`EE2C90` uSkyStableShadow
  3.9, `4B9FF0`/`C70790` uSoundGenerator 0.47, `4B9FF0`/`BAF4F0` uNpc 0.46,
  `FED7E0` cDraw 0.25, `4B9FF0`/`B53650` uCmc 0.24, `5D12B0` uPawnIntel 0.2.
- **Serial shadow chain** (inside one job callback):
  ```
  DBD8D0 -> DB7620 -> EE2C90 (uShadow::vf11) -> vtable+60 EFEF80 (uStableShadow::vf24, tail)
    -> EFDC70 (per cascade: EE5CE0 split math, EFD240 setup [ported shadow_pass::setup], EEA950)
      -> DB63E0 (sUnit::vf16 shadow traversal [ported shadow_traversal::traverse])
         -> units via DBE560/DBD8D0 mode 1-3 -> DB7620 per unit:
            C52D70 -> 886880 (uObjModel::vf11) -> FFE5C0 -> EA3130 (uBaseModel::vf11 [ported])
                 -> FFC350 / FFC9F0 / EA5320 -> FEEF20 -> FEE320 -> E12A20 (material/cbuffer commit)
            C25EA0 (update_instancing [ported]) -> FA5860 (uSwingModel::vf11) -> EA3130 -> C24640
            B6A070 (uPlayerBase::vf11) -> B89790 (vf208) / B8B800 -> effect/model helpers
         + sPrimitive 107FF00/30 -> E61010 / E68390 (-> 111E180 sort, 10B7440 draw) [ported]
  ```
  EFD240 mutates shared resource state; DB7620 units assume exclusive
  access to the objects they traverse, so view-mode parallel publish crashed
  (L172, memory ddda-rs-state 23:20). Cascade independence is NOT proven.
  The scheduler/lane/waits side of this belongs to the L172 owner.
- Draw path (per model, in the units and the main pass): `EA3130` -> virtual
  +0x68 -> `FFC350 draw_model_mesh` / `EA5320 draw_mesh` -> `FEEF20`
  (`commit_current_state_native`) / `FEE320 commit_primitive_state` ->
  `F11610 draw_with_resources` -> `FED7E0` cDraw -> the D3D9 command stream the
  render worker replays (`mtdraw::executor::execute` -> `ndraw_native`).
  Material: `EA23F0 primitive_material`, `FED190 begin_material`, `E0F3C0
  package_key`, `E12A20 shader_lookup`, `E0E5E0 dependency_hash`
  (`semantic_draw_*`, ~6 ns-us each). Cull/lights: `FF0780/0AD0/0D20/0E60`
  (`mtdraw::cull`), `49B1A0 scene_lights`, `EA42E0 cull_bounds`
  (port exists, 0.78x: not shipped), `FEB1F0 scene_collect_visible`.

## 3. Already ported (registered in `crates/ddports/src/lib.rs`)

Measured speedups are lifted body vs port in the unit benchmarks (commit
messages are the source; "x" = lifted time / port time). No port is credited
with a game fps gain individually; the combined effect of the L184 round is
31.7 -> ~32 (train14-18, within noise), the big jump (to 39.9) came with the
L180 render-worker sink plus the L172 view-mode scheduler tree (base5).

| Where | Ports (addr) | Speedup |
|---|---|---|
| `mtcore::semantic_p2c` | `EA3130` base_model_draw_unit, `EA1FD0` select_shadow_state | 1.8x (1402 -> 790 ns), 1.45x |
| `semantic_s9` | `F11610` draw_with_resources, `FEB1F0`, `FF2EE0` set_camera_matrices (100x vs lifted), `FF04C0`, `EA5B50`, `FED5D0` | 3.9x (2711 -> 695 ns) ... |
| `semantic_draw` / `_c` / `_a` / `_b` | `EA5320` draw_mesh (folded variant 3.8-4.2x), `FEE320`, `FFC350`, `FFC9F0`, `EA23F0`, `FED190`, `E0F3C0`, `FF1920`, `FF14D0` | 1.2-4x (median 1.24x over 77 pairs, L178) |
| `semantic_s1` | `FEDB80/FEDC00` save/restore recording state (8.4x/5.9x), `F23260` draw_attachment 2.5x, tick_gauges, update_instancing | |
| `semantic_s2/s3` | cast chain `FAC6F0 DEA870 11787C0`, shader_lookup, cull helpers, classify_overlap `EA1EE0`, any_member_test `E61010` | 2-6x |
| `semantic_s4..s8` | actor/joint/pose/motion filter (`6419 -> 203 ns` 31x), GJK sweep `D7CF70` (3.4x), CRT math | 3-30x |
| `semantic_l190` (L190) | `FA5860` uSwingModel::vf11 swing_model_draw_unit, `B8B800` rebuild_skin_remap (ESI = model), `B89790` uPlayerBase::vf208 player_draw_parts, `B89FE0` player_draw_part (ESI = context), `B8A770` prepare_part_blocks (EDI = part) | 1.9x, 17-19x (7100 -> 400 ns), 2.3x, 1.9x, 2.0x (paired bench on the recorded vectors, guest calls replayed) |
| `job_*`, `shadow_*` | `E0E5E0` dependency hash, `FED480` save_state, `DB63E0` traverse, `EFD240` setup, `E61010/E68390` sPrimitive vf11/12, `111E180` pair sort (7.9x) | 1.4-8x |
| `mtframe` / `mtdraw` / `mtmath` | frame logic, cull, lights, set_render_target, math | see NATIVE_PIPELINE.md |

Dispatch cost matters for small ports: `override_of` is an open-addressing
table (L180, ~12 ns/call saved); a port calling a ported callee goes through
`call_guest` -> direct (no re-dispatch). `DDRUNTIME_SEMANTIC_SKIP/ONLY`
(hex list) bisect ports in game.

## 4. Ranked next candidates (excluding the shadow dispatch/scheduler)

Inclusive CPU-s are from base5 / s2e profiles (all workers, 30 s). Gain =
inclusive share x (1 - 1/speedup) at an assumed 2-3x on lifted bodies.

| # | Function | Role | Incl. CPU-s | Size | Difficulty | Expected |
|---|---|---|---:|---:|---|---|
| done L190 | `B89790`, `B89FE0`, `B8A770`, `FA5860`, `B8B800` ported (section 3); `B6A070` uPlayerBase::vf11 measured 0.91x and `B8B430` skipped (below) | | | | | |
| skip | `B8B430` | player model (re)build: allocates a 0xe50-byte object (`CFE630`/`FF9740`), runs ~20 virtuals, then `B8B800` and `7F4260`/`7F42D0` per part. A setup path, not per frame; dominated by guest calls, so a port would stay below 1.1x | | 0x3C6 | not worth it | |
| 3 | `886880`/`886530`/`885DC0` uObjModel::vf09/vf11/vf08 and `0084C720` uCharacterBase::vf09 | per-model draw prep | 2.3-3.2 each | small | easy-medium | ~1-2 |
| 4 | `01174730`, `0116B670`, `01165B40` | probe/sky tree search used by the ambient probe (chain with the ported `11787C0`) | 4.0 | ? | medium | 1-2 |
| 5 | `EE5CE0` (cascade split math), `EEA950`, `EE2C90` | shadow chain top (floating point; per frame, not per unit) | 2.3-6.8 | | medium, overlaps the shadow owner | small: runs once per cascade |
| 6 | `80A8B0`, `FEEF20` unfolded callers | seen only in s2e | 4.4 | | | |
| 7 | render worker `ndraw_native::draw_call` etc. | Rust, not lifted: L180 | 24.6 | | medium | the largest single thread; see L180 Next |
| 8 | main thread `4B97F0` sUnitExt::vf09 / `DB6740` pass A | unit move | 3 | | hard (many virtuals) | |

Not worth porting: the sound thread's `DC77A0/DDB860/DDDFA0/CFE6B0` chain
(off the critical path, host audio is already native), `01314570/01323460`
(CRT/loader, 6 CPU-s on the sound/loader thread).

Reading the order: the shadow job is the lever (parallel shadow, L172);
until that is accepted the best use of ports is cutting the per-unit cost
inside `DB7620` (rows 1-3), because that serial path is the frame's tail.

## 5. How to port (the pattern)

See NATIVE_PIPELINE.md "Porting conventions" and "Vector file format". In
short: a semantic port is `fn(m: &mut dyn Mem, c: &mut Cpu)` in
`crates/mtcore/src/semantic_*.rs` with the original's exact register/stack
ABI (`semantic_p2c::base_model_draw_unit` is the template; callees go
through `call_from(..., guest(addr))`), registered in the
`ddports::ports()` semantic list, with an oracle test in
`crates/mtcore/tests/semantic_*.rs` that runs recorded vectors
(`research/native/record_*.py`, Unicorn on the original exe) and mutation
checks, and a benchmark of port vs lifted body.

## 6. Function roles (Ghidra, L190)

| Address | Role |
|---|---|
| `B89790` | `uPlayerBase::vf208(context, record)`, ret 8: player per-unit draw, parts as records (`7FFCC0`) or ordinary parts (`B89FE0`) |
| `B89FE0` | player part draw loop over the part's 0x30-byte pass records (`ret 0x20`, ESI = context) |
| `B8A770` | per-part block preparation: applies `part[0x1c][k]` (or the alternate list) with `C85A00(sink, entry, context)`; EDI = part |
| `B6A070` | `uPlayerBase::vf11(context)`: fade -> bit 30 of `[0x108]`, `FED480`, virtual +0x340, `FED5D0` |
| `B8B430` / `B8B800` | player model rebuild; `B8B800` (ESI = model) rebuilds the skin joint remap tables and the pose buffer |
| `FA5860` | `uSwingModel::vf11(context)`: base draw `EA3130` then retires pending state slots |
| `EA3130` / `EA1EE0` / `EA1F40` / `EA1FD0` | `uBaseModel::vf11`; visibility mask; world position; shadow state selection |
| `C52D70` / `886880` / `FA5860` / `B6A070` | the `vf11` (draw_unit) of their classes, called from `DB7620` units |
| `7FFCC0` | submit pass records (ESI = context, EDI = part), ported (`semantic_s10`) |
| `C85A00` | apply parameter block, ported (`semantic_s10`) |
| `FED940` | the context's view matrix pointer; `FED480` / `FED5D0` save / restore recording state; `FED890` grow the constant pool |

Implicit-register conventions matter for ports of these (the oracle tests check
the registers each recorded guest call reads): `B89FE0` and `7FFCC0` take ESI =
context, `B8A770` and `7FFCC0` take EDI = the part, `B8B800` takes ESI = model.

## 7. The main/game thread (L190 follow-up)

The frame waits on the main thread, so its time is what counts. Mean of 16
`guest main thread` samples (profiles `C:/ddda-targets/L172/runs/profile_*.txt`,
about 16 CPU-s of 30 s each; inclusive CPU-s, children not subtracted):

| Incl. s | Function | What |
|---:|---|---|
| 14.8 | `0041DD60` | the frame |
| 10.7 | `00DBE560` (9.3 of it in `DBD8D0`) | job batch: the main thread runs jobs itself and waits; most of it is the shadow job (`DB7620` 3.9 on this thread) |
| 5.0 / 4.1 | `00DAF000` sRender::vf06 / `00DC4770` sCamera::vf09 | per-viewport pass replay; 3.5 of it inside the ported `set_pass` (draw-pass replay, whose callees are ported) |
| 4.4 | `job_shadow::save_state` (`FED480`, ported) | inclusive of the guest call it wraps |
| 2.6 | `004B97F0` / `00DB6740` | pass A: every unit's move (`sUnitExt::vf09`, a dispatch loop over virtuals) |
| 2.0 / 1.7 / 1.5 | `restore_recording_state` / `layer_walk` / `node_dispatch` | ported, inclusive of their callees |
| 0.3 each | `DB5690`, `4B99D0`, `B89790` | pass C, lines, player draw |

Findings:

- **There is no per-function self time for lifted code in these profiles.**
  Every `-- self` line is a runtime helper (`fast_u32` 18%, `fast_set_u32` 6%,
  flag helpers) and `-- nearest lifted function` is "(no lifted frame)" at
  100%: the profile script that made them (`E:/ddda-rs-wt/L63/scripts/sample_profile.py`,
  used by `autorun.sh`) looks for `::f_` in frame names, which the Rust v0
  symbols of the shards do not contain. The repo's own `scripts/sample_profile.py`
  decodes them (`pretty()`); pointing `autorun2.sh` at it gives the nearest
  lifted caller of every helper sample, which is the missing self table.
- **Older profiles with lifted self do exist but pre-date the ports** (`E:/ddda-re/notes/profile_fast1c.txt`,
  100 s of loading): the main thread's top self was `FE9580` (procedural
  geometry builder, 3.1 s; already a readable port behind `DDRUNTIME_GEOMETRY_PORT`)
  and a string comparison helper (13%, the `find_identifier` family, ported).
  Both are loading-time, not frame time.
- **The remaining unported main-thread functions are thin dispatch loops**
  (`4B97F0` over unit lines, `DB6740`, `DC4770` over eight viewport records:
  a handful of reads and a virtual call per element, all the cost in the
  callees). A port of them would stay under the 1.1x bar. `DB0DE0` shows up
  in the inclusive list but is the display-mode enumeration of the renderer's
  constructor (EnumAdapterModes, sprintf, a std::vector): the symbol that
  carries that name in the shard is not that function's time; do not port it.
- So on the main thread the lever is still the shadow job (the shadow agent's
  scheduler work) and the memory-helper cost of lifted code (codegen, not
  ports); a port only pays where a lifted body is heavy and runs per unit,
  which is the worker-side player/model draw work ported in section 3.

## 8. Profiling with self time per lifted function (L190)

The L172 profiles named in section 7 have no lifted self time because their
script (`L63/scripts/sample_profile.py` via `autorun.sh`) cannot read the Rust v0
symbol names. The repo's `scripts/sample_profile.py` can: its "nearest lifted
function (self + non-lifted callees)" section assigns every helper sample
(`fast_u32`, flags, x87, ...) to the lifted function or port that called it.
Validated offline on the saved capture `scratchpad/stacks_ab_o1b.json`
(`--input CAPTURE --threads "guest main thread"`): it ranks `00DBE560`
7.2%, `00DD42C0` 4.1%, `00E12A20` 2.9%, `00FEE320` 2.1% of main-thread busy samples.

- Run: `bash C:/ddda-targets/L172/runs/autorun3.sh EXE TAG` (a copy of the
  scratchpad `autorun2.sh` that calls the repo script; override with
  `PROFILER=path`). It writes `profile_TAG.txt` and the raw `stacks_TAG.json`.
- Re-report without the game: `python scripts/sample_profile.py --input
  stacks_TAG.json --threads "guest main thread" --top 40 --callers 15`.
- Rank what runs under the shadow unit callback: `python
  scripts/profile_units.py stacks_TAG.json [--root 0x00db7620]` (inclusive share
  per lifted function/port below `DB7620`, and its direct callee, the unit's
  virtual method).

## 9. Shadow units: what runs under `DB7620` (L190)

`scripts/profile_units.py` over five saved captures (`stacks_ab_o1a/o1b/o2a/o2b`, `stacks_bis29`,
3067 guest-thread stacks that contain `DB7620`; the captures pre-date most ports, so shares
show where the work was, not what is left). Share of those stacks a function appears in
(inclusive), direct callee of `DB7620` in brackets:

| Incl. | Function | Status |
|---:|---|---|
| 41.9% | `EA3130` uBaseModel::vf11 (direct 18.6%) | ported (P2c) |
| 23.0% | `EE2C90` uShadow::vf11 (direct 23%) | shadow chain top, shadow agent |
| 23.5% | `EA5320` draw_mesh / `FEE320` / `FEEF20` / `E12A20` | ported (folded) |
| 18.8% | `B6A070` uPlayerBase::vf11 (direct 14.2%) -> `B89790` | `B89790`, `B89FE0`, `B8A770` ported (L190); `B6A070` itself 0.91x, left lifted |
| 17.7% | `F20640` uSimSoftBody::vf11 (direct 7.5%; via `87D260` 10.2%) | **mechanical port checked equal on 60,000 random states, but 1.0x: not shipped** (see below) |
| 16.1% | `886880` uObjModel::vf11 | Haiku L191 (6.5x on a 21 ns body) |
| 13.7% / 12.7% | `FFE5C0` uModel::vf11 / `C52D70` | thin wrappers (`FED480`, `EA3130`, `FED5D0`) |
| 5.7% | `0080A8B0` | small (0x112 bytes) |
| 3-4% each | `01144C10`, `0049B110` (0x6b bytes), `E75EA0`, `010CC240`, `5D8DC0` (effect wrapper) | not ported; small or guest-call bound |

**`F20640` finding.** `scripts/semport.py` (now with `imul r,r,imm`, `sete/setne`, FLD/FSTP
dword pairs and a `cover` feature that reports the blocks never run) translated all 2,098
instructions mechanically; a differential test against the lifted body (random memory reading
as finite floats, the same 26-callee stub for both, 60,000 states, 150 of 153 blocks reached,
the other three being alignment padding) found the two bodies identical, and found a real
bug in the shared `semantic_rt` (below). But the paired benchmark on cases that run the full
path (about 35 guest calls each) gave 0.89x-1.07x: the body is dominated by its guest calls
(every call syncs the 8 registers and 8 XMM registers to and from `Cpu`), so a register-level
translation cannot beat the lifted code. The port, its test and the lifted copy were removed
from the tree (commit history keeps the generator and the recipe: `semport.py 0xf20640
0xf23256`). Lesson for the next candidates: **count guest calls first**; a body with more
than ~10 guest calls per invocation will not speed up from a port, only from cheaper calls
(`call_guest` direct dispatch, and syncing only the XMM registers a callee reads).

**Fixed on the way.** `semantic_rt` (used by the mechanically translated ports: `shadow_primitive`
and the s4 family) picked the second operand when both operands of an SSE arithmetic instruction
were NaN and the second was signalling; the hardware returns the first operand. Test:
`crates/mtcore/tests/rt_nan.rs` (hardware vs helper over NaN pairs).

## 10. Per-call and per-instruction overhead (L190)

Micro-benchmarks, release, best of repeated rounds, ns. "O3 caller" = the helper called from
optimised code (`crates/mtcore/tests/helper_bench.rs`); "O0 caller" = the same call from code
compiled at opt-level 0 like the play-profile shards (a standalone `rustc -C opt-level=0` harness
against the optimised `ddnative-base`; its empty loop body costs 7.2 ns of each number).

| Piece | O3 caller before | after (`lean-access`) | O0 caller before -> after |
|---|---:|---:|---:|
| `m.fast_u32` (hit) | 2.02 | 1.77 | 12.8 -> 12.8 |
| `m.fast_set_u32` (hit) | 2.01 | 1.77 | 9.0 -> 9.0 |
| `emit::push + pop` | 3.53 | 3.52 | 14.4 -> 13.5 |
| `m.fast_u8` | 1.52 | 1.5-1.8 (code untouched; layout noise) | 12.6 |
| `fl.alu_sub` / `alu_and` | 1.77 | 1.77 | 12.7 |
| `fl.cond("le")` / `("ne")` | 7.0 / 2.0 | not changed (a packed-key version gave 5.3 / 2.5: no gain overall) | 15.1 / 14.3 |
| `x86::call_local` (nop) | 0.25 | | |
| `x86::call_ext` (trivial handler) | 2.9 | | 9.5 incl. loop |
| `x87 real_memory` fld+fstp | 9.9 | | |

How the numbers read:

- **The helper bodies are at their floor.** A guest memory access is one out-of-line call, one
  atomic load of the page entry, a permission test and the load: 1.8-2 ns. `lean-access`
  (default feature of `ddnative-base`; `--no-default-features` restores L77) gives the aligned
  dword load and store their own path (no page-end test, one alignment test) and makes `push`/`pop`
  one optimised out-of-line copy with the store inlined: -12% on `fast_u32`/`fast_set_u32` from an
  optimised caller and -0.9 ns on push+pop from an opt-level-0 caller. From an opt-level-0 caller
  the access is not faster, because the caller's own spills and argument set-up (5-6 ns)
  dominate. Expect about 1-2% of CPU, not more.
- **Calls are cheap**: a not-overridden `call_local` is a bit test (0.25 ns); `call_ext` into the
  dispatch cache is 3 ns; the runtime's cache (L74/L101/L171/L185) is already direct-mapped
  with a lean path; a port reached from a port through `call_guest` is a bit test plus one probe of
  the open-addressing table. No cheap change is left in dispatch.
- **The large factor is the opt-level.** The skin-remap function (`B8B800`, loops and memory
  helpers) runs in 30.6 us at opt-level 0, 12.1 us at opt-level 1, 11.5 us at opt-level 3
  (2.5x). `-Zmir-opt-level=3 -Zinline-mir` at opt-level 0 gains 4%; nothing else short of
  opt-level 1 helps. The play profiles build the 32 address shards at opt-level 0 (`play-fast`:
  15 of them at 1, chosen for loading and the title) and only 83 functions in `lift_hot_00` at 1.
- **Static helper sites in the generated code**: `fast_u32` 866k, `fast_set_u32` 674k, `emit::push`
  573k, `emit::pop` 135k, `alu_sub` 101k, `alu_and` 69k, `call_ext` 137k, `call_lifted` 55k,
  `call_local` 23k, `cond` 336.

**Where the busy lifted time sits** (five saved gameplay captures, busy guest-thread samples
attributed to the nearest lifted function without a port, `scripts/hot_list.py`): lift_hot_00
13%, shards 28 11.6%, 02 10.4%, 27 9.6%, 31 4.8%, 01 4.7%, 30 4.6%, 00 4.4%, 06 3.4%, then
`play-fast`'s opt-level-0 shards 14 2.9%, 08 2.9%, 03 2.7%, 11 2.5%, 29 2.4%, 24 2.4%, 09 2.3%,
16 2.3% (19.4% together; the other opt-level-0 shards 3.4%).

**What to build.** (Superseded: the user's play builds, `l172_replay_build.sh` with profile `play-o2` in R172, already build every shard at opt-level 2, so `play-hot` adds nothing there; it only matters for `play-fast`-style builds.) `play-hot` (new profile, `scripts/build_play.sh --profile play-hot TAG`,
target `C:/ddda-targets/playhot`) is `play-fast` with those eight shards at opt-level 1 too:
the 19% of lifted samples they hold should run about 2.5x faster (an estimated 8-11% of lifted
CPU; unmeasured in game). `scripts/hot_list.py` picks the functions for a finer cut from a
fresh `autorun3.sh` capture (it skips functions that have a port). Not done: moving the hot list
inside the generated code. `lift.py gen` with the *committed* hot list rewrites 756 of the 1575
generated files and takes 11.5 minutes, so the tracked shards no longer match the generator and
its data; a deliberate regeneration plus gate by the lifter owner is needed before the hot
package can grow (its capacity is 2000 functions; 83 now).

## 11. What dominates lifted code at opt-level 2, and the stack window (L190)

**Where the time is, measured in the game.** Two gameplay captures from `play-o2` builds
(`stacks_ab_o2a/o2b`, 3,000 busy samples of the guest threads outside the render worker), leaf
function of each sample, attributed with `scripts/sample_profile.py`'s symbol decoder:

| Share | What |
|---:|---|
| 24.8% | `fast_u32` (guest dword loads) |
| 9.1% | `fast_set_u32` (guest dword stores) |
| 3.0% | `fast_u8`, `fast_u16` |
| 2.8% | `GuestMemory::page_table` (the call every lifted function makes at entry) |
| 2.8% | memory slow paths (`u32`, `set_u32`, `read_bytes`) and 2.1% `write_bulk` |
| 18.5% | the lifted bodies themselves (leaf) |
| 6.8% | call/dispatch (`call_ext`, `run_binding`, the install closure) |
| 5.9% | x87 (`X87::real_memory`, hardware x87 per operation, 10 ns) |
| 4.6% | flag helpers; 2.7% SSE helpers; 0.7% push/pop; 1% ports |

So at opt-level 2 **44% of the guest-thread time is the memory helpers**, three times the lifted
code itself. Every x86 instruction with a memory operand is one out-of-line call.

**What does not help** (all measured):
- Inlining the accessors into O2 shards (`ddnative-base` feature `inline-access`): remap 1998 ->
  1950 ns, swing 60 -> 63 ns, +9% shard size. The acquire/release atomics keep LLVM from merging
  or moving the loads, so inlining only moves the call.
- Load CSE in the generator: in `lift_31` only 1.2% of the dword loads repeat an address with no
  store, call or register write in between (the s4 ports' CSE gains came from their frame
  array, not from repeated loads).
- A cheaper helper body: the aligned fast path is already 11 instructions (`lean-access` took
  it from 2.02 to 1.77 ns); `page_table()` is a relaxed load and a pointer.
- More opt-level: opt-level 1/2/3 are within 5% of each other on the lifted functions; only 0
  (2.5x slower) is different, and `play-o2` has none.

**What the accesses are.** 52% of the generated code's dword loads and stores are through ESP or
EBP (`esp` 48.5%: stores 26.5% and loads 22.0% of all accesses; `ebp` 3.5%), the stack, which is
one contiguous region per thread. Those do not need the page table.

**The change: a stack window** (commit "L190: stack window ..."). Every guest thread's `Cpu` carries
`stack_lo / stack_span / stack_host`: the top 256 KiB of its guest stack, backed by ONE contiguous
host block that the page table's entries point into (`PageTable::install_external`,
`GuestMemory::install_window`), so the table and the window serve the same bytes. The lifter output
uses `emit::stack_u32 / stack_set_u32 / push_s / pop_s` for ESP/EBP-based dwords, push and pop:
inside the window one range check and a plain load or store, outside it (or with the feature off)
`m.fast_u32` as before. The text pass is `research/native/stack_window.py`; it is applied to the
1,468 tracked shard files that contain such accesses (1,306,065 sites) and hooked into
`lift.py gen` (`LIFT_STACK_WINDOW=0` disables). The tracked shards were not regenerated: `lift.py
gen` with the committed hot list reproduces them except 756 files of shift formatting (4.9k lines,
about 12 minutes), so the pass was applied to the tracked text instead.

- Features: `ddnative-base/stack-window` (off by default: with it off the helpers are one trivial
  call that inlines to the old code, so opt-level-0 builds are unchanged), `ddruntime/stack-window`
  (sets the window in `run_thread` and `Session::call`; `DDRUNTIME_STACK_WINDOW=0` runs without it).
- Prototype on the recorded cases (O3 harness, lifted vs lifted with the window): `B89790` 1.08x,
  `B89FE0` 1.07x, `B8A770` 1.26x, `FA5860` 1.22x. These functions are call- and port-heavy; code that
  is stack-heavy gains more. At the share above (52% of accesses, 34% of the guest-thread time in
  the two access helpers) the whole-process gain should be a few percent to ten percent; it is not
  measured in game yet.
- Cost: shard compile time +60% and size +16% with the feature on (`lift_31` at opt-level 2: 18 s ->
  29 s, 9.3 -> 10.8 MB).
- Evidence: `crates/ddnative-base/tests/stack_window.rs` (window and table agree at the edges, unaligned,
  outside; an empty window never reaches the host), `l190_bench.rs stack_window_variants_equal_the_originals`
  (1,860 recorded cases: the rewritten lifted bodies give the same registers and memory as the
  originals with the window installed), `cargo check -p ddnative` (all 33 shards compile), the gate's
  `cargo test -p ddnative -p ddnative-base` with the feature off.

**To measure it.** In `l172_replay_build.sh` the ddplay build line becomes
`cargo build -q --profile play-o2 -p ddplay --features emu,ddruntime/parallel-shadow,ddruntime/stack-window`
(`ddruntime/stack-window` enables `ddnative-base/stack-window`). A/B: the same exe with
`DDRUNTIME_STACK_WINDOW=0` (window never installed: the helpers fall to the page table) against
the default. Merge `task/L190-hot-paths`; `ddruntime/src/threads.rs` gets a 3-line hunk in `run_thread`.

## 12. State and next steps (end of the L190 night shift, 2026-10-14)

**Done and committed (branch `task/L190-hot-paths`, merge it whole):**
- This document; `scripts/profile_units.py`, `scripts/hot_list.py`, `autorun3.sh` (repo sampler with self time per lifted function).
- Ports (`mtcore::semantic_l190`, registered in `ddports`): `FA5860` 1.9x, `B8B800` 17x (partly its bulk copy), `B89790` 2.3x, `B89FE0` 1.9x, `B8A770` 2.0x. Rejected: `B6A070` 0.91x, `B8B430` (setup path), `F20640` soft body (equal on 60k states, 1.0x: 35 guest calls).
- `semantic_rt` NaN fix (SSE returns the first operand when both are NaN).
- `ddnative-base` `lean-access` (default on): measured 2.02 -> 1.77 ns per dword access. `inline-access`: no gain, off.
- `stack-window` (off by default, sections 11): shards rewritten, runtime installs the window. NOT yet measured in game.

**Gate status:** `cargo test -p ddnative -p ddnative-base` passes (vectors required); the second half (`mtmath mtcore mtframe mtdraw ddports mtstate`, built with `--features ddnative-base/stack-window`) passes except the two known `semantic_s5` failures (its vectors are newer than the port; fails on the base too). `scripts/inventory.py` was not run.

**Next steps, in order:**
1. Measure `stack-window` in game (section 11, "To measure it"). If it gains, make `ddruntime/stack-window` a default feature; if the exe crashes or a thread stops, `DDRUNTIME_STACK_WINDOW=0` is the quick off switch and `git revert` of the shard rewrite is not needed (the feature off compiles to the old calls).
2. Capture a fresh profile with `bash C:/ddda-targets/L172/runs/autorun3.sh EXE TAG` on the current build, then `python scripts/profile_units.py stacks_TAG.json` and `python scripts/hot_list.py stacks_TAG.json --top 1000 --out hot.txt`: the old captures pre-date most ports.
3. Memory helpers are still 44% of guest time. Ideas not yet tried: a second window for the thread's TEB/TLS; `DDNATIVE_BACKING=regions|large` (existing env, 2 MB regions or large pages: fewer host TLB misses; untested on the game); caching `page_table()` in `Cpu` instead of the dyn call at every function entry (2.8%, needs a lifter change); a one-entry last-page cache for ESI/EDI object accesses (needs invalidation on `set_perm`/`unback`).
4. Port only bodies with few guest calls (under ~10 per invocation): count the calls first. Candidates left in the shadow units are small or call-bound (section 9).
5. Regenerating shards: `lift.py gen` reproduces the tracked shards except 756 files of shift formatting; do a deliberate regen + gate before changing the lifter, with `LIFT_DATA`/`INVENTORY_DB` copies as in `scripts/l161_gate.sh` (about 12 minutes, 1.5 GB).

**Commands.** Tests: `CARGO_TARGET_DIR=C:/ddda-targets/L190 cargo test --release -p mtcore --test semantic_l190` (ports), `... --test rt_nan`, `... -p ddnative-base --features stack-window --test stack_window`, `... -p mtcore --features ddnative-base/stack-window --test l190_bench stack_window_variants -- --nocapture`. Benches (ignored tests): `--test l190_bench -- --ignored --nocapture --test-threads=1`, `--test helper_bench`. Vectors: `E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/record_l190_*.py OUT.json N` into `E:/ddda-re/notes/native-L190/`. Stack-window rewrite: `python research/native/stack_window.py in.rs out.rs`.

