# L84: tonight's docs and numbers (2026-10-12)

Owner: codex-5. Branch: `task/L84-docs`, from `task/opt-int` at `afb8f8fc`.
Scope: MILESTONES, NATIVE_PIPELINE, RUNTIME, INVENTORY. Light work only;
no builds, GPU tests, game runs, private data copies or pushes.

## Results

- Updated tonight's L61-L80 performance evidence, build profiles, hot/cold
  layout, L73/L74/L77 changes, play helper and L76 replay timings. L75
  `57b73344` and L76 `225574c7` are on separate branches, not integrated
  into the reviewed base; documented that distinction explicitly.
- Added missing runtime rows for L61/L62/L62b/L63/L67/L78/L80. Preserved
  existing L64/L65/L69-L72 numbers and separate L79/L81/L82 ownership.
- Read-only `python scripts/inventory.py summary`: 59,450 / 59,469
  verified non-thunks (99.968%); 19 residuals; 15,657,151 / 15,690,444
  bytes (99.788%). No rebuild of the shared inventory.
- Evidence: `git log 2fc64ef6..task/opt-int`, commit messages (including
  the L61 merge parent), L66 Results, board history through this task,
  private `profile_*.txt` CPU samples. Kept only aggregate findings;
  no raw profiles or private data copied into the repository.
- Gameplay samples indicate CPU render-service pressure: later render
  thread 85% of a core, main 23%; overlapping constants 22.2%, snapshot
  17.8%, pass encoding 9.5%, guest-lock acquisition 8.8%.
- Limits: no numerical steady-gameplay FPS receipt in the supplied
  profiles/commits; requested head evidence (#10094/#10129). Cranelift
  rejection comes from the assignment; reason/timing receipt requested
  from L75 (#10098), not invented. No separately identified L68 commit.
- Validation: `git diff --check`; profile settings and source paths checked
  against the reviewed commits. No tests/builds needed for documentation.
- Next: head supplies gameplay-rate/Cranelift receipts and integrates the
  pending L75/L76 work; retain the L66 profile option when merging the gate.
