# From the title screen to the first gameplay frame

Brief: [L38](tasks/L38-gameplay-path.md). The sequel to
[TITLE_PATH.md](TITLE_PATH.md).

**What it covers:** what the original does between the title's menu and
the first frame in which the player controls a character, for New Game
and for Continue, and what the rebuild still lacks on that path.

**Method.** Static, as in TITLE_PATH:
- Ghidra, the inventory's call graph, scans of the lifted sources;
- the lifter's own `lift()`, run read-only on every function that is not
  verified.

The route itself (prologue, stage 800 movie, character creation, the
dragon) was traced before; see OPENING_SLICE.md, QUESTS.md "Worked
example: the opening" and EVENTS.md.

## 1. From the title to the areas

### 1.1 The title's states

`aTitle` runs a state function stored at `+0xF0` (slot 7, 0x005317B0).
In order:
1. **0x00531800, "press start".** It creates `uGUITitle` and the movie
   (TITLE_PATH 1.6). When the GUI reports a choice (`+0x1C8 == 1`) it
   releases both, resets `sSave`, and switches through `0x00533320` to
   the menu. That switch also sets the "first entry" flag
   `sGameSys+0xBE441`.
2. **0x005325E0, the menu.** The menu logic is `0x0052FC30`.
   - Result 2 goes back to the press-start state.
   - With the first-entry flag set it goes to the label 0x00532660.
   - Otherwise it goes to `0x00532090`, which calls 0x00531EF0 and then
     runs the label 0x00532C40 (*inferred*: the load path).
   - `0x0052FC30` itself issues the storage request kind 13 (`+0x648 =
     0xD`) when the flag is set, and `0x00498BE0` otherwise.
   - `0x00532110` (installed at 0x00531ED5) issues kind 9 (TITLE_PATH
     5.4).
3. **The handover.** The scan of every lifted `sArea` vf+0x28 call
   (`*0x018D099C`, the area-change request) finds these title calls:

| Call site | Target area | Before it |
|---|---|---|
| 0x005331E7 (label state 0x005330E0) | **`aGameMain`** (DTI 0x0198CDF0) | `sSave`/`sGameSys` flags (`+0x20E719`, `+0x21AFDA`) |
| 0x00533277 (state 0x00533200) | **`aStage801`**, character creation (0x0198D5D8) | a countdown `+0xBC` −= `mDeltaTime` from [0x015184E0]; then `0x00498DD0` (`sSave`) |
| 0x00533302 / 0x0053330B (state 0x00533280) | **`aStage802`** (0x0198D600) or **`aStage804`** (0x018FA5C8), by `+0xC0` | the same countdown; `0x00498FA0(sSave)` |

*Inferred.* `aGameMain` is the Continue / New Game route: its slot 7
runs `0x004FB7B0` once, "new game or load" (FRAME.md 6.2). Stage 801 is
character creation (OPENING_SLICE.md); 802 and 804 are not identified.
*Not traced:* which menu item reaches which call site.

**Where the other areas come from.**
- No code references `aBbsRpg`'s DTI; only its DTI registration does.
  The live stack is `[aBbsRpg, aGameMain, aStageNNN]` (ORACLE.md).
  *Inferred:* `aBbsRpg` is created as `aGameMain`'s parent when the area
  stack is built. The area-change request checks the transition through
  `0x00419720` → `0x00419D50`, which consults 256 registered checkers at
  `sArea+0x384C`.
- Other `aGameMain` requests: 0x0050B655 (in `aStage` code).
- `aTitle` is requested from 0x00401DDB (in 0x00401AA0, the end
  credits), 0x004FAD43, 0x0050F5CD and 0x0052CF97 (the returns to the
  title; the others are not identified).

### 1.2 `aGameMain` and the stage

- **`aGameMain` slot 7** (0x004FB960, read from the disassembly) runs
  `0x004FB7B0` once, then the managers' slot 6 every frame (FRAME.md
  6.2).
- **`0x004FB7B0` creates the stage area.** It calls `0x004FBD50`, which
  formats `"aStage%03d"` (0x0156CCE4) and requests that area (call site
  0x004FBDB5). The stage number:
  - for New Game: stage **380**, SceNo 100, the prologue (QUESTS.md,
    traced);
  - for Continue: the save's stage (*inferred*).
- **The opening's route**, per OPENING_SLICE.md:
  1. stage 380 prologue;
  2. stage 800 (q0000; the movie `movie\event\st100ev00.wmv`);
  3. stage 801 character creation;
  4. stage 200 (the dragon);
  5. stage 100, Cassardis.

### 1.3 Every area waits for its shader archive

Every area's slot 5 starts with `0x00533750` and does nothing until it
returns true:
- `aInitialize` 0x004FC270;
- `aTitle` 0x00531290;
- `aBbsRpg` 0x004FA5C0.

That check calls **`0x00DA63F0`** (verified). It:
1. builds `"sa\\%s\\%s"` (0x01428E8C) from the API name (`sShader2`
   vf+0x24 with `sRender+0x34`, i.e. `DX9`) and the area's name (area
   vf+0x2C);
2. returns ready at once if the file does not exist;
3. otherwise loads it as an `rArchive` (asynchronously, flag 2) and waits
   until it is loaded.

So `aTitle` waits for `sa\DX9\Title.arc`, `aGameMain` for
`sa\DX9\GameMain.arc`, and each stage for `sa\DX9\StageNNN.arc`
(*inferred*: the `Stage380_PROLOGUE` and `_T` archives belong to areas
of those names).
`aInitialize` has no archive and goes straight on.

The common shaders come earlier: **`0x00E0E470`**, from `sShader2`'s
constructor 0x00E121D0 and from 0x00E10410, loads `sa\DX9\root` and its
`sc\DX9\root` shader package (`"sc\\%s\\root"`, rShaderPackage)
synchronously. **0x00E0E470 is not lifted** (it lifts with today's
lifter, calling unlifted code), and it is on the title path. This closes
TITLE_PATH's open item 5.1.

### 1.4 Files on the way

- **Areas:**
  - `aBbsRpg`: `system\TexDetail\TexDetail`, `rom\bbsrpg_core` (id
    24146), `rom\bbs_rpg` (id 1);
  - `aGameMain`: `rom\game_main` (id 3, *inferred* from its name);
  - stages: `rom\stage\stage300\stage380` (id 17), and `stage800`
    (id 41), `stage801` (id 42), `stage200` (id 6), `stage100` (id 5).
  - The shader archives as above.
- **Events:** `rom\event\sSSS_EE.arc` (EVENTS.md); the prologue's
  st380 ev00, then st100 ev00 (the movie event), st200 ev00/01/05.
- **Movies:** `movie\event\st100ev00.wmv` (WMVCore, L18).
- **The save (Continue):** `DDDA.sav` through `ISteamRemoteStorage`
  (TITLE_PATH 5.4, L26).
- **Character creation** (stage 801): the edit GUIs (CHARA_EDIT.md).

**Imports and services.** The closure of the gameplay path (section 2)
reaches 156 imports. **None is unbound beyond those already on the title
path** (TITLE_PATH 2.2): IMM32 on IME messages, the conditional WS2_32,
capture and fault paths, and D3DX at shader links. Nothing new is
needed from claude-2, claude-5 or claude-10 for this leg.

## 2. The lifter cross-check: every non-verified function

The lifter's own `lift()` was run on **all 586 functions the inventory
does not mark verified**. Each was placed by whether it is reached:
- **title:** L35's likely title path;
- **gameplay:** the title→gameplay closure (21,580 functions). Its
  roots are the title's functions and the methods of 1,581 classes:
  - the areas, the singletons and the resources;
  - the player, pawn, NPC and enemy units, and the player and pawn
    actions;
  - the cameras, events and movies;
  - the stage, sky, weather, grass and light units;
  - the GUIs, the FSMs, weapons, shells, effects, soft bodies and
    chains;
- **elsewhere:** neither.

| Verdict | Title | Gameplay | Elsewhere | All |
|---|---:|---:|---:|---:|
| lifts now, calls unlifted code | 50 | 63 | 104 | 217 |
| lifts now | 49 | 45 | 99 | 193 |
| x87 return | 19 | 16 | 49 | 84 |
| SSE approximation (`rsqrtps` 21, `rcpps` 9, `rcpss` 1) | 1 | 4 | 26 | 31 |
| jump table | 4 | 1 | 4 | 9 |
| `stmxcsr` / `ldmxcsr` | 4 | 2 | 3 | 9 |
| `rcr` | 4 | 0 | 2 | 6 |
| disassembly overlap | 0 | 0 | 5 | 5 |
| other x87 (`fyl2x`, `fxam`, `fptan`, `fpatan`, `frndint`, `fldpi`, `fnclex`, overflow) | 5 | 1 | 7 | 13 |
| platform-dependent (CPUID, `pushfd`, `rdtsc`) | 3 | 1 | 0 | 4 |
| other instructions (`pmaxsw`, `pslldq`, `ss`, `enter`, `pause`, `int1`, `les`, `lodsb`, `bound`, `clc`, `repne`) | 5 | 1 | 9 | 15 |

(The title column counts L35's likely closure, CRT included. All 586:
410 lift now, 176 rejected.)

### 2.1 For claude-3 (lift and record): 410 functions that lift now

They are missing only because they were not in the last lift run.
- 99 are on the title path (L35, already queued with L28).
- 108 more are on the gameplay path:

  0x004734E0 0x0048CC00 0x004EE180 0x004F5200 0x00535CE0 0x005A5180
  0x005EC920 0x00606090 0x00606670 0x006076F0 0x006335A0 0x00633A70
  0x00633B70 0x006692F0 0x00677930 0x0068A160 0x0068A9F0 0x006A1770
  0x00705E00 0x007975D0 0x007DF090 0x007F2020 0x0080AED0 0x00815C60
  0x00B53220 0x00B55900 0x00D13AE0 0x00D4EE60 0x00D60080 0x00D93880
  0x00DB9C30 0x00EDE880 0x00EE3710 0x00F276C0 0x00F47E90 0x00FE3560
  0x00FF0AD0 0x00FF0D20 0x00FF0E60 0x00FFD6F0 0x01054800 0x0111D670
  0x011C15F0 0x012E1AD4 0x0132A1EC 0x00408520 0x00475DD0 0x00498220
  0x004C7530 0x004EE3E0 0x004EE720 0x004F53B0 0x005F3450 0x0062E3A0
  0x00633EF0 0x00634E00 0x0064DD20 0x0064E2B0 0x00685030 0x00689960
  0x00691E50 0x00692A40 0x006E0EB0 0x006F1590 0x00712AC0 0x0071C0F0
  0x00796420 0x007BEA60 0x007DF790 0x007F2120 0x008282E0 0x0085B520
  0x0085E220 0x00860A60 0x0086D7E0 0x00874F90 0x008757B0 0x00878250
  0x0087B710 0x00ACD2C0 0x00AFD8D0 0x00B25810 0x00B54BE0 0x00B5A8D0
  0x00B74E80 0x00B75EC0 0x00B7FF30 0x00BAB3A0 0x00BD45C0 0x00C1D790
  0x00CE8C50 0x00CE8CC0 0x00CEEBC0 0x00D0C590 0x00D8BCE0 0x00DBA780
  0x00E04EB0 0x00E0D750 0x00E0D940 0x00E111A0 0x00E68BA0 0x00EAFD90
  0x00F6A380 0x00F77CC0 0x00F7DE40 0x011251F0 0x012EE1A1 0x012F9E95
- The other 203 are not reached from these roots.

The full list, with verdicts and call chains, is private
(`C:\ddda-targets\L38\queues.tsv`).

### 2.2 For claude-6 (lifter rejections) on the gameplay path

| Function | Cause | Reached from |
|---|---|---|
| **`cPlActJumpBegin::vf06` 0x00ACC6D0** | x87 return | the player's jump start |
| `uSimSoftBody::vf09` 0x00F18DB0, 0x00F16670 | x87 return | cloth |
| `uWeaponEv::vf96` 0x00C1D6E0 | x87 return | event weapons |
| `uEffect2D::vf09` 0x00FD60B0 | x87 return | 2D effects |
| `uShlArrowInc::vf108` 0x00BD0B40, `uShlLightningThunder::vf108` 0x00BF7CD0 | x87 return | projectiles (Strider arrows, magick) |
| 0x006051C0, 0x007126C0, 0x00B1BF90, 0x00B48600, 0x00CF9220, 0x00D55D50, 0x00F23830 | x87 return | GUI, actions, number parsing |
| CRT `__cintrindisp2` 0x01304690, 0x01304753, `__trandisp2` 0x01300497 (`fxam`) | x87 | `uGrass::vf08` → CRT math (0x012F4E6A) |
| `uOmObj11000::vf105` 0x00C2A570, `uOmObj7520::vf105` 0x00C45420, 0x010819E0, 0x01119610 | `rsqrtps` | objects; the result depends on the CPU's approximation |
| 0x004594A0 | jump table | GUI (0x0064CA50) |
| 0x011BA3C0 | `rdtsc` | 0x010C8320 |
| CRT `_strrchr` 0x012EF010 | `repne` | file paths (0x010A92A0) |
| 0x012EF110, 0x012F3690 | `stmxcsr` | CRT float control (0x005DDB70, 0x0111D290) |

**On the title path** (L35 2.1, already sent):
- CPUID and `pushfd` in `sMain`'s constructor;
- `fyl2x` 0x00E22520;
- the number-parser x87 group;
- `pmaxsw` (3), `rcr` (4), `stmxcsr` (4).

## 3. Not done

- No run.
- The title's menu texts are not mapped to the menu results (4.1).
- The registration of the area tree's parent links is not found (4.2).
- The closure's roots over-approximate the gameplay path (whole
  classes).

## 4. Follow-up (L38b)

### 4.1 The title's menus and their area requests

`aTitle` slot 6 (0x00531400) picks the first state.
- **Normally `0x00531800`**, press start (TITLE_PATH 1.6). It is
  followed by `0x00533320` → `0x005325E0`, the menu run by `0x0052FC30`
  (1.1).
- **`0x00531A20`, the top menu**, when `sGameSys+0xB8840` has bit
  0x40000000 or 0x10000, or `+0xB883C` has bit 4. Before it,
  0x00531400 resets the save structures (0x0043C580, 0x004965A0) and
  sets `sSave+0x21AFDC = 1`. *Inferred:* the game sets these flags when
  it returns to the title area.

**`0x00531A20`** creates **`uGUITopMenu`**, or **`uGUILoadGame`** when
`+0xB883C` bit 4 is set. It then switches on the menu's result
(`+0x1C8`):

| Result | Next |
|---|---|
| 0x10 | state 0x00532FB0 |
| 0x11 | `0x00449FD0` (clears the four party slots), `0x004373C0` (sets `sGameSys` flags), then state 0x00533200 → **`aStage801`**, character creation |
| 0x12 | the same set-up, then state 0x00533280 → **`aStage802`** |
| 0x14 | the same set-up, then state 0x00533280 → **`aStage804`** (`+0xC0` = result ≠ 0x12) |
| 1, 2, 5, 0xF, 0x13, 0x15 | other states (not followed) |

**`aGameMain`** is requested by the label state 0x005330E0. That state is
installed by:
- the load flow `0x00532110` (after storage request kind 9: Continue);
- the code at 0x00531E72 and 0x00531EC8.

*Not traced:* which menu text gives which result. The texts are in
`id\menu\title_2\menu_01` / `id\DDN\menu\title_1\top_01`. A live read of
`uGUITopMenu+0x1C8` per item would settle it.

### 4.2 How `aBbsRpg` is created

**The mechanism.** `sArea` keeps an area tree: 512 `sArea::AreaInfo`
nodes from `+0x20`, 0x1C bytes each (vtable, the area's DTI at `+8`, the
parent node at `+0xC`).
- An area request (`sArea::vf10` 0x00DB4C10) only stores the target DTI
  at `+0x3828`.
- `sArea::vf06` then calls `0x00DB4F10`, which:
  1. finds or adds the target's node;
  2. walks its parent links to the root;
  3. compares that chain with the current stack (`+0x382C`, up to 8)
     through each area's DTI and its DTI parents;
  4. pops and pushes the levels that differ.
- So requesting `aGameMain` (or a stage) brings up every ancestor area
  first. That is how `[aBbsRpg, aGameMain, aStageNNN]` arises without any
  code naming `aBbsRpg`.

**What it rules out.** All area DTIs register with the same parent DTI
0x0198D678 (*inferred* `cArea`; e.g. `aGameMain` at 0x013306A0,
`aBbsRpg` at 0x013305E0), so the tree is not the class hierarchy.

*Not found:* where the parent links (`aGameMain` under `aBbsRpg`, the
stages under `aGameMain`) are registered. No code references these DTIs
outside their registration. *Inferred:* registered at run time from
another source (by name, or from a resource).

### 4.3 The gameplay path's archives by name

| Requester | Archives |
|---|---|
| `aBbsRpg` slot 5 (0x004FA5C0) | `system\TexDetail\TexDetail`, `rom\bbsrpg_core` (24146), `rom\bbs_rpg` (1) |
| 0x004FB050 (no direct caller; *inferred* `aGameMain`'s slot) | `rom\dl1\game_dl1` (24148) |
| `aGameMain` (*inferred*) | `rom\game_main` (id 3): the requester was not found |
| every area, slot 5 | `sa\DX9\<area>.arc`: `GameMain`, `Stage380` (also `Stage380_PROLOGUE`, `Stage380_T`), `Stage800`, `Stage801`, `Stage200`, `Stage100` |
| the stage loader **0x004FFBA0** (called from 0x00525160, 0x0052ADB0, 0x0052B940) | see below |
| stage event mode 3 (0x0050BE20 → 0x005171A0), simple events (0x0050DCD0 → 0x00517470) | the event archives `rom\event\sSSS_EE` (computed ids; EVENTS.md) |
| Continue | `DDDA.sav` (TITLE_PATH 5.4) |

**What the stage loader 0x004FFBA0 requests:**
- with `i` = the stage's index (0x00508B50):
  - `i + 5`: `rom\stage\<group>\stageNNN`;
  - `i + 0x8407`: `..._nav` (when listed);
  - `i + 0x8448`: `..._om` (when present);
- `rom\gui\GUIuGUITimer` (33795), when `sSave+0x20E77B` has bits 0xC;
- weapon and NPC archives from the stage's lists (e.g. `rom\wp\w0\one`,
  `rom\npc\f02\npc223` id 1114 by default);
- the dragon `rom\enemy\em5800` / `em5801` (162/163) on stages
  0x1F5/0x1F6 and 0x25D;
- a fixed weapon list (`rom\wp\w1\dag`, `dag000`, `rom\wp\w0\one`,
  `one004`, `one006`) on stage 0x263.

**The opening's files** (present in the install, sizes in bytes):

| File | Size |
|---|---:|
| `rom\stage\stage300\stage380.arc` | 124,689,241 |
| `stage380_nav.arc` | 148,157 |
| `stage380_om.arc` | 36,003,921 |
| `rom\stage\stage800\stage800.arc` | 1,377,577 |
| `stage801.arc` | 1,967,053 |
| `rom\stage\stage200\stage200.arc` | 153,950,909 |
| `stage200_nav.arc` | 261,264 |
| `stage200_om.arc` | 43,673,401 |
| `rom\stage\stage100\stage100.arc` | 10,160,171 |
| `rom\game_main.arc` | 23,775,211 |
| `rom\bbs_rpg.arc` | 11,177,818 |
| `rom\bbsrpg_core.arc` | 4,590,287 |
| `rom\dl1\game_dl1.arc` | 1,018,928 |
