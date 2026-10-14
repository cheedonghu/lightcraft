Status: resolved
Type: task

# 01: 预整理轮次身份，保持现有AI闭环可用

**What to build:** 引入稳定的照片/图库/轮次关联及兼容适配，让当前点评、实际修改、停止与历史回看仍可正常使用，为后续替换提供小接口。

**Blocked by:** None（可立即开始）

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 原有解释、初次修改、一次修正、停止流程可用，实际结果不变。
- [x] 切照片/图库、迟到结果仍能区分原任务归属，不以当前选中照片接收。
- [x] 过渡期间旧界面及旧会话继续可读，不在本任务清空或升级用户数据。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../photo-companion-v1/client-spec.md)、[设计定稿](../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A03、A05、A14。

原首版阶段对照：11；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，工作分支codex/ux-01；采用用户授权的常规TDD替代缺失tdd技能。

## Answer

# UX-01 combined integration merge evidence

Assigned approved branch: `codex/ux-01`, HEAD **`2e2f4442fe2fd5f4755ce30d31cd6843f2237ac2`** (identity prefactor plus accepted disconnect-retirement fix).

Integration worktree: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.

Pre-merge HEAD: `3eb9411be3837b25c67a25e855ced1a9c0e80988` (approved UX-02 already integrated).

Final merged HEAD: **`7de70b393031298db29bcf4b81f028e5422a1c40`**, `UX-01: merge stable round identity and disconnect retirement`.

Parents are exactly the pre-merge integration HEAD and approved UX-01 worker HEAD. UX-05 was not merged.

## Preconditions and merge reconciliation

Read the original review, updated implementation report, independent `01-rereview.md`, and merger contract. The accepted Important abandoned-job finding is independently cleared. Inspected the fixed retirement implementation and surrounding source, which finishes all disconnected-generation jobs before attempting every save, returns any save failure, and only creates a replacement connection after success. The re-review traces failed archive saves into retained dirty/retry state and existing close guards, without claiming a new permanent durability admission gate.

Worker branch was clean at the assigned SHA and based on `363e5d592a64af41cefbe3ff0642ee347c61cf18`; ancestry was verified. Its report records merging that then-latest integration tip before completion. UX-02 advanced integration afterward, so ordinary combined integration was performed under the updated merger contract.

`git merge --no-ff --no-edit -m 'UX-01: merge stable round identity and disconnect retirement' codex/ux-01` had one localized conflict in `crates/ui-egui/src/ai_companion.rs`, conversation clear. Resolved by keeping **both** UX-01's scoped empty Conversation and UX-02's `library.as_mut()` for Store-based attachment deletion. No product behavior was otherwise changed. After combined tests passed, `git commit --no-edit` completed the merge. Comparing the final companion bridge against approved UX-01 shows only the expected `as_ref()` → `as_mut()` integration difference.

## Ticket acceptance evidence

- **Existing explanation/initial edit/correction/stop flow:** the 13-test merged UI companion suite exercises an explanation completed after another photo is selected, real procedural initial and correction rendering/commits, actual succeeded feedback, duplicate-plan non-reapplication, stop before backend admission, stop after correction preserving the actual latest result, and late worker rejection. Explanation leaves edit state and the selected other photo unchanged. Public undo continues restoring the complete round start while retaining its conversation.
- **Original photo/library/round ownership:** work and HTTP results use the full local identity before any result effects; the merged tests vary local ID, submission, connection generation, library generation and target photo. Old errors cannot terminate the current task or connection. Local round ID remains stable across backend admission and original-photo save/load. Current-photo `ai.status` still reports its prior fields while `activeRounds` identifies the originating task photo.
- **Disconnected generation closes safely:** public command-before-poll regression covers admitted/unadmitted A crossed with another-photo submission/same-photo retry, with a second old-generation job in each case. Both old jobs cancel and save, published actual edits/history/checkpoint remain intact, new identity/connection survives polling, and stale old worker/HTTP results have no effect. The failed-stop-delivery regression still closes only its current owning connection after local job removal.
- **No cross-library saves:** stale conversation scope refuses archive writes; invalid/locked destination library leaves the running task alive; a successful library change archives the original conversation and cancels it while the old library still owns it. Reopen retains the history. The existing before-change path and runtime generation resynchronization preserve distinct library ownership, including unadmitted jobs.
- **v1 compatibility and storage combination:** schema version and core catalog format remain unchanged, old messages are not fabricated/upgraded, additive local IDs survive persistence, and model history removes that local field before projecting the existing v1 input. The combined existing save/reopen/clear test verifies only the conversation's attachments are removed. UX-02 native/Mem bounded reads and idempotent deletion, faults, alias handling, and archive persistence regressions also pass after this merge.

These are automated behavioral and inspected-source evidence for this prefactor, not a claim to have completed the later full A03/A05/A14 matrices. No layout, widget ID, localized label or dynamic format key changed, so screenshot acceptance is not applicable. No paid-model run was used.

## Actual combined checks

All cargo tests used the prescribed shared integration build target and fonts:

```sh
CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration
CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts
```

| Command | Actual result | Log |
| --- | --- | --- |
| `cargo fmt --all --check` | Passed after conflict resolution and after tests | No diagnostics |
| `git diff --check`, `git diff --cached --check` | Passed while resolving/staging | No diagnostics |
| `cargo test -p lightcraft-catalog store::tests --lib -- --nocapture` | **15 passed**, 0 failed; no unavailable-fixture message | `merger/01-store.log` |
| `cargo test -p lightcraft-engine companion --lib -- --nocapture` | **21 passed**, 0 failed, including all 7 current archive regressions | `merger/01-engine-companion.log` |
| `cargo test -p lightcraft-ui-egui companion --lib -- --nocapture` | **13 passed**, 0 failed; current UI/engine library compilation succeeded | `merger/01-ui-companion.log` |
| `git diff --check 3eb9411be3837b25c67a25e855ced1a9c0e80988..HEAD` | Passed after merge commit | No diagnostics |
| `git status --porcelain`, `git show -s --format=... HEAD` | Clean integration worktree, final SHA and both merge parents verified | Recorded above |

No selected integration test failed or was skipped. UX-02's preceding full catalog/journal/background suite (207 passed with its exact independently established lock-stress baseline exclusion) and engine reset-filter suite (38 passed) remain recorded in `02-merged.md`; Store/journal and Reset implementation were not changed again, so they were not repeated broadly. Both approved workers already recorded affected strict no-deps clippy, format and layer checks; the tiny interface reconciliation is covered by the compiled merged UI tests. No redundant lint, platform or comprehensive ticket-22 run is claimed. The unchanged full-dependency clippy baseline is recorded in `02-baseline-clippy.log`.

## Outcome and limits

**Assigned UX-01 merge and combined affected validation complete.** Root owns tracker resolution. Global admission, leases, request queue cancellation, format2 storage, drafts, cards/compare/revert and final v2 durable guarantees remain later tickets. This merge preserves the current v1 compatibility behavior and provides the intended small identity interfaces.

No tracker edits, pushes, PRs, original checkout mutations, `.env` reads, paid model calls or OS synthetic input. Shared integration target retained for the next assigned merge.
