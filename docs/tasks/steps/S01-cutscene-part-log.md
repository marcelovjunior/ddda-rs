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

## Result

Implemented in `5846616` on `step/s01-part-log` (diagnostic only).
`DDVIEW_PART_LOG` enables one `info!` line for every mesh considered by
`character::spawn_meshes`, including all early exits. Actor is the Bevy
root entity, shared by every assembled part. `alpha=not-built` explicitly
means the skip preceded material construction; reporting an inferred
alpha there would misrepresent the viewer. The standard and DD material
paths report their actual constructed Bevy alpha mode.

No stage edits were needed. Unlike the stage path, the existing character
path does **not** filter MaterialNull; this diagnostic preserves that
behaviour. No render fixes or material construction on skipped meshes.

Validation:

- `cargo test --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  **52 passed, 0 failed, 2 ignored**, 2.93 s (compile 25.88 s).
- `cargo build --release -p ddview --target-dir E:/ddda-rs-wt/m5-cassardis/target`:
  passed, 42.25 s; existing `Args` visibility and `forget_actor` dead-code warnings.
- Requested event 22 / frame 1010 / hold / unattended / shot-delay 15 capture,
  private executable and stdout/stderr logs under
  `E:/ddda-re/notes/steps/s01/`; `frame1010.png` visually inspected and
  process exited 0 within the 60 s watchdog. Frame HUD confirms 1010.
- Both visible actors retain the reported defects: elder ear opening,
  incomplete-looking hair, brows absent, Quina expression distorted.
  This is a viewer diagnostic capture, **not** original-game acceptance.

All 237 considered meshes were logged, including the 103-mesh saved Arisen
(off-screen). Below are the full 70 elder and 64 Quina lines, grouped by
model. The elder is root `2885v0` (male base, e_npc004, beard104, hair104),
Quina root `3161v0` (female base, e_npc005, hair105). The elder has 51 spawned
meshes; Quina has 40. All ten colour-pass hair104 meshes spawn, so the hair
problem is not wholesale failure to load that part. Skips below are the
first rejecting condition, matching the original short-circuit checks.

Next: S02 group-colour capture for the ear and hair, S03 forced-brow
capture, S04 face-joint diagnostics. Do not infer a fix from these logs alone.

### Elder

`model\pl\m\m_base\m000\m000`

```text
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=0 group=7 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=226 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=1 group=4 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=185 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=2 group=14 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=224 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=3 group=15 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=345 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=4 group=6 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=222 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=5 group=5 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=197 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=6 group=22 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=88 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=7 group=12 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=52 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=8 group=10 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=128 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=9 group=13 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=176 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=10 group=9 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=48 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=11 group=11 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=82 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=12 group=8 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=100 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=13 group=3 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=120 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=14 group=2 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=132 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=15 group=1 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=337 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=16 group=15 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=90 skipped:lod0-disabled
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=17 group=0 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=335 skipped:lod0-disabled
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=18 group=0 lod=255 draw_mode=0x0 material=XfB_N__E_00_m003_ alpha=not-built vertices=116 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=19 group=0 lod=255 draw_mode=0xe6c8 material=XfB_N__E_00_m003_ alpha=not-built vertices=593 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=20 group=0 lod=1 draw_mode=0xfdf7 material=XfB_N__E_01_m001_4 alpha=Opaque vertices=37 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=21 group=0 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m007_ alpha=Opaque vertices=831 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=22 group=0 lod=1 draw_mode=0xfdf7 material=XfB_N__E_01_m002_3 alpha=Opaque vertices=37 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=23 group=20 lod=1 draw_mode=0x1020 material=XfB_N__E_00_m004_ alpha=not-built vertices=10 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=24 group=20 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=400 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=25 group=23 lod=1 draw_mode=0x1020 material=XfB_N__E_00_m004_ alpha=not-built vertices=10 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=26 group=21 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=746 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=27 group=23 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=1242 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=28 group=0 lod=6 draw_mode=0x1020 material=XfB_N__E_00_m004_ alpha=not-built vertices=31 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=29 group=0 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=not-built vertices=155 skipped:lod0-disabled
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=30 group=0 lod=1 draw_mode=0x1020 material=XfBAN__E_00_m005_ alpha=not-built vertices=308 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=31 group=0 lod=1 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Mask(0.4137255) vertices=460 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=32 group=17 lod=1 draw_mode=0x1020 material=XfBAN__E_01_m008_ alpha=not-built vertices=6 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=33 group=17 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=Blend vertices=52 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=34 group=0 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=Blend vertices=144 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=35 group=16 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=Blend vertices=62 spawned
PART actor=2885v0 model=model\pl\m\m_base\m000\m000 mesh=36 group=24 lod=1 draw_mode=0xfdf7 material=XfBAN____01_m006_ alpha=not-built vertices=222 skipped:part-group-hidden
```

`model\pl\npc\e_npc\e_npc004\e_npc004`

```text
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=0 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E1__m000_ alpha=not-built vertices=16 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=1 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E1__m000_ alpha=not-built vertices=32 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=2 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E1__m000_ alpha=not-built vertices=41 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=3 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E1__m000_ alpha=not-built vertices=10 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=4 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=253 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=5 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=105 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=6 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=334 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=7 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=40 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=8 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=176 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=9 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=264 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=10 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=705 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=11 group=300 lod=255 draw_mode=0xe6c8 material=XfBAN__E1__m000_ alpha=not-built vertices=255 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=12 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_ alpha=Mask(0.47254902) vertices=438 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=13 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E1__m000_1 alpha=not-built vertices=182 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=14 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_1 alpha=Mask(0.47254902) vertices=289 spawned
PART actor=2885v0 model=model\pl\npc\e_npc\e_npc004\e_npc004 mesh=15 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E1__m000_1 alpha=Mask(0.47254902) vertices=511 spawned
```

`model\pl\hair\hair104\hair104`

```text
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=0 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=72 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=1 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=89 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=2 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=90 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=3 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=119 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=4 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=96 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=5 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=98 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=6 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=84 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=7 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=264 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=8 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Blend vertices=98 spawned
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=9 group=50 lod=255 draw_mode=0x1020 material=XfBAN__E_00_m005_1 alpha=not-built vertices=8 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\hair\hair104\hair104 mesh=10 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_1 alpha=Mask(0.2764706) vertices=1010 spawned
```

`model\pl\beard\beard104\beard104`

```text
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=0 group=66 lod=255 draw_mode=0x1020 material=XfBAN__E_00_m005_1 alpha=not-built vertices=70 skipped:no-colour-pass
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=1 group=66 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_1 alpha=Mask(0.2764706) vertices=761 spawned
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=2 group=66 lod=255 draw_mode=0xfdf7 material=XfBAN__E_00_m005_1 alpha=Mask(0.2764706) vertices=543 spawned
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=3 group=66 lod=255 draw_mode=0xfdf7 material=XfBA_N__E_00_m005_ alpha=Blend vertices=299 spawned
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=4 group=66 lod=255 draw_mode=0xfdf7 material=XfBA_N__E_00_m005_ alpha=Blend vertices=341 spawned
PART actor=2885v0 model=model\pl\beard\beard104\beard104 mesh=5 group=66 lod=255 draw_mode=0xfdf7 material=XfBA_N__E_00_m005_ alpha=Blend vertices=126 spawned
```

### Quina

`model\pl\f\f_base\f000\f000`

```text
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=0 group=14 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=224 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=1 group=15 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=345 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=2 group=6 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=227 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=3 group=4 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=174 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=4 group=5 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=179 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=5 group=7 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=230 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=6 group=1 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=350 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=7 group=12 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=52 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=8 group=11 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=147 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=9 group=9 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=40 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=10 group=8 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=80 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=11 group=13 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=176 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=12 group=10 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=116 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=13 group=22 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=88 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=14 group=2 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=132 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=15 group=3 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=133 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=16 group=30 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=148 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=17 group=31 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=Opaque vertices=458 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=18 group=15 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=76 skipped:lod0-disabled
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=19 group=0 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m003_ alpha=not-built vertices=343 skipped:lod0-disabled
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=20 group=0 lod=255 draw_mode=0x0 material=XfB_N__E_00_m003_ alpha=not-built vertices=139 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=21 group=0 lod=255 draw_mode=0xe6c8 material=XfB_N__E_00_m003_ alpha=not-built vertices=762 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=22 group=20 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=454 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=23 group=23 lod=1 draw_mode=0x1020 material=XfB_N__E_00_m004_ alpha=not-built vertices=4 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=24 group=23 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=1258 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=25 group=21 lod=1 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=Opaque vertices=722 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=26 group=0 lod=6 draw_mode=0x1020 material=XfB_N__E_00_m004_ alpha=not-built vertices=27 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=27 group=0 lod=6 draw_mode=0xfdf7 material=XfB_N__E_00_m004_ alpha=not-built vertices=136 skipped:lod0-disabled
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=28 group=17 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=not-built vertices=48 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=29 group=0 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=Blend vertices=144 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=30 group=16 lod=1 draw_mode=0xfdf7 material=XfBAN__E_01_m008_ alpha=not-built vertices=98 skipped:part-group-hidden
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=31 group=0 lod=1 draw_mode=0xfdf7 material=XfB_N__E_01_m001_ alpha=Opaque vertices=37 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=32 group=0 lod=1 draw_mode=0x1020 material=XfBAN__E_00_m005_ alpha=not-built vertices=312 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=33 group=0 lod=1 draw_mode=0xfdf7 material=XfBAN__E_00_m005_ alpha=Mask(0.35490197) vertices=464 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=34 group=0 lod=1 draw_mode=0xfdf7 material=XfB_N__E_01_m002_ alpha=Opaque vertices=37 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=35 group=0 lod=1 draw_mode=0xfdf7 material=XfBAN__E_00_m007_ alpha=Opaque vertices=831 spawned
PART actor=3161v0 model=model\pl\f\f_base\f000\f000 mesh=36 group=24 lod=1 draw_mode=0xfdf7 material=XfBAN____01_m006_ alpha=Blend vertices=222 spawned
```

`model\pl\npc\e_npc\e_npc005\e_npc005`

```text
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=0 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_ alpha=not-built vertices=84 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=1 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_ alpha=Mask(0.47254902) vertices=489 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=2 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_ alpha=not-built vertices=50 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=3 group=301 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_ alpha=not-built vertices=100 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=4 group=300 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_ alpha=not-built vertices=49 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=5 group=301 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_ alpha=not-built vertices=40 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=6 group=300 lod=255 draw_mode=0xe6c8 material=XfBAN__E0__m000_ alpha=not-built vertices=79 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=7 group=301 lod=255 draw_mode=0xe6c8 material=XfBAN__E0__m000_ alpha=not-built vertices=35 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=8 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_ alpha=Mask(0.47254902) vertices=535 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=9 group=301 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_ alpha=Mask(0.47254902) vertices=230 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=10 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_ alpha=Mask(0.47254902) vertices=229 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=11 group=301 lod=255 draw_mode=0xffff material=XfBAN__E0__m000_ alpha=Mask(0.47254902) vertices=72 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=12 group=301 lod=255 draw_mode=0x1020 material=XfBAN__E0__m000_1 alpha=not-built vertices=40 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=13 group=301 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_1 alpha=Mask(0.47254902) vertices=128 spawned
PART actor=3161v0 model=model\pl\npc\e_npc\e_npc005\e_npc005 mesh=14 group=300 lod=255 draw_mode=0xfdf7 material=XfBAN__E0__m000_1 alpha=Mask(0.47254902) vertices=81 spawned
```

`model\pl\hair\hair105\hair105`

```text
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=0 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=182 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=1 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=144 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=2 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=132 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=3 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=188 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=4 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=512 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=5 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=258 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=6 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_ alpha=Blend vertices=185 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=7 group=50 lod=255 draw_mode=0xffff material=XfBAN__E00_m005_ alpha=Blend vertices=48 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=8 group=50 lod=255 draw_mode=0xe6c8 material=XfBAN__E00_m005_2 alpha=not-built vertices=119 skipped:no-colour-pass
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=9 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_2 alpha=Blend vertices=76 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=10 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_2 alpha=Blend vertices=283 spawned
PART actor=3161v0 model=model\pl\hair\hair105\hair105 mesh=11 group=50 lod=255 draw_mode=0xfdf7 material=XfBAN__E00_m005_2 alpha=Blend vertices=316 spawned
```
