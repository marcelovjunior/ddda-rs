# S41: opt-in material feature specialization

Branch: `step/s41-material-specialization`, based on `1dd6fb1`.
Approved plan: board #6117/#6123, head #6134.

The default remains the GPU mesh builder and uniform-driven material shader.
`DDVIEW_MATERIAL_SPECIALIZATION=1` adds the material's full current feature
flags to `DdPipelineKey` and supplies them as an unsigned shader definition
in both vertex and fragment stages. The shader selects a local constant
feature alias when defined, otherwise the existing `dd.flags` uniform.
Only feature tests change; equations, numerical uniforms, texture bindings,
geometry, animation, LOD and fixed 30 Hz simulation remain unchanged.

## Evidence and hypothesis

- The prior static-field trace's opaque pass was 4.393 ms; animation and
  streaming were much smaller in that standalone scene. This justifies
  investigating material shader work, not claiming all opaque time belongs
  to these feature branches. Pipeline specialization is an unmeasured
  optimization hypothesis until an identical-picture comparison succeeds.
- `dd_material.wgsl` has eighteen uniform feature tests across vertex and
  fragment entrypoints. A constant alias allows the compiler to discard
  unused branches without changing the surviving branch's equations.
- Pinned Bevy 0.19.1 `MaterialExtension::specialize` receives
  `MaterialExtensionKey::bind_group_data`, derived from the extension's
  `AsBindGroup` key; `DdExt` already uses this for imported depth state.
  Full current flags include runtime `FLAG_BLENDED` fading and resolved
  `FLAG_SKY_ENV`/`FLAG_DD_LIGHTING` updates. Lighting values remain uniforms,
  so changes to sun/SH/sky numerical values reuse an existing feature key.
- Pinned `naga_oil` 0.22.0 `compose/preprocess.rs` checks `#ifdef` by key
  presence, so the unsigned definition with value zero remains defined.

## Verification status

Source `1a0911b` passed the full `cargo test --release -p ddview` suite:
**84 passed, 0 failed, 2 existing ignored**, compilation 43.71 s / tests
14.24 s. This includes the three regressions for actual character fade/restore
and concurrent sky-flag changes, stable keys under lighting-value changes,
and matching vertex/fragment definitions (including vertex-only descriptors
and zero flags). Default `cargo build --release -p ddview` completed **exit
0**, 59.93 s; combined session 97776 actually exited 0. Existing
private-interface and unused audio-method warnings remain. These headless
tests do not compile the WGSL on a real adapter.

Private pin: `E:/ddda-re/notes/steps/s41/ddview-material-1a0911b.exe`, SHA-256
`9B9BD7C428AC8C2200AB33AB1387EB7C1143A82A7EEA942E83887D7A77E097CE`.
The M5 lease was released after actual exit and pinning. Harness commit
`f98aad7` adds explicit `-MaterialSpecialization off|on` (default off), records
the mode in `.run.json` and restores the caller's environment. PowerShell
parse and `git diff --check` passed; no additional product source changed.

The separate CPU experiment's two captures both failed visual equivalence;
their lower frame times are invalid as optimization evidence. They are not
this candidate's baseline. No shader-candidate GPU process or capture has
been launched. Acceptance requires one pinned executable, GPU builder on
both sides, identical scene/camera/counts, actual exits and visually inspected
PNGs before comparing preselected warm frame/pass windows. New feature keys
may increase pipeline compilation and cache size; startup/stutter numbers
must be reported alongside steady-state frame times.

One bounded off/on request is queued with head, after its integration route:
same pin and GPU builder on both sides, default backend, stage 100/cell64,54,
same spawn/camera/two players/clock12 as the previous field comparison,
1080p, screenshot at 45 s, timer 50 s and watchdog 90 s per process. Private
prefixes `field-material-{off,on}-1a0911b` keep the baseline and candidate
separate. Require PNG fidelity and identical counts before comparing warm
20–40 s frames, GPU passes and stutters. CPU mode is not part of this batch.
### Actual off/on comparison: no measured gain

Head completed the requested batch (board #6266): off PID 33632 exited
**0**, 52.4 s; on PID 33512 exited **0**, 52.5 s. Both private exit files
contain zero and run metadata records the same pinned executable/hash,
GPU builder, scene/camera and explicit off/on specialization mode. Both
logs have no ERROR entry. No process/build/GPU lease remains owned here.

Codex-66 actually viewed both PNGs before comparing timings. Both have
intact terrain, two bodies, shadows and HUD. These two captured pictures
are visually equivalent, not byte-identical: 2,019 of 2,073,600 RGB pixels
differ, only 23 have a maximum channel difference over 10; mean absolute
channel differences are 0.000912/0.000751/0.000581 on the 0–255 scale.
This is a static field snapshot, not full-route/fading/night fidelity.

Preselected warm 20–40 s windows, reports ending at approximately
25/30/35/40 s:

| Metric | Off | On |
|---|---|---|
| Frame medians (ms) | 6.7 / 6.7 / 6.7 / 6.7 | 6.9 / 6.7 / 6.6 / 6.7 |
| Median of frame medians (ms) | **6.7** | **6.7** |
| Frame p99 (ms) | 9.8 / 9.6 / 10.3 / 10.0 | 10.6 / 9.6 / 10.3 / 10.9 |
| Frame worst (ms) | 10.9 / 10.6 / 11.8 / 12.0 | 11.6 / 11.4 / 12.6 / 14.9 |
| Opaque GPU average-report median (ms) | 3.395866 (5 reports) | 3.420587 (3 reports) |
| Bin-unpack GPU average-report median (ms) | 0.509773 (4 reports) | 0.520794 (4 reports) |

GPU values are medians of the diagnostic logger's reported averages,
not a reconstructed per-frame GPU distribution; diagnostic schedules
produce different report counts. Neither frame nor opaque GPU timing
shows a useful improvement. **Keep specialization off by default.**

Both sides report 1,908 entities / 982 mesh entities / 365 visible meshes
(263 unique) / 208 visible DD materials (745 allocated) throughout the
warm windows. Each has zero warm frames above 16.67/33/50 ms. The first
10 s after perf startup contain four off / five on frames above 50 ms,
worst 171.5 / 133.6 ms; the first window's p99 is 11.4 / 31.7 ms. Startup
order/cache variation prevents attributing these differences to compilation.
The two screenshot entities raise the final report to 1,910 after the warm
window; they do not change its counts.

Four UTC-aligned warm GPU samples per side retain original PID 23408 at
0% Windows 3D share. Viewer share is 48–53% off / 52–78% on. NVIDIA
clocks are stable 2745/8501 MHz, P0, utilization 86–96% / 91–97%.
Windows-engine and NVIDIA utilization are different metrics; their
difference is not evidence for an unidentified GPU process.

The scoped hypothesis that feature specialization usefully reduces this
static field's opaque/frame cost is disproven by this batch. It does not
prove that materials are inexpensive or establish full-route 180 fps:
6.7 ms is about 149 fps, still above the 5.5 ms target. No optimization
default is accepted. The independent CPU startup correction remains a
separate diagnostic experiment pending review and actual picture validation.

Harness follow-up `5488338` removes the unnecessary timer/watchdog ordering
guard and records the time-scale environment. `DDVIEW_PERF_SECONDS` uses
`Instant` wall time after its first update; `--shot-delay` uses virtual time.
The launch-relative OS watchdog stays independent. Both owned harness
branches passed PowerShell parse and diff checks; product pins did not change.

Next action: independent review of these images/source/results and the
separate CPU startup contract; then select the next evidence-backed
optimization. Ordinary stage material extraction initializes each `DdSlot`
once and removes `PendingDd`; animation pose allocations are on requested
motion changes, while their measured steady-frame sections are tiny. A
future relight-stutter trace should focus on render preparation around
clock updates, without altering the source-derived rate/lighting quality.
