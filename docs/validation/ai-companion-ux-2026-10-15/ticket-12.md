Status: resolved
Type: task
Blocked by: 06, 07

# 12: 每照片草稿与阅读位置独立保留

**What to build:** 用户切照片或编辑工具后找回各自未发送文字和阅读位置；发送清空、失败/停止恢复请求供编辑。

**Blocked by:** [06：右侧AI面板与编辑工具共用区域](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/06-shared-ai-sidebar.md)、[07：新会话从提交到保存、重开完整可用](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/07-new-conversation-persistence.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 草稿不进入已发送历史，切照片/工具不会串草稿或丢滚动位置。
- [x] 正常保存重开恢复草稿，删除/失败流程有独立可保存的草稿状态。
- [x] 失败/停止只恢复对应请求供编辑，不自动发送，已提交要求不原地修改。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A08、A14。

原首版阶段对照：12、15；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX12 authorized integration merge

Worker branch: `codex/ux-12`.
Reviewed worker commit: **bf70f647fcc8f8f43e741c0a6a4cc2b4f979ce59**.
Previous integration HEAD: **66b6b739b5db6c7fc7f6b2fc59ac5dd743681721**.
Merged integration HEAD: **72717addc30f439d753989512674c0e800866cbc**.
Integration checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.

Root accepted whole UX12 review plus focused backoff re-review and explicitly authorized this separate merger role. Read merger-context, execution metadata, both repository AGENTS/collaboration rules. Confirmed clean worker/integration checkouts and approved integration ancestry before the single assigned merge. Executed:

`git merge --no-ff codex/ux-12 -m 'UX-12: merge photo drafts and paged reading position'`

Merge completed without conflicts or additional fixes. Parents are exactly66b6b739 and bf70f647. `git diff --exit-code HEAD bf70f647...` passes, so the integration tree equals the reviewed candidate. Candidate ancestry, full-range diff whitespace and clean final integration/worker status all pass. No push/PR, original checkout changes, backend/tracker mutations or tracker closure.

## Behavior and accepted review evidence

Whole review: `/tmp/lightcraft-ux-execution-20261013/12-review.md`; accepted narrow correction: `12-review-fix.md`. The sole Important save-backoff issue is corrected: automatic frame retries preserve the existing two-second delay after an editor failure; dirty/unsaved truth remains visible, another photo can save, and explicit Save can immediately retry/reopen the literal draft.

The reviewed current user path includes independent library-generation/PhotoId editor ownership; all dirty jobless drafts in normal Session persistence; photo/tool/library switching; immutable leased submitted prompt and version-guarded stop/failure restoration with no auto-send; required native UX08 record/fragment/pixel/follow-tail preservation and newest-navigation ownership; unknown-root/legacy protection; unreadable draft fallback preservation until explicit authored replacement; no small-metadata full-history rewrite; UX04 cold-editor first-feedback storage deferral; and approved UX11 strict projection/envelope behavior. These actual caller paths remain covered by the merged integration suites below.

Existing final EN/ZH screenshots `12/drafts-reopened-en.png` and `12/drafts-reopened-zh-hans.png` were independently inspected in the whole review. The integrated tree is exactly the final reviewed candidate; backoff correction changes only engine persistence/test adapter and no layout. Existing screenshot evidence is retained, with no fresh screenshot or physical-window claim.

## Actual integration checks

macOS; all Cargo commands use `CARGO_BUILD_JOBS=2`, `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`, `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`; normal parallel tests, no skips/suppressions/serial-test overrides. Build durations include integration recompilation. Logs and exact execution summary: `/tmp/lightcraft-ux-execution-20261013/merger/12/`.

| Command | Actual result | Log |
| --- | --- | --- |
| `cargo fmt --all -- --check` |passed |fmt.log|
| `cargo test -p lightcraft-engine companion --lib` |56 passed,0 failed |engine-companion.log|
| `cargo test -p lightcraft-engine tests_persist --lib` |6 passed,0 failed; includes feedback deferral, explicit flush, failed Undo append and close/retry persistence |engine-persist.log|
| `cargo test -p lightcraft-ui-egui ai_companion --lib` |95 passed,0 failed; required native cursor/fragment/read-intent, cold feedback, no-job/library drafts, compact history, strict projection included |ui-ai.log|
| `cargo test -p lightcraft-ui-egui i18n --lib` |30 passed,0 failed |ui-i18n.log|
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` |passed |clippy.log|
| `cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown` |passed; existing baseline warnings only |wasm.log|

Additionally ran the just-compiled integration engine test binary with `--exact` for `tests::reset_edits_restores_a_raws_import_defaults`, `tests::versions_history_and_reset`, and `tests::undo_and_redo_show_the_photo_they_change`: all three1/1 passed. Exact binary commands/results are in `merger/12/core-summary.json` and named logs. This retains ordinary Reset/history/Undo/Redo coverage without a blanket workspace rerun.

No failed checks or baseline-failure waivers were needed. All own commands finished; integration and worker remain clean at the hashes above.

## Limits and handoff

This merge does not claim full-workspace, physical power-loss/compositor, Windows/Linux, paid-provider or real-camera corpus certification. Comprehensive platform/fixture reporting remains UX22.

Separate assigned UX09 GC/reconciliation must preserve editor files, UX10 migration must retain/flush dirty overlays, UX13 clear must preserve drafts/reset reading epoch, UX14 recovery must retain independently readable/authored drafts, UX15 owns every shutdown/cancel-before-save route, UX19 historical mode must preserve current editor controls/state, and UX21 extends rich rendering. Their requirements remain required; no handoff is waived. Current UX12 required native08 position integration is completed here, not deferred to21.

Authorized merge and affected integration validation are complete. Root remains responsible for tracker resolution and scheduling subsequent work; this merger made no tracker changes.
