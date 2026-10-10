# S31: ev00 -> dragon fight -> ev01 -> ev05 in the opening

- **Branch:** `step/s31-dragon-route` (after S30)
- **Files:** `crates/ddhost` (opening route, replacing the ScriptedHost
  shortcuts of this scene), `crates/ddview/src/opening.rs`, tests
- **Kind:** one feature

## Context (S24 map, "Fight end, event prerequisites")

After ev00, q0000 raises SceNo 1025 (start position 3), runs camera 17
with GSF520, reaches state 16. State 16's condition 20 starts ev01 on
**GSF379 OR EnemyCheck (the dragon's killed-state flag) OR NOT
AreaStayFlag[1]**; otherwise after 17 s it goes to state 5, which sets
GSF378 and exits after 43 s more. `aStage200::vf12` 0x0051CCC0: while
GSF378 is on, it records the dragon's first-life-region HP and requests
GSF379 when HP < baseline - 4 (strictly). Seven NPC victims (group 3,
LOT 1) take part; no soldiers.

## Do

Run the real sequence in the opening: ev00, then the playable fight with
the S29/S30 dragon and the seven NPC victims, the state 16/5 exits as
authored, then ev01 and ev05, then Cassardis as today. Remove the scripted
stand-ins for this part only.

## Done when

An `--opening` run plays ev00 -> fight -> ev01 -> ev05 by the authored
exits (hit the dragon for GSF379, or wait for the timeouts), a test covers
each exit, and the route continues to q0001 as before.

## Results

**2026-10-10, codex-63: source checkpoint; S31 remains open.**

- `OpeningWorld::enable_dragon_scene` disables the q0 camera/quest-bit
  injections and requests real event playback for rows19/20/21. It binds
  an explicit installed enemy target; production scene entry is pending.
- Story-phase combat observation supplies the actual opening dragon's
  first life pool and every combat Arisen except party pawn seats. The
  original `0x0051CCC0` reads the first pointer at actor+0x1D18, rather
  than searching for region ID0. `0x00770910` appends selected RST elements
  in resource order; the opening ctor selects kind0 at `0x00A49F2D`.
  Missing bodies/pools remain absent; unload and HP0 do not set a
  persistent placement-killed bit. Aggregating Arisen is explicit co-op
  policy; the native stage rule reads seat0.
- Installed q0 regressions exercise GSF379, leaving AreaStayFlag1 and the
  persistent-killed `EnemyCheck(flag1)` exits in both states16/5. The
  timers are strictly >17 then >43, and state5 starts its own counter.
  State5's setting bit0 checks transitions after callbacks and enters the
  destination on the next update. Event requests do not complete ev01.
- Installed LOT1 admission exposes seven group3 records704..710; all
  authored graphs are Demo actors with no Attack process. Admission is
  tested separately from materialized bodies and controller playback.
- Head5475/5491 approved an independent NPC Demo adapter and a narrow
  `ddai::Core::new_with_cluster_work` API. Native ClusterWork is a list
  of once-only from-all node IDs, not copied owner variables:
  `0x00E087E0 -> 0x010C9310(8)`, `0x00E059B0/0x00E05A00`,
  `0x00E092A0`. Its key is `mOwnerNodeUniqueId` (native cluster+8;
  reflector `0x010CB1F0`), not the repeated `mId`. The opt-in preserves
  these lists across subcluster reentry. Default hierarchical rejection
  remains for callers that have not opted in.

**Tested:** prescribed shared core target: `ddhost --test dragon_route`
6passed (compile15.51s, tests13.85s); `ddai --lib` 18passed (cached0.36s,
tests0.00s). The new core test also passed separately (compile2.51s).
Actual command exit0; no skipped install tests on this machine. The
combat snapshot regression sets HP scalars explicitly to check boundary
and observation behavior; it is not a normal-input hit or route replay.
No game files, resource dumps or decompiler output are committed.

**Demo command checkpoint:** head5475/5579 approved the independent
`ddnpcrt::demo` controller and the parser-only `Motion::from_demo_record`
hook. Installed beach graphs now execute body readiness, authored quest
flags, actor flag masks and typed Wait/Motion/GotoEx/Damage requests.
The controller keeps the native ClusterWork lifetime and stops on
Finish/Kill rather than recreating its Core. Source callbacks are
`0x00533DC0/0x00533A90`, `0x0056CFF0/0x0056F160`,
`0x0056DCC0/0x0056E230` and `0x0056EBB0/0x0056EDA0`.
Command tokens reject stale completions; Wait remains running. GotoEx
retains the installed trace record. Damage requests a native reaction,
not an HP subtraction. Actual actor movement, reactions, interface
results and the body-manager retry byte remain unintegrated.

Installed readiness writes are q0 bits10..14, including NPC708's bit14
(the earlier S24 prose omitted it). Values are read from each graph,
without a production NPC-to-bit table. Three installed Demo tests passed
(compile0.82s, tests3.00s), all eleven NPC unit tests passed
(compile1.25s, tests0.00s), and the four existing installed applied-action
tests passed (compile0.77s, tests3.25s), all actual exit0 on the prescribed
shared target. Interface completion in the command-token test is an
explicit scalar fixture; no native motion playback or full route replay
is claimed.

**Host composition checkpoint:** applied graphs with authored
`SetInterfaceDemo` now use the independent Demo owner. Class-only
selection initially broke Reynard because its graph also has a Demo
owner but uses the inherited NPC interfaces; an installed regression
now preserves that existing adapter, including Set Fix Pos and motion3226.
There is no fallback after a runtime error. Readiness comes from the
current lifetime's actual bound body and resolved motion banks, rather
than intel presence. Quest/GSF/LOT writes reach the real story World in
stable owner order; intra-step read-after-write remains a snapshot
limitation. Flags update the canonical intel/body storage. Generic
motions use the existing launch/lifetime checks and real actor clock.
Replacement destroys the previous layer. GotoEx and Damage remain
typed pending requests without a fake Wait, HP change or Done;
Damage(flag) remains false until a real reaction action is ported.

Final shared-target regression command exited0: compile8.77s; installed
NPC combat2passed (9.64s), Demo2passed (5.82s), motion6passed (6.47s),
placements9passed (2.45s). The Demo body test separately passed before
the compatibility fix (compile13.33s, tests6.72s). It withdraws NPC708's
body and proves q0 bit14 stays false, then validates real lifetime
retirement and rejection of stale body bindings. No full-route or
original comparison was performed. The core lease was released after
the command's actual exit.

**Base motion-bank checkpoint:** native `uNpc::vf192` at `0x00BAE100`
calls inherited `0x00B5B260`; disassembly at `0x00B5B272/0x00B5B288`
passes actor+0x4C24 to `0x008282E0`. The constructor `0x00B574D0`
sets this pointer to `0x014F1D30`. Its sentinel-terminated packed-bank/
path pairs register `rMotionList` through `0x01057670`: base banks0/1/2
are co/dm/gp, before the existing NPC banks11/12/13. The unrelated
`0x014EAC20` collision survey tuple must not supply animation bank keys.
The host loader now reads the actual table at runtime; codex-61 is
coordinating the same correction in its owned `NpcBody` reader. Existing
CC6/CC7/C9A motions use NPC bank12 and remain separate.

The Demo Motion No getter uses the actual current packed layer word,
with the sourced after-change mask (`0x007AB940`). A live body without a
layer observation raises an explicit error. NoEscape uses the inherited
`0x007AA5E0` mask. Installed bank1 slots109/115 match the dm resource.
An explicit installed NPC704 state9 checkpoint runs the unchanged
authored motion109 through the real host clock to terminal, without
setting NpcAppDie. This fixture bypasses navigation/reactions and is not
an ordinary route replay. NPC709 separately runs its real authored loop;
explicit q0:1/GSF465 fixtures exercise flag changes without HP/death.

Complete `ddnpcrt` suite35passed (compile8.07s; suite durations0.06,
6.66,4.18,2.87,3.59,2.84s). Focused host12passed (compile9.32s;
combat2/11.09s, Demo4/8.45s, motion6/8.80s). Actual command exit0.
An initial missing-new-field compile error proved stale shared ddnpcrt
metadata; a narrow `cargo clean -p ddnpcrt` removed205.8MiB and resolved
it. No behavior was changed for that error; no new target/full clean.
The core lease was released for the parent's observer review.

**Open:** production scene entry and event player continuation to q1;
native Demo root movement/GotoEx/SetDamage actor integration;
actual S30 damage/FSM integration; persistent dragon-death lifecycle;
normal-input route capture and original comparison. Existing simple/
unsupported camera adapters remain explicitly labelled stand-ins.

**Next:** port native reaction/ground and root-motion/Trace actors, then
enter ev00 after editor confirmation instead of fast-forwarding through
the fight. Damage3 is `0x0080E4E0/0x0080E4B0`; Damage4 is
`0x0080E520/0x0080E560` and requires actual ground contact between
motions102/10A/10B/103. DemoGotoEx uses `0x0056DCC0 -> 0x0057B9A0`.
A timed replacement for these action results would not match the source.
**Guard integration regression (head5837):** commit `dae5292` selected the
new Demo adapter from the process container alone. Installed
`ai\fsm\npc\st200\quest\q0002_n001_ai` uses `SetInterfaceDemo` for
`cAIActionInterfaceDemoSetPos`, which the existing NPC adapter already
handles before inherited NPC Guard dialogue. The new adapter rejected that
placement interface and quarantined the graph; body readiness was not the
cause. Selection now reads the authored interface payload, preserving
DemoSetPos and inherited NPC callbacks on the existing adapter while beach
Demo commands still use the dedicated owner. Native dispatch
`0x00533A90 -> 0x0056C940` uses the interface object's type. Unsupported
interfaces remain explicit errors; there is no runtime fallback.

The fix is isolated on `step/s31-demo-regression` from committed main
`861c94e`. Full `cargo test -p ddhost` on the prescribed shared core target
actually exited0: 9 unit and 84 integration tests passed across all29
integration binaries (compile20.16s, no failures). Both reported failures
now pass: Guard dialogue/quest boot and applied NPC observation/restoration.
Existing beach body-readiness, Reynard, unsupported-error, NPC combat,
motion, opening and co-op regressions also passed. This is installed-data
host acceptance, not an original-game comparison or full playable S31 route.

**Combined bank acceptance:** after integrating reviewed main `797922f`
and its Guard fix into the bank checkpoint, the complete `ddnpcrt` suite
passed35 tests (compile2.84s), and complete `ddhost` passed95 tests
(9 units plus86 integration, all29 integration binaries, compile29.18s).
Both commands actually exited0 on the prescribed shared core target;
the host run included the new native-bank motion109 and NPC709 regressions
alongside Guard, NPC death, co-op, camera and opening suites. Verified stale
metadata initially hid `ddnpcrt::demo` and `ddpawnai::commands` despite their
source exports; narrow cleans of those crates removed469.6MiB/56.6MiB and
resolved the compile errors without source changes. The core lease was
released after actual exit. Reaction helper work is isolated on another
branch and was not part of this acceptance run; the playable route remains
open.
**Independent reaction phase helper (head5831/5842):**
`ddnpcrt::demo_reaction` translates extra/blow action phases from
`0x0080E4E0/0x0080E4B0` and `0x0080E520/0x0080E560`. It preserves native
motion arguments (115 for extra; 102/10A/10B/103 for blow), flags&5
termination, ground byte exactly1 and the current body's Down observation.
The action+0x50 completion byte and blow+0x7C down endpoint remain distinct;
interface completion polls at `0x0056EBB0`. `0x0056EDA0` copies the
authored payload, rather than polling completion. Unknown ground/layers
cannot advance to landing/completion, and missing adjustment/Down inputs
remain explicit errors. No HP write, death flag or timer completion exists.

The helper is not connected to the host. Common damage entry
`0x0080E190`, real motion/body updates and horizontal velocity
`0x00B73490` remain explicit requests for a future body adapter. In
particular `0x0080D500 -> 0x00B86010` toggles selected collision-record
byte+0x90 bit0x40, not the landing byte. Actual NPC ground comes through
`0x00BAFB00 -> 0x00B61A20 -> 0x0084BA60 -> 0x00885E30`; the last function
writes body+0x1ED0 from real scenery flags+0x1B68 under update gate
+0x2398 bit4, with axis-present/mode and ground-check masks. The capsule
mode3 height is set by `0x0080B450` from max(HumanEdit+0xF74,+0xF78),
then `0x0076C540 -> 0x00CCE580 -> 0x0079DAB0/0x0079E200` builds/sweeps it.
Those actual HumanEdit/body/physics providers remain unintegrated.

On `step/s31-demo-reaction`, complete `cargo test -p ddnpcrt` actually
exited0: 38passed, 0failed, compile18.87s. Three new scalar-observation
regressions passed in0.02s, covering missing/terminal layers, strict ground
byte, missing adjustment/Down observations and distinct down/recovery
endpoints; all existing installed NPC suites also passed. These fixtures
do not execute a native body, ground query or normal-input route. The core
lease was released after actual exit. Next: supply source-owned NPC morph
capsule/scenery/body requests before enabling the helper in the host.

**2026-10-10, codex-80 continuation; helper checkpoint:**
Head8216 approved integrating the independent reaction helper `f128add`
with reviewed character-creation main `5192882` on this branch; merge
`f5842a2` retains both previous acceptance sections. No new test count is
claimed for the combined source yet.

The nested host module `npc_demo_physics.rs` translates the scalar
prerequisites of the reaction adapter, without activating that adapter:

- `0x00885E30` preserves the previous ground/wall/ceiling bytes unless
  body update flags+0x2398 contain bit4. Actual scenery flags+0x1B68 use
  mask3, or mask0x100003 under the extended check. An existing axis and
  flag0x400 veto ground; an absent axis does not. Unknown scenery is not
  a synthetic zero observation.
- `0x00B73490` projects the original horizontal speed through the body's
  resolved sine/cosine and runtime base vector, preserving vertical
  velocity. Only afterwards does it add body delta times acceleration to
  speed, then clear speed/acceleration when old times new is nonpositive.
  Input validation is project policy; original CRT trig bit parity is
  still unverified.
- `0x0080E190` restores only flags0..5 and9..11 from the saved body flags,
  sets damage mask0x10 and clears masks0x4/0x8. Common damage
  entry still requires its other body and collision-record effects.

Three new regressions cover disabled/axis/extended ground observations,
projection-before-deceleration and a sign crossing, and preservation of
unrelated flags. On frozen `d3afc70383bccde9065b6a103f0cc716cae3edf5`, the
**complete** `cargo test -p ddnpcrt -p ddhost --jobs 2` run exited0:
178passed, 0failed, 0ignored across45 summaries (including both doc suites),
in336.36s wall. All three new host tests and all three merged reaction
tests occurred exactly once and passed; no installed tests were skipped.
The log SHA256 is
`80D52E41678C9B747A44ABC47A70ECFDB54BF68DD5BD4654E7A5F63BD7CB9619`.
Source hashes and the clean HEAD were unchanged through the full command;
normal core lease was released after its actual exit. The first Windows
PowerShell5 runner aborted on Cargo's ordinary stderr under its error
policy before any tests; that rejected attempt is preserved privately.
The successful fresh run used PowerShell7. The six new unused-helper
warnings reflect that production actor composition is not enabled yet.

Further source evidence: `0x00B5B950` copies the shared player-manager
resource pointer at+0x970 to every body+0x3348, including NPCs. The manager
loader at `0x0047E6F8..0x0047E71A` walks the installed path table
`0x0163E660`; its first pointer resolves `param\pl\other\m_pl000_00`.
The existing installed adjustment reader therefore supplies slots18/19/1A.
`0x0076C540` mode3 consumes collision height+0xD88 and radius+0xD8C,
then delegates to `0x00CCE580`; heights shorter than twice the radius
collapse the segment to a sphere. The NPC collision mode and axis binding
still require their actual source fields (see the correction below). No default human height,
fake ground byte, elapsed landing, navigation completion or HP loss was
added. Production reaction/GotoEx actors and the full S31 route remain open.

**Capsule source correction (head8286):** the earlier handoff and initial
provider proposal conflated two inline collision fields. The collision
object starts at body+0x10E0. HumanEdit max(F74,F78) is written at
body+0x1E64 = collision+0xD84 by `0x0080B450`; it is **not** the mode3
height. `0x00AC50B2..0x00AC50BA` and `0x00B5F0A0` write installed adjustment
D3 into body+0x1E68 = collision+0xD88. Disassembly proves the mode3 branch
reads radiusD8C at `0x0076C72D`, endpoint heightD88 at `0x0076C753`, calls
`0x00CCE580` at `0x0076C785`, and jumps directly to `0x0076D3A9`.
The HumanEdit factorD84 reads at `0x0076D1E3/0x0076D270` belong to mode1
and are bypassed by mode3. `0x01629540` is the native default up vector.
The optional capsule API remains approved, using actual D3 height and
the sourced ordinary radius, with no HumanEdit multiplication or default
height. No capsule API or production actor was activated in this batch.

**Optional mode3 capsule provider continuation (source only, unrun):**
`NpcBody::mode3_scenery_capsule` now accepts the caller's actual installed
shared adjustment table. It returns `None` for an unsupported/missing
HumanEdit controller, preserving other NPC load behavior, and resolves
the height from D3. Further disassembly resolves the active radius too:
`0x00AC50D0..0x00AC50DA` writes D4 into body+0x1E6C = collision+0xD8C,
also written by `0x00B5F0A0`; `0x0076C72D` passes this word to `0x00CCE580`.
The constructor's35 radius is therefore not the authoritative value after
common action entry. The new provider uses D3/D4 and does not change the
existing gauge's constant or unrelated NPC load policy.

`ordinary_mode3_capsule` translates the original positive-height
normalization, endpoint shortening and short-span collapse, preserving
the actual radius and the default up direction below the source epsilon.
Invalid/nonpositive radius, nonfinite values and overflowing squared
height/radius are rejected as explicit project validation policy.
No default height/radius, HumanEdit multiplication, actor sweep, damage,
ground contact or interface completion was introduced.

Two new regression names are **uncompiled/unrun**:
`ordinary_mode3_capsule_shortens_real_height_and_collapses_short_spans`
and `installed_beach_mode3_capsule_uses_shared_adjustment_height_not_human_factor`.
The installed case resolves the manager's first adjustment resource from
the player's exe, checks NPC704's distinct HumanEdit factor, and verifies
that D4 controls the radius and missing/invalid observations do not obtain
a fallback. A bounded original-instruction `0x00CCE580` proof script is
prepared privately but has not been run. The previous178-pass acceptance
belongs only to source `d3afc703`; it does not validate this continuation.

**Native capsule instruction check:** source `5af3ab0` was unchanged and
clean during a parent-approved bounded check of original `0x00CCE580`.
All four synthetic height cases0/20/70/200, with radius35, matched both
endpoint rows and the radius, reached the normal native RET (ESP+4), and
preserved the output canary. No instructions, outputs or callbacks were
substituted. The four cases were bounded to1000 original instructions
each and less than5s total; the command actually exited0. The private
JSON SHA256 is `0C276B617BA26C2723A9F2532372348642C806D55BF3DD729A0617A3C478BF09`.
Two earlier attempts lacked the original file-backed negative-one constant
page and were rejected; adding that original page resolved the long branch.
This is a narrow native-function comparison with synthetic input, not
whole-body ground or route validation. The new Rust tests remain unrun.

**Damage flag writer found (source only):** `uCharacterBase::vf111` at
`0x00850100..0x00850181` clears bytes2EA1/2EA2/2EA3 each update. Its active
predicate is body flags273C mask0x10, or NPC category206D mask8 with linked
vf180 flags mask0x80, or body collision flags2360 mask0x4000. A transition
from previous active byte2EA0=0 writes rising-edge byte2EA1=1 at
`0x00850171`; falling from1 writes byte2EA2=1. Demo's getter `0x007AB800`
reads the one-update rising edge. `0x0084BA60` invokes vf111 before the
ObjModel ground update. A new scalar `DamageEdges` helper preserves this
distinction; unknown linked NPC flags reject its update without mutation.
The regression
`demo_damage_flag_is_a_native_rising_edge_with_distinct_body_npc_and_collision_sources`
is **uncompiled/unrun**. No graph input or production actor was changed;
real body/collision flags still must be supplied by the actor adapter.

**Axis field correction before actor integration:** the earlier phrase
"NPC axis kind3" conflated body+0x1DEC (`0x00B5F0A0` word index77B) with
the reflected `cScrAdjust::mAxisCheckType`. The latter is adjust+0x1A0 =
body+0x1B60 for inline adjust+0x19C0. Native constructor `0x0079D8E9`
writes axis type2; the player branch in `0x00B5F0A0` also writes2 at word
index6D8, while the ordinary NPC branch's3 is a different field.
`0x00770A90` allocates a collision record and does not overwrite this axis
word. No type3 axis query was implemented. The actor adapter must preserve
the actual axis field and use the existing supported sphere query rather
than choosing a query from the unrelated body+0x1DEC value. The tested
ground helper accepts actual `axis_present`; the new capsule provider
contains no axis policy, so neither implementation needs this correction.
