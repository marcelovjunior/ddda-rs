# missing-ground: Cassardis floors vanish at distance

- **Owner:** codex lane
- **Branch:** `task/missing-ground`
- **Board area:** `render/lod`
- **Status:** resolved: the original has the same hole, covered by an auto-closing door (maintainer, 2026-10-09); see doors

## Goal

The maintainer sees sky through the ground in Cassardis (stage 200): from
the top of the path by the gate, looking down the alley toward a house
doorway, the floor in front of the doorway is blue. Head's findings:

- The probe (`--probe`, F9) hits at stage cm (2733.6, -81.7, -2239.1)
  and (2722.6, -82.7, -2263.5). Only two meshes in all st200 models cover
  those points: `scr\st200\model\st200_md06` mesh 28 (LOD mask 0x3, the
  floor, draw mode 0xFDF7) and mesh 2 (mask 0xFF, 4 cm lower, draw mode
  0xE6C8 = shadow casters only, no main pass, so never drawn in colour).
  No mask-0x4 floor exists there.
- md06: sphere centre (2543.2, 217.1, -1975.4), radius 1004, mid 1000,
  low 3000. The viewer follows 0x00EA5320 as head read it: level 4 when
  |camera - centre| > low x k, k = 1 for ViewRange NORMAL (config object
  DAT_018D1D20 +0x38, read in 0x00EDE0F0: NORMAL 1, FAR 2, FARTHEST 3,
  which always draws level 1). The maintainer's config has
  ViewRange=NORMAL. At about 38 m from that centre the floor (0x3) hides
  and nothing covers it.
- Since the original surely shows that floor, one of these readings is
  wrong. Candidates: what `param_2 +0x184..0x18C` and `param_5` are in
  0x00EA5320 (the point measured from: camera eye, camera target/player,
  or a LOD focus); whether rModel +0xE0/+0xE4 are the header's mid/low
  unscaled; whether DAT_018D1D20 +0x38 changes at runtime; whether mtmod's
  `Mesh::lod` is the byte 0x00EA5320 tests (`puVar11 >> 0x18`).

Settle it live: stand where the maintainer stood (stage 200, around the
probe eyes (5267, 1014, -4580) and (4616, 730, -3985) stage cm, looking
toward the points above), read the values that 0x00EA5320 uses for md06
(debugger trace of its args, or memory reads), and fix
`stage::lod_level` / `event::lod_update` to match. Then re-check the
maintainer's earlier missing-ground spots on the field.

**Rules.** Trace in the exe and label any guesses (AGENTS.md). Live
checks follow the safety rules in `docs/CODEX.md`.

## Scope

`crates/ddview/src/{event,stage}.rs` (lod_update, lod_level, Part),
`crates/mtmod` if the mask byte is wrong, `docs/SHADING.md` or
`docs/STREAMING.md` LOD section, this brief's Results.

## Done when

The alley floor stays drawn from the maintainer's spot, with the rule
traced live. Fill in Results and report to `head`.

## Results

### Investigation checkpoint (2026-10-09, codex-29)

Not fixed or compared against the original at the reported Cassardis
location yet. No rendering behaviour changed at this checkpoint.

- **Measured in the original:** a bounded main-pass snapshot of
  `uBaseModel::vf26` (0x00EA5320) for a loaded field model confirms
  `param_5` is its world sphere centre and render context +0x184 is the
  camera eye (within floating-point rounding of the camera unit).
  The live ViewRange is NORMAL (1), the model's middle/far thresholds
  are 1000/3000 cm, its LOD override is -1, and its parent is null.
  The temporary capture hook was restored and verified; the game remains
  paused with background execution off. Save files were backed up before
  attaching; no movement, save, or installation modification occurred.
- **Traced:** 0x00EA1F40 substitutes the outermost parent model's sphere
  centre when a model has parents. This is a possible general viewer
  omission, but is not established as this floor's cause.
- **Original whole-stage setup found:** aStage's 0x00506800 loads the
  `scr\st200\sdl\st200_s00` scheduler. The installed scheduler creates
  md06 as `uScrBaseModel`, with a model resource, light group, draw mode,
  and `mDisableZone=true`; it authors no parent or LOD override for it.
  Its constructor 0x00C572E0 delegates to 0x00EA3490, whose initial
  +0x108 bit field gives LOD -1. Its update 0x00C574F0 handles zone
  visibility, and draw 0x00C579F0 delegates to the normal model draw.
  The existing comment attributing this to 0x00C25234 was incorrect:
  that instruction belongs to `uOmSwingInstancing::vf08`, not a stage
  model constructor. The comment is corrected without asserting that
  all runtime stage-model writes have been ruled out.
- **Validated:** the installed scheduler was parsed by the existing
  `ddquest` `sdl_dump` example (release build passed); independent private
  byte inspection agreed. Raw captures and exports remain in the private
  research tree, not in the repository.

**Blocker / next action:** the running original is in a field canyon,
not Cassardis, and md06 is not loaded. Head has been asked to arrange a
Cassardis save load with the maintainer. Capture md06's actual draw state
at the reported camera position before changing the LOD rule; meanwhile
continue tracing model binding and the authored scheduler's setup.
