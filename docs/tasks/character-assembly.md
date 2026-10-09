# character-assembly: build characters the way the game does

- **Owner:** claude-7
- **Branch:** `task/character-assembly`
- **Board area:** `render/characters`
- **Status:** open

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

(filled in by the agent)
