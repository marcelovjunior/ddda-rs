# quest-runtime: a headless story interpreter that runs the opening

- **Owner:** claude lane
- **Branch:** `task/quest-runtime`
- **Board area:** `story/runtime`
- **Status:** review

## Goal

Proposals 1 and 3 in `docs/QUESTS.md`: a headless crate (`ddstory`) that
holds the world state and runs the story's scripts the way the original
does, so that the opening plays from scripted input:
new game → prologue → q0000 → q0001 → q0002 boot.

**World state:**
- SceNo;
- the global story flags;
- the 96 bits per quest;
- layout flags;
- the per-script scratch flags and timers.

Plus a save/load of that state. Design it for N players: the host owns
the story (`docs/COOP_DESIGN.md`).

**Interpreter:**
- `cFSMOrder` scripts: the 70 order types. Implement the ones the opening
  uses; every other order returns an explicit "unimplemented" error, never
  a silent no-op.
- QCT evaluation: one row per pass, disabled after firing unless
  repeatable.
- Stage scripts that start on load, and the trigger volumes (`rScenario`
  areas) fed with player positions.
- **Outputs as requests to the host,** never direct calls into rendering:
  play event X, open menu Y, give items Z, move the party.

## Evidence

- `docs/QUESTS.md` and `docs/EVENTS.md`.
- `crates/ddquest` (merged).
- Text through `mtgmd`.
- Live comparisons through claude-4's oracle: read SceNo and the flags at
  key points.

## Scope

- `crates/ddstory/**` (new)
- `crates/ddquest/**` (reader fixes if needed)
- `docs/QUESTS.md` (runtime section)

## Done when

- A test drives the opening from scripted inputs and asserts the SceNo
  ladder and the flag and quest-bit states at each step, matching QUESTS.md
  and, where available, the original's values.
- Every order the opening touches is translated with its source address.
- Fill in Results and report to `head`.

## Results

**Delivered.** New crate `crates/ddstory` (about 4,400 lines). The
design is described in `docs/QUESTS.md`, section "Runtime".

- **World state:** SceNo and band, GSF, quest bits, layout,
  AreaHit/AreaStay, clock, NPC and OM flags, with save/load.
- **Interpreters:**
  - the cFSMOrder driver and condition evaluator;
  - the orders the opening uses;
  - QCT;
  - stage scripts that start on load;
  - rScenario volumes fed with N players' positions.
- **Host interface:** outputs are `Request`s; inputs are `HostEvent`s
  and the `Host` query trait.
- **Errors:** every untranslated order, variable, judgment, result,
  scenario task and branch returns `Error::Unimplemented`.

**Done criteria:**

1. **Opening test** (`crates/ddstory/tests/opening.rs`, about 2 s in
   release): it drives new game → prologue → q0000 → q0001 → q0002 boot
   from a scripted route and a scripted host.
   - Ladder: 100, 200, 350, 370, 650, 750, 800, 850, 1000, 1025, 1050,
     1100, 1200, 1300, 1400.
   - Quest boots: 28, 0, 31, 1, 2.
   - Event rows: 62, 0, 19, 20, 21, 22, 23.
   - Flags and quest bits at three checkpoints and at the end.
   - Against claude-4's live read of the maintainer's save:
     - q0001's quest words match exactly;
     - q0000's bits are a subset of the live word;
     - GSF 88, 372, 378, 381 are set and 360, 377, 520, 530 are clear,
       as live.
2. **Source addresses:** every order handler cites the routine it
   translates (`src/orders.rs`), and so do the driver, conditions, QCT,
   scenarios and world setters.
3. **Prologue-end set-up** (0x00444c20: 27 GSF, 272 layout flags, 6
   revives): decoded at run time from the player's DDDA.exe, never
   committed. The installed exe's `.text` is plain.

**Not done / open:**
- SceNo 150 and 600 never appear. Both are camera-end raises that the
  translated timing skips; this is not checked live.
- GSF 36, 92, 157, 203, 704 and 1257 are set in our run but clear in the
  live save at SceNo 3400. They may be cleared later; checking needs a
  live read near SceNo 1400.
- Guessed:
  - the order of the system phases within a frame;
  - the variable binder (by property name);
  - the atan2 operand order;
  - two exe conditions at the prologue end and the q0000 boot.
- Untranslated and erroring:
  - scenario boot type 7;
  - scenario tasks 5 and 11;
  - AreaHit acts 5, 7 and 8;
  - PlCheck kinds 0/1;
  - the EnemyCheck sub-objective branch;
  - the other orders of the 70.
- `ddquest`: only additions (`MsbBits::from_words`, a public `Pe`), no
  reader fixes needed.

**Commits on `task/quest-runtime`:**
- f8043cf: the crate and the test;
- 4f491c6: per-entry process memory, explicit PlCheck errors, interact
  and menus per player;
- the docs commit that follows them.
