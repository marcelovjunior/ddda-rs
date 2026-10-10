# S01: Log every part of the cutscene characters

- **Branch:** `step/s01-part-log`
- **Files:** `crates/ddview/src/character.rs` (and `event.rs` only if the
  spawn loop is there)
- **Kind:** diagnostic, no behaviour change

## Why

In the st200 ev10 cutscene (`--event 22`) the elder is missing hair and
has a hole by his ear, and both actors' brows don't show (maintainer
screenshot at frame 1010). Head needs a full list of what each actor is
built from and what the viewer skips.

## Do

1. Find where a character's model meshes become Bevy entities (start at
   `character::spawn_look` / `character::spawn` and follow to the mesh
   loop; `stage.rs` has the skip rules for shadow-only, MaterialNull and
   missing UVs).
2. When the environment variable `DDVIEW_PART_LOG` is set, log one line
   per mesh with `info!`: actor/model path, mesh index, `group`, `lod`,
   `draw_mode` (hex), material name, the material's blend/alpha mode as
   the viewer built it, vertex count, and **whether it was spawned or
   skipped and why** (log skips too).
3. Without the variable nothing changes.

## Run

The cutscene command from README.md with `DDVIEW_PART_LOG=1`, frame 1010,
shot to `E:\ddda-re\notes\steps\s01\frame1010.png`.

## Done when

The log lines for **both** actors are pasted into this file's Result
(grouped per model), plus the build/test lines. No fixes in this step.
