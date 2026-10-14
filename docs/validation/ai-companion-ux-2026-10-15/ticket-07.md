Status: resolved
Type: task
Blocked by: 01, 02

# 07: 新会话从提交到保存、重开完整可用

**What to build:** 在没有旧会话的图库中完成一轮请求，将完整消息、实际预览、轮次和执行事实保存到新格式，重开后回看。

**Blocked by:** [01：预整理轮次身份，保持现有AI闭环可用](01-round-identity-prefactor.md)、[02：预整理会话存储能力，保留旧读写行为](02-bounded-storage-prefactor.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 新请求、解释和编辑结果可实际保存/读取，不只完成底层类型或文件工具。
- [x] 只有完整数据和引用就绪才提交可见记录；失败保留旧已提交记录及当前内存结果。
- [x] 记录按照片隔离，核心照片格式与普通历史不改变，重开不续跑旧任务。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../photo-companion-v1/client-spec.md)、[设计定稿](../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A05、A14。

原首版阶段对照：12；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-07 approved integration merge

Client integration branch: `codex/ai-companion-ux`.
Integration checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
Prior integration HEAD: **b9e2c8a44b763ad891e85d3dedab719e46dcf625**.
Approved worker branch: `codex/ux-07`, final HEAD **d2332bb94a7a0f550483ec122f248461ba0a0bbe**.
New integration merge HEAD: **ca1a57021624c1f75bc574ca2e2d35f41151ecbc**.
Merge title: `UX-07: merge fresh conversation persistence and truthful failure records`.

## Merge evidence

- Read merger contract/context/execution and both repositories' AGENTS/collaboration instructions. Initial full review `07-review.md` and approved focused fix review `07-fix-review.md`, updated implementation report and coordinator scope rulings were carried into this merge.
- Verified both checkouts clean, worker branch/HEAD exact, integration branch/HEAD exact, and integration tip an ancestor of approved worker HEAD. The worker had integrated approved UX-03 before delivery.
- Merged only `codex/ux-07` with `git merge --no-ff codex/ux-07 -m 'UX-07: merge fresh conversation persistence and truthful failure records'`. No conflict and no merge-only repair. Merge parents are exactly the two HEADs above; final integration tree equals approved worker tree. Final status clean. UX-04 and UX-06 were not merged.

## Acceptance and behavior

Fresh submit, real procedural-photo rendering, guarded initial/correction publication, actual complete messages/tool associations/previews, explanation-only rounds, per-photo sharded saves and reopened history remain connected. Reopen creates no active job/connection and does not resume execution. Record/metadata/index/preview verification precedes HEAD publication; failures retain actual memory and valid committed records, and independent photos can save. Old v1 and unsupported roots are not silently migrated or discarded, and no core catalog format change is introduced.

The returned Important blocker is fixed: common validation/error and non-user POST failure records failed before transport cancellation; explicit user stop records cancelled. Published results stay succeeded and retain actual settings even if backend delivery fails. Exact late receipt ownership can update confirmation without applying a photo or restoring an invalidated checkpoint. UX-03 Job.lease and exact round/revision `commit_companion` publication survive this integration, as do storage and mutation-guard module declarations.

No new widgets/layout/labels are introduced by UX-07. Merger inspected approved UX-03 synthetic screenshots `03/submitting-en.png` (paused-edit notice and disabled task-photo controls) and `03/other-photo-zh-hans.png` (independent editing with enabled controls and exposure +0.70). The combined headless control/widget admission test ran in the 23-test UI check. No new windowed screenshot or paid/live backend run is claimed.

## Actual post-merge validation

Cargo commands ran in the integration checkout with `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts` and `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`. Cargo builds were sequential. Two independent filters used the newly integration-compiled engine executable while the separate UI executable compiled; no worker build target was used.

| Actual command | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --all --check` | Passed, exit 0 | Merger tool output |
| `git diff --check HEAD^1..HEAD` | Passed, exit 0 | Merger tool output |
| `cargo test -p lightcraft-engine companion --lib` | **40 passed**, 0 failed/ignored | `merger/07-engine-companion.log` |
| `cargo test -p lightcraft-ui-egui ai_companion --lib` | **23 passed**, 0 failed/ignored | `merger/07-ui-companion.log` |
| `.../ux-integration/debug/deps/lightcraft_engine-fe98cba74e750162 reset` | **39 passed**, 0 failed/ignored | `merger/07-engine-reset.log` |
| `.../ux-integration/debug/deps/lightcraft_engine-fe98cba74e750162 undo` | **33 passed**, 0 failed/ignored | `merger/07-engine-undo.log` |
| `.../ux-integration/debug/deps/lightcraft_catalog-b8df42355c73dfab store::tests:: --nocapture` | **15 passed**, 0 failed/ignored | `merger/07-catalog-store-corrected.log` |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --lib --no-deps -- -D warnings` | Passed, exit 0; changed production crates compiled | `merger/07-clippy.log` |
| `git diff --exit-code d2332bb94a7a0f550483ec122f248461ba0a0bbe HEAD --` | Passed; merged tree equals approved worker tree | Merger tool output |

The binary command prefix `...` above is `/Users/liujinju/lightcraft-project/lightcraft/target`; the exact executable names are recorded. Counts overlap and are not a unique aggregate. The Reset substring includes preset/model-cache cases. Initial `cargo test -p lightcraft-catalog tests_store --lib -- --nocapture` matched **0 tests** (`merger/07-catalog-store.log`); it was not counted as validation. Inspection identified the actual module `store::tests::`, and the corrected run executed all 15 bounded Store tests. Nocapture output contains no unavailable-fixture warning. No failed check or skipped required check remains.

## Limits / handoff

No broad baseline/workspace suite, new desktop launch, WASM/platform rebuild, RAW corpus run, live backend or performance benchmark was repeated. Future scope assignments remain: page cache/seek08, transaction recovery/retirement09, migration10, complete projection11, drafts/lifecycle12/15, clear/recovery13/14, cards/comparison16/19, full explicit/durable revert17/18 and comprehensive certification22. Current merge does not claim those future requirements completed.

No tracker modification, PR, push, original-checkout edit, .env, user-photo, paid-model or OS-input operation. Merger did not mark the ticket resolved or remove worktrees. **Approved UX-07 merge and all required affected checks are complete; coordinator may verify ca1a5702, resolve UX-07 and manage dependent work/cleanup.**
