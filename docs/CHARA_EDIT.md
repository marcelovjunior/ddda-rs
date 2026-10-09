# Character edit records and gameplay measurements

Evidence is from DDDA.exe 2017-12-13 Steam, the maintainer's private save
backup and installed resources. `ddedit` supplies strict decoding and small
source-backed calculations. It does not implement an editor or reconstruct
every runtime body measurement. No save or game process is written.

## Saved appearance

`cSAVE_DATA_EDIT::vf03` at `0x00787010` describes the saved schema. The
reader reuses `ddpawn::cmc::Edit` and its named `EDIT_U8_FIELDS`: 41 u8
selectors, a u32 nickname and 25 bytes of name storage. It checks scalar
widths, duplicate/missing fields and the exact name-array shape. It leaves
unknown fields in the source XML and does not log identity values.

The Arisen is under
`mPlayerDataManual/mPlCmcEditAndParam/mPl/mEdit`; the main pawn is the first
`mCmc` entry's `mEdit` under the same parent. The private original-written
save tested here contained 228 complete edit records, all decoded strictly.
Both primary records resolved all five body preset resources.

The selectors cover gender, voice/personality, face-part presets and
offset/scale choices, body slenderness/muscle/bust, five body presets,
movement/posture, wrinkles/makeup/scars, colors, hair and beard. A saved byte
is not itself a physical measurement. See [CHARACTERS.md](CHARACTERS.md)
for rendering, face and color assembly and [PAWNS.md](PAWNS.md) for the
surrounding pawn record. Voice selection is preserved, not synthesized or
audibly compared by this brief.

`0x00780fa0` chooses body preset numbers: add 100 when gender is 1 and
`mHumanBodyAll < 200`, otherwise add zero. The bodyall, body, head, arm and
leg resources are `edit\\parts\\<part>\\<part>_NNN`, class
`rHumanPartsEdit`. `ddedit::body_presets` reads these through the caller's
resource source and reuses `ddlook::morph`; missing presets are explicit
errors rather than an attempted imitation of the engine's reset fallback.

## Ground-motion scale

The ordinary enabled path at `0x0081c170` calculates, in f32 operation order:

```
horizontal_scale = joint_14.scale_y * joint_13.scale_y * owner.model_scale_y
```

`0x0081c330` maps joint IDs at controller `+0x34 + 2*id`. Thus the index
fields `+0x4e` and `+0x50` mean joint IDs 13 and 14; they are not themselves
joint-array positions. The array pointer is controller `+0x0c`, records
have stride `0xd0`, and each raw `mScale.y` is at record `+0x84`.
Do not substitute the separate child scale or guess bone names from IDs.
Both active records must exist and not be disabled. Controller `+0xf94`
enables horizontal scaling; `+0xf95` affects vertical normalization.

The reset/composition path `0x0081ba40` obtains the owner's base scale,
applies clamps (`0x0081c6c0`), entries (`0x0081cff0`), and joint records
(`0x0081db50`), then calls `0x0081c170`. The existing `ddlook` translation
provides the corresponding joint scales. `0x00b6b350` writes the action's
scale fields including actor `+0xda8`; special actions can disable or
normalize these factors. This value is a root-motion multiplier, not a
complete sprint speed or an encumbrance class.

The private saved Arisen and installed presets produce **0.847109079**;
the main pawn produces **0.974198282**. The preset test assumes these
relevant joints exist; it does not verify arbitrary missing-joint models.
The nine neutral measurement floats in `HumanEdit::neutral` are not used
by this selected scale path and are not evidence for reconstructed height.

A passive live observation by codex-19 at 2026-10-09 15:19:56 UTC found:

| Field | Observed value |
|---|---:|
| owner model scale Y | 0.8593593835830688 |
| controller joint indices for IDs 13/14 | 15 / 16 |
| joint 13 raw scale Y | 1.211538553237915 |
| joint 14 raw scale Y | 0.8136305212974548 |
| controller horizontal/vertical flags | 1 / 1 |
| actor +0xda8 | 0.8471090197563171 |

The live inputs reproduce the live output in f32. Saved-preset output
differs by about 6e-8 (one f32 step); the source of that final rounding
difference is not established. Sequential process reads were not atomic.
This reproduces the brief's recorded 0.8471 factor; it is not a comparison
of complete movement trajectories or every possible appearance.

## Body weight, classes and downstream effects

`cPlayerInfo::vf06`, `0x00780ea0`, computes body weight from runtime
measurements, then truncates to an integer with `CVTTSS2SI` and returns
that value as a float. The implemented helper takes already resolved
resource values, not saved selectors:

```
height = human[+0x68] * 1.8
weight = ((human[+0x70] + human[+0x6c]) * 0.5263158)
         * ((height * height) * 22.5)
if body resource exists:
    weight *= ((body[+0x60] * 0.01) * 1.6666666) * 0.24 + 1
    weight *= (body[+0x64] * 0.01) * 0.08 + 1
    if female:
        weight *= player_info[+0x108] * 0.02 + 1
return float(truncate_to_i32(weight))
```

An absent human resource returns 76. `ddedit` rejects nonfinite or
out-of-i32 results rather than exposing the hardware invalid-conversion
sentinel. The body and bust factors apply only when the body resource
exists; unused values do not change the fallback.

**The saved-edit-to-weight producer remains incomplete.** `0x00cbbf00`
constructs a temporary model and measures original/morphed joint-world
positions to produce the nine `rHumanEdit` floats at `+0x68..+0x88`.
That measurement algorithm is not translated here. `0x00780fa0` also maps
saved slender/muscle/bust selectors through lookup values into the runtime
inputs above; those lookups must be read/verified before exposing a
saved-selector weight API. Supplying neutral floats cannot close this gap.

`0x00b81120` stores body weight at actor `+0x34c0`, total weight at
`+0x34c4`, body class at `+0x34c8`, and the `PlEditWeight` resource at
`+0x34d0`. Body classes 0..4 (SS..LL) compare resource rows 1..4;
equality goes to the higher class. Installed boundaries documented in
[PARAMS.md](PARAMS.md) are 50, 70, 90, 110, with row zero 24. Missing
resource falls back to class 2. `ddedit::body_class` accepts caller-loaded
rows and does not embed a shipped table.

`0x00b81270` chooses `PlTotalWeight` for that body class. Carrying thresholds
include the difference between body weight and the class's `PlEditWeight`
row plus the ability adjustment from `0x00b7e710`. Class and load rows also
feed stamina and motion rates; see [PARAMS.md](PARAMS.md) and
[STAMINA.md](STAMINA.md). These existing consumers are not reimplemented
or claimed to be governed solely by the ground-motion multiplier.

## Validation and next action

Five tests cover strict saved shapes, selector boundaries, missing resources,
source multiplication order/live scalar observation, body-class equality,
body-weight resource gates and truncation, and optional private save/install
integration. The integration explicitly reports skipped private inputs and
does not embed game data or identities. `cargo test -p ddedit` and Clippy
are the local checks; only the ground-scale scalar has a live comparison.

Next: translate `0x00cbbf00` using installed skeletons and reproduce the
runtime measurement floats and body weight for both primary characters.
Keep per-actor inputs explicit and leave simulation ownership in core.
