# ddexe: one reader for tables in the installed DDDA.exe

- **Owner:** claude lane
- **Branch:** `task/ddexe`
- **Board area:** `tools/exe`
- **Status:** review

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

claude-9, branch `task/ddexe`.

**Built:** `crates/ddexe` (no dependencies):
- `locate(native_pc)` (case-insensitive `DDDA.exe` in nativePC's parent),
  `ExeFile::open` / `next_to` / `image`.
- `Exe::parse` refuses any PE timestamp but 1513180209; the message gives
  both timestamps and dates (e.g. "PE timestamp 1600000000 (2020-09-13);
  ddda-rs reads only DDDA.exe 2017-12-13 Steam ..."). `parse_any` for
  inspection only.
- `bytes(va, len)`: the range must lie in the file-backed part of one
  section (the zero-filled tail of `.data` is refused); typed reads
  `u8/u16/i16/u32/i32/f32`, `u8s/u16s/u32s/f32s`, `cstr` (printable ASCII),
  `record` / `records` with typed field access, `section_of`, `sections`.
- `class_names()`: `MtDTI` object address -> class name from the
  registration calls (`push name; mov ecx, dti; call 0x00cf5780`).

**Ported:** `ddskill` (its `exe` module removed; `ExeTables::from_image`
added) and `ddpawn` (`rdata` and `EXE_TIMESTAMP` removed; `SkillMasks` and
`ItemKeyTable` read records through `ddexe`, with `from_image` added; the
builder test finds the exe with `ddexe::locate`). Public `from_exe(&[u8])`
signatures are unchanged.

**Tested:** `cargo test -p ddexe`: 4 synthetic (hand-built PE: build check
and message, typed reads and range refusals, class names, locate) + 2
install (sections, a known string, more than 3000 class names including
`cPlActWpnDaggerCstmHyakuretsu` and `rSkillList`); install tests skip
without the game. `ddskill` (14 unit + 9 install) and `ddpawn` (8 + 6)
still pass. Clippy clean for `ddexe`; `ddpawn`'s existing warnings in
`rift.rs` and `tests/install.rs` are untouched.

**Docs:** METHODOLOGY.md "Reading tables from the installed exe";
VOCATIONS.md points to `ddexe`.

**Not done:** `ddquest::events` (claude-7's) still has its own reader; the
migration was posted to claude-7 on the board.
