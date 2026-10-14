# UX-16 integration merge certification

Worker: `codex/ux-16`, reviewed clean commit `4ea742cb07b2388057d3f0ea75ceae56593301dd`.
Integration: `codex/ai-companion-ux` in `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
Previous integration HEAD: `72717addc30f439d753989512674c0e800866cbc`.
Merged integration HEAD: **`03f8cdb164d566d0809a13ad23508c69578b5faa`**.
Merge title: `UX-16: merge truthful round result cards and bounded history display`.

Entered the separately authorized merger role only after independent final review16-review-fix.md approved4ea742cb. Read merger contract/execution/context/rulings and both repositories’ AGENTS/collaboration instructions. Worker was clean, included actual72717add, and integration was clean at that same base. A normal no-ff merge completed without conflicts or additional source changes. `git diff --exit-code 4ea742cb HEAD` proves the merged tree is exactly the reviewed tree; worker/base ancestry and final clean status verified. No09 WIP or other ticket candidate merged.

## Behavior and review evidence

The full review and correction review establish one truthful card per recorded editing round, absolute initial/correction values, published-vs-proposed/retained truth including failed execution confirmation, frozen effective RAW/relative WB and associated changes, exact historical message/fragment ownership with bounded native/fallback continuation, read-only ordinary History navigation, and explicit bounded legacy parameter degradation. All four accepted Important findings and the concrete ZH retained-value Minor are addressed in this exact tree.

Real shared Tools tests cover EN/ZH normal/narrow/floating120/240/stacked120/240, full parameter literal galley bounds and bidirectional scrolling, clipped controls/canvas separation, plus12 draft/read cursor/pixel/follow-tail and cold-feedback/native result acceptance/cancellation regressions. The actual updated candidate tests cover OkLab samples and mask combination/inversion, common chunk-layout errors preserving valid message slices, legacy parameterDetailsUnavailable notice and translated categorical actual values.

Worker regenerated and inspected all12 base layout images and10 affected EN/ZH images. Independent review inspected the affected sample/combine/legacy/failed-actual ZH images and compact120 representative; root accepted these screenshots. Their evidence remains valid because integration tree equals the reviewed candidate. No duplicate screenshot generation, physical compositor or live desktop/provider claim.

## Checks executed from this integration tree

Cargo environment for all build/test/xtask commands:

`CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`

Normal test parallelism retained. Cargo ran from the integration checkout and recompiled its changed engine/UI code; results below are fresh integration executions, not stale worker artifacts.

| Command | Actual result | Evidence under `/tmp/lightcraft-ux-execution-20261013` |
| --- | --- | --- |
| `cargo test -p lightcraft-engine companion --lib` |56 passed,0 failed/ignored; test5.18s |16-merge-engine.log |
| `cargo test -p lightcraft-ui-egui ai_companion:: --lib` |115 passed,0 failed/ignored; test19.44s |16-merge-ui.log |
| `cargo test -p lightcraft-ui-egui i18n:: --lib` |30 passed,0 failed/ignored; test1.53s |16-merge-i18n.log |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` |passed |16-merge-clippy.log |
| `cargo fmt --all -- --check` |passed |16-merge-fmt.log |
| `cargo check --target wasm32-unknown-unknown -p lightcraft-engine -p lightcraft-ui-egui` |passed compilation, documented cfg warnings remain |16-merge-wasm.log |
| `cargo xtask layers` |31 crates,no violations |16-merge-layers.log |
| `cargo xtask parity` |all command/control/path references valid;531 in-scope rows |16-merge-parity.log |

Additional targeted ordinary editing regressions used the freshly compiled integration engine artifact `/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration/debug/deps/lightcraft_engine-fe98cba74e750162`, exact filters with normal harness settings:

- `tests::reset_edits_restores_a_raws_import_defaults --exact`:1 passed.
- `tests::versions_history_and_reset --exact`:1 passed.
- `tests::undo_and_redo_show_the_photo_they_change --exact`:1 passed.

Evidence16-merge-edit-smoke.log records each actual nonzero test count. Also parsed all18 locale JSON files; `git diff --check`, exact-tree equality, base/worker ancestry and final empty `git status --porcelain` passed. No failed/skipped current check, baseline exemption or test removal was needed. No full workspace rerun claimed.

## Required follow-on combinations and limits

- **09/16:**09 remains unapproved while fixing implicit Archive drop/release scheduling, owning Windows namespace routing and stable MemStore identity. No HistoryRetention consumer is falsely claimed here.09, as the later integration, must bind the actual16 HistoryView via Session::retain_companion_history(&PhotoHeader) BEFORE its first fallback step, preserve that guard across inactive pauses/successive scans until actual view retirement, and preserve native physical-drop/correct Store identity semantics. Root must verify that real combined caller; no WIP merged/no waiver.
- **15:** reliable bounded unfinished-activity summary and complete actual shutdown/switch retirement remain required. It must extend the shared selected parser with real recovery association fields; current card projection/legacy notice is not a completed lifecycle recovery interface.
- **17/18/19:** explicit bound revert, durable source/version proof/non-requalification/Q30, modified-state work and actual historical preview remain their separate required assignments. Card binding/hash/History navigation grants no qualification or restoration.
- Remaining migration/clear/repair/health/global-entry/rich-reading and22 comprehensive paired native acceptance scopes remain required as previously ruled. No tracker status changed by this merger.

The WASM compile emits inherited/documented warnings: codec1, preview1, engine1, UI4. Native strict changed-crate lint passed; no strict-WASM or browser-runtime certification. Reader limits remain engineering input/work bounds, not wall-clock or allocator guarantees; native cancellation may wait for an arbitrary large single record off-frame. Existing reports retain RAW corpus/other-platform/provider/physical power-loss/compositor limitations. Tests use procedural/temporary fixtures, no user photos, paid models, .env or OS synthetic input.

**Handoff:** integration is clean at03f8cdb164d566d0809a13ad23508c69578b5faa; all assigned merge checks passed. No push, PR, original-checkout edit, worker cleanup or tracker resolution performed. Root owns resolution and subsequent worker cleanup.
