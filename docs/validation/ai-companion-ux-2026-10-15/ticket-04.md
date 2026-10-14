Status: resolved
Type: task
Blocked by: 03

# 04: 停止与撤销同意真正阻断未发送内容

**What to build:** 用户停止或撤销同意后，未发送创建/图片请求不再交付，迟到候选不再发布，最近完整结果保留。

**Blocked by:** [03：单任务提交与任务照片编辑锁](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/03-single-task-and-edit-lock.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 一秒内给本地反馈；队列、编码后及发送过程中检查本轮取消与授权。
- [x] 停止/心跳控制仍能收尾，重新同意不复活旧请求。
- [x] 交付未知的提交不被新任务越过；后台尚忙仅给手动重试出口。
- [x] 分别验证先停止后候选、先发布后停止及迟到HTTP/SSE，终态只关闭一次。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A01、A02、A04、A14。

原首版阶段对照：11；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-04 integration merge

- Approved worker: `codex/ux-04`, final `52e3fa8dc1aa0a8e67eba178d2963c047e244c30`.
- Previous integration: `ca1a57021624c1f75bc574ca2e2d35f41151ecbc`.
- New integration HEAD: **`b801ff351944af6b41f4250dcc2ddbfc7a88dce5`** on `codex/ai-companion-ux`.
- Integration checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
- Merge command: `git merge --no-ff codex/ux-04 -m 'UX-04: merge cancellation transport and complete feedback frame protection'`.

Verified clean worker/integration state, exact expected branch tips, and prior integration ancestry before merging. Only approved UX-04 was merged. Merge completed without conflicts or repairs; integration tree is byte-identical to the approved worker tree (`git diff --quiet 52e3fa8d HEAD`). Integration HEAD advance was sent to root before checks finished so UX-06/08 could integrate the approved transport/feedback seam. Final integration remains clean at the exact HEAD above.

## Acceptance evidence

`04-review.md` accepted transport cancellation/identity/candidate behavior and found the full-frame feedback blocker. `04-fix-review.md` approves its complete production repair. UI tests on the merged tree exercise real checkbox revocation and Stop, clean-store command/control Stop, prior failed archive autosave plus discarded frame, next-frame failing Store and storage-only retry, actual saved/reopened terminal, unknown-create blocking, reserved priority Stop and independent heartbeat, partial-write cancellation, fresh regrant identity, manual photo_busy retry, initial/correction candidate ordering and late HTTP/SSE facts.

The engine regression proves companion deferral preserves dirty/error state and old HEAD until persistence succeeds, while core rating remains crash-durable. Ordinary Reset/history and Undo/Redo behavior was separately rerun. Public explicit Save and Close still flush even when the runtime feedback flag is set. The accepted first-feedback proof uses one completed production frame; aggregate `Headless::request` elapsed including its permitted extra frame is explicitly excluded as first-feedback evidence.

Visual evidence was inspected during review (actual English and Simplified Chinese cancelling UI); root additionally inspected English cancelling and Chinese revoked controls. Images remain under `/tmp/lightcraft-ux-execution-20261013/04/`. UX-06 shared Tools presentation is not merged or claimed complete by this ticket.

## Integration checks

All cargo commands used:

`CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`

`CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`

Logs: `/tmp/lightcraft-ux-execution-20261013/merger/04/`.

| Command | Actual result | Log |
|---|---|---|
| `cargo fmt --all -- --check` | Passed | `fmt.log` |
| `cargo test -p lightcraft-ui-egui ai_companion` | 38 passed, 0 failed/ignored; includes both full-production feedback tests | `ui-companion.log` |
| `cargo test -p lightcraft-engine companion` | 41 passed, 0 failed/ignored | `engine-companion.log` |
| `cargo test -p lightcraft-engine tests_persist` | 6 passed, 0 failed/ignored | `engine-persist.log` |
| `cargo test -p lightcraft-engine --lib tests::versions_history_and_reset -- --exact` | 1 passed | `core-reset-history.log` |
| `cargo test -p lightcraft-engine --lib tests::reset_edits_restores_a_raws_import_defaults -- --exact` | 1 passed | `core-reset-defaults.log` |
| `cargo test -p lightcraft-engine --lib tests::rating_flag_undo_redo -- --exact` | 1 passed | `core-rating-undo.log` |
| `cargo test -p lightcraft-engine --lib tests::undo_and_redo_show_the_photo_they_change -- --exact` | 1 passed | `core-selection-undo.log` |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | Passed | `clippy.log` |
| `git diff --check ca1a57021624c1f75bc574ca2e2d35f41151ecbc..HEAD` | Passed | Command output was empty |

No test exemptions or failures were needed for these scoped checks. Zero-test integration sections in filtered cargo output are not claimed as executed docking/shutdown tests. No broad UI baseline repeat was performed. Final worker WASM compilation and independent reviewer focused binary/probe evidence are retained in their reports; WASM was not additionally rerun during this exact-tree merge.

## Limits and retained scope

Controlled macOS loopback, procedural memory libraries, egui/headless inputs and companion Store fault barriers are verified. Windows/Linux live behavior, power-loss certification and real providers are not claimed. Already-delivered bytes cannot be recalled; no backend physical-worker-release protocol was invented. First feedback precedes companion archive retry, while general large-history asynchronous I/O remains UX-08.

UX-06 layout, UX-08 I/O, UX-09/10 storage/migration, UX-11 projection, UX-13/14/16/19 cards/history/comparison, UX-15 complete lifecycle, UX-17/18 revert and UX-20 health remain assigned later; none is dropped or falsely completed here. Comprehensive platform truth remains UX-22.

No backend/tracker edits, push, PR, original-checkout mutation, .env access, paid services, real-photo writes or OS synthetic input. Root controls tracker resolution and any user-facing completion claim.
