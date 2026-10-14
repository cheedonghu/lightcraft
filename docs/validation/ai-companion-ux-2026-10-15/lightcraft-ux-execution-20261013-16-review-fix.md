# UX-16 independent fixed-range review

Repair range: `a41331f9cc77a5290cc5da4f3e36bb1c88a66d0b..4ea742cb07b2388057d3f0ea75ceae56593301dd`.
Whole originating range: `72717addc30f439d753989512674c0e800866cbc..4ea742cb07b2388057d3f0ea75ceae56593301dd`.
Checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-16/lightcraft`.
All source references below are relative to that checkout. Exact HEAD, clean status and approved base ancestry verified. No candidate/index/HEAD mutation, subagents, merger action, push, .env, paid models, OS input or user photographs.

Read the updated 16 implementation cycles15–19, authoritative final handoff/checks, prior review and scope rulings. Earlier whole-range requirement/source review remains applicable; audited the full repair diff and actual candidate, execution-time capture, persistence, native/fallback projection, paint and loading/read-position callers. No code was merged from the unapproved09 candidate.

## Accepted findings: all addressed

1. **Same-count Color Range values:** `crates/ui-egui/src/ai_companion/card.rs:414` now formats each actual sampled OkLab triple rather than array length, labels its unit OkLab, and preserves it through the shared composition interface. `capture_pair` uses this same composition for proposal and publication at `ai_companion.rs:731` and :1005; therefore new frozen display and compatible persisted history both receive the fix. The actual supported candidate→Session→Headless test asserts the independent `(0.50, 0.10, 0.10) → (0.50, -0.10, 0.10)` literals, not an internal card function or assumed effect. The current test execution passes and the refreshed ZH screenshot shows the complete wrapped values plus localized sample label.

2. **Combination and component inversion:** `card.rs:340` and :346 add separate stable op/component_invert rows; shape inversion remains separate. Actual update_manual_mask still replaces an existing Subtract/inverted component with Add/non-inverted at engine companion.rs:271; the card now records those dependent changes. The supported-candidate public persistence/status/paint test observes Subtract→Add and On→Off in EN and their ZH translations. Source audit verifies the same rows are frozen during actual execution, rather than a legacy-only display fix.

3. **Zero chunk layout:** `crates/engine/src/companion/storage/pagination.rs:58` validates the captured Entry before returning it. Both fallback DisplayScan and native rounds_display/message_round_id obtain their Entry through this interface, so zero payload cannot reach their modulo/division. The actual fault test publishes a checksum-consistent malformed index/manifest/HEAD only into an isolated MemStore, checks Session fallback and synchronous/native reader errors, and then checks valid messages survive actual UI reading with both adapter modes. This is a real caller/filesystem seam; no private Entry assertion or catch-and-ignore workaround. Existing separate range validation remains intact.

4. **Explicit legacy degradation:** `pagination/scan.rs:291` emits a bounded stage availability marker when a legacy settings field is discarded at the unchanged64KiB limit. `card.rs:159` suppresses the warning only when the frozen before/target display arrays are present; :186 shows the localized notice independently of parameter expansion. Applied/not-applied facts and readable messages remain. The native/fallback old-schema2 large-brush + exposure scenario now reports parameterDetailsUnavailable=true and paints the EN/ZH notice. No cap increase, whole-value reconstruction, source/WB recomputation, or inferred execution state was added. The parser's selected1MiB, source256KiB-step, stage/depth/key budgets remain unchanged.

5. **ZH retained categorical value:** `card.rs:218` routes actual retained strings through tr(), matching before/target. The failed-proposal public disclosure test requires standalone 关闭 in ZH and excludes standalone Off; refreshed screenshot shows the complete target/retained wording. Existing numerical strings stay unchanged. Locale changes are scoped additions; existing Invert Component/Add/Subtract/Intersect translations are reused.

## Strengths and regression audit

The repairs stay within the existing card/display module and common entry interface. No UI docking, paging request identity, worker cancellation, accepted-page bookkeeping, draft ownership or scroll restoration logic was altered. The original whole-range facts remain intact: one card per edited round, pure answers omitted, absolute initial/correction values, immutable effective RAW/relative WB, published result truth despite failed receipt, separate current-state indication and ordinary History navigation without restoration.

The complete current suite re-exercises exact old fragment ownership, giant ignored-field fallback continuation/hash verification, explicit native projection errors preserving messages, native worker/cold feedback and request retirement,12 draft/read cursor/pixel/follow-tail isolation, and all12 normal/narrow/floating120/240/stacked120/240 EN/ZH card layout cases. Loading readiness still depends on actual accepted request/card completion; physical busy is not cleared early. New masks/availability values use existing clipped disclosure widgets and do not introduce a new canvas hit path.

Directly inspected freshly worker-generated screenshots: sample-zh-hans, combine-zh-hans, legacy-detail-true-zh-hans, failed-category-zh-hans and card-float120-zh-hans. Normal screenshots show readable changed values/localized notice; compact screenshot shows a scrolled portion. Complete literal reachability across all12 layouts is supported by the passing bidirectional wheel/full-galley-bounds regression, not a claim everything fits120px simultaneously. Did not independently generate new screenshots or claim physical compositor coverage.

## Issues

Critical: none remaining found.

Important: all four prior findings resolved; no new Important finding.

Minor: prior categorical-actual localization finding resolved; no new concrete Minor finding.

## Checks actually executed by reviewer

- Existing updated worker-built UI test binary `/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-16/debug/deps/lightcraft_ui_egui-d7241b0d37537103 ai_companion::`, normal test threads: **115 passed,0 failed,0 ignored**. `16-fix-review-ai-tests.log`. Includes all five named repair tests and original12 geometry/full-galley tests.
- Existing updated engine test binary `.../lightcraft_engine-fe98cba74e750162 companion`: **56 passed,0 failed,0 ignored**. `16-fix-review-engine.log`.
- Same UI artifact `i18n::`: **30 passed,0 failed,0 ignored**. `16-fix-review-i18n.log`.
- Parsed all18 locale JSON files; whole-range git diff --check passed; exact HEAD/empty status/base ancestry verified.
- Source and screenshots inspected as described above. These are independent executions of existing built artifacts, not an independently rebuilt cargo certification. No own repository build/target was used.
- An attempted re-execution of earlier standalone public probes linked stale normal-library rlibs from the worker target. Those normal artifacts still contain old behavior, unlike the updated115-test binary. Both resulting logs are explicitly marked EXCLUDED and are not evidence about this fixed tree; no false green or current regression is claimed from them. Current verdict rests on fixed source, correctly updated test artifacts and fresh screenshots.

Worker-recorded final source checks reviewed but not independently rerun: strict changed-crate no-deps clippy, fmt, layers, WASM compile. Initial new-test err_expect lint was honestly corrected, then final checks/test binary were rerun; no suppression. Prior parity reference check remains its actual earlier evidence, not a fabricated new run. No full-workspace/native-compositor/all-platform/provider/power-loss certification inferred.

## Declined to judge / required future work retained

- **09/16 real HistoryView retention remains required.**09 is still unapproved while its second fix addresses implicit Archive drop/release scheduling, owning Windows namespace routing and stable MemStore identity. This16 tree has no HistoryRetention consumer. Whichever merges later must bind Session::retain_companion_history(&PhotoHeader) BEFORE first fallback step and hold it through inactive pauses/successive scans until actual view retirement, while native guards follow physical drop and correct Store identity. Root must verify this actual combination; no claim it is already implemented or waived.
- **15 reliable bounded unfinished-activity summary and all actual shutdown/switch retirement remain required.** Current card selected whitelist is not a complete recovery summary;15 must extend the shared parser with its real association fields after approval. This review's legacy parameter notice does not satisfy or replace15.
- **17/18 explicit bound revert, durable original-content/version proof, non-requalification after equality/Undo/Redo, modified-state work and Q30 fail-before-edit gate remain separately required.** Card hash/binding and ordinary History navigation do not grant eligibility.
- **19 actual historical before/after preview mode remains separately required.** Typed stored preview identity is not an implemented historical-view consumer.
- **10 full streaming migration/backup,13 confirmed clear/epoch path,14 outer-container repair,20 health/global entry and21 rich long reading remain separately assigned.** Current16 scanner errors/parameter degradation preserving messages were reviewed and not deferred to those tickets.
- **22 paired native acceptance matrix and honest platform reporting remain required.** No universal camera/RAW corpus, other-platform runtime, paid provider, physical power-loss or OS compositor certification is inferred from current procedural macOS/headless evidence.

## Assessment

**Ready to merge: Yes, for current UX-16 scope at4ea742cb.** All accepted blocking/minor findings are fixed at the actual caller interfaces, relevant independent test-artifact executions pass and no regression was found. Approval does not authorize merger action by this reviewer and does not certify the unimplemented09/16 retention combination or any retained future assignment. Root must verify the combination on whichever branch merges later.
