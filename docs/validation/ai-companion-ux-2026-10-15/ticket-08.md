Status: resolved
Type: task
Blocked by: 07

# 08: 大规模历史仍可保存和分页回看

**What to build:** 用户长期积累多照片及单照片长对话后，仍能保存新内容并按需回看，不受原全图库64MiB容器限制。

**Blocked by:** [07：新会话从提交到保存、重开完整可用](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/07-new-conversation-persistence.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 多照片总历史超过旧界限、单照片跨多页均能正常保存及回看。
- [x] 根、索引页、数据块、缓存、打开句柄和单次扫描量有界；不自动删减历史。
- [x] 无关照片保存不因一个会话的规模或损坏失败；服务输入限制与本地保存规模分开。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A10、A14。

原首版阶段对照：12；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-08 integration merge evidence

Distinct merger role authorized after original08-review.md,08-review-fix.md and focused08-review-compact-fix.md; coordinator accepts all explicitly retained09–22 assignments and stated coverage limits.

Worker: `codex/ux-08`, clean reviewed commit **32241c47f2a1f19c6e635abd19d0721e11eea430**.
Integration: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.
Pre-merge integration: **8e4360e37f64d098012720e65c306b43d7dc2656**.
Merged integration HEAD: **bbe911ab3f62c4bab1133e3c888fc184cfdbb914**.
Parents:8e4360e37f64d098012720e65c306b43d7dc2656 and32241c47f2a1f19c6e635abd19d0721e11eea430.
Merge subject: `UX-08: merge reviewed bounded history and readable shared Tools pagination`.

## Merge integrity

Read merger-context/context/execution and applicable client/backend collaboration/AGENTS instructions. Worker and integration were committed/clean; worker included the then-current approved06 integration8e4360e3 as ancestor. Integration had not advanced. Merged **only codex/ux-08** with `git merge --no-ff`; no conflicts or merge-only repairs. No other branch, tracker, original checkout, backend product, push or PR was changed.

`git diff --exit-code 32241c47 HEAD` and `git diff --exit-code codex/ux-08 HEAD` pass. Both reviewed worker and merged HEAD have exact tree **11f9d2bb7b2fc31d876df59d7d4605c7d960ffc5**. Therefore inspected worker screenshots and source correspond exactly to the merged product tree. Post-check branch/status remains clean atbbe911ab.

## Acceptance/integration evidence

- Real independent per-photo persisted history exceeds72MiB and supports bounded old/middle/new reads,10k indexed seek, byte/escape-correct streamed oversized prose and independent corrupt-photo isolation. These engine regressions reran in integration.
- Same-session immutable-prefix/current-tail saves and late acknowledgement patches are preserved. Local storage has no arbitrary message quota; separate service limits reject oversized calls without deleting retained history.
- Ordered mixed continuation items display old prose before next record/current tail; native cold and rapid explicit page requests preserve latest desired cursor and reject obsolete completion. Bounded producer completion ownership waits actual worker retirement before parameters/HTTP feedback and preserves held-result stop. Integration's full normal-parallel63 UI suite includes these outcomes and approved04 full-app first-feedback/discard regressions.
- Shared06 normal/narrow Tools fixed input region, wrapped paging controls and valid120/240 floating/stacked EN/ZH reading are approved. All8 `08/compact-readable-{en,zh-hans}-{float,stack}-{120,240}.png` and regenerated normal/narrow EN/ZH screenshots were visually inspected during final focused review; final visible row259 is actually inside the clipped message region. Actual Older page196/visible227 and Newer228/visible259 are tested through real controls, wheel input and public paint/status. Normal allocated-height and compact positive bounded160px host constraints remain intact.

## Commands and actual results on merged integration

Environment for every Cargo command:
`CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`
`CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`

| Command | Actual result | Log |
| --- | --- | --- |
| `cargo fmt --all -- --check` | passed | `merger/08/fmt.log` |
| `cargo test -p lightcraft-engine companion --lib` | 50 passed | `merger/08/engine-companion.log` |
| `cargo test -p lightcraft-catalog store::tests --lib` | 15 passed | `merger/08/catalog-store.log` |
| `cargo test -p lightcraft-engine tests_persist --lib` | 6 passed | `merger/08/engine-persist.log` |
| `cargo test -p lightcraft-engine reset_edits_restores_a_raws_import_defaults --lib` | 1 passed | `merger/08/core-reset.log` |
| `cargo test -p lightcraft-engine versions_history_and_reset --lib` | 1 passed | `merger/08/core-history-reset.log` |
| `cargo test -p lightcraft-engine rating_flag_undo_redo --lib` | 1 passed | `merger/08/core-undo-redo.log` |
| `cargo test -p lightcraft-engine undo_and_redo_show_the_photo_they_change --lib` | 1 passed | `merger/08/core-undo-selection.log` |
| `cargo test -p lightcraft-engine modification_badges_follow_source_reset_copy_filters_and_history --lib` | 1 passed | `merger/08/core-modification.log` |
| `cargo test -p lightcraft-ui-egui ai_companion --lib` | 63 passed, normal parallel (8.45s test execution) | `merger/08/ui-ai.log` |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | passed, no suppression | `merger/08/strict-clippy.log` |
| `cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown` | passed; inherited warnings only | `merger/08/wasm.log` |
| `cargo test -p lightcraft-ui-egui i18n --lib` | 30 passed | `merger/08/i18n.log` |

`git diff --check 8e4360e3..HEAD` passed; final source/status/tree equality verified. Structured exact command/exit/runtime evidence: `merger/08/results.json`. All selected tests/checks succeeded; no baseline exemptions were needed. Tests ran through Cargo from the integration checkout, so no stale worker or CLI test artifact substituted for merged-source checks.

WASM warnings are in unchanged codecs/jxl_encode, preview/pool DROP_WAIT, engine/devices and ui/lightroom_import (unused imports/dead variants/method); there are no new08 warnings. Strict native engine/UI all-targets no-deps Clippy passes. No blanket full-workspace suite, physical compositor, all-platform runtime, physical power-loss or paid-provider certification is claimed; comprehensive actual coverage remains22.

## Remaining assignments/limits retained

09 still owns pending/HEAD reconciliation, exact failed COW receipt retry, ambiguous durability/retirement and preservation of captured native reader graph roots through actual retirement. The fault and graph-root obligations remain recorded in09-io-note.md and the reviews; current08 has no deleting collector.10 streaming migration/source identity/backup,11 lawful full projection/exact current envelope,12 drafts/editor state and13–21 assigned UI/lifecycle/clear/recovery/cards/revert/comparison/health/rich-reading work remain required. Native11 and HistoryCursor12 combinations are **not** claimed done by this merge. Exact after-send producer preemption remains source-audited with public poll/HTTP outcomes tested, per accepted ruling; no private hook or forced physical scheduling claim.

## Result

UX-08 is merged cleanly with all requested affected integration checks passing. Root owns backend tracker resolution and starting09; merger did not mark the ticket or overall22 objective complete. No additional product edits were made.
