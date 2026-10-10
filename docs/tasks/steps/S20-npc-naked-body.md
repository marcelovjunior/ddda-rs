# S20: A Cassardis NPC drawn as a bare base body

- **Branch:** `step/s20-npc-body`
- **Files:** most likely `crates/ddview/src/opening.rs` (NPC
  materialization) and `crates/ddlook` (NPC looks)
- **Kind:** diagnose and fix one visible defect

## The defect

In the opening, after ev10 (demo shot `4-control`, stage 200 SceNo 1200,
the class-selection room), a bald, naked male stands next to the Arisen:
an NPC drawn with only the base body `m000`, no hair, no clothes. It
appears with or without head's weapon-swap change, so it predates it; the
NPC materialization merges (upon-a-pawn, S13) are the likely source.
Reproduce: `ddview <nativePC> --opening --skip-intro --demo --unattended
--shots <dir>` (140 s timeout), see `4-control.png`.

## Do

Identify which NPC it is (id, record, layout), why its look has no parts
(missing look record, wrong NPC id to look mapping, a look built before
its data loaded, a stand-in body), and fix the shared NPC look path. If
the original doesn't show this NPC here at all, fix its admission instead.

## Done when

The demo's `4-control.png` shows no naked NPC (or the NPC correctly
dressed), ddview tests pass, and the Result names the NPC and the cause.
