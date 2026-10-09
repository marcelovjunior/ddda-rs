# Per-viewer Rift list projection

`ddpawn::rift_ui` implements a pure projection of an explicitly supplied,
ordered list. Each viewing player owns a `Viewer` with separate filters,
online state and filtered scrolling offset. It returns exclusion flags,
the full visible count and at most seven rows with their original raw
indices. It neither hires pawns nor changes the shared pool.

## Source-backed behavior

All addresses below refer to DDDA.exe 2017-12-13 Steam. This brief re-read
the decompilation and checked the filter assembly; [RIFT_UI.md](RIFT_UI.md)
provides the GUI bindings and resource survey.

`0x006d4bb0` fills one exclusion byte per raw row and decrements the visible
count. Exact state `0x8000` is special: exclude if any of three filter
bypasses is clear; otherwise retain its failed-load placeholder. It must
not resolve a pawn pointer or skills for that row. State comparisons are
equality tests, not bitmask membership.

`0x006d4c50` ANDs these restrictions when their bypass is clear:

| Restriction | Predicate |
|---|---|
| Vocation | Selected index is job-1 for jobs 1..6, job-4 otherwise |
| Skills | Any of six equipped skills has any selected category bit |
| Gender | Selected index is 0 for gender zero, 1 for every nonzero value |

The six vocation choices therefore accept mappings 1/2/3 to 0/1/2,
4/5/6 to 3/4/5, and 7/8/9 also to 3/4/5. This is the observed predicate,
not an assertion that hybrid-vocation pawns can normally be hired.

Twelve selection bytes at GUI `+0x58e..+0x599` become bits 0..11 in the
same order. Assembly at `0x006d4ca0..0x006d4cf7` shifts right and inserts
bit 11 for each selection; the first choice finishes in bit zero. The
six runtime equipped IDs are at `cCmcInfo+0x270..+0x284`; these are not
the saved twelve per-weapon arrays.

For each ID, `0x006d4d04..0x006d4d3f` rejects -1 or >=430, then checks
the skill record's action field `+4 != -1`. The text index at `+0x1c`
selects a row whose flags at `+0xa` are masked by `0x3fff`; index -1
contributes no flags. These are tables at `0x014f3658` (stride 0x20)
and `0x01520278` (stride 0xc). No selected categories means no match.

The module accepts a lookup returning `SkillCategory { action,
text_flags }`. Callers can adapt `ddskill::catalogue::ExeTables.skills`
and `.texts`, which already read these tables from the installed exe.
No bulk table is copied and no online service is needed for filtering.

`0x006d5090` skips excluded raw entries and fills up to seven row positions.
Exact `0x1000` and `0x8000` states render loading/failure placeholders;
exact `0x2000` becomes a network placeholder when offline. Other states
with data remain pawn rows. Being visible does not establish hireability.

## Deliberate API boundaries

`None` for a filter means bypass, `Some(all false)` means an active empty
selection. The Rust default bypasses all restrictions as an explicit API
default; original UI checkbox initialization has not been reproduced.

`first_visible` is an ordinal in the filtered sequence. Seven-row windows
retain raw ordering, a past-end offset yields an empty window, and filters
do not mutate another viewer's offset. This is an explicit interface for
pagination, not a translation of the original cursor/wrap/animation code
at `0x006d4d80` or `0x006d4f60`.

The original can index outside its arrays for malformed inputs. This API
returns `InvalidVocation` for jobs outside 1..9 when that restriction is
active, rejects other negative skill IDs without looking them up, and
uses the caller's bounded lookup for invalid text indices. A missing pawn
with an active filter returns `MissingPawn` instead of dereferencing null.
With every filter bypassed it is retained as explicit `MissingData`, a safe
API extension rather than a newly inferred original state. Exact failed
load state always follows the separately traced bypass rule above.

Names, costs, icons, skill text, checkbox interaction, ranking sorting,
download progression and permission to summon remain the responsibility
of other layers. In particular, no global "the player" is introduced.

## Verification and comparison status

Six new regression tests cover all nine vocation mappings, all twelve
category bits (including rejecting upper flag bits), six-slot skill OR,
cross-filter AND, empty selections, all eight bypass combinations,
failed-load short circuit, exact state equality, nonzero gender, skill
boundaries/absent action/text, malformed inputs, independent viewers,
filtered seven-row windows and empty/past-end edges.

`cargo test -p ddpawn`: 20 tests passed, including existing private
save/install checks. `cargo clippy -p ddpawn --lib --test rift_ui --no-deps
-- -D warnings -A clippy::manual_clamp` passes. Unrestricted all-targets
Clippy reports pre-existing `manual_clamp`, `identity_op` and
`manual_repeat_n` findings in unchanged Rift/install-test code; these were
not fixed under this brief.

**Original-game list capture comparison is unavailable in this batch.**
The shared running game is at a tutorial/combat scene used by the streaming
investigation, not an open Rift list. Existing pool-level observations in
[PAWNS.md](PAWNS.md) are not evidence for these per-viewer filters. Game
control is authorized, but this brief did not move the shared session or
write process/install/save state. The implementation is source-tested,
not visually compared against the original.

Next: capture an offline Rift list with its raw rows, filter selections and
visible raw indices in the same scene, and compare each selection and a
second seven-row window. Then connect this projection to a per-viewer menu.
