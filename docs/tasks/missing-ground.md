# missing-ground: Cassardis floors vanish at distance

- **Owner:** codex lane
- **Branch:** `task/missing-ground`
- **Board area:** `render/lod`
- **Status:** open (head traced the data; needs a live check)

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

(filled in by the agent)
