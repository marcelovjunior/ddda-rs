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
Next action: independently review the source, then inspect the queued
batch's actual images/exits and measurements. No gain claimed.
