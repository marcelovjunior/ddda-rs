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
hashes in this frame have distinct prefixes (225 VS, 177 PS). These are analysis identities,
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

SoftBody is positively named by captured reflection (`CBSoftBodySim`,
`CBSoftBodyCollision`, `tSoftBodySrcTex1..4`), not inferred from size. L113
owns its producer/algorithm mapping and replacement. The six PS counts in
0..863 are `e345ff88f547`:704, `ecea700eef8c`:80,
`1055ef87dedd`:40, `d43f1431cf70`:20, `61a8b0a10d70`:10,
`f52c4e32c4e4`:10; all use VS `dd7cbb612465`. Their output resources
are sampled by vertex shaders in later shadow and scene draws. The document
does not establish that the loop is removable or equivalent to a CPU solver.

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

Consumer sampler names disambiguate several targets: `@2f8f24f0` is sampled
as `tAlbedoMap`; `@2f858b70` as `tDetailNormalMap`; `@2f93b830` as
`tNormalMap`. Thus the three mip chains are material textures. The exact
unit producing their base images (864,878,887) remains unresolved.
`@22fa6d80/@33504b50` feed `tRayleighScatterMap`, while
`@2332b8b0/@33504c50` feed `tMieScatterMap`, establishing the output
names rather than guessing their order. Main-scene `@2076c0b0` is sampled
as `tGBuffer`, `@2076beb0` as `tLightMaskMap`, `@2076bfb0` as
`tAmbientMaskMap`. Run925 is therefore ambient-mask generation.

Run935 reads the half-resolution effects output934 but writes a full-size
texture which was already populated by copy4816 from the main scene.
Its attachment load/blend dependency on that copy is essential: the texture
read column alone is not a complete render graph.


## Ordered target runs

Each row covers every draw in that run. Shader sets below list **all** VS/PS
hashes, not just the most common shader. `@` identifies a capture-local texture
resource (or standalone surface); `/L` is captured subresource level. These
addresses are not stable across runs and must never become runtime selectors.

| Runs | Operation | Draws | Targets | Reads | Shaders |
|---|---|---:|---|---|---|
| 0..863 | SoftBody GPU simulation (L113) | 864 | 624 runs64x8 +240 runs64x16 F32; initial MRT then ping-pong; no depth | Previous-frame/input textures and earlier loop targets | S0 |
| 864 | Texture/effect source (issuer unresolved) | 3 | 256x256 A8 @2f8f24f0/L0 +depth 256x256 D24S8 @26073c40/L0 | external | S1 |
| 865 | Texture mip generation | 1 | 128x128 A8 @2f8f24f0/L1 | P864 | S2 |
| 866 | Texture mip generation | 1 | 64x64 A8 @2f8f24f0/L2 | P865 | S2 |
| 867 | Texture mip generation | 1 | 32x32 A8 @2f8f24f0/L3 | P866 | S2 |
| 868 | Texture mip generation | 1 | 16x16 A8 @2f8f24f0/L4 | P867 | S2 |
| 869 | Texture mip generation | 1 | 8x8 A8 @2f8f24f0/L5 | P868 | S2 |
| 870 | Texture mip generation | 1 | 4x4 A8 @2f8f24f0/L6 | P869 | S2 |
| 871 | Texture mip generation | 1 | 2x2 A8 @2f8f24f0/L7 | P870 | S2 |
| 872 | Texture mip generation | 1 | 1x1 A8 @2f8f24f0/L8 | P871 | S2 |
| 873 | Sky Rayleigh scatter | 1 | 8x8 F16 @22fa6d80/L0 | external | S3 |
| 874 | Sky Mie scatter | 1 | 8x8 F16 @2332b8b0/L0 | external | S4 |
| 875 | Sky auxiliary clear | 0 | 8x8 F16 @22c22240/L0 | - | - |
| 876 | Starry-sky color | 1 | 128x1 A8 @33159120/L0 | - | S5 |
| 877 | Reflection sky/clouds | 12 | 128x128 A8 @2f93c390/L0 +depth 128x128 D24S8 @2066aa10/L0 | P873, P874, P876, external | S6 |
| 878 | Texture/effect source (unresolved) | 2 | 128x128 A8 @26073c80/L0 +depth 128x128 D24S8 @26073c90/L0 | external | S7 |
| 879 | Normal-map conversion | 1 | 128x128 A8 @2f858b70/L0 | P878 | S8 |
| 880 | Texture mip generation | 1 | 64x64 A8 @2f858b70/L1 | P879 | S9 |
| 881 | Texture mip generation | 1 | 32x32 A8 @2f858b70/L2 | P880 | S9 |
| 882 | Texture mip generation | 1 | 16x16 A8 @2f858b70/L3 | P881 | S9 |
| 883 | Texture mip generation | 1 | 8x8 A8 @2f858b70/L4 | P882 | S9 |
| 884 | Texture mip generation | 1 | 4x4 A8 @2f858b70/L5 | P883 | S9 |
| 885 | Texture mip generation | 1 | 2x2 A8 @2f858b70/L6 | P884 | S9 |
| 886 | Texture mip generation | 1 | 1x1 A8 @2f858b70/L7 | P885 | S9 |
| 887 | Texture/effect source (unresolved) | 3 | 256x256 A8 @2066a1b0/L0 +depth 256x256 D24S8 @2066aa00/L0 | external | S10 |
| 888 | Texture conversion | 1 | 256x256 A8 @2f93b830/L0 | P887 | S11 |
| 889 | Texture mip generation | 1 | 128x128 A8 @2f93b830/L1 | P888 | S9 |
| 890 | Texture mip generation | 1 | 64x64 A8 @2f93b830/L2 | P889 | S9 |
| 891 | Texture mip generation | 1 | 32x32 A8 @2f93b830/L3 | P890 | S9 |
| 892 | Texture mip generation | 1 | 16x16 A8 @2f93b830/L4 | P891 | S9 |
| 893 | Texture mip generation | 1 | 8x8 A8 @2f93b830/L5 | P892 | S9 |
| 894 | Texture mip generation | 1 | 4x4 A8 @2f93b830/L6 | P893 | S9 |
| 895 | Texture mip generation | 1 | 2x2 A8 @2f93b830/L7 | P894 | S9 |
| 896 | Texture mip generation | 1 | 1x1 A8 @2f93b830/L8 | P895 | S9 |
| 897 | UI-consumed texture/effects | 10 | 384x384 A8 @335089f0/L0 | external | S12 |
| 898 | Cube-map filter | 1 | 32x32 A8 @223bf180/L4 | external | S13 |
| 899 | Cube-map filter | 1 | 32x32 A8 @223be960/L5 | external | S13 |
| 900 | Local shadow casters | 17 | 192x128 G16R16 @20669ca0/L0 +depth 192x128 D24S8 @20669c80/L0 | external | S14 |
| 901 | Cube-map filter | 1 | 32x32 A8 @223bed70/L4 | external | S13 |
| 902 | Cube-map filter | 1 | 32x32 A8 @22233790/L4 | external | S13 |
| 903 | Shadow atlas casters | 489 | 512x1536 R32 @33450820/L0 +depth 512x1536 D24S8 @33450990/L0 | SoftBody, external | S15 |
| 904 | Cube-map filter | 1 | 32x32 A8 @223bf590/L4 | external | S13 |
| 905 | Local shadow casters | 15 | 192x128 G16R16 @20406520/L0 +depth 192x128 D24S8 @20406580/L0 | SoftBody, external | S16 |
| 906 | Shadow texture transform | 1 | 192x128 G16R16 @20669cb0/L0 | P900 | S17 |
| 907 | Shadow texture transform | 1 | 192x128 G16R16 @20669ca0/L0 | P906 | S17 |
| 908 | Shadow texture transform | 1 | 192x128 G16R16 @20380ac0/L0 | P905 | S17 |
| 909 | Shadow texture transform | 1 | 192x128 G16R16 @20406520/L0 | P908 | S17 |
| 910 | Shadow variance filter | 6 | 192x128 G16R16 @20669cb0/L0 | P907 | S18 |
| 911 | Shadow variance filter | 6 | 192x128 G16R16 @20669ca0/L0 | P910 | S19 |
| 912 | Shadow variance filter | 6 | 192x128 G16R16 @20380ac0/L0 | P909 | S18 |
| 913 | Shadow variance filter | 6 | 192x128 G16R16 @20406520/L0 | P912 | S19 |
| 914 | Sky Rayleigh scatter | 1 | 32x32 F16 @33504b50/L0 | external | S20 |
| 915 | Sky Mie scatter | 1 | 32x32 F16 @33504c50/L0 | external | S21 |
| 916 | Sky auxiliary clear | 0 | 32x32 F16 @33504d50/L0 | - | - |
| 917 | Starry-sky color | 1 | 128x1 A8 @33158df0/L0 | - | S5 |
| 918 | Scene depth/clip material phase | 698 | 1920x1080 A8 @2076bbd0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, external | S22 |
| 919 | Scene normal/material phase | 1354 | 1920x1080 A8 @2076c0b0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, P886, external | S23 |
| 920 | Shadow receive/mask phase | 75 | 1920x1080 A8 @2076beb0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, P903, P918, external | S24 |
| 921 | Light accumulation phase | 9 | 1920x1080 A8 @2076c1b0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | P911, P913, P918, P919, external | S25 |
| 922 | Half-resolution scene/material MRT | 145 | 960x540 A8 @2076c3a0/L0; 960x540 A8 @2076c4a0/L0 +depth 960x540 D24S8 @2076c320/L0 | SoftBody, P886, P918, external | S26 |
| 923 | Half-resolution lights | 8 | 960x540 A8 @2076c5a0/L0 +depth 960x540 D24S8 @2076c320/L0 | P911, P913, P922, external | S27 |
| 924 | Scene auxiliary clear | 0 | 1920x1080 A8 @2076bfb0/L0 | - | - |
| 925 | Scene auxiliary depth-reading phase | 2 | 1920x1080 A8 @2076bfb0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | P918, external | S28 |
| 926 | Main color initialization | 1 | 1920x1080 A8 @2076b410/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | - | S29 |
| 927 | Depth/ROP helper | 1 | 1920x1080 A8 @2076bbd0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | - | S30 |
| 928 | Main scene + reflective materials | 814 | 1920x1080 A8 @2076b410/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, P877, P886, P896, P899, P902, P914, P915, P917, P920, P921, P925, P927, external | S31 |
| 929 | Depth/ROP helpers | 3 | 1920x1080 A8 @2076bbd0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | - | S30 |
| 930 | Transparent/DSF materials | 4 | 1920x1080 A8 @2076b410/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | P872, P886, P899, P922, P923, external | S32 |
| 931 | Depth/ROP helper | 1 | 1920x1080 A8 @2076bbd0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | - | S30 |
| 932 | Scene + particles/effects | 181 | 1920x1080 A8 @2076b410/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, P920, P922, P923, P931, external | S33 |
| 933 | Scene depth/clip material tail | 72 | 1920x1080 A8 @2076bbd0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | SoftBody, external | S34 |
| 934 | Half-resolution depth + effects | 4 | 960x540 A8 @2076c6a0/L0 +depth 960x540 D24S8 @2076c320/L0 | P933, external | S35 |
| 935 | HDR-aware scene copy/composite | 1 | 1920x1080 A8 @2076b9d0/L0 | P934 | S36 |
| 936 | Half-resolution downsample | 1 | 960x540 A8 @2076c7a0/L0 | P935 | S37 |
| 937 | Quarter-resolution downsample | 1 | 480x270 A8 @2076c8a0/L0 | P936 | S38 |
| 938 | God Rays extraction | 1 | 240x135 A8 @3425de70/L0 | P933, P936 | S39 |
| 939 | God Rays radial iteration | 1 | 240x135 A8 @3425ddf0/L0 | P938 | S40 |
| 940 | God Rays radial iteration | 1 | 240x135 A8 @3425de30/L0 | P939 | S40 |
| 941 | God Rays radial iteration | 1 | 240x135 A8 @3425ddf0/L0 | P940 | S40 |
| 942 | God Rays filtered output | 1 | 480x270 A8 @3425deb0/L0 | P941 | S41 |
| 943 | Color-correction LUT | 4 | 1024x1 A8 @26073c60/L0 | - | S42 |
| 944 | Scene composite + ImagePlane | 2 | 1920x1080 A8 @2076b8d0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | P935, external | S43 |
| 945 | DOF extraction | 1 | 640x360 A8 @3425cde0/L0 | P933, P936 | S44 |
| 946 | DOF filter | 1 | 640x360 A8 @3425ce20/L0 | P945 | S45 |
| 947 | DOF filter | 1 | 640x360 A8 @3425cde0/L0 | P946 | S46 |
| 948 | DOF composite | 1 | 1920x1080 A8 @2076b8d0/L0 | P933, P947 | S47 |
| 949 | God Rays + color correction + GUI | 425 | 1920x1080 A8 @2076b8d0/L0 +depth 1920x1080 D24S8 @2076bcd0/L0 | P897, P942, P943, C4912 from P949, external | S48 |
| 950 | Gamma/display output | 1 | 1920x1080 A8 @200dd7a0/L0 | C5337 from P949 | S49 |

### Copies and ordering

Command indices are zero-based within frame7539. `C4912 from P949` is
intentional: the God Rays draw occurs in949 before a copy used by the color
correction draw in the **same** target run. Texture provenance is tracked in
command order, rather than assuming all copies fall between runs.

| Command | Source | Destination |
|---:|---|---|
| 938 | 192x128 G16R16 @20669b60/L0 | 192x128 G16R16 @20669ca0/L0 |
| 939 | 192x128 D24S8 @20669860/L0 | 192x128 D24S8 @20669c80/L0 |
| 1449 | 192x128 G16R16 @20406640/L0 | 192x128 G16R16 @20406520/L0 |
| 1450 | 192x128 D24S8 @20669fc0/L0 | 192x128 D24S8 @20406580/L0 |
| 4816 | 1920x1080 A8 @2076b410/L0 | 1920x1080 A8 @2076b9d0/L0 |
| 4905 | 1920x1080 A8 @2076b8d0/L0 | 1920x1080 A8 @2076bad0/L0 |
| 4912 | 1920x1080 A8 @2076b8d0/L0 | 1920x1080 A8 @2076bad0/L0 |
| 5337 | 1920x1080 A8 @2076b8d0/L0 | 1920x1080 A8 @2076bad0/L0 |

### Exact shader sets

Prefixes are SHA-256 identities as defined above. A set is the union for the
run, not a claim that every VS pairs with every PS in that set. For mixed
material/UI runs the counts and reflection evidence are discussed separately.

- **S0** — VS: `dd7cbb612465`; PS: `1055ef87dedd`, `61a8b0a10d70`, `d43f1431cf70`, `e345ff88f547`, `ecea700eef8c`, `f52c4e32c4e4`.
- **S1** — VS: `4f4df0cfd24d`, `cd5e37078899`; PS: `80ed3f996e22`, `9612c5868b12`, `c6ff63999897`.
- **S2** — VS: `9ef746277e7d`; PS: `bce5826d8216`.
- **S3** — VS: `6c4397fb897b`; PS: `2e7cfba0667b`.
- **S4** — VS: `6c4397fb897b`; PS: `7c5371c7463b`.
- **S5** — VS: `7c2edb3bf0fe`; PS: `40b4a4bd1cbf`.
- **S6** — VS: `1f852f9f6134`, `6da26c0e3add`, `a1c07f8d0470`, `c49b43069cb2`, `fa21d57d6a76`; PS: `15c3e2a74ff2`, `1c05dbea221d`, `39eb029ed6b0`, `a561ea9f7ae0`, `b3be3e58023c`, `b55779837436`.
- **S7** — VS: `4f4df0cfd24d`, `cd5e37078899`; PS: `78e98d01a83d`, `c6ff63999897`.
- **S8** — VS: `9ef746277e7d`; PS: `ee1e79b552ef`.
- **S9** — VS: `9ef746277e7d`; PS: `b3c0edb26c85`.
- **S10** — VS: `4f4df0cfd24d`, `cd5e37078899`; PS: `78e98d01a83d`, `80ed3f996e22`, `c6ff63999897`.
- **S11** — VS: `9ef746277e7d`; PS: `813b5c9f1d56`.
- **S12** — VS: `d04e5973fdc7`; PS: `9612c5868b12`.
- **S13** — VS: `0e1ab0cca9c1`; PS: `fe1ca9c2e8d3`.
- **S14** — VS: `682782d29a1d`, `6b6501b2a123`, `f4d5c07714e5`; PS: `1eb0d40a5148`, `d65225523a09`.
- **S15** — VS: `141fde36259f`, `17ac8ddea0d0`, `3c8d2807e8ad`, `4f4df0cfd24d`, `717cd970fcce`, `76ebc93ba835`, `85db52aeec79`, `9530915fd664`, `9ff6b51c87df`, `c164891ac634`, `c6dbd43b7b72`, `ea8ef4b22f8e`, `f78f90347785`, `fa197d1ef7a7`; PS: `bfc3b651a905`, `c4581111e3e0`, `c6ff63999897`, `f23ecedb7261`.
- **S16** — VS: `02a1f4bcf20a`, `5d3ec29372e7`, `6b6501b2a123`, `ae5ea2cc6623`, `f4d5c07714e5`; PS: `1eb0d40a5148`, `d65225523a09`.
- **S17** — VS: `9ef746277e7d`; PS: `f12186ddfcf9`.
- **S18** — VS: `9ef746277e7d`; PS: `1118ec331d85`.
- **S19** — VS: `9ef746277e7d`; PS: `bf39a42cb327`.
- **S20** — VS: `6c4397fb897b`; PS: `5e92d798065b`.
- **S21** — VS: `6c4397fb897b`; PS: `089597d6a840`.
- **S22** — VS: `04f94d025d60`, `09309cd8730c`, `0bb04e1d917c`, `0caf4e212864`, `101c7583a98e`, `182ee1cb3c82`, `1e50745e6f75`, `1f1dcac8248a`, `2d5043d3ef12`, `322bec4f5dac`, `5415459ec89f`, `57ea2a7f1f28`, `61aa8a46db3e`, `61e1b4208a72`, `67d301a5f1f0`, `8378703c662e`, `880a53703611`, `8962b577210a`, `8b900fa9beac`, `8c99d64bb546`, `8d96ead23376`, `95a7b5a01fe8`, `9ebcd7938844`, `bb46f6b82dfd`, `c0c55dc8f6c9`, `c71e82f5892d`, `c7a9ea23a3c3`, `cce48ca41148`, `cf83f4e25a67`, `d25e10b939ae`, `d6e744784f2e`, `eba32c6895aa`, `ec32c672b9f9`, `ef5961f5d6ab`, `f10bc2e109f0`, `f61ff1a3f9d1`, `f9c5c341c1b1`, `f9e4ced97a8a`; PS: `16277828814a`, `302b3501c57b`, `45c67c0bf0b5`, `7e072adc5420`, `9b01d8f98a10`, `9ec7add6d6d7`, `a9b4b06f5b10`, `bae4960a9596`, `d2625abaa76d`, `ee9e813007a8`.
- **S23** — VS: `045561a0d7a5`, `14b706e98701`, `20ae2c295ad9`, `2db408447616`, `3d343466eac8`, `3df71ed1d60e`, `49b928fe09cc`, `4cf413b9f60b`, `4e5104945552`, `5edde0bbda98`, `5f65b2b36dc3`, `6bf5d119ae07`, `75fe4ab14fac`, `7b03cb11e126`, `7fac798b528c`, `82d61eeee522`, `832ae1086ef7`, `8b2164c99896`, `8d494018e69b`, `8f7e55e2714f`, `93232a47050c`, `9454bb8592a7`, `9468a62d1928`, `96ddff5532c3`, `984027c3db94`, `9867f637366a`, `999d0cf9c2f8`, `9a1d28f6cf30`, `9ae07eb86d97`, `9d01f03fa633`, `a6b6862322c4`, `aa1b33c00946`, `ab17b64b5b4c`, `b135e7ed8db6`, `c490465515a4`, `c64feec542bb`, `c72d93918a02`, `c7f2f23a934c`, `cac28dda6fba`, `ccda5526cf1d`, `cd050293b9a8`, `d11e4f617d1a`, `d14bdce96418`, `d39c7c41763e`, `d6b166947670`, `d9ef4e60ad7c`, `dc8ed3a1cadd`, `e1c30d9df86c`, `e4ab2d55d78b`, `ef7db5bae4be`, `f19c58c86c61`, `f28893184706`, `f48f53d9072e`, `f686c4656051`, `fc136cbd3a58`, `fcf84829c0d9`, `fe65a3e28f41`; PS: `0313424a19c8`, `0e1ddd24a3b0`, `132c29b73d92`, `24b7a7c69d85`, `2d05424db688`, `43774494f846`, `44159d4cde22`, `48c6f04fbf56`, `4994ac0e5bda`, `645948e5c747`, `6492ec7ef15d`, `66a2f034c909`, `6bb7e5a7308d`, `810fb1bae2e8`, `97bec73886df`, `a30c105d9953`, `bb27d7870f43`, `bc0027ebfa2e`, `c30c6a55d664`, `c5d2934a9c65`, `ca8bba09685c`, `d4ee5f8cbd93`, `d9b08990a242`, `e3f7d0e897a3`, `eb521a4b3053`, `ef7b612d7c71`.
- **S24** — VS: `0ae0bdb0a507`, `39cc38c3b2d0`, `6b51a900a3ba`, `7a84a6b835fd`, `ae402082f875`, `ccada3d2b9b8`, `e5b543da04d8`, `ec0139db1436`; PS: `35435eeb5aec`, `cea0b9fe72a6`, `dbc8d14546da`.
- **S25** — VS: `4f4df0cfd24d`, `b97d9de7a2c6`, `ec0139db1436`; PS: `7abb85fde648`, `862e9b59cde4`, `c6ff63999897`, `d321178b41f1`, `f292356eb3dd`.
- **S26** — VS: `0747141a2a75`, `0d7bc4d59992`, `1ae7a36db517`, `215f4530ac54`, `38fbcc35d2a1`, `44c85446b3ba`, `9ef746277e7d`, `c49b43069cb2`, `dd5b57556936`, `e6193462b4e6`; PS: `08f8ea670563`, `13bd23c16509`, `409f3dc1ee92`, `4d594041eac6`, `c29e0adda92e`.
- **S27** — VS: `4f4df0cfd24d`, `b97d9de7a2c6`; PS: `474eeb3d3a3e`, `b26021a02201`, `c6ff63999897`.
- **S28** — VS: `7e6439335635`; PS: `8533b1b89a70`.
- **S29** — VS: `4f4df0cfd24d`; PS: `c6ff63999897`.
- **S30** — VS: `bb46f6b82dfd`; PS: `a5d50f05d92a`.
- **S31** — VS: `003e63b275c4`, `00d10873fcb5`, `0c3c2a7cfafa`, `0d7bc4d59992`, `0e0d8272cb89`, `1136130509c2`, `16d8031b6738`, `1ae7a36db517`, `1dc4e6e96d1a`, `1f852f9f6134`, `1ff7dd7eb58f`, `215f4530ac54`, `23f41cba1137`, `24922a8c1c2d`, `275a2a428407`, `278fda41643b`, `38c92fd099f5`, `38fbcc35d2a1`, `3d8d066946e8`, `48c8f530bbc3`, `49d5632e436e`, `53b33abfe5d0`, `545cb8a87cc0`, `59a17c15d2b3`, `5c3822a58d2d`, `5ee008873b23`, `62008095ad31`, `640c24654c57`, `68511b17e0d5`, `69a5c678bcdb`, `6da26c0e3add`, `70a5ed675e6e`, `713d7b685078`, `757de861627f`, `7fec93255935`, `8a0d61a570e1`, `8b8fe27b93b9`, `8f9660140467`, `9569d005b4d9`, `9c4a07827679`, `9f82c13d5ea1`, `a1c07f8d0470`, `a5b3e00a6cad`, `a5ce73eb3745`, `a9a1e9f39095`, `a9c3ed817454`, `abe7eb0667cf`, `b191e6d55649`, `b658f26e2a41`, `bbd92b589c27`, `bff6c33f1e50`, `c33a4c2ab80e`, `c39b14ffee1c`, `c49b43069cb2`, `c6b50a5ed9f0`, `ca7ef17d7a5e`, `cc92b37f5150`, `cfa0d232c73c`, `d0964351dfc1`, `d7df94938a11`, `dd5b57556936`, `e1859807f04a`, `e6193462b4e6`, `ea71eff77ce6`, `ead06b2c384b`, `f38d87fad0c5`, `fabd7cdbcb33`, `fd098d15f577`, `ffb251e1ffd0`; PS: `078255e6c55a`, `09825ab15c72`, `0ada76fa022c`, `0c81ffc41037`, `0f2fab7adb4c`, `1205fdeccc1a`, `19b0ccbe8734`, `19ecd615ca00`, `2035536e1249`, `23ad58d870bb`, `24f8838e2ddd`, `28fa2566df7e`, `2c22a7f67116`, `2c344c6826d0`, `2f04c1f4d9fb`, `31cd1945747f`, `341557e0efb7`, `3a1aaf243eca`, `3ecc6b5aa486`, `41c664e1e332`, `5212295b2082`, `5980e5f94bc1`, `6051d5e5ff35`, `6bae3ec0e637`, `6e4f714b4c00`, `7246d24f63de`, `744b021372e1`, `795b51ca4876`, `8c0dccf62262`, `9234ffe0762e`, `977680fe7ea2`, `9ba7a0fc7308`, `a177d5814175`, `b0bde6c547fd`, `b65cfc919594`, `b860493482a2`, `b986b675167c`, `c86f6175ef8e`, `d0e35924ddd1`, `d2ec5cf7e8a9`, `d4e3b030d4b9`, `d4f92ec0bdb3`, `d54f6a95e875`, `d6a47dd262ad`, `dcf7877c05f4`, `dcff21786a51`, `e652d5123ff0`, `ed1c5f084220`, `eef13100c127`, `f9dc69736c50`.
- **S32** — VS: `1060b8bed74d`, `2c05e3b29492`, `4951a4be5fd0`, `517a2babc1d9`; PS: `05efa685c36a`, `a856c8c6555a`, `c8e35dd7a31e`, `f452e087ef9c`.
- **S33** — VS: `0d7bc4d59992`, `1ae7a36db517`, `1fce3c9faad0`, `215f4530ac54`, `23d442cf31b2`, `307342750e90`, `38fbcc35d2a1`, `7eff91045b39`, `94c158125332`, `ae84a845e722`, `b1a1d6b0775b`, `b658f26e2a41`, `c14af8702d1d`, `c49b43069cb2`, `ccb5b36ffdfa`, `d1d3aa225bea`, `dc0dacc9a9d3`, `dd5b57556936`, `e6193462b4e6`, `fa21d57d6a76`, `fee46f892197`; PS: `121749ea14dc`, `19641c838dd4`, `1b084280aee7`, `3e0333a6f3c6`, `5ca8bbf3e0ac`, `5fa919d6f5ae`, `619e254480f0`, `6d6174ffc416`, `792fec18a3a2`, `8b5ba19769a0`, `982005359793`, `ad9ed81cc405`, `f6356da56da7`, `f79e93a11706`.
- **S34** — VS: `1f1dcac8248a`, `2d5043d3ef12`, `368b218194f2`, `880a53703611`, `8c99d64bb546`, `9453eafd092d`, `9ebcd7938844`, `bb46f6b82dfd`, `c7a9ea23a3c3`; PS: `45c67c0bf0b5`, `9ec7add6d6d7`, `a5d50f05d92a`, `bae4960a9596`.
- **S35** — VS: `4f4df0cfd24d`, `9ef746277e7d`, `dc0dacc9a9d3`; PS: `13bd23c16509`, `6d6174ffc416`, `c6ff63999897`.
- **S36** — VS: `9ef746277e7d`; PS: `944036d9538c`.
- **S37** — VS: `9ef746277e7d`; PS: `07a62ee02b21`.
- **S38** — VS: `9ef746277e7d`; PS: `621ce8689727`.
- **S39** — VS: `9ef746277e7d`; PS: `f0dab2cff437`.
- **S40** — VS: `9ef746277e7d`; PS: `01effc093648`.
- **S41** — VS: `9ef746277e7d`; PS: `7de7d674b508`.
- **S42** — VS: `4f4df0cfd24d`, `9ef746277e7d`; PS: `9f091c383572`, `c6ff63999897`.
- **S43** — VS: `9ef746277e7d`, `e5e56493e32f`; PS: `4408bf6af00a`, `75b17978faa8`.
- **S44** — VS: `ee119e843b7e`; PS: `ee48c9d34914`.
- **S45** — VS: `9ef746277e7d`; PS: `293526de6663`.
- **S46** — VS: `9ef746277e7d`; PS: `e5b49d3a4df1`.
- **S47** — VS: `ee119e843b7e`; PS: `9c2167754844`.
- **S48** — VS: `45d901814531`, `9ef746277e7d`, `d04e5973fdc7`; PS: `15f4d8dd5eaa`, `700e099884b0`, `765447383655`, `987e75439df5`, `b2c3bb6833fa`, `e5467e55258d`, `eb9fffb824b7`, `fc9df5d4b4b6`, `fe30811df8ac`.
- **S49** — VS: `9ef746277e7d`; PS: `e542ba187fc7`.

## Native takeover order

Take over command execution while retaining the original system's producer,
resources, constants, blend/depth state and scheduling. A fixed processing
chain still has game-controlled enable flags and parameters. These rankings
are implementation scope, not measured speedups.

| Priority | Group | Why it is bounded | Required dependencies / limits |
|---|---|---|---|
| First | Gamma/display950 (1 draw); image-plane draw944 (1 draw) | Single textured fullscreen operations with identified uniforms | Preserve fGamma, input copy5337, viewport, output format/sRGB behavior, ImagePlane blend and external image. Do not infer identity transforms. |
| First | Mip865..872,880..886,889..896 (23 draws) | Repeated one-draw shader chains; known texture/level outputs | Preserve explicit mip selection, fMipTargetColor/rate, constants and per-level resource hazards. Native generate-mips is not automatically equivalent. Base-image producers remain original. |
| First | Color-correction LUT943 (4 draws) and its application in949 (1 draw) | Identified LUT + matrix/table application | Preserve clear draw, three Hermite/gamma draws, configured parameters, copy4912 after God Rays, and composition order before GUI. Do not assume LUT never changes. |
| Next | God Rays938..942 + first draw949 (6 draws) | Extraction, three iterations, filter and composition are isolated | Depth933, scene936, border sampling, exact per-iteration constants and original enabled/configured order. |
| Next | DOF945..948 (4 draws) | Extraction, two filters and composite | Depth933, scene936, original focus/near/far parameters, kernels, blend/load behavior. This is not the BokehPlug algorithm. |
| Next | Shadow transform/filter906..913 (28 draws) | Two known texture-processing tails | Leave shadow caster selection900/905 original; retain transforms, six draws per variance-filter run, texture regions, kernels and ping-pong ordering. |
| Later | Sky873..877,914..917; scene/shadow casters918..934/903; reflective water-material draws928; texture sources864/878/887/897 | Inputs include camera, light/weather, visibility, skinning/SoftBody, materials and dynamic geometry | Drive native execution from original commands/handles. Reimplementing producers requires substantially more game-state evidence. |
| Separate lane | SoftBody0..863 | Dominates pass count; six recurring PS identities | L113 owns algorithm research/replacement. N3 should consume its verified resource contract rather than duplicate it. |
| Last producer replacement | GUI421 draws in949 | Native draw execution can be incremental, but full GUI production has layout, clipping, fonts, animation and dynamic atlases | Keep original order, buffers, constants, GUI sampler/state semantics. Count excludes the two generic textured draws, which are not proved GUI by reflection. |

Bloom, tone-map/log-average and Bokeh producer replacement cannot be selected
from this frame alone. Capture a configuration which actually exercises their
characteristic shaders before defining a native chain. No system should be
silently disabled just because it is not positively identified here.

## Verification, remaining joins and reproduction

- Independent v1 decoder follows `crates/d3d9frame/src/lib.rs`'s `Dec::draw`
  layout and checks complete payload consumption for all5,295 draws. Every
  L107 draw command offset resolves to one decoded draw; totals cover all951
  runs. Empty/clear-only runs875,916,924 are retained.
- All402 captured shader streams (225 VS,177 PS) reach END at the exact
  token-array boundary. DCL sampler registers select potential read slots;
  CTAB supplies resource/constant names. All12-digit hash prefixes are unique
  within this measured frame. Private `package-match.json` records334 exact
  byte matches and their stage/record indices in root.spk; unmatched68 shaders
  are not assigned speculative entry-point names.
- Recomputed SHA-256 of all108,502 selected command payloads equals L111's
  `aa3b155e8bede1cf104e2e23aa27c685ee9521deb4b7579a66226a296b14e269`.
  This verifies the analytical slice's command provenance. The slice omits
  earlier uploads and is **not a standalone image-replay capture**.
- Static sources: L108's reviewed producer evidence plus read-only Ghidra
  queries for01032B20,0104A840,01048750,01013320,00F39290,00F30E10,
  00F2F710 and00EE2C90; DTI registry maps class names to the cited slots.
  Shader-package object IDs named above were checked against the installed
  MFX name hash/index, rather than interpreting an arbitrary immediate as a
  name. No decompiler output or game bytes are committed.
- `instr1.log` frame10080 has5,091 raw draw records,222 VS handles,183 PS
  handles and67 state identities. It is a different run from cap1 and counts
  pre-coalescing command records, so differences from5,295/225/177 are not a
  parser failure or a direct performance comparison. Even repeated pointer
  values across runs do not prove identical objects. L111's existing observer
  has target runs but **does not record producer class/caller identities**.
- No Rust/runtime code changed; no cargo, GPU, shard or game execution was
  needed. No pixel equivalence, original-game comparison or FPS result is
  claimed. Documentation validation checks counts, full ordinal coverage,
  hash-prefix uniqueness, command digest, UTF-8 and `git diff --check`.

Private reproducibility files under `C:/ddda-targets/L122/`:
`analyze.py`/`passes.json`, `shaders.py`/`shaders.json`,
`package.py`/`match.py`/`package-match.json`, `audit.py`, and `sequence.txt`.
They derive observations from L111's private slice and the read-only install;
shader blobs and package/decompiler outputs stay private. The sequence table
is a human-readable analytical summary, not a game-data payload.

**Next head-run request:** use `DDRUNTIME_NDRAW_STATS=1` and
`DDRUNTIME_NDRAW_STATS_EVERY=1` with a same-run capture, starting before the
selected gameplay frame. To close the unresolved issuer joins, a separate
observer enhancement must also attach producer entry/class, active camera,
engine pass/sort key, command record pointer and shader metadata+0x1C source
IDs to that stream. Existing L111 alone cannot recover those caller tags.
Specifically resolve texture-base producers864/878/887/897, exact shadow
subclasses, water/particle command ownership, and whether Bloom/ToneMap/Bokeh
are disabled, bypassed or simply not exercised in this scene. Only the head
runs the game; board requests10916/10949 describe the correlation needed.
