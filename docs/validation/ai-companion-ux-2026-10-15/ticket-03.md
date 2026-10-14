Status: resolved
Type: task
Blocked by: 01

# 03: 单任务提交与任务照片编辑锁

**What to build:** 所有入口每实例仅允许一个活动任务；点击提交即保护任务照片，其他照片可浏览和独立编辑。

**Blocked by:** [01：预整理轮次身份，保持现有AI闭环可用](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/01-round-identity-prefactor.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 未同意、多选、不可用照片或已有任务时，界面与命令一致拒绝提交。
- [x] 提交中、排队和处理期间真实目标照片受保护，明确拒绝/安全取消后解除。
- [x] 命令实际目标、同步派生目标及Undo/Redo均不可绕过；含任务照片的批量操作整体拒绝。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A01、A03、A04。

原首版阶段对照：11；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-03 approved integration merge

Approved worker branch: `codex/ux-03`.
Worker final commit: `713a8061fc69f8d1f6db4f6acd8874ab7b6f3f2f`.
Previous client integration HEAD: `fe9133a10e5aea51aabd15a138d9645c28ecf937`.
Merged client integration HEAD: **`b9e2c8a44b763ad891e85d3dedab719e46dcf625`**.
Merge title: `UX-03: merge single-task leases and physical mutation guards`.

Client integration checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.

## Approval and merge evidence

- Read merger contract/context/execution state and both repositories' applicable AGENTS/collaboration instructions. Worker report and three review reports (`03-review.md`, `03-fix-review.md`, `03-chain-fix-review.md`) plus coordinator scope rulings cover the completed ticket and both returned blockers. Final focused verdict is ready to merge.
- Verified worker and integration checkouts clean before merge, source branch at exact approved final head, integration at exact expected previous head, and integration tip an ancestor of the worker. Worker has integrated its latest required integration tip; approved final head includes both alias fixes.
- Merged only `codex/ux-03`, with `git merge --no-ff --no-edit -m 'UX-03: merge single-task leases and physical mutation guards' codex/ux-03`. No conflicts or merge repairs. Merge parents are precisely the previous integration head and worker final head above. Final integration status is clean; approved worker head is an ancestor of integration HEAD. UX-07 was not merged.

## Behavior and visual acceptance evidence

The merge includes common actual-target task admission, real checkbox consent, submitting/queued/processing/closing lease ownership, typed publication/identity guards, disabled task-photo controls, independent B editing, actual Auto Sync targets, Local-forget and conversion checks, and file/folder/repair preflight. File and history aliases plus nested absolute/relative symlink traversal now reject before files, repair queues, parameters, catalog/revisions, interaction, history stacks or logs change. Independent directory alias-entry history remains available.

Merger inspected existing synthetic worker screenshots: `03/submitting-en.png` shows the paused-edit notice and disabled task-photo edit controls; `03/other-photo-zh-hans.png` shows independent B with enabled controls and exposure `+0.70`. Root already inspected the complete EN/ZH worker evidence. The integrated UI test `real_checkbox_submission_and_other_photo_editing_headless` ran and passed in the 16-test AI companion check; it uses the actual control/widget event path and mocked request queue. No new windowed UI launch or backend/paid model run was performed.

## Actual post-merge checks

Every Cargo command used:
`CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`
and `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`.
Checks ran sequentially on the merged integration checkout. Logs are under `/tmp/lightcraft-ux-execution-20261013/merger/`.

| Command | Actual result | Log |
|---|---|---|
| `cargo fmt --all --check` | Passed | `03-fmt.log` |
| `cargo test -p lightcraft-engine companion --lib` | 33 passed, 0 failed | `03-engine-companion.log` |
| `cargo test -p lightcraft-engine rename:: --lib` | 19 passed, 0 failed | `03-engine-rename.log` |
| `cargo test -p lightcraft-engine tests_folders --lib` | 41 passed, 0 failed | `03-engine-folder.log` |
| `cargo test -p lightcraft-engine sidecar:: --lib` | 18 passed, 0 failed | `03-engine-sidecar.log` |
| `cargo test -p lightcraft-engine reset --lib` | 39 passed, 0 failed | `03-engine-reset.log` |
| `cargo test -p lightcraft-engine undo --lib` | 33 passed, 0 failed | `03-engine-undo.log` |
| `cargo test -p lightcraft-ui-egui ai_companion --lib` | 16 passed, 0 failed | `03-ui-companion.log` |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | Passed; changed crates/all targets compiled | `03-clippy.log` |
| `git diff --check` | Passed | `03-diff-check.log` |

Each filtered test run reports 0 ignored; no test was explicitly skipped. Filters select affected tests and some counts overlap (not a unique aggregate test count). Engine companion output includes all five mutable lease-guard tests. Rename output includes all seven physical history/alias tests. The reset substring also selects preset and model-cache reset cases; its actual 39-test count is reported as run.

## Limits and handoff

No broad baseline suites, WASM rebuild, live backend, RAW/platform matrix or performance benchmark was repeated for this merge. Earlier recorded full-suite baseline failures and worker WASM/layers evidence remain in `03-implementation.md`; this report claims only the checks actually performed above. Future scope rulings remain required by later tickets: transport 04/20, panel 06, storage/projection 07–12, cards/history 16/19, durable Q30/revert 17–18, full certification 22.

No tracker updates, pushes, PRs, original-checkout edits, `.env`, paid models, user photos or OS synthetic input. Merger did not archive worktrees or mark the ticket resolved. **The approved UX-03 merge and affected checks are complete; coordinator may verify this head, resolve UX-03, and manage cleanup/dependent work.**
