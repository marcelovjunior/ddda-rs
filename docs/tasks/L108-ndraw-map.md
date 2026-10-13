# L108: nDraw map

Assigned to codex-10 by head via session brief, 2026-10-13. Branch
`task/L108-ndraw-map`, based on `task/fps-line`.

Scope: research and `docs/NDRAW.md`; no builds, game runs or Ghidra changes.
Map D3D9 owners, resource layouts, commands, thread/frame ordering, shader
binding and ranked native-renderer hook candidates. Commit in steps and
post each hash to head. Do not commit private research or raw dumps.

## Results

Initial checkpoint: device root, four packed-state constructors, object-aware
apply boundary, command-cache copying and render-worker entry confirmed by
read-only Ghidra. Full census, layouts, pass ordering and ABI work continues.
No runtime implementation, tests, game run or visual comparison performed.

Milestone integration is deferred to head while another lane owns
`docs/MILESTONES.md` (board message 10585).
