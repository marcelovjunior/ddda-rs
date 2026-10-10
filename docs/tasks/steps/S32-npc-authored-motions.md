# S32: NPC authored motions (Reynard's rescue script)

- **Branch:** `step/s32-npc-motions` (after S23's continuation)
- **Files:** `crates/ddnpcrt` (motion request payload), `crates/ddhost`
  (headless NPC actor motion state), `crates/ddview/src/opening.rs` (body
  presentation), tests
- **Kind:** one feature (proposed by codex-54, approved by head)

## Goal

Play the installed motions of `q0002_n304` (Reynard): 3226, 3073, 3074,
3089, keeping their loop/target/fall flags and source completion, so his
graph continues past its motion waits. Sources: NPC wrapper 0x00576E10 /
0x00576EF0, generic `cEventActMotion` 0x0057DB10. A loop stays pending; a
non-loop finishes only on the sourced motion-terminal observation, with
lifetime and command validation (a finished old motion can't satisfy a new
command).

## Done when

The real q2 NPC graph continues past its motion waits on the route, tests
cover loop/non-loop/stale completion, and the Result says where Reynard's
script stops next.

## Result (checkpoint; ordinary rescue replay pending)

Code `b4277ae`, then main integration `f7ab591`. The NPC request now retains
all reflected generic motion fields and NPC wrapper fields from
`cAIActionInterfaceNpcSetMotion`: motion, blend/start/speed, loop, root
fix/off/symmetry, target type/position/group/id, arrival and rotation/look
parameters, stop/fall/force, talk, intel wait and object priority. The
`SetActionEx` payload remains a separate unported case. Parameter copying
is sourced to `0x00576EF0` and `0x0057E290`.

The isolated headless `ddhost::npc_motion` actor runs the installed motion
layer. Sources are `uModel::vf30` (`0x0105B480`, frame max and resource
loop override), the positive-speed frame/terminal update `0x01060730`,
terminal getter `0x010570F0`, and `cEventActMotion::vf06` (`0x0080DB80`).
Authored loops restart their layer and stay pending; non-loops produce the
actual action-terminal observation. Completion also requires the same
intel lifetime, interface command and unique host launch, the current
event motion action, and the requested low motion byte (`0x0057E0E4`).
These identities are port protocol tokens, not original game constants.
State changes, graph replacement, body recreation and unloading reject
old completions. Talk temporarily owns the actor instead of the motion.

NPC banks are read from the player's install, using the installed exe's
pointer table `0x0163C980` and the loader `0x00BB6060`. The common NPC bank
is 11; the two type-specific banks use `c`/`j` resource suffixes. Missing
bank-12 slots fall back to bank 11 (`0x0080D750`); missing resources never
mean completion. The opening's generic assembled NPC bodies use constructor
type zero (`cNpcInfo`, `0x00784880`) and the regular `00` common set.
Specialized NPC type overrides and the zone-dependent `99` common set are
not connected by this opening adapter.

Reynard's 3089 is not a simple non-loop: the installed graph specifies
speed 1.3, blend 30, Target Flag true, Target Type 1, arrival length zero
and rotation speed 180. Type 1 resolves the actual host party slot zero
(original `sPlayerManager+0x99C`), independent of the viewing co-op seat.
The terminal angle/radius latches are translated from
`0x0057E015..0x0057E0DE`; SSE disassembly was needed because the decompiler
lost the helper return registers. `0x007AD9D0` supplies target yaw and
`0x007AD870` its shortest signed difference. Missing targets stay pending.

The host only starts a layer after an explicit current body/bank binding.
The thin viewer binds successfully spawned bodies and samples the host's
motion/frame with the authored blend length. It does not report visual
animation completion. The original look-time steering, root displacement,
ground-mode/fall physics and specialized actor flags' consumers remain
open; their parameters are retained, not claimed as simulated. The target
angle uses the current canonical placement yaw; moving NPC yaw integration
belongs with the headless mover. Reverse playback is explicitly unsupported.
NPC motion replication to a remote client is also not implemented here.

Validation before main integration: `cargo test -p ddhost -p ddnpcrt`
actually exited zero with **63 host + 28 NPC** tests, none ignored.
Five installed motion regressions cover all four authored motions,
loop/non-loop/frozen layer terminal behavior, strict arrival radius,
missing targets, stale lifetime/command/launch/action/motion observations,
body recreation and unloading. The installed host graph proceeds through
3226 -> 3089 -> 3073 -> 3074 using loaded motion headers and a real two-seat
party target. Quest bit 20 is an explicitly labeled fixture observation in
that isolated test; no actor or production adapter manufactures it or calls
the generic `Done` shortcut.

After merging main's ordinary rescue driver and non-zero AppExit handling,
the source-compiled viewer suite passed **61 tests, 2 existing ignored**
(11.33 s). Release build exited zero in **49.33 s**. The shared release
target initially reused another worktree's libraries; explicit narrow
`cargo clean --release` for the affected packages removed that mismatch.
Private logs and the pinned executable are under
`E:/ddda-re/notes/steps/s32/` (`core-tests.txt`, `viewer-tests.txt`,
`viewer-build.txt`, `ddview-f7ab591.exe`). No private data was committed.

The remaining acceptance check is the ordinary `DDVIEW_DEMO_FIGHT` rescue
route against actual goblin deaths and authored q2 flags. GPU access is
queued behind head's current route and codex-1's bounded dragon capture.
This checkpoint is implemented/tested against installed data and static
exe logic; it is not yet a route pass or comparison with the original game.
