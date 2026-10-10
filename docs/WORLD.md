# World layout and streaming

Findings about how stages are split, placed and streamed. "Live" means
read from the running Steam game (read-only memory access, see
`METHODOLOGY.md`); "data" means read from nativePC.

## Stage 100 (Gransys overworld)

**Files (data):**
- `rom\stage\stage100\stage100.arc` holds stage-wide resources: 579 entries, including parameters, layouts and cameras.
- `split\m<A/10*10>\n<B/10*10>\st100_<A>m<B>n.arc` holds one grid cell
  (e.g. `split\m50\n40\st100_58m49n.arc`). Each cell has:
  - one `rModel` and one `rMaterial` (`scr\st100\model\m<A/10*10>\st100_<A>m<B>n`;
    the folder comes from A, checked with `ddtool arc-ls` on 58m49n, 58m50n and 57m48n)
  - water models and materials for rivers, lakes and sea crossing the cell
    (`scr\st100\model\water\...`); not yet drawn by the viewer
  - two `rCollision` (`...\collision\...\st100h_<A>m<B>n_mrg00` and `st100e_...`)
- `split_sub\`, `split_way\` hold per-cell extras with the same naming. *Roles not yet traced.*
- `area\area_indexNN_{high,low}.arc` and `fm\fm_indexNN_{high,low}.arc` hold the terrain textures in two LOD sets, referenced by the cell MRLs.

**Cell geometry (data):**
- Cell models and collision use **cell-local** X and Z (0..10000) and **absolute** Y.
- 1,459 cell models were checked this way.
- Units are presumably centimetres, which makes a cell 100 m square. *Unresolved: confirm the unit.*

**Placement (live, 2026-10-08):**
- The game keeps a **floating origin** at the current cell. The ten loaded `uStageSplitMdl` objects had positions (`+0x40`) at multiples of 10000 around (0, 0, 0).
- The cell at the origin was `st100_58m50n`. The player's position was in the same local frame:
  - `uPlayer` at `+0x40`: (205.4, 41019.0, 3542.5)
  - the same value is the translation row of the world matrix at `+0x90`

| live cell | offset (x, z) |
| --- | --- |
| st100_57m48n / 57m49n / 57m50n | (-20000, -10000) / (-10000, -10000) / (0, -10000) |
| st100_58m48n / 58m49n / 58m50n / 58m51n | (-20000, 0) / (-10000, 0) / (0, 0) / (10000, 0) |
| st100_59m48n / 59m49n / 59m50n | (-20000, 10000) / (-10000, 10000) / (0, 10000) |

**Placement rule:**
- For cell `st100_<A>m<B>n`: X = (B - B0) * 10000 and Z = (A - A0) * 10000, relative to the origin cell (A0, B0). Y is unchanged.
- So a global frame is X = B * 10000 + x, Z = A * 10000 + z, up to a constant.

**Layout positions (data):**
- `rLayout` placements (`scr\st100\etc\st100_<A>m<B>n_<kind><N>`, read with `mtlot`) use a
  **fixed stage frame with its origin at cell 50m50n**: x = (B - 50) * 10000 + local,
  z = (A - 50) * 10000 + local, y absolute.
- Checked on three cells: 58m50n (x 0..10000, z 80000..90000), 61m51n (x 11800..17800,
  z 110800..117700), 50m40n (x about -98000, z about 9000).
- The `_s00` layout holds a cell's static scenery (`cSetInfoInsModel`). A record's name
  starts with its model id (`om0528_58m50n_U102` uses `model\om\om0525\model\om0528`).

**Verification against the original (2026-10-08, reference screenshots):**
- The reference poses' player positions sit on the terrain of the recorded origin cell
  within 1 cm (ref001, 002, 004, 005), which confirms the cell placement rule.
- A single placed prop (a sword in the ground, ref001) appears at the same place and angle
  in our render, which supports the layout frame and the Euler order assumption so far.
- No mirroring: sea and land sides match in every reference.

**Collision (data):**
- Each cell has two collision meshes, `scr\st100\collision\m<A/10*10>\marge\st100h_<A>m<B>n_mrg00`
  and `st100e_..._mrg00` (`mtsbc`), cell-local like the terrain model. `ddtool sbc-scan`: all
  7,362 `rCollision` entries parse, 3.28 M triangles, every stored normal matches its corners.
- The recorded feet of the four verified references lie on this collision within 0.25-1.04 cm.
- Placed objects (rocks, trees) have their own collision, not loaded yet: ref007's player stood
  on a rock 1.7 m above the cell collision.

## Live object access

**Addressing:**
- DDDA.exe loads at 0x400000 with no relocation. The live DTI hash table at `0x018768A0` and the descriptors match the static registry.
- Live objects are found by their class vtable. `uPlayer` vtable is `0x015E90D0`; one instance existed in stage 100.
- The current stage is the one `aStageNNN` class with a heap instance (`aStage100` here).

**Class census.** One sweep over the heap counted 391,858 objects of 1,358 DTI classes. The method:
1. Read every committed, readable region outside the image.
2. Match each u32 against the 4,343 known vtables.

**Resources:** a resource object (e.g. `rModel`) holds its path as a C string at `+0x08`.

## Hinged doors

`dddoor` resolves authored `_p` object layouts through the installed
name/class table (0x0150fe40, longest-prefix lookup 0x0075f0a0). Regular
`uOmObj7010` hinged variants use the source phases at 0x00c3f130:
closed → opening → open → closing → reset. An actor's message 0x1a opens
them; player proximity alone does not issue that message. Authored LMT
slots 3/4 open from either side, 1/2 hold open, 5/6 close, and 0 holds
closed. The opening/closing actions disable collision immediately on
opening and restore it only after closing completes. Small embedded
variant and excluded-enemy lists carry source addresses in the crate.

`ddhost::doors` owns requests, motion time, all-seat occupancy, policy
observations and replay state. Its h00 collision layer overlays existing
terrain and shares enable state with the camera. `ddview::doors` presents
host models/motions and reconciles loaded cells; it does not decide when
the animation completes or which player keeps a door open.

Cassardis's reported floor gap is authored and also appears in the
original; group18/record2 om7121 normally covers it. At scene1400/noon,
23 eligible hinged doors occur in groups17/18/19. The opening viewer
materializes these. Its interaction focus is currently an explicit
nearest-door stand-in on an interact press. Ambient enemy occupancy uses
GPL-authored spawn positions by head's explicit authorization (#2725),
pending the ambient-creatures runtime; the source enemy-kind filter still
applies. Missing population coverage prevents automatic closing. This is
implemented/tested behavior, not yet compared live against the original.

Unsupported: saved persistent startup poses, tilted/sliding variants,
full original focus arbitration, and LAN door snapshots. The opening host
supports N seats; guests do not locally invent door simulation. Details,
validation and remaining work are in `docs/tasks/doors.md`.
