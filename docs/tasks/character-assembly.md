# character-assembly: build characters the way the game does

- **Owner:** claude-7
- **Branch:** `task/character-assembly`
- **Board area:** `render/characters`
- **Status:** review

## Goal

One correct character builder for the viewer: Arisen, pawns, NPCs and
event actors.
- **One skeleton** shared by every part (body, outfit, hair, beard,
  equipment), not one copy per model.
- **Part on/off masks**, so skin doesn't show through clothes.
- **Body and face morphs, and colours** from edit records (save
  `cSAVE_DATA_EDIT`, NPC looks).
- **Equipment models** from the item ids that `dditem` / `ddeconomy`
  bags hold, for the Arisen and pawns.

Start from the event-actor builder (0x0083e010) and the player's
equipment path. The `chara-edit` research in the Codex lane may help:
coordinate on the board.

## Scope

- `crates/ddview/src/character.rs`
- `crates/ddview/src/event.rs`
- `crates/mtmod/**`: readers, if needed
- a new crate `ddlook/**` for the headless look data
- `docs/ANIMATION.md` or a new `docs/CHARACTERS.md`

## Done when

- Event NPCs and the saved Arisen and pawns render dressed, with no skin
  bleeding through and one skeleton.
- Their morphs and colours come from the save or the NPC looks.
- Screenshots are described.
- Fill in Results and report.

## Results

Details are in `docs/CHARACTERS.md`.

**Done.**
- **`ddlook` (new crate):** headless looks from `uActorModelPl::vf05`
  0x0083e010.
  - Readers for rEditPawn, rArmorTable, rArmorModel, rArmorModelNpc,
    rArmorTableNpc and rArmorPartsOff.
  - The 512-bit part mask.
  - NPC event looks: set models (0x00841960) and the set's on/off mask
    (0x00841e20).
  - Dressed looks: equipment, hair and beard models (0x008407f0), and the
    mask from rArmorPartsOff (0x008449b0 / 0x00bc1060).
  - Save looks for the Arisen and the party pawns (`ddlook::save`).
  - Tests:
    - the ev10 NPCs' models and masks;
    - every armour model path resolving;
    - the private save's characters dressed with parts hidden.
- **`ddview character.rs`:** `spawn_assembled` gives one skeleton. Part
  bones bind to the base joints by joint id, and meshes are filtered by
  the part mask. `spawn` keeps its signature for `main.rs` / `play.rs`.
- **`ddview event.rs`:**
  - event NPCs are assembled (Quina and the elder, outfits, hair, beard,
    hair colour);
  - `--save DDDA.sav` dresses the player actor as the saved Arisen;
  - `--lineup DDDA.sav` shows the Arisen and pawns.
  - Screenshots are described in CHARACTERS.md.
- **Colours:** NPC hair and beard colour from the edit floats (traced),
  applied as an albedo tint (approximation).

**Not done (missing links named in CHARACTERS.md).**
- **Morphs:** not applied. The face, body and human edit formats are
  known, but their application through `uEditModel` / `rJointEx` was not
  traced.
- **Save colours:** the palettes for `mColor*` are unknown.
- **Skin and eye colours.**
- **Weapons.**
- **Inferred:** the item -> piece link (item record +0x40, 640 matching
  records) and the save hair/beard ids.
- **Guessed:** no beard for female characters.

**Housekeeping.**
- `ddstory::look` still holds an older copy of the NPC readers; switching
  it to `ddlook` is a small follow-up for the ddstory owner.
- `crates/ddview/Cargo.toml` gained the `ddlook` and `ddsave` lines (asked
  in #1021).
