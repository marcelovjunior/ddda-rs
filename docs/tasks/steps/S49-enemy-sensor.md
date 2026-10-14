# S49: Enemy perception: the sensor pick and hate (who notices whom, from how far)

- **Branch:** `step/s49-enemy-sensor`
- **Status:** parent review; headless checkpoint complete
- **Files:** `crates/ddenemyrt/src/goblin.rs` (target choice), the existing
  `npc_sensor.rs` work, sensor resources/readers; tests (headless)
- **Kind:** research + one feature; headless acceptance
- **Priority:** TOP: it decides whether the q0002 rescue can succeed

## Evidence (head integration route, main c7c0505+, 2026-10-10)

Per-attacker damage on Reynard (owner 106) during the rescue: rescue goblins
100-102 (attack rate 0.2): 34 hits, ~1270; **ordinary goblin 107** (group 250,
default rate 1.3): **6 hits, 1448**. Goblin 107 started ~40 m away targeting
the Arisen, ran in (RunTrace) and switched to Reynard; he died (q2:30).
Neither the Arisen nor Rook hit him.

Today `Goblin::step` picks "the nearest live target at any distance" (the
sensor pick 0x00AB52E0 is *not ported*); S34 added NPC candidates with a
distance-only hate (0x00AB5A50 / 0x00AB52E0, codex-61). With no perception
range, every goblin on the field converges on the fight.

## Do

1. Port the enemy sensor: the em0100 sensor resource (sensor group /
   radius / FOV / LOS per state; the placement's `センサー半径倍率を使用する`
   / `センサー半径倍率` multiplier), what makes a goblin notice a player, a
   pawn or an NPC (proxy subtype masks: em0100 target mask 0x16 incl. NPC 4),
   alert/forget timing, and the hate weighting that picks among noticed
   targets (0x00AB5A50 influence, 0x00AB52E0 pick).
2. Replace the "nearest at any distance" choice with it, for players, pawns
   and NPC bodies alike; an unnoticed target is not chosen.
3. Headless installed tests: a goblin 40 m away (group 250 placement) does
   not notice the Arisen/Reynard; one inside its sensor does; the q0002
   rescue goblins target Reynard as authored.

## Done when

Goblins only engage what their sensors notice, with sources documented, and
the installed rescue test shows the group-250 goblins staying out unless the
party comes near.

## Results (2026-10-10, codex-69)

Source checkpoint: `d281e308259b8b7ec83f88a8fb2aaeb9b4a2db85`, based on
reviewed committed main `b4e142fb348a8d35076f513118dc64f0b42046d7`.
Implemented and tested headlessly against installed data; not yet compared
against an original-game route or accepted as complete enemy AI.

The initial diagnosis needed refinement: S34 already loaded sensor resources
and used the distance hate pick. The unconditional Arisen fallback and combat
flags derived from target presence bypassed perception. Native `0x008D2930`
retains that fallback for the think slot, but `0x008D1280` supplies independent
alert flags. The translated implementation preserves that distinction.

Runtime `AI\\Sensor\\Enemy\\em0100A` supplies all five sphere rows: group 1
has a 2000 cm notice radius; group 2 has 1500 cm / 60-degree joint-3 vision
with LOS, 150 cm proximity, and 1500 cm notice rows; group 4 has 1500 cm
notice. These records are read from the player's installation, not copied
into source. Sensor queries apply group, row, class and proxy-subtype masks
equally to Arisen (1), pawn (2) and NPC (4), alongside transformed joint
offsets, normalized basis, FOV and LOS. Sources include `0x00CA8D90`,
`0x00CA8EC0`, `0x00CA8AE0`, `0x00D134F0`, `0x005AF1F0` and `0x005AF9A0`.
Unsupported shapes, directions and flags return explicit errors.

Placement sensor-radius enable/value and authored initial think-table fields
are now consumed (`0x007A0000`, `0x008CDD80`). Ordinary alert/search/forget
uses `0x008D1280`, the 900-frame embedded timeout at `0x008D1B81`, the
220 cm search-point arrival threshold, and state-dependent group switching
at `0x009BB750`. Think-slot replacement waits for action completion, while
alert updates and hate decay continue (`0x008D2360`, `0x00AA96E0`). Distance
influence uses the native 3000 cm distance subset (`0x00AB5A50`) and strict
maximum pick (`0x00AB52E0`); decay is one influence point per unit call
(`0x00AB5860`). Vacant native slot order remains stable for equal-score ties.

Installed tests read actual stage-100 group-250 and group-23 placements and
Reynard's body. The three ordinary goblins begin 4278.056, 2513.353 and
5771.0566 cm from Reynard in the controlled fixture and remain unnoticed,
without RunTrace, movement or a selected target for 300 calls. A pawn
100 cm away is noticed. All three authored rescue goblins preserve Reynard
as target for 60 calls, with their authored initial root and radius fields.
The action-completion test proves a live cached target is retained until
rethink, while its hate decays. These are controlled flat-ground tests,
not a claim that group 250 stays idle after the party enters its sensor.

New source regressions cover joint orientation and scale removal, FOV/LOS,
all three candidate subtypes, radius scaling, NaN rejection, native alert
transitions and expiry, influence-slot reuse, and near-vertical joint-forward
fallback. Existing NPC contact and melee regressions still pass. Three old
host assertions assumed unlimited engagement or skipped the native alert
entry; head approved updating those fixtures in board message 6350. Close
combat still verifies real hit windows, damage, FSM ordering, death, replay
and XP; the distant case now verifies no approach or attack over 900 calls.

Final verification of the committed source: actual process 90716 exited 0
for `cargo test -p ddenemyrt -p ddhost -p ddnet`, using the exclusive core
lease and `E:/ddda-rs-wt/item-use-review-target`.

| Crate | Passed | Failed | Ignored | Summed test runtime |
| --- | ---: | ---: | ---: | ---: |
| ddenemyrt | 165 | 0 | 2 existing | 99.22 s |
| ddhost | 96 | 0 | 0 | 173.86 s |
| ddnet | 17 | 0 | 0 | 17.80 s |

Compilation took 35.78 s; doc-tests had no failures. Total: 278 passed and
two existing ignored tests. Private log:
`E:/ddda-re/notes/steps/S49/full-committed-core.log`.
The core lease was released after the actual exit in board message 6417.
No original game process, installation writes, GUI run or GPU lease was used.

Remaining fidelity limits: the current Ground adapter does not reproduce
native collision layer/mask/slope filters, target-proxy radii or LOS-bypass
metadata. Ally/noise/damage notifications, the non-distance influence sum,
additional protection/ownership/augment rules and grabbed-target overrides
remain unported. Existing melee flag placeholders and action/navigation/RNG
limits remain. No full q0002 outcome or original comparison is claimed.

Next action: parent/head review and integrate this checkpoint, then use the
current `RescueApproach.ddhs` route checkpoint (board 6401) for a bounded
integrated rescue capture with per-attacker contacts, targets and party
positions. Viewer/release downstream review is queued by codex-1; this
child handoff does not replace that review or head acceptance.
