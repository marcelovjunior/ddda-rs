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


## Result

**Ready for review.** Implementation commit `645d076` on
`step/s04-face-joints`; diagnostic only, no animation or morph changes.

`DDVIEW_JOINT_DUMP=<frame>` dumps once per cutscene actor at the requested
motion frame, after motion sampling, blending and face/body morphs. Each
line identifies the event actor, base model, semantic joint and parent IDs,
original rest local and displayed animated local translations (cm), XYZ
Euler rotations (degrees), scales, shortest quaternion angle and translation
distance. Original rests are retained as opt-in components when skeletons
are bound; a part reusing a joint preserves its existing skeleton rest.
Joint locals are in model centimetres: the `UNIT=0.01` metre conversion is
on the actor root, so the 3 cm comparison uses locals directly.

Small related edit: `event.rs::apply_actors` supplies diagnostic event-frame
context. Event 22 timeline frame **1010** actually samples actor motion frame
**100**, for `pl_00`, `npc001_00` and Quina `npc002_00`. A motion-frame 1010
trigger alone would not fire on the README command. The optional
`DDVIEW_JOINT_DUMP_EVENT_FRAME=1010` trigger therefore captures the requested
event pose without changing the motion-frame meaning of the primary flag.
Both variables were set to 1010 for the capture; every line records both
clocks to keep the distinction explicit.

Private output: `E:/ddda-re/notes/steps/s04/joints.txt` (672 lines, exactly
224 joints for each of three actors), with full logs `stderr.log` and
`stdout.log`, the copied viewer `ddview.exe`, and `frame1010.png` beside it.
The hidden unattended README run had a 60 s watchdog and exited 0 after its
screenshot; no owned viewer process remains. The screenshot SHA-256 is
`f4d6efc1e18193469d894cf1f94febb6dadf1041496704b728741a836c2f49c0`,
identical to the earlier S01 screenshot: enabling this diagnostic changed
no displayed pixels.

Validation:

- `cargo test --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **52 passed, 0 failed, 2 existing ignored**, tests 3.22 s, build 25.64 s.
- `cargo build --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **passed**, 32.65 s; two existing warnings (`opening_world` visibility and
  unused `audio::PlayerMotionSounds::forget_actor`).
- `git diff --check`: passed. Build/GPU leases released.

Quina's complete `!!` list follows. Values are measured local changes after
morphs at event 1010 / motion 100; these threshold hits are diagnostic
findings, not proof that any particular joint is wrong.

| Joint ID | Parent ID | Angle (degrees) | Move (cm) |
|---|---|---:|---:|
| 0 | - | 0.000 | 8.693 |
| 6 | 5 | 64.053 | 0.640 |
| 7 | 6 | 88.635 | 0.000 |
| 10 | 9 | 65.420 | 0.640 |
| 11 | 10 | 87.071 | 0.000 |
| 12 | 11 | 49.096 | 0.000 |
| 20 | 19 | 70.799 | 0.000 |
| 25 | 8 | 53.527 | 0.000 |
| 26 | 25 | 54.119 | 0.000 |
| 27 | 26 | 45.078 | 0.000 |
| 28 | 8 | 52.067 | 0.000 |
| 29 | 28 | 65.479 | 0.000 |
| 32 | 31 | 52.400 | 0.000 |
| 33 | 32 | 71.580 | 0.000 |
| 34 | 33 | 64.097 | 0.000 |
| 35 | 31 | 52.684 | 0.000 |
| 36 | 35 | 64.198 | 0.000 |
| 37 | 36 | 53.330 | 0.000 |
| 150 | 1 | 17.218 | 23.313 |
| 151 | 150 | 0.000 | 7.745 |
| 152 | 150 | 0.000 | 13.287 |
| 153 | 1 | 132.225 | 20.927 |
| 154 | 153 | 0.000 | 23.113 |
| 155 | 153 | 0.000 | 3.714 |
| 171 | 2 | 180.000 | 0.000 |
| 173 | 4 | 180.000 | 0.000 |
| 175 | 5 | 180.000 | 0.000 |
| 176 | 54 | 180.000 | 0.000 |
| 177 | 55 | 180.000 | 0.000 |
| 178 | 6 | 180.000 | 0.000 |
| 179 | 55 | 180.000 | 0.000 |
| 180 | 56 | 180.000 | 0.000 |
| 181 | 7 | 180.000 | 0.000 |
| 182 | 57 | 180.000 | 0.000 |
| 183 | 8 | 180.000 | 0.000 |
| 185 | 9 | 180.000 | 0.000 |
| 186 | 58 | 180.000 | 0.000 |
| 187 | 59 | 180.000 | 0.000 |
| 188 | 10 | 180.000 | 0.000 |
| 189 | 59 | 180.000 | 0.000 |
| 190 | 60 | 180.000 | 0.000 |
| 191 | 11 | 180.000 | 0.000 |
| 192 | 61 | 180.000 | 0.000 |
| 193 | 12 | 180.000 | 0.000 |
| 194 | 13 | 180.000 | 0.000 |
| 237 | 82 | 62.595 | 0.673 |
| 238 | 83 | 62.595 | 0.807 |
| 239 | 84 | 62.595 | 0.673 |
| 240 | 85 | 62.595 | 0.673 |
| 241 | 85 | 63.178 | 0.293 |
| 242 | 86 | 63.178 | 0.289 |
| 243 | 87 | 63.178 | 0.289 |
| 244 | 82 | 63.178 | 0.260 |
| 245 | 89 | 62.595 | 0.673 |
| 246 | 90 | 62.595 | 0.807 |
| 247 | 91 | 62.595 | 0.673 |
| 248 | 92 | 62.595 | 0.673 |
| 249 | 92 | 63.178 | 0.293 |
| 250 | 93 | 63.178 | 0.289 |
| 251 | 94 | 63.178 | 0.289 |
| 252 | 89 | 63.178 | 0.260 |

Quina has 61 marked joints. Main facial joints 80..147 have no
`!!` hit (largest measured angle 18.531 degrees; largest translation change
1.704 cm). Auxiliary joints 237..252 parented to facial joints 82..94 do hit
the angle threshold at roughly 62.6..63.2 degrees. The log also retains
local scale, which these rotation/translation thresholds do not classify.
Some body-shape joints show 180-degree local rotation differences; interpreting
those requires their signed scale/matrix context rather than treating the
threshold as an error verdict.

Still open: no comparison with original-game joint matrices, no identified
cause, and no facial expression fix. Next: S12 should inspect auxiliary
facial joints 237..252 and the face-edit/motion channels of the main facial
joints, preserving event frame 1010 versus sampled motion frame 100. Use
the full rest/animated scale and parent context in the private log when
comparing against original-game matrices.
