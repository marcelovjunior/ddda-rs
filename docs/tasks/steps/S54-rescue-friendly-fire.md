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

## Results (native gauge permission checkpoint; viewer rescue acceptance open)

Research establishes a missing NPC actor permission, rather than a demo
strategy problem. Collision callback **0x0076D730**, at **0x0076D817**, admits
player attack kind0 against NPC hurt kind3 through the ordinary mask. That
path bypasses the team matrix. However, an admitted collision is not yet
an accepted damage result.

**uNpc vf66 0x00BB6EC0** passes the attack's owner to **0x00BB7680** before
inherited player mitigation. Its branch **0x00BB778B..0x00BB77D4** rejects
player/pawn actor bits +0x206D & 6 when the victim has linked intel, its
byte +0x3D1 is zero, actor hostile bit +0x206E & 0x10 is clear, and intel
byte +0x547 is nonzero. It also rejects an NPC attacker with class bit8
and its own linked gauge byte set. That attacker test is independent of
the owner's later hostile-NPC classification. This guard has no region
check and does not depend on the civilian ledger byte. Goblins remain
eligible attackers.

The gauge byte is authoritative behavior state. **SetGaugeDisp 0x00534BC0**
calls **0x00BB8C10** for true and **0x00BB8CA0** for false. They write linked
intel +0x547 at **0x00BB8C90 / 0x00BB8CE2**; the absent-body handler writes
the same byte directly at **0x00534C10**. The installed q0002_n304 graph
keeps SetGaugeDisp true after state5 clears NoDamage and Invincible. The
existing host retained the gauge request for presentation, but its combat
pass omitted this permission. The Japan-only HP1/escape path from S34 is
a separate rule and cannot substitute for this region-independent veto.

Implemented within head's board approval #6984:

- An explicit `GaugeHitGuard` for the ordinary linked NPC adapter, separate
  from HP-only NoDamage and valuation-level Invincible. Host binding inherits
  an already-enabled gauge before the body's first hit pass; subsequent
  actor synchronization follows the canonical gauge request.
- The per-contact vf66 veto runs after attack valuation and before frame
  merging, matching **0x0076B7E0 -> 0x0076C4F0 -> collision +0xC7C**. Rejected
  contacts retain hit history and consumed critical rolls, but contribute
  no HP damage, status build-up or reaction. The first accepted contact
  supplies the resulting attacker and hurt region.
- Party classes come from registered Arisen/pawn identities, generalized
  across seats. Shells, arrows and spell shells resolve their actual shooter
  before classification, as **0x00BB7680** resolves native uShlBase +0x2544
  and uShellBase +0x25B0. Projectile histories keep their distinct owners.
  No pawn decision rule, damage scaling, immunity flag or demo input was
  invented or changed.

Tests prepared: scalar guard bypasses and actor-class branches; simultaneous
party/goblin contacts with independently gauged victims, preserved rolls and
held history; installed sword motion/hurt contacts for Arisen seat0 and pawn
seat6 with gauge on/off; gauge-enabled natural goblin damage; installed host
gauge persistence after q2:25 and disappearance after q2:20; body admission
after the gauge was already enabled. Explicit quest observations in host
fixtures isolate authored transitions; they do not claim a route success.

Tested source **551e965be5a1c00bbe9bbfd2ec745e1e50b5eda0**: full
`cargo test -p ddenemyrt -p ddhost -p ddnet`, actual process **93467 exit0**,
compile **42.42 s**. Enemy runtime **172 PASS / 2 existing ignored**,
test time **87.99 s**; host **106 PASS**, **202.83 s**; networking **25 PASS**,
**26.60 s**. Total **303 PASS**. All new installed contact/gauge/late-body
tests ran; the installed sword fixture verifies real overlaps for both
owners, so its protected cases cannot pass merely by missing Reynard.
The exclusive core lease was released only after the actual completion.
Private full log: `E:/ddda-re/notes/steps/S54/full-core.log`.

The corrected rejection/merge/history regression separately passed
(actual exit0, compile2.76 s, one test). The first focused run failed an
incorrect synthetic RNG-count assertion: accepted hits also roll statuses.
The corrected rejection-only case checks its two attack rolls and unchanged
status/reaction pools separately. Its synthetic DLC fixture also supplies
finite formula denominators. No production formula or RNG was changed.

Independently emulated actual native **0x00BB778B..0x00BB77D4** instructions
in private Unicorn: **35/35 cases** match the guard, including missing intel,
victim hostility, gauge-off, and gauged hostile-NPC attackers. This checks
the executable branch, not an original-game recording. Emulator scripts,
bytes and detailed outputs remain private.

Compared against the original: static native disassembly and installed
quest/resources only. No new original-game control, live damage recording or
viewer rescue run. Other global/event/action guards in **0x00BB7680**, its
special vf66 hit-info +0x110==0x1F check, specialized NPC bodies, and native
proxy families outside the implemented projectiles remain unported. The
preexisting separate actor/projectile numeric allocators can theoretically
overlap after enough actor allocations; this change does not fix that
broader ownership limit.

Next action: parent/head review and downstream viewer/release acceptance,
then use the current ECONv2 RescueApproach checkpoint in a bounded viewer
run, recording genuine quest success/failure and all accepted Reynard
attackers. The full S54 done gate remains open until that resumed route
succeeds for the sourced reason. No damage veto tuning or demo-policy change
is proposed.
