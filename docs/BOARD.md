# The agent board

Several agents work on this repo at once: the **head** (the coordinating
Claude session) and its subordinates (Claude subagents, Codex instances).
They coordinate through one shared board, `scripts/board.py`. It is a
SQLite event log at `E:\ddda-board\board.sqlite`, shared by every worktree
(override the directory with `DDDA_BOARD`). It is not in the repo.

Run it with any Python 3: `python scripts/board.py <command>`
(`--help` on any command).

## Names

- `head`: the coordinator. It assigns work, settles conflicts and merges
  branches into `main`.
- Subordinates: `<kind>-<n>`, e.g. `claude-4` or `codex-2`. The head gives
  you your name. Keep it for the whole job.
- `human`: the maintainer, if they post.

## What every subordinate does

1. **On start**:
   `board.py join <you> --kind claude|codex --role "<your brief>"`.
   This prints the board. Read it, then `board.py inbox <you>`.
2. **Before editing anything**, claim the system and the files:
   `board.py claim <you> <area> --path <file-or-glob> ... --note "<what>"`.
   - Areas are slash paths named after `docs/ROADMAP.md` systems, e.g.
     `world/terrain`, `world/streaming`, `player/movement`, `player/stamina`,
     `formats/text`, `ai/fsm`. A parent and a child conflict (`world` vs
     `world/streaming`); siblings do not.
   - Paths are repo-relative files or globs (`crates/ddworld/**`).
   - **A refused claim (exit 2) means another agent owns that ground.**
     Do not edit those files. Message the holder (or `head`) and agree who
     changes what. Use `--force` only after `head` says so.
3. **Before touching a file you did not claim**, run
   `board.py check <you> <file>`. Exit 1 means it belongs to someone else.
4. **Report status** when you start a step, finish one, get blocked or
   change plan, and at least every 30 minutes while working:
   `board.py status <you> "<one line>" --state working|blocked|waiting|review|done --task <brief id>`.
5. **Read your inbox** after each status report and before each commit:
   `board.py inbox <you>`. Answer questions addressed to you with
   `--re <id>`.
6. **When you finish:** set status `review` with a one-line result, release
   your claims (`board.py release <you>`), and post a summary to `head`
   (branch, commits, what is verified, what is not).

## Messages

`board.py post <you> <to> "<text>" --kind <kind> [--re <id>]`

- `<to>` is an agent, a comma list (`claude-4,codex-2`), `head` or `all`.
- Kinds:
  - `question`, `answer`
  - `request`: please do or change something
  - `info`: FYI, e.g. "I changed a shared API"
  - `blocker`: I cannot continue
  - `conflict`: sent automatically on a forced claim
  - `assign`: head only
- Talk to the agent you collide with directly. Copy `head` when the
  outcome changes someone's scope or a shared API.
- Keep messages self-contained. The reader has not seen your context.
  Name files, functions, commits and addresses.
- To block until something arrives: `board.py wait <you> --timeout 600`.

## What the head does

- Assigns briefs (`docs/tasks/`) with `assign` messages and gives agents
  their names.
- Watches the board (`board.py show`, `wait head`), settles conflicts,
  releases stale claims (`release head <area> --owner <agent>`; an agent is
  stale after 2 hours without an event), and merges finished branches.
- Updates `docs/MILESTONES.md` and `docs/ROADMAP.md` from the agents'
  results.

## Other commands

- `show [--agent <you>]`: agents, states, claims, recent messages.
- `thread <id>`: a message and its replies.
- `log`: raw events.
- `render`: writes `E:\ddda-board\BOARD.md`, a snapshot for humans.
