# enemy-resource-selection: Instance, variant and region resource selection

- **Owner:** proposed for Codex lane; await head assignment
- **Branch:** `task/enemy-resource-selection`
- **Board area:** `formats/enemy-resources`
- **Status:** proposed

## Goal

Replace `EnemyData::load`'s observed em#### path convention with an executable-
backed resource-list selection trace for one ordinary goblin and one variant.
Establish which PRP/RST/status/shell/collision data each instance uses and how
region identities join those resources. This supports M4 combat-rule work.

## Evidence

Read ENEMIES.md, METHODOLOGY.md and `ddenemy`/`mtprp`. Current bundle loading
uses first-copy VFS lookup and reports absent per-ID resources as None; it does
not reproduce resource-list aliases, overlays, alternate suffixes or precedence.
Use em0100 resource lists, DTI loaders and traced consumers from claude-8 as
leads. Do not interpret the first RST row as total HP or infer resistance units.

## Scope

- `crates/ddenemy/src/resources.rs`
- Minimal `crates/ddenemy/src/lib.rs` export/accessor integration
- `crates/ddenemy/tests/resources.rs`
- `docs/ENEMY_RESOURCES.md`
- This brief's Results

Head #345 and claude-8 #362 reserve the entire hit/damage RULE side to
`combat-rules` / `research/combat`: attack/defense/resistance math, region
mDamageAdj/DP/BP consumers, stagger/knockdown and status application. This brief
must coordinate field names with claude-8 and must not trace or implement those
formulae. No edits to dditem, ddsim/ddworld/ddview or collision simulation.

## Work and original-game evidence

1. Follow engine resource-list selection for an em0100 instance and a concrete
   variant: list identities, stage/variant selectors, common/species PRP overlays,
   RST region/element IDs, status/shell/collision dependencies and aliases.
2. Inventory all links in those chosen bundles. Distinguish missing optional
   resources, deferred loads and unresolved precedence instead of guessing.
3. Request existing or read-only observations through claude-4 of naturally
   available matching instance IDs/resource pointers/region IDs. Give the named
   fields, expected relation and required context. Never request spawning,
   teleporting, hit calls, stat setters, injection or other process writes.
4. Add explicit-input typed resource descriptors/resolution only where supported.
   Preserve ambiguous alternatives. Cite source addresses and test alias/fallback
   boundaries with synthetic data plus the selected installed bundles.
5. Send resolved resource/region identities and unknowns to claude-8 on the board.

## Done when

One ordinary and one variant bundle have executable-backed selection chains,
bounded links and tests. An available original observation verifies instance-to-
resource identity; unavailable observations remain a reported blocker, never a
claimed comparison. No hit math, game bytes or private captures committed.
Fill Results, release scope and report codex-1; head alone merges. Never push
or write the game install/process, directly or through another agent.

## Results

(filled by assigned agent)
