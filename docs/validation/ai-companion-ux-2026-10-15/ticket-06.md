Status: resolved
Type: task
Blocked by: 03

# 06: 右侧AI面板与编辑工具共用区域

**What to build:** 用户从菜单或工具入口打开可收起AI面板，查看对话、授权、开始和停止；普通编辑继续可用。

**Blocked by:** [03：单任务提交与任务照片编辑锁](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/03-single-task-and-edit-lock.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 重复打开幂等，收起后菜单真正展开，无选片/多选有可理解空态。
- [x] 不强制改变网格/详情视图，AI与普通编辑通过同一区域切换。
- [x] 消息区独立滚动，阶段/输入固定；替换独立窗口并保留控制可操作入口。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A03、A15。

原首版阶段对照：15；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-06 integration merge

- Approved worker: `codex/ux-06`, final `172e8734fb8e386e327c5d54e5d884d3877d5116`.
- Previous client integration: `b801ff351944af6b41f4250dcc2ddbfc7a88dce5`.
- Merged integration HEAD: **`8e4360e37f64d098012720e65c306b43d7dc2656`** on `codex/ai-companion-ux`.
- Integration checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
- Merge command: `git merge --no-ff codex/ux-06 -m 'UX-06: merge reviewed shared AI Tools panel and compact controls'`.

Confirmed clean committed worker and clean expected integration branch/tip, and that the worker contained the latest approved integration before merging. Only assigned UX-06 was merged. Merge was conflict-free, with no source repairs or omissions. `git diff --exit-code 172e8734fb8e386e327c5d54e5d884d3877d5116 HEAD` passed: integration has exactly the reviewed worker tree. Final integration is clean at the HEAD above. Original checkouts, backend and tracker were not modified.

## Acceptance evidence

Original full review `06-review.md` accepted the shared Tools implementation and found the valid-short-pane blocker. `06-review-fix.md` approves its repair and integration of the approved UX-04 feedback guard.

The merged AI suite exercises idempotent menu open/reveal without changing Grid, custom collapsed Tools, no/multiple selection, real panel Start/revoke/collapse and independent Presets, ordinary Edit/other-photo behavior, actual long-message pixel scrolling with a fixed composer, full 50-line draft and one registry entry per visible target. New tests cover active Stop/consent and idle field/Clear/Start in English and Chinese across floating and stacked 120-point minimum panes and larger short panes, with real clicks/scroll input and unchanged photo zoom. Approved UX-04 full-app failed autosave plus discarded feedback frame and direct control Stop regressions pass on the actual shared Tools path.

Worker screenshot evidence was visually inspected during the immediately preceding final-source review: eight final 19:31 images under `/tmp/lightcraft-ux-execution-20261013/06/`, including active float 200/120 English, active stack 200/120 Chinese, idle float 120 English/240 Chinese and idle stack 120 Chinese/240 English. They show readable fonts, reachable real controls and compact scrolling within the supported Tools surfaces. The exact-tree merge preserves this reviewed visual evidence. These are procedural headless screenshots, not claims of OS/live desktop automation.

## Integration checks

All Cargo checks used `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration` and `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`. Logs are in `/tmp/lightcraft-ux-execution-20261013/merger/06/`.

| Command | Actual result | Log |
|---|---|---|
| `cargo fmt --all --check` | Passed | `fmt.log` |
| `cargo test -p lightcraft-ui-egui ai_companion --lib -- --test-threads=1` | 49 passed; 0 failed/ignored; 753 filtered; rebuilt merged UI source | `ui-ai.log` |
| `cargo clippy -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | Passed | `clippy-ui.log` |
| Existing exact integration engine test binary with `companion --test-threads=1` | 41 passed; 0 failed/ignored | `engine-companion.log` |
| Same binary with `tests_persist --test-threads=1` | 6 passed; 0 failed/ignored | `engine-persist.log` |
| Same binary with `tests::versions_history_and_reset --exact` | 1 passed | `core-reset-history.log` |
| Same binary with `tests::reset_edits_restores_a_raws_import_defaults --exact` | 1 passed | `core-reset-defaults.log` |
| Same binary with `tests::rating_flag_undo_redo --exact` | 1 passed | `core-rating-undo.log` |
| Same binary with `tests::undo_and_redo_show_the_photo_they_change --exact` | 1 passed | `core-selection-undo.log` |
| `git diff --check b801ff351944af6b41f4250dcc2ddbfc7a88dce5..HEAD` | Passed | Empty output |

Engine regression binary: `/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration/debug/deps/lightcraft_engine-fe98cba74e750162`, built and verified for approved UX-04. `git diff --exit-code b801ff351944af6b41f4250dcc2ddbfc7a88dce5 HEAD -- crates/engine` passed, so its tested engine source is unchanged by this merge. It was reused for focused regressions instead of rebuilding identical engine code. No screenshot waits or environment were used for the merged 49-test UI run, preserving the rapid real-widget regression.

No failures/exemptions were needed for these scoped checks. Broad UI/workspace tests were not repeated. Final worker WASM/parity evidence is retained in the implementation/review reports; those checks were not additionally rerun for this exact-tree merge and are not claimed as merger-run checks.

## Limits and retained scope

General bounded/asynchronous message and history I/O remains UX-08. Storage reconciliation/migration, full projection, drafts/read positions, clear/damage recovery, complete lifecycle, cards/comparison, durable revert/Q30 and global health/task entry remain required UX-09/10/11/12/13/14/15/16/17/18/19/20. Root accepted the review's declined lines as retained later scope; none is dropped by this merge. UX-22 owns comprehensive actual platform evidence.

Verified controlled macOS procedural headless behavior and local network/Store fixtures. Windows/Linux live UI/network, real providers and power-loss certification are not claimed. No .env, paid models, user photos, OS input, push or PR was used. Backend/tracker resolution and user-facing ticket completion remain controlled by root.
