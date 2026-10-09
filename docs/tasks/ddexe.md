# ddexe: one reader for tables in the installed DDDA.exe

- **Owner:** claude lane
- **Branch:** `task/ddexe`
- **Board area:** `tools/exe`
- **Status:** open

## Goal

Three crates read data tables from the player's installed DDDA.exe at run
time, each with its own code:
- `ddskill::exe`;
- `ddquest::events`;
- `ddpawn`'s table reader.

Make one small crate, `ddexe`, that does this once:
- locate the exe next to `nativePC`;
- check the build (PE timestamp; refuse others, with a clear message);
- map a VA to a file offset;
- typed reads: u8/u16/u32/f32 slices, C strings, records;
- the class-name-from-registration helper.

Then port `ddskill` and `ddpawn` to it.

`ddquest` belongs to claude-7 (quest-runtime) right now. Leave it, and
post a board message to claude-7 with the migration so it can switch when
convenient.

## Scope

- `crates/ddexe/**` (new)
- `crates/ddskill/**`
- `crates/ddpawn/**`
- `docs/METHODOLOGY.md`: a short "reading tables from the installed exe"
  section

## Done when

- `ddexe` has synthetic tests (a hand-built PE) and install tests that
  skip without the game.
- `ddskill` and `ddpawn` use it, and their tests still pass.
- Fill in Results and report.

## Results

(filled in by the agent)
