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

**2026-10-10, codex-80: real-body scenery/joint entry helper checkpoint,
tested.** Head8627 and parent8655 authorize the real-body continuation;
accepted main `d02c0cf` was integrated before this batch. `NpcBody` now
retains the installed model bone-ID map. The common damage joint write
uses current lifetime-owned attribute bytes and preserves unrelated bits:
`0x0080D500 -> 0x00B86010` selects model joints through map IDs96/97/99/9B,
then sets or clears mAttr40. These are **model-joint attributes**, not
collision records. `uModel::vf23 0x00FFDDB0` copies rModel+110 to body368
at `0x00FFE00A` and initializes joint+90 at `0x00FFE257`; subsequent
`0x01054460` and `0x00FFD5D0` do not change that byte. Runtime updates
such as `0x00B8D980` remain distinct and must retain their current values.
The API accepts actual held-goods predicate observations; it cannot assume
that a body has no held goods. Missing/truncated maps or wrong joint count
reject before any mutation.

The independent `npc_demo_reaction` module queries the existing
`ddsim::sweep::sweep_capsule` for the actual `NpcFighter`, runtime D3/D4
capsule, target origin and cScrAdjust settings. Command/launch/generation
checks precede queries. Missing geometry at the initial provider check or
any subsequent query preserves the body position. Axis type2 uses the
actual separate sphere radius and lower capsule endpoint;
`0x0079E58A..0x0079E6C3` widens capsule radius to f64, multiplies the
original `0x0161C9F0` double **1.41421356**, narrows to f32, then adds the
airborne oldY-targetY difference. Type0 disables the axis, while other
types remain unsupported. The returned scenery observation is deliberately
separate from vf90 ground-byte projection, preserving frame-order work
still required. Geometry remains the shared documented exact-distance
replacement for native MtCollision GJK/MPR.

Four new regressions passed once each in the complete package run:
`demo_damage_entry_changes_mapped_joint_attributes_and_preserves_other_bits`,
`installed_beach_damage_entry_uses_model_bone_map_and_current_joint_attributes`,
`demo_scenery_axis_uses_current_radius_native_double_and_airborne_target`,
and `installed_demo_scenery_moves_current_body_only_with_available_triangle_provider`.
The last uses an installed NPC body with an explicit triangle/pool fixture;
it is not an original scenery observation or production-route replay.

**Verified:** parent8718 granted the full gate after its actual completion
and normal release. On clean source
`4a1e3f8c27c0f350d64e0ac13d320ce1a900c653`, the PS7 strict runner executed
`cargo test -p ddenemyrt -p ddhost --jobs 2` on the existing shared core
target with canonical install/exe variables. Session43056 actually exited0,
Cargo0: **347passed, 0failed, 2existing ignored, 66summaries** (64nonempty
and2doc tests). Independent package parsing yields enemy181passed/2ignored/
29summaries and host166passed/0ignored/37summaries. All four new names
occur once as PASS, with no installed-test skips. Compile1m53s, total
434.3973128s. The six frozen source/test/brief hashes remained unchanged;
only the five owned Rust file mtimes were refreshed before launch for the
shared target. Eleven unused-helper warnings reflect the still-disabled
Host integration. Normal core lease released after actual completion;
global Cargo/rustc/all ddview pins were quiet. No viewer, original game or
save was used. `git diff --check` passed.

Private evidence: `E:/ddda-re/notes/steps/S31/full-body-scenery-core-4a1e3f8-first.private.log`
SHA256 `55DC8C65EA3884CA4DDE6552FB18BC14841B2D1B472A7ABE44054D9C97F50047`;
the matching `full-body-scenery-core-4a1e3f8-first.proof.private.json`
SHA256 `136A4D3CF9E2B21E9B90281FAB9349943D9B5F850447D4E1D9E659E7B2D5E4BF`.
The strict runner SHA256 is
`3D3C5CF9F41A1C761CA3BE80CDDB6E12A62413A7992EC2AF1B019EC54F7D7D2D`;
the frozen manifest SHA256 is
`0FDF51497637D3A90CB4D0DCF96B2EF81DD7D5FBE9265CE9FF775F0D3884A422`.
No private source exports, game data or raw native bytes are committed.

**Source continuation:** native saved flags are a current-action snapshot:
uNpc vf134 `0x00BB3F40 -> 0x004C7350`, whose `0x004C73B2..0x004C73B8`
copies body273C into2740. Common entry `0x0080E190 -> 0x00AC4F70 ->
0x007F1890` clears current273C for low category206C bit2 before restoring
the saved subset, so the existing scalar entry helper must consume
post-base flags. Damage vf13 `0x0080D490` also copies actual bodyE0/E4/E8
into70F0/70F4/70F8. These prerequisite effects remain source findings,
not an implemented complete common entry. Native NPC vf111
`0x00BB3293` calls850100 before its separate2EA3 override; DemoDamage reads
the rising2EA1 only. Raw-layer init callback `0x010571E0..0x0105754A`
interpolates custom attributes and does not replace the explicit initial
layer58 loop restart0 with the positive installed LMT loop marker.

**Next:** fulfill common cPlAction/cEventAct entry and raw layer clock,
then consume Damage requests on the exact body lifetime and coordinate
the host frame hook. This module is not scheduled by Host yet. GotoEx
awaits accepted S59 navigation; the production editor/ev00 patch remains
disabled and unapplied. No whole S31 route or safe DragonFight checkpoint
is claimed by this batch.

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
particular `0x0080D500 -> 0x00B86010` toggles selected model-joint
`mAttr` byte+0x90 bit0x40, not the landing byte (binding corrected in the
continuation below). Actual NPC ground comes through
`0x00BAFB00 -> 0x00B61A20 -> 0x0084BA60 -> 0x00885E30`; the last function
writes body+0x1ED0 from real scenery flags+0x1B68 under update gate
+0x2398 bit4, with axis-present/mode and ground-check masks. The capsule
mode3 height comes from installed adjustment D3 through `0x00AC50B2`
and `0x00B5F0A0`; HumanEdit `0x0080B450` supplies the separate D84 factor,
not mode3 height. `0x0076C540 -> 0x00CCE580 -> 0x0079DAB0/0x0079E200`
builds/sweeps the actual capsule. Those body/physics providers remain
unintegrated at this helper checkpoint.

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
  entry still requires its other body and model-joint attribute effects.

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

**Native editor handoff continuation (head8361/8380; source only):**
`aStage801::vf07` at `0x0052B2D0` waits for editor result+0x1C8 ==1.
`0x0052B740` selects the opening context only when GameSys+0xB8844 has
mask0x20000 and SceNo is1000. `0x0052B3D9..0x0052B3F7` then requests
stage200/start3 through `0x00401160`, which records the pending stage and
start, changes stage-request state+0x808 from1 to2, and clears+0x814.
The stage210/start10 alternate exit remains unsupported. This request
comes from the native editor stage; it is not an authored q0 JumpArea.

`dragon_route::editor_exit_request` returns the existing typed JumpArea
only after all local profiles confirmed and the actual opening context
is present. It does not publish StageLoaded or complete an event. Fade
is an explicit caller presentation policy: the native editor routine does
not write the quest JumpArea fade parameter, and byte+0x814 is not assumed
to be that parameter.

The installed regression
`installed_editor_confirmation_requests_beach_and_waits_for_loaded_stage_before_ev00`
is **uncompiled/unrun**. It uses the existing prologue/movie preparation
driver solely to reach801; after confirmation there is no fast-forward.
Two distinct profile EDIT records survive the actual StageLoaded200 report.
Pending synthetic callbacks cannot start ev00; only the completed-load
report, installed start3 and installed q0 produce its row19 request.
The test's non-rendered stage/ground and enemy binding are explicit fixtures;
it does not prove original scenery, NPC reactions, production damage or
whole-route acceptance.

The context writer is now sourced: the reset routine `0x00444840` tail
jumps to `0x00444B20` at `0x00444B19`. That post-prologue routine sets
stage800/SceNo1000 and, with EBX zero, loads ECX=14 at `0x00444B6E`, then
calls `0x004373C0` at `0x00444B83`. The setter right-shifts the original
64-bit top-bit mask through `0x012F2AF0`; shift14 yields upper word0x20000,
which `0x004373F7` ORs into GameSys+0xB8844. Head8405 approved the narrow
Story::event_end row0x3E hook: it now writes this actual context mask beside
the existing stage800/SceNo1000 transition, preserving unrelated flags.
The handoff regression asserts that a fresh story lacks the bit and the
translated prologue end supplies it, then passes the editor predicate
without a flag fixture. No similar B8840/B883C flag was substituted. This
new Story hook and all four continuation Rust regressions remain unrun.

**Exact unapplied opening.rs patch for head:** the following patch retains
`LIVE_DRAGON_ROUTE=false`; head can apply it at the final S31 merge and
only enable after the complete route gate. It queues the native editor
request after S56's last-profile confirmation, enables live observations
before the first beach tick, and consumes that request with `enter_stage`
using `keep_places=false` before materialization or ev00 dispatch. The
stage adapter reports StageLoaded only after its real load. The existing
stand-in fast-forward remains solely in the disabled branch. Viewer code
was not edited or built; this patch is not runtime acceptance.

```diff
--- a/crates/ddview/src/opening.rs
+++ b/crates/ddview/src/opening.rs
@@ -59,6 +59,8 @@

 /// The Cassardis stage.
 pub const STAGE: u32 = 200;
+// Head enables only after the complete S31 route gate.
+const LIVE_DRAGON_ROUTE: bool = false;
 /// NPCs within this distance (cm, horizontal) of the Arisen answer E.
 pub const TALK_RANGE: f32 = 220.0;
 /// The view's NPC walking speed (cm/s, *viewer choice*).
@@ -247,6 +249,8 @@
     dragon_capture: bool,
     /// Authored event movie currently awaited by the story (if intro enabled).
     pub intro_movie: Option<PathBuf>,
+    /// Native editor exit waiting for the stage adapter, never a load callback.
+    editor_stage_request: Option<Request>,
     pub host: HostHandle,
     vocation_menu: crate::vocation_menu::Selection,
     guest: Option<ddnet::Client<ddnet::Udp>>,
@@ -443,6 +447,7 @@
     host.new_game_items(std::sync::Arc::new(ddhost::items::ItemData::load(&vfs)?), bag_phase);
     let mut s = Session {
         intro_movie,
+        editor_stage_request: None,
         host: HostHandle {
             local: Some(Box::new(host)),
             server: None,
@@ -586,6 +591,23 @@
     /// Editor confirmation resumes the remaining stand-in dragon route.
     pub fn intro_edit_done(&mut self) -> Result<(), String> {
         if self.world().stage_no != 801 { return Err("editor confirmation outside stage 801".into()); }
+        if LIVE_DRAGON_ROUTE {
+            if self.editor_stage_request.is_some() {
+                return Err("editor stage request already pending".into());
+            }
+            let story = self.host.story.as_ref().ok_or("editor exit needs story")?;
+            let request = ddhost::opening::dragon_route::editor_exit_request(
+                story.world.stage_no, story.world.sce_no, &story.stage.game_flags,
+                true, 0, // S56 calls only after every local profile; fade is project policy.
+            )?.ok_or("editor confirmation incomplete")?;
+            let target = self.host.opening_dragon_data.as_ref()
+                .ok_or("opening dragon installed placement absent")?.target;
+            self.host.character_created_items();
+            self.host.world.manual_stages = true;
+            self.host.world.enable_dragon_scene(STAGE, target);
+            self.editor_stage_request = Some(request);
+            return Ok(());
+        }
         self.host.character_created_items();
         let route: Vec<_> = to_newly_arisen().into_iter().filter(|w| w.stage == STAGE).collect();
         let f = fast_forward(&mut self.host, &route, 240, 30_000, |h| {
@@ -1539,6 +1561,21 @@
             return;
         }
     }
+    if let Some(request) = s.editor_stage_request.take() {
+        match request {
+            Request::JumpArea { stage, pos, .. } => {
+                if let Err(error) = enter_stage(&mut stage_view, st, s, stage, pos, false) {
+                    s.failure = Some(format!("editor stage {stage} handoff failed: {error}"));
+                    return;
+                }
+                s.host.story.as_mut().unwrap().world.start_pos = pos;
+            }
+            _ => {
+                s.failure = Some("invalid editor stage request".into());
+                return;
+            }
+        }
+    }
     if s.guest.is_none() {
     kit_weapons(&mut stage_view, st, s);
     hired_pawn_bodies(&mut stage_view, st, s);
```

**Reviewed-main integration before the next gate:** merge `479e1ae`
consumes only head's accepted/reviewed S53 main
`6b621e66f0b21a4916c5c1740a7750bc668f76c1` (protocol11, durable VITL,
atomic retry). All six owned source/test file hashes from `a522cb8` are
unchanged; all four new regression names remain **uncompiled/unrun**.
The unapplied viewer patch also passes `git apply --check` against this
integrated baseline, actual exit0, without editing the viewer. Next gate
is the complete ddnpcrt/ddenemyrt/ddhost/ddstory packages on the existing
core target, only after the parent CPU handoff. Previous178-pass helper
acceptance and the narrow original CCE580 comparison do not validate this
integrated continuation or the full S31 route.

**Bounded source planning for the next actor; not activated:**

- Native reaction motion banks below10 follow `0x00BB827B -> 0x00B800C0
  -> 0x00886E70 -> 0x00828350 -> 0x0105B480`. The keep-attribute mask at
  cMotionControl+0x58 initializes to0 at `0x00828173`; runtime overrides
  still need their actual observations. A negative installed LMT loop
  marker adds attribute bit4 at `0x0105B5D9`. A bounded installed read
  confirms all five reaction motion slots are present in the resource
  resolved by native bank table `0x014F1D38`. Frame counts remain runtime
  data; no duration table is embedded.
- `0x01060730` sets model layer+6 bit1 from its next-step lookahead at
  `0x01060935..0x0106096C` / `0x01060992..0x010609A0`; bit4 requires the
  actual current frame to reach the end at `0x01060970..0x01060989` /
  `0x010609AC..0x010609CB`. Damage's `0x01057100` checks flags&5, so a
  generic cEventActMotion actor's warm update and bit4-only terminal are
  unsuitable as its body-layer observation. The positive resource loop
  marker is not assumed to set model layer+0x58's restart value.
- The type2 axis query has a separate cScrAdjust+0x1A4
  `mAxisCheckSphereRadius`, read at `0x0079E628`. Constructor `0x0079D936`
  initializes it from `0x014EA574` (15.0), independently of capsule D4.
  Runtime override provenance remains open. Cast length
  `0x0079E58A..0x0079E5B8` multiplies the actual capsule radius by the
  native double at `0x0161C9F0` (1.41421356), then narrows to float;
  airborne motion adds oldY-targetY. The query starts at the lower actual
  endpoint plus the old body position. Existing `sweep_capsule` accepts
  a separate AxisQuery, so no new collision engine is needed. Its
  `triangles_in` availability result must not be replaced by a fabricated
  ground byte.
- `0x00B63F70` sets NPC body+0x1E80/+0x1B6D only when body+0x273C bit1
  is set and the linked NPC flags lack mask0x20000000; otherwise both
  bytes clear. The extended ground mask is dynamic, not always enabled
  merely because the body is an NPC.
- NPC704's installed GotoEx Trace records enable path use and path update.
  Reflection `0x0057B560` maps the authored values to trace+0x10/11/14/
  18/1C/20. Mode bytesA1/A2/A3 come separately from the actual interface
  owner in `0x0057B770`; `0x0057B880` copies authored values without
  supplying those modes. Do not force A3 to bypass navigation or replace
  the native trace with a timer/straight line. A source-owned Nav/BBS
  provider remains a movement dependency; the other victim graphs were
  not surveyed in this bounded read.

**2026-10-10, codex-80: complete continuation helper gate passed; S31
remains open.** Frozen source
`a01ee7cc388c1f9839a015f18612b7e01f83386c`, including only head's reviewed
S53 main integration, passed the complete command
`cargo test -p ddnpcrt -p ddenemyrt -p ddhost -p ddstory --jobs 2` on the
prescribed existing core target. Session30212 and Cargo both actually
exited0 after 516.535s; compile 2m 38s. The result was 428 passed, 0 failed,
2 pre-existing ignored diagnostics across 84 summaries (80 nonempty and
4 doc suites). Package counts were ddenemyrt 179, ddhost 164, ddnpcrt 38 and
ddstory 47. The ignored tests were `installed_reynard_rescue_contact_trace`
and `debug_trace`; no installed test reported SKIP. The canonical nativePC
and DDDA.exe were explicitly supplied.

Each new exact regression name appeared once and passed:

- `ordinary_mode3_capsule_shortens_real_height_and_collapses_short_spans`
- `installed_beach_mode3_capsule_uses_shared_adjustment_height_not_human_factor`
- `demo_damage_flag_is_a_native_rising_edge_with_distinct_body_npc_and_collision_sources`
- `installed_editor_confirmation_requests_beach_and_waits_for_loaded_stage_before_ev00`

All nine source/test/doc hashes remained unchanged before/after the run,
and the worktree remained clean. Private full log
`full-capsule-edge-core-a01ee7c-first.private.log` has SHA256
`35825209CC8C16D9CABB154E96B45042A584B43DB3B83076A065049F18003B6A`;
strict proof JSON SHA256
`2B11F31250E1C558FE0447339D67084B0B72D8C0A7C16934E99E4BAEA10D9157`.
The normal core lease was released after actual completion and global
Cargo/rustc/ddview quiet checks. Eight unused-helper warnings reflect the
unconnected actor providers. This verifies the capsule, edge, real story
context writer and editor-request checkpoint, including the existing
S53/VITL regressions; it does not activate or validate the full body actor,
viewer patch, original ground contact, production ev00 route or GPU gate.
The prior original CCE580 synthetic four-case comparison remains a
separate narrow comparison, not a whole-route observation.

**Further source corrections and actor prerequisites; no new actor code:**

- `0x00B86010` operates on model joints. `0x00FF9A20` (uModel::vf22)
  resolves the same body+0x368 bone-ID map and body+0x364 array of
  0xA0-byte records, returning the joint transform at record+0x10;
  `0x00FF96D0` returns the indexed record. The uModel::Joint DTI allocation
  at `0x0104F820` confirms size0xA0. Its reflector `0x0104F880` binds
  `mAttr` to `0x0083D1B0` / `0x0083D1C0`, which read/write byte+0x90.
  Therefore common damage entry's selected IDs0x96/97/99/9B receive
  joint `mAttr` bit0x40 changes, rather than scenery contact or hurt-node
  flags. Earlier collision-record terminology is corrected above; exact
  bit0x40 semantics and the full joint-state provider remain open.
- cScrAdjust reflection `0x0079DB30` identifies +0x1AC as `mIsSleep`,
  +0x1AD as `mIsGroundCheck` and +0x1AE as `mIsCellingAdjust`.
  `0x0079E3B0` bypasses the query when asleep; the ready body lifecycle
  uCharacterBase::vf05 `0x0084B980` clears body+0x1B6C at `0x0084B9A8`.
  `0x0079E3D7` reads actual ground-check+0x1AD, corresponding to the
  already sourced dynamic body+0x1B6D. Constructor sleep is not a ready
  actor's observation, and later action overrides cannot be discarded.
- uCharacterBase::vf91 `0x0084BF60 -> 0x0084CCC0` adds actual velocity
  times body delta to position at `0x0084D030..0x0084D062`, then adds
  acceleration to velocity at `0x0084D09D..0x0084D0F0`, and clamps to the
  current body limits at `0x0084D0F8..0x0084D1A9`. Constructor `0x0084A100`
  initially supplies +/-10000 from `0x01520BA8` / `0x0161D138`; runtime
  overrides, status/root/pull branches still need actual providers.
  Separately, uObjModel::vf09 `0x00886530` clears contact+0x1B68 and calls
  `0x0076C540` at `0x008865EC` under the real readiness/update gates.
- Model layer+0x58 initializes to0 at `0x0105B53A`. A positive LMT loop
  marker must not be copied there without its actual runtime writer.
  `0x00886E70` restarts only a changed motion or a forced request; Damage3's
  force=false can retain the previous body's layer frame/status. Body
  layer lifetime and new interface-command identity remain distinct.
- Damage interface `0x0056EBB0` stores authored Down at body+0x2E90, and
  `0x0056ECEB..0x0056ED1D` computes facing from the X/Z differences between
  AttackPos and the real body origin, performs f32 subtraction then the
  double CRT atan2, and writes body+0x2578/+0x257C and mode0x100. Reusing
  prior yaw or inventing an opposite angle would bypass this behavior.

**S59 navigation handoff:** bounded installed reads now cover all seven
unchanged beach graphs, each actual exit0 using the existing ddtool;
exports and metadata remain private. NPC704/708/709 each author two
GotoEx commands. Every Trace enables path use, path update and avoidance,
with width disabled. NPC704 authors Damage3, NPC705 Damage4 with Down,
and NPC706/707/710 Damage4 without Down; production continues to read
those payloads from the install. `0x0056DCC0` maps only actual trace
result4 to Done, while result3 moves/runs and0/1/2 remain running.
WaitEnd=false does not bypass arrival. Head8560 assigned this new
  navigation system to S59, ahead of S58; a private incoming handoff lists
the Trace/BBS sources, mode ownership, capsule service and lifetime
constraints. S31 retains real-body reactions, editor handoff and damage
edges. Next action: connect the sourced reaction body providers and
preserve current-lifetime observations, consume accepted S59/S30 work,
then validate the whole route before enabling head's viewer patch or
writing a positive live-profile DragonFight checkpoint.
