# S21: Enemy HP bars above their heads

- **Branch:** `step/s21-enemy-bars`
- **Files:** `crates/ddview/src/hud.rs` (and a new small module if
  cleaner), `crates/mtgui` if the GUI data needs a reader extension
- **Kind:** research, then one feature

## Why

The tutorial's goblin fight needs the enemy health bars the original shows
over each enemy's head (small enemies), and the large-monster bar for
bosses like the cyclops. The viewer has no enemy HUD at all: `hud.rs`
draws only the party's bars (codex-22's game-hud, original GUI layouts).

## Do

1. Find the original's enemy gauge: the GUI resource and class (search the
   `rom\gui` archives and exe strings for the enemy/target gauge, as
   game-hud did for the party bars), what it shows (HP, the red "recoverable"
   part, the HP layers ("bars") of big monsters), where it anchors (a head
   joint plus an offset, or a screen position), and when it appears (always,
   after first hit, when targeted, within some distance).
2. Draw it in the viewer for the fight's enemies, reading HP from the
   host's fight (`ddenemyrt::fight::Fight`, goblins' `fighter.victim`), using
   the original layout and textures like the party HUD does.
3. Label anything untraced.

## Done when

A `--play --goblin --goblin` capture shows the last-hit goblin's bar,
which drops with its HP and expires after 450 frames, matching the
original's look and anchor as traced; ddview tests pass; the Result lists
the GUI resources and rules. Head confirmed this correction in board3668.

## Research checkpoint (codex-1; implementation pending)

Priority changed to S24 in board3727 before any HUD implementation. The
following source/data findings are preserved for continuation:

- `uGUILifeGauge::vf58` **0x006781c0** loads the installed
  `id/DDN/cockpit/cockpit0_1`. Small enemy artwork is `enmy_gauge`, node19
  / id142, and `enmy_gauge_back`, node34 / id164. It is not the blue
  `id/cockpit/npc_gauge` widget: that separate class is created by NPC and
  uEm8300 paths (**0x00bb8c10**, **0x00a9c2ca**).
- Damage handling `uEnemy::vf69` (callsite **0x00aaafbf**)
  enters manager wrapper **0x00452ee0**, then **0x0067c110**. For a small
  enemy, it sets the single actor pointer at gauge +0x400 and refreshes
  +0x404 to **450 frames**. Large-enemy primary/additional pointers and
  their +0x3f0 collection use separate paths; per-goblin always-visible
  bars would contradict this source.
- **0x0067a340** projects that small enemy and calls **0x0067ac40** for
  its HP crop, living/dead state and timer expiry. GUI selectors and atlas
  geometry remain runtime data. Recoverable player HP is a different
  layer; no small-enemy recoverable-HP layer was identified.
- Anchor **0x0067c970** selects one of actor position/bounds/joint modes.
  Mode0 reads actor +0x1a0 and adds +0x1b4 on Y; mode1 uses +0x1a0;
  mode2 uses the authored joint-map entry. **0x0067c750** composes the
  model transform and configured offset, subtracts the actor's stage
  offset, then adds the camera-facing offset from **0x0067c870**.
  The exact producer/meaning of +0x1a0/+0x1b4 still needs tracing.
- `uEnemy::vf34` **0x00aa9160** copies gauge mode/joint/offset from
  `cCharParamEnemy` at actor +0x5870, members +0xb4/+0xb8/+0xc0. Reflection
  **0x0074a410** associates these with `基点タイプ`, `参照関節番号`, and
  `基点オフセット`. Read them from `charparam/em/em0100_cmn` at runtime;
  the installed ordinary goblin selects mode0.

Private reader survey and traces are in `E:/ddda-re/notes/steps/S21/`.
No game data or raw source exports entered this branch. No feature/tests
or GUI acceptance are claimed by this checkpoint. Next: implement the
last-hit/expiry adapter without losing hits across multiple host ticks
per rendered frame; finish the anchor producer trace and bind the
installed small-gauge geometry to canonical enemy ownership and HP.
