# S54: The rescue and friendly fire: what protects Reynard in the original?

- **Branch:** `step/s54-rescue-friendly-fire`
- **Files:** research first; then `crates/ddcombat` / `crates/ddenemyrt`
  (hit pairing), `crates/ddpawnai` (pawn target/attack choice), or the demo
  driver; tests (headless)
- **Kind:** research + fix
- **Priority:** high (the rescue outcome is borderline)

## Evidence (codex-1 #6873/#6874, head runs, 2026-10-10)

From the RescueApproach checkpoint (S46), after q2:25 clears Reynard's
NoDamage/Invincible (q0002_n304 state5), the goblins crowd him and the
party's swings land on him: one failing run: Arisen 5 hits / 990,
hired pawn Rook 5 hits / 634, rescue goblins 21 hits / ~777 (rate 0.2),
two others ~202. He has 2500 HP (S34, sourced). One head run succeeded,
two failed. The civilian HP1/escape safeguard is Japan-only (S34, #5874).

## Do

1. Find in the source whether player and pawn attacks can damage a quest
   NPC like Reynard at all outside Japan: collision pair masks / hurt
   groups of NPC bodies (S34 noted "pair mask permits player-kind0 against
   NPC-kind3"), any party-vs-NPC damage scaling, Reynard's own flags after
   state5, the NPC ledger.
2. Pawn behaviour: does the pawn AI avoid attacking when a non-hostile NPC
   is in the swing (target choice, attack-permission checks in the GOAP /
   priority think, 'mIsAINoTarget', knowledge flags)? Port what exists.
3. Then: if the original really allows it, make the demo play the rescue
   like a careful player (e.g. engage goblins that are not on top of
   Reynard, or lure them) and document that; if the original prevents it,
   port the rule.

## Done when

The rescue from the RescueApproach checkpoint succeeds reliably for the
right reason, with the rule or the demo strategy sourced/documented.
