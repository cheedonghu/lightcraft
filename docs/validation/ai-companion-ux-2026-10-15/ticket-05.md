Status: resolved
Type: task

# 05: 统一修改标记、角标与已修改筛选

**What to build:** 照片当前参数偏离整体Reset基准即显示修改标记，并在网格、胶片条和筛选中一致呈现。

**Blocked by:** None（可立即开始）

**Triage:** ready-for-agent；仅当阻塞任务全部resolved后可认领。

## Acceptance criteria

- [x] 默认导入外观/RAW白平衡/系统归一化不误判，实际Custom白平衡不漏判。
- [x] 虚拟副本沿用原图基准，手动/AI/复制/同步及Undo/Redo重新计算。
- [x] 修改图标默认常驻且尊重主动关闭，元数据不产生编辑标记，其他角标策略保留。

## Validation

验证本任务完整用户路径，完成受影响自动化检查并记录命令、环境和结果；界面改动同时包含中英文、动态格式键、稳定控制入口及必要截图。只完成类型或组件不能作为本任务交付。未执行项不记通过。

## References

[客户端spec](../../../../photo-companion/.scratch/photo-companion-v1/client-spec.md)、[设计定稿](../../../../../../docs/ai-companion-client-ux-design.md)、[实施方案](../../../../../../docs/ai-companion-client-ux-implementation-plan.md)。验收对照：A11。

原首版阶段对照：14；该对照不是阻塞依赖，不自动修改或关闭原阶段任务。

## Comments

2026-10-13：用户确认推荐拆分并授权发布。本工单已发布，尚未认领、实现或验收。

2026-10-13：自动执行已认领，工作分支codex/ux-05；采用用户授权的常规TDD替代缺失tdd技能。

## Answer

# UX-05 integration merge evidence

Assigned branch: `codex/ux-05`, final approved candidate **`edf6a18d2534dffa61d4c68fdbcf00c806797caf`**.

Integration: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`, branch `codex/ai-companion-ux`.

Pre-merge integration HEAD: `7de70b393031298db29bcf4b81f028e5422a1c40` (UX-01 and UX-02 integrated).

Final integration HEAD: **`fe9133a10e5aea51aabd15a138d9645c28ecf937`**, `UX-05: merge source-aware modification state and persistent badges`.

Merge parents are exactly the preceding integration HEAD and assigned worker candidate. `git merge --no-ff --no-edit -m 'UX-05: merge source-aware modification state and persistent badges' codex/ux-05` completed without conflicts or merge-only source changes. UX-03 and UX-07 were not merged.

## Preconditions and final-candidate inspection

Read UX-05 ticket, implementation report, independent fixed-range re-review, and merger/context contracts. Inspected product changes in catalog Reset/edited comparison, shared develop WB normalization, pipeline re-exports/control conversion, engine Reset, grid/filmstrip badges, and command behavior tests.

Worker was clean at the exact assigned SHA. Verified the current integration HEAD is an ancestor of worker HEAD. The candidate's parents are documentation commit `a44a3b522ed9e9d431e369fd1f83bbc65aafd4b3` and current integration `7de70b393031298db29bcf4b81f028e5422a1c40`. Independently ran `git diff --exit-code 531f1bd563c8b0d32b718ae18ac706b1d43541e3 codex/ux-05 --` for all ten UX-05 product/test files: **no differences**. Thus the independent product approval remains applicable after the worker's integration merge.

The extra documentation change updates four existing parity rows' evidence/notes (RAW defaults, edited filters, grid, filmstrip), Last updated, and revision history. Statuses, tiers and summary counts are unchanged in the inspected diff. Added paths/commands describe implemented source Reset, logical WB comparison, shared queries and explicit badge settings; no new feature-status or AI provenance claim is introduced. Integrated `cargo xtask parity` validates every referenced command/control/path.

## Acceptance evidence

- **Logical source Reset and WB:** seven catalog edited tests cover RAW defaults, import look, Custom WB changes, AsShot reference refresh, process bookkeeping, relative/Kelvin source facts, metadata, and retained disabled settings/masks. Nine engine preference tests include command-level neutral and non-neutral Legacy Custom baseline conversion, genuine temperature/tint changes, manual return, intentional Custom Reset preservation, and ordinary/text/smart-rule edited queries.
- **Consistent recomputation:** command regression `modification_badges_follow_source_reset_copy_filters_and_history` exercises actual RAW/rendered photo changes, virtual copies inheriting the source baseline, Reset, Undo/Redo, copy/paste and sync. The predicate depends on current develop settings rather than conversation or edit provenance. Existing AI real candidate commits, correction, stop and round undo remain green on the combined code; their operations feed the same catalog predicate without added provenance state.
- **Original blocking bug independently checked:** recompiled `/tmp/lightcraft-ux-execution-20261013/05-review-probe.rs` against freshly built merged libraries and ran it. Output is:

```text
initial edited false WB WhiteBalance { mode: Custom, temp: 6500.0, tint: 0.0, scale: Legacy }
same effective Custom WB edited false WB WhiteBalance { mode: Custom, temp: 4986.0, tint: -2.0, scale: Kelvin }
```

  An initial probe link selected mismatched Cargo feature artifacts and failed at compile time; corrected by selecting catalog/develop artifacts using the chosen engine's Cargo dependency fingerprints. The successful rebuilt probe uses matching integration libraries, not the old reviewer executable. Build and run logs are `merger/05-wb-probe-build.log` and `merger/05-wb-probe.log`.
- **Rendering compatibility:** actual pipeline RAW WB render equivalence tests pass, the normalization regression proves a relative conversion occurs exactly once, and the engine legacy RAW test retains the old look while learning the source's AsShot white. Shared arithmetic moved below pipeline/catalog without a new reverse dependency; pipeline public conversion entry points remain re-exported.
- **Default-visible UI, explicit opt-out and localization:** the rebuilt integration headless badge test passes visibility without selection and respect for grid Never/filmstrip disabled settings. Stable widget IDs are `badge:modified:grid:<id>` and `badge:modified:film:<id>`, with existing bilingual `Has Edits` tooltip. Independently opened/inspected all three supplied screenshot files with `view_image`: `05/grid-en.png`, `05/grid-zh.png`, `05/film-zh.png`. Photo 5 (`LC01235`) has a visible sliders modification icon while Photo 2 is selected in both English/Chinese grids and Chinese filmstrip; Chinese glyphs and ordinary metadata badge layout render correctly. These screenshots show the approved unchanged product/UI code; the actual merged headless test confirms the same consumers. No extra screenshot replay was warranted by a merge conflict or new UI concern.
- **Earlier tickets preserved:** integrated Store native/Mem bounded/range/delete/alias/fault tests, seven engine archive tests within the companion filter, and thirteen UI companion tests pass. Scope/save isolation, stopped HTTP fallback, command-before-poll old-generation retirement, v1 history projection, actual initial/correction results, reopen/clear, and complete round undo remain working.

## Actual merged checks

Cargo commands use:

```sh
CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration
CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts
CRAFT_FONTS_REQUIRED=1
```

| Command | Actual result | Log |
| --- | --- | --- |
| `cargo fmt --all --check`, `git diff --check` | Passed before and after checks | No diagnostics |
| `cargo test -p lightcraft-catalog model::edited_tests --lib -- --nocapture` | **7 passed** | `merger/05-edited.log` |
| `cargo test -p lightcraft-catalog store::tests --lib -- --nocapture` | **15 passed** | `merger/05-store.log` |
| `cargo test -p lightcraft-develop -p lightcraft-pipeline white_balance --lib -- --nocapture` | **2 develop + 2 pipeline passed** | `merger/05-white-balance.log` |
| `cargo test -p lightcraft-pipeline normalize_wb --lib -- --nocapture` | **1 passed** | `merger/05-normalize.log` |
| `cargo test -p lightcraft-engine tests_prefs --lib -- --nocapture` | **9 passed** | `merger/05-prefs.log` |
| `cargo test -p lightcraft-engine reset --lib -- --nocapture` | **39 passed** | `merger/05-reset.log` |
| `cargo test -p lightcraft-engine old_relative_white_balance_on_a_kelvin_camera --lib -- --nocapture` | **1 passed** | `merger/05-legacy-render.log` |
| `cargo test -p lightcraft-engine companion --lib -- --nocapture` | **21 passed** | `merger/05-engine-companion.log` |
| `cargo test -p lightcraft-ui-egui modified_badges_stay_visible --lib -- --nocapture` | **1 passed** | `merger/05-ui-badge.log` |
| `cargo test -p lightcraft-ui-egui companion --lib -- --nocapture` | **13 passed** | `merger/05-ui-companion.log` |
| `cargo xtask layers` | **31 crates, no violations** | `merger/05-layers.log` |
| `cargo xtask parity` | **Every referenced command/control/path exists** | `merger/05-parity.log` |
| Recompiled original Legacy WB probe | **Passed, equivalent effective WB remains unmodified** | Probe logs above |
| `git status --porcelain`, `git rev-parse HEAD` | **Clean**, final SHA confirmed | Recorded above |

All selected tests passed with no test failure or ignored case; filters intentionally limit testing to affected behavior. Test builds and parity's real CLI command/control-registry build also succeeded. No broad workspace/lint repetition was needed after already-approved strict worker checks. Known catalog lock-stress and dependency/pipeline-clippy exceptions remain exactly those recorded at unchanged original baseline; these filtered integration tests do not claim a new full suite or strict dependency-clippy pass.

## Outcome and limits

**Assigned UX-05 merge and combined affected validation complete.** Root owns tracker resolution and worker cleanup. No claims of full Lightroom/RAW parity, live desktop run, paid-model acceptance, all-platform execution, or completion of later AI UX tickets. Screenshot inspection and actual merged headless behavior are recorded separately.

No tracker edits, original checkout mutations, push, PR, `.env` reads, paid model calls or OS synthetic input. Integration build target retained for subsequent assigned work.
