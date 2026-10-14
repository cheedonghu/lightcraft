Status: resolved
Type: task
Blocked by: 04, 07

# 11: 停止后仍可发送完整合法历史继续对话

**What to build:** 用户停止工具处理或遇到方案校验失败后，下一轮仍能携带全部有效历史继续，工具结果与真实执行事实正确配对。

**Blocked by:** [04：停止与撤销同意真正阻断未发送内容](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/04-stop-and-consent-cancellation.md)、[07：新会话从提交到保存、重开完整可用](../../../../photo-companion/.scratch/ai-companion-client-ux/issues/07-new-conversation-persistence.md)

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 调用编号唯一、结果顺序合法，完整工具调用不悬空；取消/失败不补成成功。
- [x] 方案提出与实际执行分别记录，未完成助手回复不进入下一轮。
- [x] 提交前有界校验全量消息/图片和请求预算，超限明确拒绝，不截断或静默省略。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A02、A10。

原首版阶段对照：12；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，采用用户授权的常规TDD。

## Answer

# UX-11 authorized integration merge

Worker reviewed commit: **85ca5380e43efbe4752b3e5a2877905948212441**, branch `codex/ux-11`.

Previous approved integration: **bbe911ab3f62c4bab1133e3c888fc184cfdbb914**.

Merged integration HEAD: **66b6b739b5db6c7fc7f6b2fc59ac5dd743681721**, branch `codex/ai-companion-ux`, worktree `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.

Root explicitly accepted `11-native-review.md` and separately authorized merger role. Both branches were clean before merging; the worker contained the latest approved integration ancestor and its native combination with approved UX-08. `git merge --no-ff codex/ux-11` completed with no conflicts. The resulting merge tree exactly equals the reviewed85ca tree; no additional product changes or fixes were made. Final checkout/index are clean, and85ca is an ancestor of the exact merged HEAD above. No push, PR, tracker/backend mutation or original-checkout reset occurred.

## Behavior and acceptance

The complete UX-11 candidate now includes the mandatory approved UX-08 native combination. Native model preparation feeds one explicitly bounded record at a time from the captured reader and sealed tail into the shared lawful projector. The leased prompt and frozen DevelopSettings drive the exact current envelope/capabilities reservation before backend creation. Quotas count complete projected context plus current input, rather than raw stored records; no history trimming, fabricated attachments or complete-archive hydration was introduced.

Actual caller tests on the merged source prove malformed native pairing, current-envelope overflow and corrupt/foreign/missing preview refusal before creation, retained stored records and available ordinary editing after retirement. Cancellation during an actual FsStore preview read stops later reads and prevents old create/upload after reconsent, while fresh admission waits for producer retirement. Full multi-tool stop/server-failure and invalid-plan paths survive save/close/reopen and emit complete legal next-round context with actual outcomes and separate proposal/execution facts.

The unchanged approved UX-04 priority cancellation and first-feedback gates, UX-08 compact visible-history geometry/paging/latest cursor/fragment chronology, sealed-tail persistence and producer-completion ownership passed within the combined83-test AI suite. Future UX-09 storage receipt/retirement and UX-12 editor/read-position interfaces remain intact and required.

Existing inspected worker refusal screenshots `11/invalid-history-en.png` and `11/invalid-history-zh-hans.png`, plus approved UX-08 screenshots and actual headless paint/navigation regressions, support the shared Tools path. No layout changed in this ticket's native adaptation; no new screenshot inspection is claimed by this merge.

## Executed integration checks

All Cargo checks ran from the integration worktree with `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts` and `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`.

| Exact command | Actual result | Evidence |
| --- | --- | --- |
| `cargo fmt --all -- --check` | passed | merger tool result |
| `cargo test -p lightcraft-ui-egui ai_companion --lib` | **83 passed,0 failed**, normal parallel run | `merger/11/ui.log` |
| `cargo test -p lightcraft-engine companion --lib` | **51 passed,0 failed** | `merger/11/engine.log` |
| `cargo test -p lightcraft-codecs --lib` | **50 passed,0 failed** | `merger/11/codecs.log` |
| `cargo test -p lightcraft-ui-egui i18n --lib` | **30 passed,0 failed** | `merger/11/i18n.log` |
| `cargo clippy -p lightcraft-codecs -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | passed without exemptions | `merger/11/clippy.log` |
| `cargo check -p lightcraft-codecs -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown` | passed, existing unchanged warnings | `merger/11/wasm.log` |
| `cargo xtask layers` | **31 crates, no violations** | `merger/11/layers.log` |
| `cargo xtask parity` | passed | `merger/11/parity.log` |
| `git diff --check`, `git diff --cached --check`, final clean status and exact tree comparison | passed | merger tool results |

The WASM warnings remain in unchanged preview/pool DROP_WAIT, codecs/jxl_encode imports, engine/devices mutability and ui/lightroom_import imports/dead variants/method. These match the reviewed worker and previous integration warning scopes; strict native Clippy passes. No test failure or exemption was used.

The preceding independent review additionally reran the actual native emitted RoundInput Go validator probe successfully; copied and actual backend messages.go SHA256 match. The merged tree is identical to that reviewed source. This is serializer/image/tool compatibility evidence, not a new live HTTP/SSE/provider certification claim.

## Remaining assignments and limits

UX-09 pending/HEAD reconciliation, durable receipts and captured graph retirement/GC; UX-10/14 migration/outer recovery; UX-12–21 assigned draft/lifecycle/UI/revert work; and UX-22 actual provider/platform coverage remain required. This merge does not certify those future implementations. No full-workspace CI, physical compositor, universal image corpus, physical power-loss, Windows/Linux runtime or paid-provider validation is claimed.

## Result

UX-11 is merged cleanly at the exact HEAD above, with every requested affected integration check passing and no conflicts or product omissions identified. Root owns tracker resolution and the overall22 objective; this merger did not mark either complete.
