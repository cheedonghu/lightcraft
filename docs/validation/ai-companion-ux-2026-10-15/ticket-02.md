Status: resolved
Type: task

# 02: 预整理会话存储能力，保留旧读写行为

**What to build:** 为磁盘、临时图库和故障测试提供一致的有界读取、原子写入和删除能力，同时保持原有照片与会话保存路径可用。

**Blocked by:** None（可立即开始）

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 实际读取时限制大小，越界/不支持操作返回可理解错误。
- [x] 磁盘和临时图库具有等价删除语义，缺失删除幂等成功。
- [x] 受控目录及原子写满足持久化约束，旧会话读写和核心照片格式不变。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A09、A10、A14。

原首版阶段对照：12；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，工作分支codex/ux-02；采用用户授权的常规TDD替代缺失tdd技能。

## Answer

# UX-02 integration merge evidence

Assigned branch only: `codex/ux-02`, worker HEAD `82a2cdd29661f827aa87f82d83a05220a2c04617`.

Integration worktree: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.

Pre-merge integration HEAD: `363e5d592a64af41cefbe3ff0642ee347c61cf18`.

Merged integration HEAD: **`3eb9411be3837b25c67a25e855ced1a9c0e80988`**, `UX-02: merge bounded companion storage prefactor`.

Merge parents are exactly the pre-merge integration HEAD and approved worker HEAD. `git merge --no-ff --no-edit -m 'UX-02: merge bounded companion storage prefactor' codex/ux-02` completed without conflicts. No merge-only fix was necessary. UX-01 and UX-05 were not merged.

## Preconditions and acceptance evidence

Read merger/context/execution contracts, both repositories' instructions and collaboration rules, UX-02 ticket, implementation report, initial review, and independent fixed-range re-review. Inspected the final Store/archive/UI caller production diff and archive tests. Worker branch was clean at the assigned SHA; ancestry against the integration base passed. Worker report records merging that same then-current integration tip before final handoff; integration had not advanced since the report. Independent re-review cleared both Important native append durability/cache findings.

Merged behavior checks cover actual bounded Fs/Mem reads and ranges, explicit Unsupported without unbounded fallback, controlled relative namespaces, symlink rejection, fallible read allocations, native atomic-write preservation/abort cleanup, idempotent deletion, native alias/cache coherency, and append parent-sync retry. The complete catalog suite also exercised real filesystem journal roundtrip, torn append recovery, streamed snapshot format/preservation, and background snapshot behavior.

Engine companion tests passed native and temporary-library save/reopen/delete, bounded-only hydration with unbounded reads prohibited, attachment deletion failure/retry, unchanged core catalog snapshot/version/journal behavior, and existing companion rendering/sidecar regressions. UI companion tests compiled the mutable removal caller and passed public round undo, history invalidation after subsequent manual edits, conversation/preview reopen and clear, library-change archival, HTTP rejection classification, and stale candidate rejection after connection invalidation. No frontend layout, control ID, or localization change is present in this ticket, so screenshot acceptance does not apply.

## Actual integration checks

All cargo build/test commands used:

```sh
CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration
CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts
```

| Command | Actual result | Log |
| --- | --- | --- |
| `cargo fmt --all --check` | Passed before and after test checks | No diagnostics |
| `git diff --check` | Passed before and after test checks | No diagnostics |
| `cargo test -p lightcraft-catalog --lib -- --skip tests_lock::dropping_the_lock_frees_the_library_while_another_thread_spawns_processes` | **207 passed**, 0 failed, 1 ignored helper-process test, 1 exact baseline exclusion | `merger/02-catalog.log` |
| `cargo test -p lightcraft-engine companion --lib -- --nocapture` | **21 passed**, 0 failed, including all 7 archive tests | `merger/02-engine-companion.log` |
| `cargo test -p lightcraft-engine reset --lib -- --nocapture` | **38 passed**, 0 failed | `merger/02-engine-reset.log` |
| `cargo test -p lightcraft-ui-egui companion --lib -- --nocapture` | **6 passed**, 0 failed; affected UI library built successfully | `merger/02-ui-companion.log` |
| `git status --porcelain` and `git rev-parse HEAD` | Clean integration worktree; final SHA confirmed as above | No source/index changes after merge |

The catalog exclusion is the exact independently reproduced baseline failure at `363e5d592a64af41cefbe3ff0642ee347c61cf18`: `tests_lock::dropping_the_lock_frees_the_library_while_another_thread_spawns_processes` fails because the spawning thread never gets a child off the ground (`02-baseline-catalog-lock.log`). The ignored `lock_holder_child` is a helper process invoked by its passing parent test. No module-wide skip was used.

Affected-crate strict no-deps clippy, layering, and WASM checks were already green on the approved worker; they were not rerun for this single conflict-free merge, whose resulting tree contains only that approved branch. Full dependency clippy has the independently recorded unchanged `pipeline/src/spots.rs:250` baseline diagnostic (`02-baseline-clippy.log`). No additional platform or comprehensive ticket-22 testing is claimed.

## Limits and outcome

Windows power-loss directory durability and hostile external path-component swaps remain the approved reported limits. Format2, migration, paging, full-history isolation and transactional cleanup remain assigned later work. This merge preserves existing v1 storage and core photo formats.

**Assigned merge and affected validation complete.** Root owns ticket acceptance/status changes. No tracker edits, push, PR, original checkout mutations, `.env` reads, paid model calls, or OS synthetic input. Prescribed integration target retained for subsequent merges; no shared target cleanup was performed.
