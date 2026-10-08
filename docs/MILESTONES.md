# Milestones

Each milestone is a route the players can actually do, with a gate checked
against the original game. Co-op comes early (M3) on purpose: it validates
the architecture before many systems are built on it.

| # | Route | Gate |
| --- | --- | --- |
| **M0** | Foundation: tooling, exe identity, DTI class registry labelled in Ghidra, `.arc` reader, data inventory | Every `.arc` in nativePC opens and lists entries; DTI names applied to the Ghidra project |
| **M1** | Look: load one area (Cassardis) and fly a free camera through it | Screenshot comparison against the original at fixed camera poses |
| **M2** | Walk: an Arisen with skeleton and animations, movement, collision and the third-person camera | Walk a fixed path; positions and timing match a recording within tolerance |
| **M3** | Together: host and client on LAN, two Arisen walking Cassardis | Both see each other move smoothly; client input is applied by the host |
| **M4** | Fight: one enemy type, basic combat, the main pawn following and fighting | Damage numbers and enemy behaviour match the original for a scripted fight |
| **M5+** | NPCs, dialogue with the interaction lock, quests, the opening, then outward through the campaign | Per-route |

## Status

**Active: M0** (started 2026-10-08)

Done:
- JDK 21 (Temurin 21.0.12) installed; Ghidra 12.1.4 runs headless.
- Analysis copy of DDDA.exe unpacked with Steamless 3.1.0.5 (SteamStub 3.1).
- Exe imported into `E:\ddda-re\ghidra\DDDA.gpr`; auto-analysis done
  (11.5 min, 33,870 functions). Backup in `E:\ddda-re\ghidra-backups`.
- GhidraMCP 6.0.0 built against 12.1.4, running on 127.0.0.1:8089,
  registered with Claude Code as `ghidra-ddda` (user scope).
- Repo created with a whitelist `.gitignore`, AGENTS.md, method and co-op
  design docs.
- Format survey (`E:\ddda-re\notes\formats-survey.md`): 8,536 `.arc` v7
  archives, 358k zlib entries, 144 resource types; MIT specs available
  (Albam, Riftstone).
- DTI class registry harvested: 4,343 classes, 4,285 vtables, 25,712
  virtual functions named ([DTI.md](DTI.md)).

- DTI labels applied to the Ghidra project: 16,004 vtable-only functions
  created, 30,446 functions and 12,971 data labels named (99 rows failed
  function creation; not yet investigated).
- `mtarc` crate (ARC v7 reader) and `ddtool` CLI. **Gate passed
  2026-10-08:** `ddtool arc-scan <nativePC> --inflate` opens all 8,536
  archives, inflates all 358,431 entries to their recorded sizes, 144
  types all named from the DTI registry, 0 failures, 33 s.

In progress:
- Ghidra re-analysis over the newly created functions.

Next action (M1 starts): TEX reader (`rTexture` v0x99, 70% of the data),
then MOD + MRL, using Albam's MIT specs and checking every claim against
the data with `ddtool`.
