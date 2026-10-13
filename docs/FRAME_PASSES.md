# Gameplay frame pass map

L122, 2026-10-13. Addresses refer to the Steam executable identified in
[METHODOLOGY.md](METHODOLOGY.md). This is capture analysis plus static producer
research, not a native implementation or an image-equivalence result.

## Measurement and confidence

Representative frame: **cap1 original frame 7539**, zero-based, the first
complete frame of L111's analytical slice. It has **5,295 draws, 951 target
runs, 15 clears and 8 copies**. Runs 0..863 are 864 one-draw simulation
passes: 624 at 64x8 and 240 at 64x16, RGBA32F. The remaining 87 runs have
4,431 draws. The 20-frame population is 951..991 runs and 5,295..5,436
draws; the ~977 figure in the initial plan is not this particular frame.
See [L111](tasks/L111-executor-stats.md) for the extraction and population.

Pass numbers below are **capture target-run ordinals**, not engine pass IDs
or necessarily backend render-pass submissions. Clear-only runs count;
copies can occur inside a target run. Different systems can share one run:
949 contains God Rays composition, color correction and GUI. Conversely one
system can produce many runs. Never dispatch a native replacement by ordinal
or target dimensions alone.

Evidence levels:

- **Measured**: target descriptors, command/draw counts, exact shader token
  hashes and resource bindings in the capture.
- **Shader-supported**: CTAB parameter/sampler names and shader declarations
  identify the operation family. This is not a captured producer call stack.
- **Static producer**: DTI virtual slots and reviewed Ghidra functions establish
  the corresponding game's issuing path. The remaining dynamic join between
  a particular object and captured command is explicitly unmeasured.

Shader hashes are the first 12 hexadecimal digits of SHA-256 over the exact
little-endian D3D token bytes, including comments and END. All **402** full
hashes in this frame have distinct prefixes (218 VS, 184 PS; see verification
below if those stage counts are revised). These are analysis identities,
not the engine's resource hash at object+0xC. Of the 402 blobs, **334** match
byte-for-byte inside installed `sa/DX9/root.arc`'s `sc/DX9/root` package.
Those matching SPK records contain bytecode length/offset and a zero key,
not a recoverable entry-point name. CTAB names below are actual reflection
names; descriptive operation labels are not asserted to be shader symbols.

## Corrections to the coarse target-size map

- **8x8 and 32x32 RGBA16F are sky scattering here**, with `CBSky`,
  `tRayleighDepthMap` and `tMieDepthMap`; they are not evidence for tone mapping.
- The three chains ending at 1x1 are **material/texture mip generation**,
  with `fMipLevel`, `fMipTargetColor`, `fMipTargetRate`. They precede the main
  scene and their textures are later sampled by material draws. Calling them
  scene luminance reduction is unsupported.
- Runs 938..942 are **God Rays**, using `CBGodRaysFilter` and `fGodRayParams`.
  Runs 945..948 are **uDOFFilter**, using `CBDOFFilter`, not evidence for the
  distinct BokehPlug implementation.
- BloomPlug and ToneMapPlug exist in the executable but their characteristic
  bloom/log-average chains are **not positively identified in this frame**.
  Their existence is not permission to relabel another small-target chain.

## Issuing systems and source addresses

The common path is `sRender::vf06` **0x00DAF000**, camera viewport dispatch
**0x00DC4770**, scene preparation **0x00FF63B0**, sorted command publication
**0x00FEE130**, executor **0x00DAB480**. Scene pass selection **0x00FF1090**
binds the 24-slot target table created by **0x011488C0**. The executor's
target/shader applies are **0x00DA6FE0 / 0x00DAA8B0**. See
[NDRAW.md](NDRAW.md) for layouts, sort keys and the command boundary.

| Family | Static issuer evidence | Capture attribution limit |
|---|---|---|
| Sky/scattering/reflection sky | DTI `uSky`, `uSkyExt`, `uSkyReflection` share vf11 **0x00F39290**; it calls **0x00F30E10**, **0x00F2F710**, **0x00F31D70**, **0x00F34390**, **0x00F34A80**. F30E10/F2F710 select package `TSky`. | 873..877 and 914..917 have sky shader evidence; exact instance/view is not captured. |
| Shadow / ShadowPlug family | DTI `uShadow::vf11` **0x00EE2C90** gates on context+0x16A and object+0x78, calls **0x00EDE9C0** and virtual slot+0x60. `uLightSpaceShadow`, `uStableShadow`, `uUniformShadow`, `uSpotShadow`, `uPointShadow` inherit this dispatcher. | 900,903,905 use CBShadowLight/CBShadowCast; 906..913 are their texture-processing tail. Exact shadow subclass/cascade ownership still needs producer telemetry. No DTI class named cShadow was established. |
| God Rays | DTI `uGodRaysFilter` / `uSkyGodRaysFilter` vf11 **0x0104A840** gates object+0xF0 then calls **0x01048750**. That producer schedules pass0x13 and configured filter order and selects `FFilterGodRaysAlpha`, `FFilterGodRaysScale`, `TFilter`; helpers **0x01044100 / 0x01044C00 / 0x01045570**. | Shader-supported 938..942 and first draw of949. |
| DOF | DTI `uDOFFilter::vf11` **0x01013320** selects `TFilter`, uses filter scheduler **0x0100B7A0**, target changes **0x00FF04C0** and filter helpers **0x0100B810 / 0x0100C070**. | 945..948 have CBDOFFilter and depth/DOF samplers; active object/configuration not captured. |
| ColorCorrectPlug | `uColorCorrectFilter::vf11` **0x010460C0 -> 0x010310D0**, conditional pass0x13 priority1000 prepass, followed by configured order. | LUT943: fColorCorrectGamma/fHermiteParam; draw in949: tColorCorrectTableMap/fColorCorrectMatrix. |
| ImagePlanePlug | `uImagePlaneFilter::vf11` **0x01017DB0** selects `TImagePlaneFilter` and `FImageBlend`, tests current pass0xB and emits draw/copy branches. | Second draw944 has fImagePlaneColor; exact producer join remains static+shader evidence. |
| BloomPlug | `uBloomFilter::vf11` **0x01033CA0 -> 0x01032B20**, Scene+0x290 gate, pass0x13/subpriority1, `TBloomFilter`. | Not positively identified among this frame's captured shader families. |
| ToneMapPlug | `uToneMapControl::vf11` **0x01035E50 -> 0x01034620**, Scene+0x290 gate, pass0x13/priority10000, `TLogAverage`/`TFilter`. | No positive log-average chain identification in7539; HDR decode/copy alone does not establish this producer. |
| BokehPlug | `uBokehFilter::vf11` **0x0103E6B0**, `FBokehBleeding`, `FBokehDefaultFarZ`, mask variants and multiple targets. | Not positively identified; keep distinct from uDOFFilter. |
| Scene models, particles/effects, water | Scene producer **0x00FF63B0**; material-specific commands converge on the common executor. DTI `uWater::vf11` **0x00F90F60** is a candidate producer, not a demonstrated issuer of every reflective material. | 928 includes a shader with tReflectWaterMap; 932 has depth-aware primitive/effect shaders. Neither defines a standalone universal water/particle pass. |
| GUI/overlay/display | **0x00DAF000** schedules Overlay and SysMenu after camera work; **0x00DAB480** executes their sorted commands. | 949 contains421 SSGUI draws plus2 generic textured draws and2 post draws; 950 uses fGamma to the backbuffer. |

Plug names themselves are allocation/group names registered at
**0x00740860**, not DTI classes or a fixed execution order. The general
filter scheduler **0x0100B7A0** uses filter+0x30 for pass and +0x34 for
priority. Preserve the captured/sorted stream and the original producers.

## Reading the sequence tables

`A8` = A8R8G8B8 (21), `F32` = A32B32G32R32F (116), `F16` =
A16B16G16R16F (113), `R32` = R32F (114), `G16R16` = format34;
depth is D24S8 (75). `Pnnn` means a prior target run writing the sampled
resource. `external` means no earlier writer observed in this selected
frame: installed texture, previous-frame resource or omitted earlier upload.

Read edges include only texture slots declared as samplers in the actual VS
and PS; stale bound slots are excluded. They are **potential shader reads**,
not per-pixel execution proof. The edge tracks the resource, not a proven
sampled mip/face; mip-chain edges therefore denote latest writes to that
texture, and do not exclude access to older levels. Attachment load/blend,
depth tests and CPU constant inputs are additional dependencies. Copies are
listed separately and preserve ordering even within one target run.

