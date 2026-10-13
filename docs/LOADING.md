# Loading and boot timing

Evidence-driven attribution for the native runtime. Service durations are
inclusive and overlap; sampled busy ownership is not exclusive CPU time.

## Boot classification

Evidence: current lifted original functions, read-only unpacked exe and
head's `run_c26p1.log` / `run_c26p4.log`. Not a new original-game comparison.
Both `005325E0` and `00531A20` are called `main_menu` by the older logger;
neither that label nor movie reader lifetime establishes visible readiness.

| Interval (c26p1) | Classification | Evidence and limit |
|---|---|---|
| First Present -> aTitle request, 7.281374 s | Original logo holds + GUI transitions + work | `004FC370` creates logo manager; `0072D5C0` selects list; `0072D670` loads holds; `0067D4C0` decrements by unit delta. Selector 1 nominally holds ESRB 120 + Capcom 60 base frames (6 s at 30 Hz), with existing input skip for Capcom. Active selector not in old logs. Includes 0.653328 s inclusive zlib and 0.545861 s pipelines, overlapping other work. |
| Movie reader start -> `005325E0`, 6.560784 s | Authored title GUI sequences + input + frame work | `0072A650`: state 0 starts sequence 5; state 1 waits `+19C`, starts 3; state 2 polls `0040A230`; state 3 waits sequence 4 completion; state 4 publishes result 1. `00531800` releases title/movie and enters storage flow. No hardcoded 6.56 s timer here. Zero loader CPU/zlib/pipelines in this interval. |
| Movie reader end -> `00532660`, 7.467707 s | Storage/dialog state machine, including timer and input gates | `005325E0` polls `0052FC30` at aTitle+58. Contains manager readiness, dialog hold/input, storage request and result gates. 0.216356 s inclusive zlib, no loader-thread CPU or pipelines. Cannot call all 7.47 s idle or all CPU. |
| `00531A20` -> `00532FB0`, 7.566077 s | Top-menu GUI/input/animation residence | `00531A20` creates uGUITopMenu then polls GUI+1C8; choice 0x10 installs `00532FB0` at `00531B2F`. `0072AED0` waits sequences, consumes menu input via `0072B3B0`, invokes selection animation/dispatch through `0072B860` / `0072BA40`. No seven-second countdown identified. Zero loader CPU, zlib or pipelines; main/jobs still do 5.797/15.938 CPU s drawing/updating the menu. |

The CPU totals measure work while a state resides; they do not prove that
state exits only after that much work. Inclusive service durations cannot
be subtracted from CPU or summed into wall duration.

### Logo selection and autoplay

`0072D5C0` reads region setting `018750F0`; value 4 resolves through
`0041E230`. Region 0 or 3 selects list 0; 1 selects list 1; others select
list 2. Lists at `017EEEC8`, timer records at `01515F48`: nominal holds
10/6/2 s respectively (L109 details). This is a source map, not proof of
which list c26 ran. Original input-mode and animation gates remain.

`crates/ddplay/src/input.rs`: autoplay sleeps 1500 ms, sends Enter down,
sleeps 80 ms, sends up, sleeps 80 ms; approximate cadence **1.66 s** after
the first Present. It stops after two >=1000 draws/frame probes. Thus a
ready prompt can wait almost one cadence for its next press, plus host
scheduling, queue consumption and guest-frame latency; several prompts
can each wait. There is no explicit seven-second sleep. AUTOKEYS is a
separate configured schedule; do not assume it was enabled in these runs.

### Storage dialog timer versus input

`00533320` initializes the pre-menu storage state to 2. `0052FC30` state 2
starts `00469510`, state 3 waits manager `+5A2E`, state 5 opens dialog 3.
`00530D30` selects 24-byte records at `017F43D0` (mode at record+4).
Dialog 3's mode is **7**; dialog 10's mode is **9** in the original exe.
The modes share initial timer **120.0**, loaded from `0153E46C` at
`00530E19`. `00530F90` mode 7 subtracts `sMain+70` and requests close at
nonpositive timer; mode 9 only tests that timer, and mode 8 waits.

At `0052FCC8`, `00531060` checks four controller masks and keyboard input
(`0040D200`); accepted input writes **-1.0** from `0161C20C` to timer+24.
It is an input predicate, not a storage-ready predicate. Therefore even
the four-second mode-7 countdown can end early through the original path.
After completion this branch opens dialog 10 and enters storage request
state 6/substate 4. Request completion later also ends mode-9 dialogs;
mode 9 itself is not a countdown. Capturing the active substates is needed
to partition the measured interval, rather than assuming a fixed hold.

### Authored GUI timing

`0072A570` loads `id/DDN/menu/title_1/top_01` from the player's
`rom/gui/GUIuGUITitle.arc`. `00E0A4E0` finds a sequence by ID in the rGUI
B8 table; `00E0A620` installs it at GUI+13C and resets sequence time.
The installed title resource's sequence 5 ends at 100 and sequence 4 at
59. Resource flags at +88 become the GUI rate flag through `01076170`;
`0114FDF0` selects 30.0 (`015F9C64`) versus 60.0 (`01408A88`). The title
resource selects 30. `00E0CE50` advances sequence time using GUI+128,
unit+14 and sMain+38; `00E0A910` exit mode 0 compares against the endpoint.
Thus entry plus exit are nominally **159/30 = 5.3 s** of animation before
input/frame residence, accounting for most of the 6.56 s interval. The
loop uses a different exit mode; do not treat it as a zero-duration hold.
This is source/data attribution, not a new measured per-animation trace.

The installed top-menu `title_2/menu_01` also selects 30 Hz. The default
root selected by `0072AC70` is 6; `0072AED0` starts entry sequence 31
(29 frames). `0072B860` routes choice 6 to sequence 36 (40 frames), then
`0072BA40` creates the submenu with `0072CA60`, root+298=6/regime+294=1;
`0072B320` selects sequence 37 (12 frames). With a save available, submenu
choice 1 is selected, exits via sequence 43 (89 frames), then creates a
uGUILoadGame child; `0072BCA0` polls its result before publishing 0x10.
Multiple animation and input gates, plus child completion, explain why
this is not one seven-second timer. Exact wall-time partition is pending
boot-detail capture; do not simply add resource lengths across branches.
No extracted records or assets are committed or embedded in runtime.

## Combo30 loading profile (L163)

Head capture `stacks_c30load.json`: PID 36768, 225 all-thread rounds over
14.0 s, roughly 16 achieved rounds/s. The capture starts at game-main
handover and spans loading plus adjacent work. Raw stacks have no sample
timestamps, so this is **not** a slice exactly matching the 7.4 s stage
interval. Known OS wait leaves are excluded; unresolved leaves remain.

| Thread | CPU seconds | Non-wait / all samples |
|---|---:|---:|
| Loader IO 0x1024 | 0.593750 | 14 / 225 |
| Decoder 0x1028 | 5.593750 | 71 / 225 |
| Decoder 0x102C | 5.765625 | 66 / 225 |
| Decoder 0x1030 | 5.609375 | 70 / 225 |
| d3d9-backend | 5.125000 | 93 / 225 |

The three decoder threads total **16.968750 CPU s** and **207 non-wait
samples / 675**, with 468 recognized waits, no empty stacks and seven
unresolved host leaves retained. All 207 have a recognized lifted frame.
Nearest-lift ownership includes its non-lifted callees:

| Nearest lifted function | Samples | Share of non-wait decoder samples | Meaning |
|---|---:|---:|---|
| 00CF02E0 | 76 | 36.7% | zlib, largely existing native helpers; 17 table-builder leaves, 12 decoder loop, 11 code lookup, 9 input-pair staging, 7 match-copy |
| 00CFEEC0 | 59 | 28.5% | Buffered stream skip: 18 lifted-body leaves, 16 INC/DEC, 9 stores, 8 carry computation, 6 loads, 2 zero-flag computation |
| 00DB9F40 | 52 | 25.1% | Decoder service/critical-section loop; not 52 samples of codec arithmetic |
| 01105240 | 6 | 2.9% | Remaining nearest lifted owner; not selected for this batch |
| Other lifted owners | 14 | 6.8% | Small individual buckets |

All 59 skip stacks have lifted chain `00DB9F40 -> 00E6B1C0 -> 00CFEEC0`.
An outlined parent host frame is unresolved; recognized lifted ancestors
still establish ownership. `00E6B1C0` calls skip with return PCs `00E6B5D5`, `00E6B71C`
and `00E6B7F3`, after obtaining the current stream position. Its buffered
stream object is on the loader's stack. Do not multiply these sample
shares into purported measured CPU seconds or promise matching wall gains.

Backend leaves are dominated by shader work: Naga Typifier::grow 20/93,
DiagnosticFilterNode arena handle 7/93, WGSL lexer 3/93, plus other parser
and allocation work. That is a separate renderer lane, reported to head.
Head's FAST=1 vs FAST=2+TABLE=1 runs measured save-load 5.81 vs 6.26 s and
large-inflate inclusive time 5.89 vs 6.85 s. The latter remains opt-in.

### Exact buffered skip port

`DDRUNTIME_ARCHIVE_SKIP=1` installs `archive_inflate::stream::skip` at
`00CFEEC0`, unless an explicit existing port overrides it. Default remains
original. At cursor < end, batch `min(request_remaining, end-cursor)`;
preserve the final EAX, CMP/INC/DEC flags, stack spills and cleanup. At
buffer end invoke the original `00CFE9D0` via existing dispatch, with the
original return PC `00CFEEDF` and complete CPU state. Failed refill still
consumes one requested skip iteration and is retried as in the original;
success increments the returned cursor, including wrapping behavior.
No stream allocation, file IO, refill, decoder concurrency or zlib policy
changes. The maximal valid u32 span completes in one batch.

Regression evidence: 192 scalar cases compare complete CPU debug state,
written bytes and every refill-boundary state. A separate 192-case oracle
executes the original skip instructions in Unicorn with explicitly
synthetic refill responses (not a test of the original refill itself).
It compares GPRs, defined arithmetic flags, object state, stack bytes and
all refill call states. No game process or lifted-shard build required.

### Cheap CPU timeline snapshots

Combo30 showed CreateToolhelp32Snapshot/Thread32Next under the main
thread's logger. L163 enumerates host threads **once at first Present**,
then only queries cached GetThreadTimes handles. Guest threads still
register before guest code. Handles retain final CPU after exit; keys use
ID plus creation time so numeric ID reuse cannot overwrite old totals.
Process CPU minus known classes supplies `cpu_other_ns`, including later
unregistered host workers. Such workers are not retroactively labeled
backend; thread descriptions are read only in the initial discovery.
First discovery can move earlier CPU from other to backend; do not sum
class deltas across that initial reclassification as an exact process total.
CPU samples remain sequential/approximate and cannot be summed with
inclusive service times. A real exited-worker regression verifies retained
CPU without discovery. The one discovery can still charge first Present;
no reduction in actual game loading is claimed until head's rerun.

