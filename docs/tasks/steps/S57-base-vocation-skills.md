# S57: The three base vocations' starting skills, complete and equipped

- **Branch:** `step/s57-base-skills`
- **Files:** `crates/ddenemyrt/src/skills.rs` (+ `skills/`), `crates/ddprogress`
  (learned/equipped state at the first vocation choice), `crates/ddsim` only
  for a skill action that has no runtime yet; tests (headless first)
- **Kind:** feature (a must-have of the current goal: "the 3 base classes")

## Context

`SkillSet::load` supports four base-level skills (sword and mage) from an
explicit support list. docs/VOCATIONS.md "Learning rules" traces the
starting skills: weapon skills with cost 0 at rank 0 (ids 40, 104, 152,
210, 220, 234, 272, 310, 350, 359, 402). The six active slots per weapon
category live at +0x120 + cat*0x18; slots 0..2 belong to the primary weapon,
3..5 to the secondary. Strider bow mode exists (S25-S27); daggers, the bow's
skills and the shield's are not all in the runtime.

## Do

1. Trace what the first vocation choice (0x00673740, `ddprogress::change`)
   and the new-game setup leave **learned and equipped** for Fighter
   (sword + shield), Strider (daggers + shortbow) and Mage (staff): which of
   the eleven starting ids, in which slots. Check it live on the running
   original (read-only, oracle rules: back up the save, never write it) if
   the source leaves doubt.
2. Make every one of those skills run: action class, motions (the per-skill
   `rMotionList` banks, 0x00b5b400), stamina (the weapon stamina tables),
   hit volumes and damage through the existing fight. Reuse the four
   existing skills' path; no stand-in motions or timers.
3. Seat each Arisen's equipped skills from the traced state (per profile,
   co-op), and the skill inputs as the original binds them.
4. Tests: per vocation, each equipped starting skill starts, plays its
   installed motion to its end, costs its stamina and has its **native
   effect**: damage skills hit a goblin; Anodyne (220) heals with the white
   cap and no attack damage; Shield Summons (272) provokes as the source
   says. No skill gets an effect it does not have (codex-81 #8501).

## Done when

All three base vocations start with the original's equipped skills, each
works in a headless installed test, and the Results list every skill with
its source addresses. One bounded GPU check from the route-v2 FieldSettle
checkpoint per vocation (`DDVIEW_DEMO_JOB`), batched into one run.

## Results (codex-81, in progress)

**Implemented, uncompiled and untested:** the starting-skill reader and
headless progression grant. No completed skill-runtime or GPU gate is
claimed by this source checkpoint. The three new installed regressions
are also unrun; earlier progression/runtime test results do not cover them.

**Compared against an original-code scalar oracle:** the complete
`0x006749a0` entry, including the real `0x00674a60`, `0x00780ba0`,
`0x00780c90` and `0x00780350` helpers, returned normally in four bounded
cases: the three base vocations from empty learned/category arrays and a
Fighter return with already-known skills and customized empty slots.
All 13 category arrays, the six active slots, all learned/augment words,
the learn counter, stack cleanup and callee-saved registers matched
independent expectations. Actual exit 0, 0.029 s total, respectively
545/544/456/346 original instructions. The immutable analysis executable's
SHA-256 matches `docs/METHODOLOGY.md`. Original pages and tables were
file-backed; only scalar character/GameSys inputs were supplied, with no
instruction patches or callback stubs. The private proof is
`E:\ddda-re\notes\steps\S57\native-start-grants.private.json`, SHA-256
`d6c5f4b8a16f0adfb7c4ef2c80f1b89029c052ff274e37d151aa0f8822e29fce`.
This proves the scalar grant/copy operation, not the running game's route
or the unrun Rust regressions.

The zero-cost learning catalogue is not the initial equipped state.
`0x00673740` clears the six active slots, provides the first-choice items,
and calls `0x006749a0`. That routine reads the 12 records of
`0x015827e8` and the zero-terminated vocation category lists through
`0x018757c4`. Only a previously unknown skill is granted and placed by
`0x00674a60`. Returning to a vocation therefore preserves customized or
empty skill slots. The new production reader loads these records from the
player's executable; the repository does not embed the table.

| Vocation | Slot 0 (primary) | Slot 3 (secondary/staff second group) |
| --- | --- | --- |
| Fighter | 40 Blink Strike | 272 Shield Summons |
| Strider | 152 Scarlet Kisses | 350 Threefold Arrow |
| Mage | 210 Ingle | 220 Anodyne |

Other slots start empty. The native writer also mirrors sword grants into
mace, longsword into warhammer, and Ingle into both staff categories.
Mystic Knight and Magick Archer exclude the Anodyne row before checking
the learned bits. `Character::new` now grants only the selected vocation;
`Character::unselected` represents the pre-choice cleared record without
granting a Fighter skill merely because the project's vocation enum uses
Fighter as its unresolved-job representation. The native reset at
`0x0077fa50..0x0077fcf3` empties the category sets and both learned arrays.

`0x00780350` resolves active slots from equipped weapon ids: all six
primary slots are copied first, followed by only nonempty secondary slots
overwriting positions 3..5. This replaces the previous guessed copy helper.
The character record does not own equipped item ids. The new
`Host::refresh_equipped_skills` takes actual supplied weapon categories and
synchronizes only that live Arisen's progression and simulation slots.
Its installed regression covers independent Fighter/Mage records,
customized empty slots, a Strider taking a departed Fighter's seat, and
the Fighter returning on another seat after the other Arisen leaves.
This fourth new Rust regression is also **uncompiled/unrun**:
`installed_equipped_skill_refresh_is_owned_by_profiles_across_seat_reuse`.
Automatic first-choice/load/retry/weapon-change callers and the
presentation adapter remain open; this helper alone does not equip a
complete playable opening kit.

New installed regression sources cover all three first choices, the exact
learned set without cross-vocation unlocks, aliases, custom empty slots,
hybrid exclusion, two independent character records, and sparse secondary
overwrites. Their names are:

- `installed_first_vocation_grants_exact_original_slots_without_cross_class_unlocks`
- `installed_free_skill_grants_preserve_custom_empty_slots_and_independent_characters`
- `installed_active_skill_refresh_preserves_primary_tail_under_empty_secondary_slots`

**Native action tracing:** Blink Strike starts motion
`0x729` (`0x00b3dbd0`) and uses real travel distance plus installed sword
adjust limits in `0x00b3de40`; Scarlet Kisses has start `0x71e`, strike
`0x71f`, end `0x720` and input/motion-completion-driven repeat variants
(`0x00b23d50/0x00b23f10`). Shield Summons starts `0x92f`; its base action
finishes on that installed motion's actual completion
(`0x00b38240/0x00b38490`). `0x00b81600` is the nearby-enemy provoke
operation, called with installed shield adjustment values. The layer-9
bank must remain distinct from the primary layer-7 bank. No stand-in
motion, guessed timing, or attack effect has been installed.

**Scarlet Kisses source port, UNRUN:** the runtime now loads skill 152's
own dagger bank and stamina row, uses the installed dagger adjustment
`0xC` as animation rate and `0xA` as maximum cycles, and follows native
`0x00b23f10` intro/strike/end decisions. Two matching skill presses during
a strike permit the next variant; intro presses do not count. The
animation clock is separate from collision/physics time; the default
rate path remains 1.0. Motion completion comes from the installed LMT
endpoint and authored action-end events, with no invented duration.
Native target-turn helper `0x00b24330` remains untranslated: the action
currently retains the host-supplied starting facing. Hit/damage against
the original and the whole S57 gate remain OPEN. Added regression source:

- `ground_motion_clock_one_preserves_default_and_rate_changes_only_motion`
- `scarlet_kisses_requires_two_strike_presses_and_keeps_other_seat_independent`
- `scarlet_kisses_repeat_variants_stop_at_resource_cycle_limit`
- `installed_scarlet_kisses_plays_native_phases_and_cost_edges_for_each_owner`

**Support-effect receipt trace, not yet a port:** `0x00887360` delivers
queued messages to actor vf98 during pass C and then frees them. Anodyne's
message `0x10` accumulates HP in `0x00b79fc0`; `0x00b5c3b0` applies that
gain on the next update using the global simulation/WorkRate delta.
`0x00b7a960` caps ordinary positive healing at saved white HP minus current
HP. Shield Summons message `0x22` routes from enemy `0x00aac5a0` to
`0x00aaca80`: ordinary goblin kind 5 receives additive evaluation type 5,
separate from distance influence type 4. No forced target or damage has
been substituted for that effect.

**Shield Summons and provoke source ports, UNRUN:** the existing `DistanceHate`
now retains native type-5 provoke separately from refreshed type-4
distance influence, sums them for selection, caps additive provoke at
900 (`0x00aaca80`) and decays it by the message's unsigned decay value
once per enemy update (`0x00ab5860`). Target slots retain their original
order and retire both contributions together. Summons' zero damage-vector
message keeps its installed amount unchanged because installed
`param/game[0x14]` and `[0x16]` are positive. Skill 272 loads its own bank 9,
plays native base motion `0x92f` and emits the provoke queue only at entry.
Admission requires an actual secondary `Weapon::Shield`, projected from
the equipped category passed to the host refresh. Fighter vocation alone
does not grant a shield. Fight captures the strict full-3D sphere before
enemy movement and delivers messages in pass C, leaving current target,
alert and damage untouched. Actor lifetime tokens prevent pending source
messages or old hate from carrying across a reused seat. Production item
equipment and viewer bank-9 binding callers remain OPEN. Additional UNRUN sources:

- `provoke_is_additive_capped_and_decays_separately_from_distance`
- `provoke_preserves_first_native_target_slot_and_does_not_require_sight`
- `shield_summons_requires_equipped_secondary_and_emits_entry_once`
- `installed_shield_summons_queues_pass_c_provoke_with_strict_sphere_and_no_damage`

Read-only installed motion metadata confirms Summons `0x92f` has no
group-2 collision activation runs; Scarlet Kisses' cost edge is in intro
`0x71e`, with repeat strikes `0x71f/0x726/0x727` lacking additional cost
edges. Blink's `0x729` has an authored loop frame, which its distance-driven
port must preserve rather than holding the final pose. The private motion
metadata is not copied into production code.

Threefold Arrow's base-level native selector `0x00b16dc0` joins the runtime
triples at `0x014f1910` to `rBowActParamList`: skill 350, learned level 1
selects record 1, rather than the basic-arrow record 0. The installed
record has three ordered shoot rows at nominal frames 0/2/4 targeting
shell group 1/index 0 and a 100-stamina action debit. These observations
are research, not a new runtime grant or copied production parameter
table. Existing ordinary-arrow helpers reject group 1, so connecting the
skill must load and use that group's actual shell and attack data.
Anodyne's healing unit emits native queued message 0x10
(`0x00c04de0..0x00c04f35`); Shield Summons emits 0x22 via
`0x00b81600` / `0x00887250`. Their receiver rules remain under trace;
neither is represented as attack damage.

Anodyne is a healing action and Shield Summons a provoke action. Their
acceptance needs their native effects, not invented goblin damage to
satisfy the broad original wording of the test instruction. Board #8501
received the correction and exact DDSIM/Fight/host hookup scope in head
#8524; unclaimed shared files remain untouched. **Next action:** port the
individually traced action and effect paths and run the
coordinated full package gates before the final batched GPU checks.
## Head scope note (2026-10-10, #8501/#8504)

Starters traced by codex-81 (0x006749A0/0x00674A60): Fighter 40 + 272,
Strider 152 + 350, Mage 210 + 220. Authorized narrow edits: ddsim skill
modes (dagger/shield/bow), ddenemyrt fight.rs hookup, and the host's
progression-to-equipped sync; viewer changes come to head as a patch.
