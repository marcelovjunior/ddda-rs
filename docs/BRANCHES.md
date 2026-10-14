# Branch triage (task/merge-clean, 2026-10-14)

Unmerged-commit count is measured against the pre-merge task/merge-clean (= task/L172-replay + L190/L192/L180). Decisions: merged / superseded / already-in-main (patch-equivalent) / needs owner (real conflict or a user decision; branch untouched, none deleted).
Already merged before this triage: task/train6, task/L172-parallel-shadow, task/L191-haiku-trial, task/L190-hot-paths, task/L192-decouple-design, task/L180-render-worker.

| branch | unmerged commits | decision | reason |
|---|---|---|---|
| step/s48-script-save | 1 | already-in-main | only a merge commit, no unique patch |
| task/L130-main-thread-ports | 5 | already-in-main | all 5 commits patch-equivalent to commits already in main |
| task/L144-render-worker | 1 | already-in-main | patch-equivalent commit already in main |
| task/L172-p3h | 2 | already-in-main | 0 patch-unique commits (only merges of L172-p3) |
| task/L25-performance | 1 | already-in-main | patch-equivalent commit already in main |
| task/L75-build-pipeline | 4 | already-in-main | patch-equivalent commits already in main |
| task/fps-line | 1 | already-in-main | 0 patch-unique commits (merge of train6) |
| task/fps-try | 1 | already-in-main | 0 patch-unique commits (merge of L99-draw-memo) |
| task/item-use-fight | 1 | already-in-main | patch-equivalent commit already in main |
| task/skills-runtime-fight | 3 | already-in-main | patch-equivalent commits already in main |
| step/s13-field-npcs | 1 | merged | merged cleanly into task/merge-clean |
| step/s31-dragon-route | 3 | merged | merged cleanly into task/merge-clean |
| step/s32-npc-motions | 1 | merged | merged cleanly into task/merge-clean |
| step/s34-npc-combat-body | 2 | merged | merged cleanly into task/merge-clean |
| step/s41-frame-rate | 3 | merged | merged cleanly into task/merge-clean |
| step/s41-material-specialization | 5 | merged | merged cleanly into task/merge-clean |
| step/s49-enemy-sensor | 1 | merged | merged cleanly into task/merge-clean |
| step/s57-base-skills | 11 | merged | merged cleanly into task/merge-clean |
| task/L107-frame-stats | 1 | merged | merged cleanly into task/merge-clean |
| task/L108-ndraw-map | 3 | merged | merged cleanly into task/merge-clean |
| task/L109-loading | 1 | merged | merged cleanly into task/merge-clean |
| task/L116-pipeline-warmup | 1 | merged | merged cleanly into task/merge-clean |
| task/L133-native-next | 1 | merged | merged cleanly into task/merge-clean |
| task/L137-parallel-encode | 1 | merged | merged cleanly into task/merge-clean |
| task/L154-worker-alloc | 6 | merged | merged cleanly into task/merge-clean |
| task/L157-waitchain | 1 | merged | merged cleanly into task/merge-clean |
| task/L166-vk-tdr | 1 | merged | merged cleanly into task/merge-clean |
| task/L178-semantic-jobs | 1 | needs owner | WIP collision port: merged, then reverted - fails the strict callee-read harness (mtcore semantic_jobs original_sweep_support_shape, call d8e400 register 3) |
| task/L183-job-ports | 2 | merged | merged cleanly into task/merge-clean |
| task/L184-s10 | 1 | merged | merged cleanly into task/merge-clean |
| task/L184-s2 | 1 | merged | merged cleanly into task/merge-clean |
| task/L184-s3b | 2 | merged | merged cleanly into task/merge-clean |
| task/L184-s5 | 9 | merged | merged cleanly into task/merge-clean |
| task/L186-sched | 1 | merged | merged cleanly into task/merge-clean |
| task/L21-boot | 4 | merged | merged cleanly into task/merge-clean |
| task/L58 | 3 | merged | merged cleanly into task/merge-clean |
| task/L79-wgsl-unlinked-input | 1 | merged | merged cleanly into task/merge-clean |
| task/L83-naga-direct | 2 | merged | merged cleanly into task/merge-clean |
| task/L84-docs | 1 | merged | merged cleanly into task/merge-clean |
| task/L89-backend-worker | 2 | merged | merged cleanly into task/merge-clean |
| task/L91-run-analyst | 1 | merged | merged cleanly into task/merge-clean |
| task/L96-encoder-hang | 1 | merged | merged cleanly into task/merge-clean |
| task/brief-pawn-actor | 6 | merged | merged cleanly into task/merge-clean |
| task/chimera-runtime | 2 | merged | merged cleanly into task/merge-clean |
| task/opening-perf-route | 1 | merged | merged cleanly into task/merge-clean |
| step/s41-camera-settings | 3 | needs owner | real code conflict in: crates/ddview/src/main.rs crates/ddview/src/play.rs |
| task/L109c-boot | 1 | needs owner | real code conflict in: crates/ddruntime/src/load_log.rs scripts/load_timeline.py |
| task/L131-profile-attribution | 1 | needs owner | real code conflict in: scripts/sample_profile.py scripts/test_sample_profile.py |
| task/L138-vk-encoder | 2 | needs owner | real code conflict in: crates/ddgpu/src/encode.rs crates/ddgpu/src/lib.rs crates/ddgpu/src/vk/mod.rs crates/ddgpu/src/vk/objects.rs crates/ddgpu/src/vk/pass.rs crates/ddgpu/tests/vk_encoder.rs docs/tasks/L137-parallel-encode.md |
| task/L142-flat-memory | 4 | needs owner | real code conflict in: crates/ddnative-base/Cargo.toml crates/ddnative-base/src/mem.rs research/native/lift.py |
| task/L143-backend-thread | 2 | needs owner | real code conflict in: crates/d3d9cmd/src/deferred.rs crates/ddgpu/src/lib.rs docs/tasks/L143-backend-thread.md |
| task/L147-nlg | 3 | needs owner | real code conflict in: crates/ddgpu/src/lib.rs |
| task/L148-vk-frontend | 4 | needs owner | real code conflict in: crates/d3d9cmd/src/deferred.rs crates/ddgpu/src/lib.rs crates/ddgpu/src/vk/mod.rs crates/ddgpu/src/vk/objects.rs crates/ddgpu/tests/vk_encoder.rs docs/tasks/L148-vk-frontend.md |
| task/L150-upload-belt | 7 | needs owner | real code conflict in: crates/d3d9cmd/src/deferred.rs crates/ddgpu/examples/backend_cost.rs crates/ddgpu/src/backend_data.rs crates/ddgpu/src/cache.rs crates/ddgpu/src/lib.rs crates/ddgpu/src/native_job.rs crates/ddgpu/src/vk/mod.rs crates/ddgpu/src/vk/objects.rs crates/ddgpu/tests/vk_encoder.rs |
| task/L153-sync | 1 | needs owner | real code conflict in: crates/ddruntime/src/sync_followup.rs crates/ddruntime/src/threads.rs docs/tasks/L153-sync.md |
| task/L159-vk-default | 1 | merged | Vulkan is now the default (user decision 2026-10-14); DDGPU_VK=0 forces the wgpu path. Smoke with DDGPU_VK unset on the pinned bench config: 48.8 fps t46-84 (26 probes), vk passes counted, no stopped/panic |
| task/L165-load-next | 9 | needs owner | real code conflict in: crates/ddplay/src/input.rs |
| task/L184-s1 | 2 | needs owner | real code conflict in: crates/mtdraw/src/material.rs |
| task/L184-s4 | 1 | needs owner | real code conflict in: crates/mtcore/src/semantic_s4_joint.rs |
| task/L184-s9 | 1 | needs owner | real code conflict in: research/native/semport_s9.py |
| task/L185-flags | 1 | needs owner | real code conflict in: crates/ddnative-base/src/emit.rs crates/ddnative-base/src/mem.rs |
| task/L185-wake | 1 | needs owner | real code conflict in: crates/ddruntime/src/threads.rs |
| task/L187-effects | 2 | needs owner | real code conflict in: crates/mtcore/src/semantic_s7_ftol.rs |
| task/L55-emu-differential | 2 | needs owner | real code conflict in: Cargo.lock crates/ddruntime/src/emu.rs |
| task/L73-lean-emission | 2 | needs owner | real code conflict in: research/native/test_lean_emission.py |
| task/L76-gate-speed | 1 | needs owner | conflicts with current native_gate.sh (NATIVE_PROFILE, extra packages); parallel-frontend gate speedup must be re-applied by hand |
| task/L90-draw-costs | 1 | needs owner | real code conflict in: crates/ddgpu/tests/draw.rs |
| task/enemy-families | 23 | needs owner | real code conflict in: crates/ddai/src/thinktbl/mgr.rs crates/ddenemyrt/src/enemy_stats.rs crates/ddenemyrt/src/fight.rs crates/ddenemyrt/src/goblin.rs crates/ddenemyrt/src/lib.rs |
| bisect29a | 38 | superseded | bisect scratch branch (38 commits, 1510 files of generated shard churn); content lives in train6/L172 lines |
| task/L104-play-o1 | 75 | superseded | play-o1 shard experiment (756 generated files); replaced by play-o2 in task/L172-replay |
| task/L144-d3d9-parked | 1 | superseded | explicitly PARKED, not for merge (native renderer attempt) |
| task/L146-gate-c24 | 8 | superseded | gate-combo experiment, 1464 generated shard files; evidence docs only worth keeping on the branch |
