# L79: unlinked pixel inputs

Assigned by the head/user on `task/L79-wgsl-unlinked-input`, based on
`task/opt-int` at `846b7c6e`. Scope: d3d9wgsl correctness and shader/driver
regressions. Never run the game or build the lifted shards.

## Results

**Implemented the driver-backed COLOR0 fallback fix**, after the head
approved the stale-claim override. The separate reported
`v0 undefined` validation failure has not been reproduced on this branch.

### Findings and regression

- The unchanged translator validates all **60,524 default/backend variants
  of 29,198 installed shaders**, with zero failures. It declares private
  variables for every DCL input independently of vertex-stage linkage.
  This declaration behavior dates to `7031a3677`.
- A scan using the existing package-extraction method found **27,070
  distinct installed pixel shaders, zero reading undeclared inputs**.
- Missing **COLOR0** reads exactly **(1,1,1,1)** on the host D3D9 driver.
  Missing COLOR1, TEXCOORD0, and TEXCOORD7 read (0,0,0,0). Checked on v0,
  v3, and v9. Float render targets avoid confusing clamped values with 1.
- The installed **root:2681fe2cf6b72f46** shader reads COLOR0 in v0.xy.
  When unlinked, D3D9 returns (1,1,0,1) but the old translator returns
  (0,0,0,1). The test reads this shader from the player's installation;
  no game bytecode is committed.
- Removing DCL reproduces WGSL's unknown-identifier error, but D3D9
  rejects those same synthetic shaders at CreatePixelShader with
  `0x8876086c` (D3DERR_INVALIDCALL). That is not evidence for accepting
  undeclared inputs or proof of the reported runtime failure's cause.

### Specification versus driver behavior

[Microsoft's SM3 matching rules](https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/shader-model-3)
call a pairing invalid when a pixel input semantic has no matching vertex
output. The [ps_3_0 register table](https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/dx9-graphics-reference-asm-ps-registers-ps-3-0)
requires DCL and lists no input default. The fallback is therefore
**measured host-driver compatibility**, not a portable documented guarantee.

### Change

Initialize unlinked COLOR0 registers to vec4(1), preserving the existing
zero initialization for other semantics and the assignment of linked
inputs. This depends on semantic usage/index, not register number or a
particular shader. Document the driver evidence in Options and at the
assignment. Tests also cover D3D9 rejection of undeclared inputs.

The regression in `tests/support/unlinked.rs` fails against the old source
and passes with the fix, including the shader read from root.arc. It uses
float render targets, so the white value is verified as exactly 1.0.

### Pairing coverage

`tests/support/pairings.rs` computes input locations by the same semantic /
index matching as ddgpu::WgpuBackend::try_draw. Its CPU-only installed
sweep pairs every v0/COLOR0 pixel shader with every distinct installed VS
output signature lacking COLOR0, deduplicating identical linkage maps.
It validates the representative vertex modules and every linked pixel
variant with alpha test, LOD bias, and depth bias enabled.

This is a **superset of possible pairings**, not evidence that gameplay
submitted each one. Cross-package signatures are included. The all-install sweep passed: **21,744 v0/COLOR0 pixel shaders**,
**10 VS signatures lacking COLOR0**, and **153,742 distinct shader/linkage
maps validated**, with zero failures (1293.49 s).

For exact runtime pairings, a second CPU-only test accepts a private frame
capture and validates all distinct VS/PS pairs, reporting frame/draw
numbers, shader tags, and missing input mappings:

```powershell
$env:CARGO_INCREMENTAL='0'
$env:CARGO_TARGET_DIR='C:/ddda-targets/L79'
$env:CARGO_BUILD_JOBS='4'
$env:DDDA_SHADER_PAIR_CAPTURE='E:/private/gameplay.ddframe'
cargo test -p d3d9wgsl captured_shader_pairings_validate -- --nocapture
```

No .ddframe was found in the research tree, worktrees, or target directory;
the head/claude-2 have been asked for the failing run's capture. The
capture checker is compiled but **has not run on a real game capture**.
No game was launched to generate one.

### Verification

- Baseline d3d9wgsl suite: 14 passed before adding the COLOR cases.
- Candidate focused suite: 3 passed, including all 12 synthetic semantic /
  register combinations and the installed root shader, using real D3D9
  and wgpu adapters.
- Candidate full d3d9wgsl suite: **14 passed** (74.34 s).
- Candidate all-shader validation: **60,524 variants passed, zero failures**
  (276.33 s), covering all 29,198 distinct installed shaders.
- ddgpu: cache 2, draw 13, frame 3, MSAA 3, presentation 5 passed; existing
  exploratory tests remain ignored as before. The target suite twice
  crashed with STATUS_ACCESS_VIOLATION at depth_precision after three
  tests passed. Depth precision passes alone (155.73 s); MRT, render-to-texture
  (44 cases), and target-format (200 cases) tests also pass in fresh
  processes. Thus all 33 active ddgpu tests passed across runs, but the
  combined target suite is not clean. No production ddgpu changes.
- Final workspace source compiled and the CPU-only unlinked-register test
  passed after the installed pairing sweep. The test harness now accepts
  WGPU_BACKEND (e.g. vulkan) without changing validation flags. This
  backend-selection option has not been GPU-tested in L79.
- No game run, lifted-shard build, or visual comparison was performed.

### GPU failures reported to head

- `C:/ddda-targets/L79/debug/deps/targets-1b47e9023427a93c.exe`
  (`--test-threads=1`, repeated with `--nocapture`) exited with
  **0xc0000005 / STATUS_ACCESS_VIOLATION** while executing
  `depth_precision_matches_the_d3d9_driver`, after the self-target, clear,
  and cube-target tests passed. Logs: `L79-ddgpu.log` and
  `L79-targets-retry.log` in `C:/ddda-targets`. Both were process crashes,
  with no Rust assertion or device-lost diagnostic. No matching Application
  Error event identified the faulting module, so GPU-driver attribution
  is not established. The depth test passes alone.
- The post-application workspace rerun (`against_d3d9-66d8418bcb0705bf.exe`)
  stalled before the first arithmetic test completed, about 0.52 s CPU
  over several minutes. The owned process was stopped at the bounded
  limit; exit 0xffffffff is our termination, not a spontaneous crash.
  `pause.py status` was **not paused** before launch and during the stall.
  L83 independently reported a similar pre-adapter-info timeout.
  Log: `C:/ddda-targets/L79-workspace-final.log`.
- Board reports: 10089 (target crash details), 10152 (workspace stall).
  No more GPU suites were run after the stall; the final pause status
  remained paused, so no GPU retry was attempted. The earlier candidate
  tests used the identical translator implementation and passed; the
  latest workspace GPU suite is **not claimed as passing**.

**Next action:** head reruns the affected game pairing with this fix and
supplies a .ddframe (or the exact shader pair/build revision) if the
undefined-v0 diagnostic persists. Offline pair validation requires no GPU.
