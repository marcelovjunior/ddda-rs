# arisen-look: Consistent saved character appearance

- **Owner:** codex-1 (head assignment #2405)
- **Branch:** `task/arisen-look`
- **Board area:** `render/arisen-look`
- **Status:** investigating

## Goal

Fix the maintainer's report that the opening Arisen looks like a male face
on a female body. Follow the actual default-template and saved/editor
paths, identify the first incorrect data or rendering transformation, and
keep the same record's body, face, equipment and appearance parameters
together. Do not replace the selected record to hide a rendering defect.

## Scope

`crates/ddlook/src/{save,lib}.rs`, relevant look tests, this brief. Coordinate
body/morph changes with root's pawn measurement work; coordinate any
`character.rs` change with codex-30's camera fade work. Shared main/viewer
hooks require coordination with codex-29. No game install changes.

## Evidence and verification

Trace gender semantics, sex-specific model and preset lookup, body sliders,
the final skeleton transforms, and the input to `Playback.morph`. Compare
default `--opening --skip-intro` against the same edit record in the editor
or original game. Use bounded unattended viewer runs, private captures and
shared build targets. Source-derived behavior requires regression tests;
keep implemented, tested and compared claims separate.

## Results

Initial source check: `0x0083fa60` selects female base model only for sex1,
male otherwise. `ddlook::base_body` matches this. The default template
selects the first installed `romPawnInfo` record with requested sex0 and
passes its actual edit fields to `edit_look`; further investigation remains.

### Body-shape controller checkpoint

- The installed first male template is record 10, female record 0. Both
  retain their sex-specific base, body and face preset fields. The male
  armour table itself references a female-named shared helmet; a filename
  prefix alone is not evidence of the wrong sex table. All requested
  presets and equipment models resolve. Reflection `0x00787010` confirms
  `mHumanBody` is +0x3f and `mHumanHead` +0x40, despite their XML ordering.
- Found an omitted controller: `cBodyEdit`, separate from `cHumanEdit`.
  `0x0080ad20` attaches plain constraints to semantic joints 0xaa..0xce;
  `0x0080aed0` blends neutral/thin/muscle/heavy shapes, multiplies authored
  per-joint scales, applies the female-only bust delta to slots 4 and 14,
  then clamps unless fixed. `0x0080add0` converts percentages to ratios;
  `0x0080bb10` passes the runtime bust field +0x108 to controller +0x2094.
- Implemented a runtime reader for shapes at `0x01830140`, upper limits
  `0x018313c0`, lower limits `0x01831860`; no bulk values are embedded.
  Save slider conversion is translated from `0x00780fa0`. The resource
  parser follows `rBodyEdit::vf10` `0x00cad170`.
- Saved/editor looks now retain these parameters and `spawn_look` adds
  their constraints to the existing skeleton. Body-shape joints are
  disjoint from human and face controller joints. The installed male
  template changes 36 mapped joints, female 34; before this change all
  those body-shape settings were omitted.
- Tested: three synthetic controller/resource regressions, one installed
  two-sex template/resource/skeleton audit, strict ddlook no-deps clippy,
  and release ddview build. Two pre-existing viewer warnings remain.
- Bounded hidden editor runs succeeded and exited: male before/after at
  the same 1280x900 pose, and female after. Private captures live under
  `E:/ddda-re/notes/arisen-look-codex1`. The extra female baseline launch
  was rejected by automatic approval review (only "blocked by policy"
  supplied), so that comparison was not performed. No original-game
  same-record comparison has been performed. This corrects a proven
  missing shape path; it does not establish that every reported visual
  discrepancy is resolved. NPC resource hookup, muscle shader blending,
  skeleton measurement integration and special constraint types remain
  separate work. Bulk tables are read during character construction;
  caching can follow if profiling shows meaningful startup cost.

### NPC body resource follow-up

`0x0083fa60`'s NPC branch also invokes `0x0080add0` for the authored
`rBodyEdit`. It skips `0x0080bb10`, so the bust parameter stays at the
controller constructor's zero. In particular, the NPC edit floats at
+0x108 are eyebrow offsets and must not be interpreted as the player's
bust field. The NPC look builder now parses its named body resource and
uses the same renderer path. An installed male/female NPC regression
checks 37 authored scales, zero bust, finite computed scales and their
binding to the base skeleton. Both appearance integration tests and strict
ddlook clippy pass. Release viewer build passed; no original
NPC comparison is claimed.

### Hand constraint scale follow-up

Human-edit hand joints 8/12 use constraint kind 9 and explicitly set
ignore-model-scale (+0xb0) in `0x0081c330`. `0x00867b70` cancels the
incoming world axis lengths before `0x00860ac0` applies the authored
scale. The viewer previously treated kind 9 as a plain multiplicative
scale. The human-hand path now uses that compensation, retaining the
source's strict epsilon boundary (a degenerate axis keeps factor one,
rather than dividing by a clamped epsilon). Two regressions and strict
ddlook clippy pass. Release viewer verification is pending the shared
build slot; no original hand comparison is claimed. General rotated
type-9 constraints, type-0xc constraints and the independent fresh-model
body-measurement pipeline remain separate work.
