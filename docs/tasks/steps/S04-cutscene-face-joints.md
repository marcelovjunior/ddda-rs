# S04: Dump Quina's face joints at frame 1010

- **Branch:** `step/s04-face-joints`
- **Files:** `crates/ddview/src/character.rs` (`animate`, or a new small
  debug function called from it)
- **Kind:** diagnostic, no behaviour change

## Why

Quina's facial expression looks distorted at frame 1010. Head needs the
face joints' animated local transforms to compare with their rest pose.

## Do

1. When `DDVIEW_JOINT_DUMP=<frame>` is set, at that motion frame write one
   line per joint of every cutscene actor to the log: actor model, joint
   id (`Bone::anim_map_index`), parent id, rest local translation, rest
   local rotation (Euler degrees XYZ), animated local translation and
   rotation, and the angle in degrees between rest and animated rotation.
   Only once per actor.
2. Mark lines where the angle is above 45 degrees or the translation moved
   more than 3 cm with `!!`.

## Run

The cutscene command from README.md with `DDVIEW_JOINT_DUMP=1010`; save
the log lines to `E:\ddda-re\notes\steps\s04\joints.txt`.

## Done when

The Result lists every `!!` joint for Quina (id, parent, angle, move) and
the path of the full dump. No fixes.
