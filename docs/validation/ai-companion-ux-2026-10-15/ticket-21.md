2026-10-14: Whole UX-21 completed, independently reviewed and fixed, then separately merged and freshly certified at client integration499e52b1da80187b4f2673066f751993211c6fc9 (codex/ai-companion-ux). Candidate7ee3545b6c1c2ff5f7c63e619157197c56000040, clean and exact-tree-equivalent to integration.

Ordered user/assistant/system/feedback rows, default folded tools, bold/list rendering, scoped bounded visible preview loading, missing-preview placeholders, independent upward scroll and explicit Back to bottom preserve full model records. All three accepted authored-text/role compatibility findings fixed and independently reproduced green; released old unversioned empty/cropped hints verified native/fallback.

Fresh integration:153 AI UI,15 commands,85 engine +3 real child reader-lock probes,30 i18n, ordinary Reset/History/UndoRedo, docking3, shutdown1; strict lint/native/WASM/fmt/layers/parity all passed.24 new integration EN/ZH layout screenshots exported with actual glyph/control checks; representative images independently inspected. All16 commands exit0.

Whole10 single staged/rawPNG reader combination,15 all-exit waits,13 confirmed clear,18 durable authority,19 stored paired preview and22 final cross-end acceptance remain required assigned tasks. No paid-provider, power-cut, browser runtime or foreign platform certification claimed. Discarded never-released b7 version1 empty artifacts excluded by explicit root ruling; released/current supported reading judged and passed. Final22 archives selected actual evidence in Git.


---

> 2026-10-14 second review fix update: b7c3dfff was independently cleared for R1/explicitR2 but rejected for known assistant-without-display-kind omission. Clean7ee3545b6c1c2ff5f7c63e619157197c56000040 now fixes that accepted finding. Current exact evidence/ruling/153-test results are in `/tmp/lightcraft-ux-execution-20261013/21-fix-role.md`; original reviewer re-clear pending. Earlier candidate evidence below is historical, not current merge approval.

> 2026-10-14 fixed candidate update: initial757ca191 was independently rejected on two Important authored-text loss findings. Both accepted findings are fixed in clean b7c3dfff242a38ac5b1c3fd04452265167e31ef2; fresh independent re-clear pending. See `/tmp/lightcraft-ux-execution-20261013/21-fix.md` for exact current evidence, genuine red→green cycles, old persisted hint compatibility, final152-test combination and current boundaries. Initial evidence below remains historical, not approval of the superseded candidate.

# UX-21 implementation — clean review candidate
Base: 32eebdbdc7a71456f7678230a85af8ae07ed8c4a; clean codex/ux-21.
Installed implement-spec, tdd/tests/mocking and codebase-design skills read. Existing approved seams reused.

## Cycle 1 — chronological readable roles and folded tools
Seam: caller-used Session.save_companion_tail and actual Headless ai.open/widgets/paint. Catches displayed user/assistant/system/local execution distinctions, basic bold/list text, tool payload hidden until explicit expansion and unchanged saved model records. Misses OS compositor and real paid-provider behavior. Independent literal expected labels/text, real MemStore adapter; no private State mutation.
Actual red: cycle1-red.log, test failed on missing role label `You` in actual painted output; original flat label omits system/execution roles and exposes Markdown delimiters. Minimal renderer now walks existing chronological HistorySlice.items, renders role groups/basic semibold/list layout and caller-visible collapsed tool headings. It reads existing bounded records without producing/changing model history. First incremental green is running; no pass claimed yet.
Interface: reading::show(ui, caller-owned Conversation) hides role/Markdown/fold ordering; no hypothetical transport or test-only interface introduced. Exact cards remain separately rendered by accepted16/17 caller.
Cycle1 green: 1 passed, 0 failures, 0.86s (cycle1-green.log).

## Cycle 2 — intentional upward reading and explicit bottom action
Seam: actual Headless ui.scroll/widgets plus caller Session.companion_editor and real injected protocol transport. Catches active streamed text + completed message/terminal preserving deliberate upward scroll, explicit bottom re-enabling following, independent saved page identity and editor. Misses physical mouse/window-system delivery. Stored independent numbered literal history and actual protocol deltas; no private State assertions.
Cycle2 initial green attempts exposed egui smooth wheel inertia being interpreted over the neighboring return button because pointer rectangle was expanded vertically. Intent now checks exact history y range, including only the horizontal scrollbar strip. Green3 passed but audit found the test terminal kind incorrectly authored as round.completed; exclude that run as terminal coverage. Corrected to actual round.ended/terminal_state=completed with public busy=false assertion; corrected rerun required before claim.
Corrected Cycle2 green4: actual message.completed and round.ended/busy=false passed, 1 test/0 failures. Upward position and follow=false remain stable; explicit caller bottom action reenables following. Native reader completion now awaited through public historyLoading rather than a guessed count of frames.

## Cycle 3 — visible native attachments, isolated missing preview
Seam: Session.save/read plus actual Headless display and real temporary FsStore adapter with read counters. Catches actual encoded preview loading only for visible rows, bounded image reads, missing attachment placeholder without losing neighboring text/valid image or changing stored records. Misses physical power loss/provider image quality. Independent procedural JPEG colors and literal available/unavailable widget/text outcomes; file deletion only inside owned temporary library.
Cycle3 red2 genuinely failed: no preview/placeholder appeared (11.55s). Green1 displayed correct local missing + valid neighbor, then independently bounded-read assertion caught 18 intermediate images during animated initial tail restoration. Fixed initial/explicit saved-tail positioning to nonanimated egui cursor alignment (also avoids f32::MAX layout precision failure), and pending visible requests now retire when their rows leave the viewport. Generation changes cancel old producer and clear cache/pins without pretending producer has retired.
Cycle3 green2 passed: actual native visible valid image + local missing placeholder, <=8 preview reads and independently known offscreen picture10 never read, unchanged exact message records; 3.45s.

## Cycle 4 — oversized records preserve roles and default tool privacy
Seam: public Session storage/page + actual Headless UI history navigation. Catches giant tool text fragments remaining folded and distinct from later assistant text, chronological forward reading, same bounded records retained. Misses arbitrary OS memory instrumentation; existing byte-bounded HistorySlice and captured producer interfaces remain the storage limits. Independent 440KB literal tool payload, explicit tool widget and assistant text expected.
Cycle4 red showed oversized tool payload painted raw. Green passed (1.64s): verified giant tool remains collapsed, forward page shows actual later assistant role/semibold/list, exact all records unchanged. Native selected metadata verifies whole record digest without full giant materialization; fallback selected owner continuation keeps existing09 view retention and one <=256KiB logical step/frame.

## Cycle 5 — intermediate fallback rows and default status privacy
Seam: actual fallback Store capability, Session.save/page and Headless frame/ai.status. Catches unverified role-scan intermediate frames remaining folded and default status text also excluding raw tool/execution fragments; checks successive real continuation frames without private state toggles. Misses native OS scheduler preemption; native completion publishes only after verified metadata.
Cycle5 actual red default ai.status exposed raw intermediate tool fragment; minimal verified-role filter fixed it. Green passed (0.77s), including several real fallback continuation frames and final folded tool widget. No default paint or status tool JSON, stored full record retained.

## Cycle 6 — existing legacy and runtime inline previews remain viewable
Seam: caller Session.save_companion_conversation, actual legacy Library attachment writer and actual Headless rows. Catches valid old referenced JPEG and unsaved/runtime inline JPEG shown from their own bytes, no current-photo substitution; ordinary source/model records preserved. Misses unsupported custom Stores without owned reader, which remain locally unavailable rather than blocking frame I/O.
Cycle6 initial fixture used save_companion_conversation on a fresh v2-capable store with a legacy path; it correctly rejected that invalid v2 preview key before display. Exclude as feature red. Corrected fixture uses the existing public legacy set_photo_companion interface on a fresh library (and in-memory session), proving real known-v1 references instead.
Cycle6 genuine red2: valid known legacy reference never displayed (10.07s). Minimal owned reader/inline decoder green passed (0.88s): native legacy and runtime inline own recorded pixels displayed, photo parameters unchanged. New caller-used engine interfaces: companion_attachment_reader(photo), CompanionAttachmentReader.read(image), into_retention(); bounded single reference read, photo/path/hash/strict codec verification, no root load/scan. Actual Store reader drops before worker alive=false, OS-lock-only guard retained with decoded view. Cache binds generation/photo/epoch/captured transaction + exact descriptor fingerprint.

## Cycle 7 — bottom of oversized final message
Seam: actual ai.history/ai.bottom/widgets plus Session editor/full stored records. Catches final literal of last oversized message actually reached, Unicode/JSON escape boundaries preserved, draft and model record unchanged, following reenabled. Misses arbitrary physical timing; each continuation remains bounded at existing store/native reader seam. Independent UTF-8/escaped literal tail and draft.
Cycle7 actual red: bottom stayed on first giant fragment (10.59s). Green passed (0.85s): one last-record bounded continuation at a time reaches final Unicode/escape-safe fragment, preserving draft/full model message and following. Existing slice decoding is reused rather than inventing suffix/escape slicing. Verified selected metadata is reused during same captured view's continuation, so giant metadata is not repeatedly materialized/scanned. Intentional history navigation/upward input cancels automatic continuation, native producer must actually retire before the sole next page launches.

## Cycle 8 — one upward wheel event immediately stops following
Seam: same already-approved actual Headless wheel/stream/terminal + Session editor test from Cycle2, strengthened from eight scroll events to one. Catches egui consuming wheel delta before caller records intent; misses physical device delivery. Independent saved editor.follow_tail=false after one deliberate upward event, then completed response and explicit bottom.
Cycle8 first strengthened run unexpectedly already green after eight inertia-settling frames. Do not claim a red. Further sharpened to assert true initial following and immediate false directly after the single real wheel frame, before settling, isolating intent timing from later offset exhaustion.
Cycle8 red2 audited invalid: Headless raw_input_hook delivers synthetic pointer moves one event/frame, so the immediate assertion was before the queued wheel had actually arrived. Exclude as genuine product red, and exclude green1/3 failures based on that fixture. green2 was only a compile error using absent InputState.raw_scroll_delta, also excluded. Corrected to pointer-move frame followed by wheel frame. Saved only owned candidate externally and reverted only the intent block to prior production behavior for a genuine corrected red; all other 7 current slices remain intact.
Cycle8 corrected-red actually already passed (0.83s) with prior production intent code; no genuine timing regression established, so speculative raw-input changes were not retained. Added explicit public scroll_offset>1000 precondition to ensure the event tests an actually scrolled loaded history rather than a loading shell. This remains verification of prior behavior, not a claimed additional red/green fix.
Position precondition exposed an owned Cycle7 edit accidentally changing the ordinary initial-page fallback from last32 to last1 as well as explicit bottom. This made the timing fixture a one-message unscrollable page. Restored ordinary last32 pagination (required12); explicit bottom alone remains last1. Rerun corrected one-wheel intent test on genuinely long history before deciding whether any intent-code fix is needed.
Cycle8 real-long-red is genuine: restored last32 page, independently verified >1000px initial scroll, pointer frame then delivered wheel frame; immediate follow=false assertion failed (0.85s). Minimal production now uses actual delivered raw wheel event after egui consumes smooth delta, scoped to the visible message rect and current UI layer; scrollbar upward movement remains supported. No private flag/harness-only API.
Cycle8 real-green immediately stopped following but a later exact-offset assertion saw 7.9px of remaining genuine wheel inertia. Corrected the measurement to wait for four stable public editor offsets (bounded 120 real frames) before injecting stream/completion, instead of guessing eight frames. Immediate follow=false assertion remains, and stream/terminal exact-position assertion is unchanged.
Cycle8 real-green2 passed (0.92s): corrected one-wheel delivery immediately clears follow intent; real streamed completion/round.ended preserves settled position and explicit bottom restores following. All invalid event fixtures remain excluded.

## Cycle 9 — typed user requests retain their own text
Seam: existing Session.save_companion_tail and actual Headless paint/default ai.status. Catches a real display_kind=request whose user-authored text starts with the legacy execution-feedback prefix being misclassified as applied edits. Also checks user-authored photo-state delimiter before an actual appended JSON state remains visible. Misses arbitrary legacy records without explicit type, which retain existing compatibility fallback. Independent user literal and no invented adapter/private setter.
Cycle9 genuine red: typed request rendered as `Edits applied.` and omitted its user-authored sentence (cycle9-red.log). Minimal shared display helpers now prioritize explicit stored kind; only final valid JSON photo-state suffix is hidden, while prompt-owned markers remain. Default status uses the same classification/visible text.

## Final layout verification seam (no new feature claim)
Existing accepted Headless resize/dock/widgets, Session history and ai.history/ai.bottom cover EN/ZH normal/narrow/120–240 floating/stacked frames. Reuse the existing outer-composer reachable control seam and inspect full visible label bounds/screens. Catches clipped return/input/tool controls and interaction reaching intended surface; misses native OS compositor. No private state assertion or new adapter.
Cycle9 green passed: typed user request remains user text in paint/default status and exact stored record is unchanged. Review-stage capture hardening: preview requests hold full immutable PhotoHeader; selected fragment metadata reuse requires exact same captured header, including epoch/transaction. Existing graph view/worker Retention stays unchanged.

## Captured attachment scope regression seam (verification of existing implementation)
Real temporary FsStore with a one-time preview-read barrier, actual photo selection and public Headless rendered pixels. Catches late A worker using same message/image IDs being displayed as B, and source retirement allowing B's own preview. Misses process crash/OS compositor; no private cache assertions or test-only production hooks. Independent procedural red A and blue B pixels, recorded references only. This characterizes already implemented scope handling; do not claim a new red/green cycle if it passes.

First complete AI combination: ai-full.log, 143 passed / 4 failed, no skips. New compact-layout fixture scrolled toward top for a button below the composer; corrected downward direction, not claimed as feature red. Existing model-history preview barrier was intercepted by newly legitimate UI preview reads; adapter now gates/records actual companion-model-history thread only and retains every original cancellation/second-preview/retirement assertion. Two UX-12 pagination tests genuinely exposed old-page tail pixels restored into a newly requested page; explicit navigate_page now begins local offset0, while ordinary same-page reopening still restores saved pixels. The old flat galley also painted whole offscreen records; the saved-position fixture now independently expects actual visible row200 after its180px scroll, rather than requiring offscreen row196 to paint. Exact public saved/restored offset/follow assertions retained. Latest full combination rerun includes actual late A→B pixel regression and all layout screenshots.

## Actual consumer/lifecycle handoff for 10 / 15 / 19
- `HistoryReader.message_display(record)` reuses the existing selected scanner/record reader, verifies full length/digest and returns role/display_kind/local_message_id/local_round_id/images without retaining ignored giant text. The existing `message_round_id` wrapper remains. Fallback `DisplayScan(Message)` selects the same fields with one <=256KiB logical continuation/frame (existing bounded physical block verification), while captured `HistoryView._retention` stays owned through scanning/error/inactive pause/Drop. Native selected fields are published only after verification; fallback unverified fragments stay folded even on intermediate frames.
- Same-captured-header continuation reuses verified metadata; full PhotoHeader equality gates reuse. This is UI display only, separate from unchanged11 complete model projection. Ordinary initial pages remain last32; explicit bottom begins only the final record and follows bounded continuation until its last fragment. User intent cancels automatic continuation and the original actual native reader must retire before replacement.
- `Session.companion_attachment_reader(photo)` / `CompanionAttachmentReader.read(image)` / `into_retention()` support the actual currently readable known-v1 JPEG path. Factory acquires owned Store handle and actual native LibraryLock guard without reading/hash-scanning a Root. Read is <=128KiB, validates requested photo/path/content hash/MIME/strict image rules. `into_retention` drops Store and transfers opaque lock guard into decoded view/cache/queue. v2 uses existing HistoryReader.preview + HistoryRetention. No guessed source/current render pairs.
- `Attachments` is the single actual UI consumer. One native producer, channel2, pending<=8, cache<=8; visible rows only. Sources carry immutable full header plus library/photo, exact descriptor fingerprint distinguishes same-ID references. Pixels decode <=640x640/128KiB from recorded bytes; unsupported owned-store capability or malformed/missing reference yields a local placeholder. Stale scope results Drop and cannot overwrite current-photo cache.
- `Attachments.cancel()` is caller-used by successful before_library_change and command generation reset: cancels producer permit, clears pending/cache/queue, but does not claim producer has retired. `busy()` is actual alive owned by WorkerAlive; actual reader/Store drops first. Waiting producer schedules50ms repaint. State Drop cancels permit; graph/lock retention lives as long as actual source/queued/cache object lives. Future15 full close/quit waiting must include this actual image producer busy state as well as existing model/paging writers. Runtime native actual09 lock tests remain part of AI/engine combinations; the new known-v1 reader's own child-process OS-lock scenario was not separately executed, so no new legacy cross-process claim.
- Parent10 pending interface handoff only, NOT imported/approved support: companion_legacy_photo_header(photo) yields staged read-only header; HistoryReader.original_image_reference(record,index) verifies raw giant syntax/digest and selects original reference; HistoryReader.preview(image) will support owner/path/hash/MIME native PNG as well.10 will merge approved21 and unify this single Attachments consumer with that actual captured legacy/staged graph.21 does not claim giant legacy full Root, PNG/raw-reference/staged migration coverage from unapproved10.19 should reuse owned recorded bytes/selected reference, never substitute a current rendered pair.
Second complete AI run: ai-full2.log, 144 passed / 4 failed, no skips. Genuine new-page offset fix clears native_pending_older_intent regression; model-only preview cancellation now passes. Remaining fixture corrections: scope barrier initially armed during real save verification and blocked the fixture's own GUI-thread setup read, so arm after saving; input sits above the reached compact-bottom button, so navigate upward; mixed giant fragment order now explicitly scrolls to real visible final text rather than expecting a whole flat galley offscreen; guessed row200 visibility is invalid with varying layout, so same-page restoration waits for public historyLoading=false / page status literal196 plus actual painted position rows, while exact public saved/restored localpixels and follow=false assertions remain independent. These fixture failures are not feature red claims. Initial first-row196 navigation remains a real painted assertion at local offset0.

## Final command evidence (ongoing)
Environment for Cargo: CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21; CARGO_BUILD_JOBS=2; CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts. No new skips or RUST_TEST_THREADS override.
- `cargo test -p lightcraft-engine companion -- --nocapture`: engine-companion.log exit0;85 passed/0 failed/1 existing ignored helper; three actual1-test child-process invocations also passed (OS-lock/retention existing09 coverage).14.57s test time after cold compile. No separate new known-v1 child-process lock claim.
- `cargo fmt --all -- --check`, `git diff --check`: passed after latest source edits; rerun at final commit boundary.
Third full combination: ai-full3.log145 passed/3 failed; late A→B actual producer/pixel regression passed. Remaining two scroll fixtures had the ui.scroll sign reversed: positive means upward, so a new page beginning0 cannot scroll further up. Corrected actual downward dy to negative for saved-page reading and giant-fragment-end order. Saved-page test now captures a real visible same-page literal and relative viewport position, asserts exact cursor196/epoch/count260/localpixels/follow, and restores the identical literal/position after tool/photo/reopen rather than accepting arbitrary content. ZHfloat120 return fixture initially stops at clipped>=12px button; continues small real downward wheel steps until the complete glyph bounds fit before clicking. Native/WASM source compiled; WASM has four pre-existing lightroom_import dead-code/import warnings only. Strict first run found4 owned style/size warnings, fixed without allowances (boxed owned large reader, clamp, collapsed ifs); rerun required.
Fourth full combination: ai-full4.log147 passed/1 failed. Correct actual negative downward wheel clears giant chronological paint check; full same-page captured visible literal/relative-position plus exact cursor/epoch/count/localpixels/follow restoration passes. Remaining layout assertion used a rectangle frozen before ui.screenshot advanced frames; removed the premature screenshot before the glyph/rect assertion (screens still captured after assertion). This stale-rectangle diagnostic fixture failure is excluded as feature red. Layout-only rerun pending.
- strict2.log: `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` exit0, no warnings/allowances added.
- wasm.log: `cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown` exit0; four existing lightroom_import unused/dead-code warnings, no changed-module warnings. Compile check only, no browser backend/runtime claim.
- i18n.log:30 passed/0failed, includes all format catalogs. New21 strings are static labels and zh-hans translations, no dynamic format key added.
- shutdown.log:1 passed; layers.log exit0.
- aux runner incorrectly named a nonexistent `smoke` target, so smoke.log exit101 is command setup error, not a test failure. Actual existing test target is docking_workflows (3 tests); corrected run started and its result will be recorded. No skip/exemption.
Layout5 corrected full-frame control test passed (1 test,5.77s), all EN/ZH normal/narrow/float120/float240/stack120/stack240 actual return and input interactions;24 exported frames. This pass uses live same-frame rectangle/glyphs before screenshot, not a frozen rectangle across screenshot-driven frames. docking-workflows.log: corrected3 real GUI workflow tests passed,3.68s.
Final public parity check found ai.history doc reference unknown because the pre-existing caller-used command had never been listed in UI_COMMANDS. Seam: actual xtask registry/reference checker catches stale public command discovery/docs; misses OS/native control delivery already covered by UI tests. Actual red parity.log1 stale reference. Minimal fix registers the existing ai.history command next to21 ai.bottom with EN/ZH label; no new renderer capability/protocol/action invented. Reference checker rerun pending.


## Final outcome / acceptance
Whole UX-21 actual native client path implemented against approved32e. Roles remain chronological across HistorySlice.items/full records/fragments/tail; default folded tools and unverified fragments never expose raw tool JSON, including fallback intermediate frames. Basic semibold/bullets and numbered lines render; distinct local execution feedback is human readable. One actual upward wheel immediately stops following, true round.ended retains reading position, explicit bottom reaches even oversized final fragments. Full legal model projection is independent and unchanged.
Visible recorded previews use one bounded captured consumer; neighboring missing/bad preview stays local. Actual delayed A→B same-ID race shows only B's own blue pixels after A retirement. Ordinary last32 pagination, per-photo editor cursor/epoch/count/localpixels/follow intent/draft and saved-reader retention remain; explicit new page begins0, reopening the same page restores the identical visible text anchor/relative position and exact public editor fields. Existing16/17 frozen cards/explicit bound undo,09 actual view/producer guards,04 first-feedback0IO and20 readonly-original/default-navigation/global-entry/health/trace regressions pass in full combination.

Final controls / docs: ai.history (registered existing command), ai.bottom; button:aiBackToBottom; button:aiTool:<local_message_id-or-record-fallback>; button:aiFragment:<record>:<index> for unverified fragments; image:aiAttachment:<local_message_id>:<index>. All use clipped widget registration. Existing public field:aiPrompt and older/newer controls remain. docs/control-protocol.md describes actual entry/scope and preview lifecycle; PC-01 remains partial with documented UX-21 behavior, no fabricated Lightroom status/count changes. EN labels and zh-hans static translations supplied; no new dynamic format pattern.

### Final exact checks
All Cargo commands used the environment above, ordinary test concurrency, no added skips, owned temporary/procedural data, no .env / paid provider / original photos / OS synthetic input.
| Command | Evidence | Result |
| --- | --- | --- |
| cargo test -p lightcraft-ui-egui ai_companion -- --nocapture |21/ai-final.log|148 passed/0 failed/0 ignored;76.09s. Complete production source after menu registration; latest subsequent test-only glyph strengthening separately verified below. |
| cargo test -p lightcraft-ui-egui reading_controls_and_input -- --nocapture |21/layout6.log|1 passed/0 failed;6.00s. Latest test-only sharpening requires complete draft glyph bounds, actual typed draft persists; all12 locale/layout combinations and24 fresh frames. |
| cargo test -p lightcraft-engine companion -- --nocapture |21/engine-companion.log|85 passed/0 failed/1 existing helper ignored; three real child invocations also passed. |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |21/strict-final.log|exit0, latest source/test;8.10s. |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |21/wasm-final.log|exit0;17.26s. Four unchanged lightroom_import unused/dead-code warnings, no21 warnings. |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |21/i18n.log|30 passed/0 failed;6.58s; all format catalogs. |
| cargo test -p lightcraft-ui-egui --test docking_workflows |21/docking-workflows.log|3 passed/0 failed;3.68s. |
| cargo test -p lightcraft-ui-egui --test shutdown |21/shutdown.log|1 passed/0 failed. |
| cargo xtask layers |21/layers.log|exit0. |
| cargo xtask parity |21/parity2.log|exit0, every command/control/path reference exists; original non-invention statuses/counts retained. |
| cargo fmt --all -- --check; git diff --check |final boundary command|both exit0 after latest edits. |

### Screens inspected
24 actual CPU Headless exported frames under /tmp/lightcraft-ux-execution-20261013/21/screens (no compositor/OS delivery claim). Six layouts × EN/ZH × return/input. Normal frames visibly show user/assistant/system groups, semibold/list and closed tool process. Compact/narrow frames show full Back-to-bottom labels and reachable actual input; screenshot QA strengthened the120px EN input into complete visible Chinese glyphs via actual user wheel, no fake layout/image editing. Files:
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-float120-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-float120-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-float240-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-float240-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-narrow-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-narrow-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-normal-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-normal-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-stack120-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-stack120-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-stack240-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/input-stack240-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-float120-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-float120-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-float240-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-float240-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-narrow-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-narrow-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-normal-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-normal-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-stack120-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-stack120-zh-hans.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-stack240-en.png
- /tmp/lightcraft-ux-execution-20261013/21/screens/return-stack240-zh-hans.png

### Fresh review boundaries / limitations
This is UX-21 only; no tracker resolution, integration merge, push or PR. No HTML/fenced-code/link renderer or new message-copy feature claimed. No native OS compositor, paid provider, browser backend runtime, arbitrary power-loss or new separate known-v1 child-process OS-lock scenario claimed. Public installed-engine/component seams and actual native/Fs/Mem/protocol/frame checks above are the evidence. Future10 raw65MiB legacy/PNG/staged source APIs remain pending/unapproved, were not imported; it must unify this actual consumer after approved merge. Future15 must account for actual image producer retirement plus queued/cache retention;19 should consume actual own recorded references. Detailed API/limits/lifetime handoff is above. Failed and invalid fixture histories remain explicitly recorded and do not become genuine feature red claims.

## Clean candidate handoff
Commit: 757ca191b357acc3b36708ecd7de2133d6af333e. Branch codex/ux-21. Source status porcelain empty: True. Approved integration32e remains ancestor; `git merge --no-edit codex/ai-companion-ux` returned Already up to date, so no integration changes imported and no affected combination invalidated. Final post-commit fmt/diff checks both exit0.19 files,1346 insertions/46 deletions. Fixed review range32eebdbdc7a71456f7678230a85af8ae07ed8c4a..757ca191b357acc3b36708ecd7de2133d6af333e. All reports/logs/screens live outside repository. Await independent fixed-HEAD review; tracker remains claimed.

## Review-fix completion update (2026-10-14)
Current clean b7c3dfff242a38ac5b1c3fd04452265167e31ef2 supersedes757ca191. Actual literal authored JSON survives submitting/start/status/pre-input stop; real awaiting_input enrichment hides only generated state while preserving exact authored literal/full model record. Typed giant user/assistant marker prefixes and final continuations read correctly through real native/fallback v2 paths. Previously persisted empty/cropped hints reopen read-only with unchanged original record blocks, and old untyped feedback remains closed throughout intermediate/final status/paint. Shared caller-used authored_message_text and optional span version1 are documented in21-fix.md; no full settings-tree parse in rendering or new private/test-only production interface.
Current final exact checks: fix-ai-final152/0/0(83.66s), fix-engine85/0/1 existing helper plus3actual child1/0(24.42s), fix-strict exit0, fix-wasm exit0 with7 unchanged warning messages total (4UI), fix-i18n30/0, fix-docking3/0, fix-shutdown1/0, fix-layers31crates0violations, fix-parity everyreferenceOK, fix-fmt and fix-diff-check exit0.24 new frames21/fix-screens all directly inspected across EN/ZH sixlayouts return/input; original screenshots/evidence remain separately retained. No source change after successful checks. Approved32e ancestor merged already up to date; empty porcelain and full-range diff-check confirmed after fix commit. Original reviewer re-clear required before root merger; no tracker/integration/push/PR mutation. Actual future10/15/19/18/22 and unsupported custom Store/WASM runtime boundaries remain as explicitly ruled, not waived.


---

# UX-21 fixed-range senior review

Review seat: fresh-context Senior Code Reviewer, read-only. Date: 2026-10-14.

- Base: `32eebdbdc7a71456f7678230a85af8ae07ed8c4a`.
- Head: `757ca191b357acc3b36708ecd7de2133d6af333e` (`codex/ux-21`).
- Checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-21/lightcraft`.
- Exactly one commit, 19 files, 1346 insertions / 46 deletions. Base ancestry verified. Checkout HEAD and empty porcelain verified before and after review. No checkout/index/HEAD/branch mutation, subagents, merge, paid provider, .env, original photos, or OS synthetic input.

Read requesting-code-review/SKILL.md and code-reviewer.md; installed tdd and codebase-design skills; execution context, review contract, skill update, review focus, whole21 ticket, authoritative design (especially §§3/7/9), implementation plan, AGENTS.md and docs/collaboration.md. `plan/` is absent in this worker, as the provided context warned; no plan-file compliance claim. Read the complete production diff/new modules, affected paging/archive/draft/engine callers, existing model/history/card/navigation/lifecycle interfaces, new and modified tests, implementation report, final logs/candidate, and root visual note.

## Strengths

- `reading.rs:4` walks ordered HistorySlice items before the runtime tail; ordinary user/assistant/system/local execution roles, semibold/bullet prose and explicit folded tool controls are much more readable. UI projection does not mutate the full saved model messages. Typed execution feedback is human readable and raw tool results stay hidden by default.
- `attachments.rs:81,140,176,196` provides meaningful depth behind a single consumer: visible rows only, one owned producer, bounded queue/cache/codec limits, full immutable PhotoHeader plus generation/photo/descriptor identity, and independent local missing-preview outcomes. Actual source ownership precedes WorkerAlive retirement; decoded/queued/cache objects retain the corresponding graph/native lock guards. Cancellation does not assert that a live producer retired. I read both native and known-v1 factories and the captured-reader guard paths.
- `ai_companion.rs:2153` observes the delivered raw upward wheel after egui consumes its delta, with visible viewport/layer scoping; the real single-wheel test begins above 1000px and sends pointer and wheel in separate delivered frames. Real message.completed/round.ended preserve settled position and follow intent.
- `drafts.rs:205,245` separates explicit new-page offset0 from same-page restored pixels, and explicit bottom follows bounded continuations of only the final record. The repaired UX-12 test retains independent cursor196/epoch/count260, exact local offset/follow assertions, plus actual same visible text and relative pixels across tool/photo/reopen.
- Accepted 09/11/12/16/17/20 caller paths remain structurally intact, with retained actual guard/model-projection/worker-retirement/explicit-authority/readonly-navigation regression evidence in the final complete combination. ai.history/ai.bottom are registered and documented; parity counts/status are not inflated.

## Issues

### Critical

None found.

### Important — must fix before merge

1. **User-authored valid JSON suffix is omitted before any actual photo state was appended.**

   Location: `crates/ui-egui/src/ai_companion/reading.rs:169-173` (also called by default status through `ai_companion.rs:1948`).

   Trigger: submit the literal prompt `Please explain this exact payload.\nActual photo state: {"exposure":0}`. The real start path stores the unmodified request at `ai_companion.rs:487` with `display_kind=request`; appended machine state only arrives later at `:1133`. During submitting, or when cancelled before input delivery, `user_text` nonetheless treats the user's final valid JSON as generated state and removes both marker and JSON. It does not prove that a generated suffix exists.

   Effect: the conversation row and default ai.status show only `Please explain this exact payload.` even though the exact saved request still includes the JSON. The user cannot accurately read what they asked from the conversation. This is current typed-request behavior, not an old-format or future-ticket limitation.

   Independent reproduction: `21/reviewer-21/request-marker.rs` uses actual Headless UI consent → public ai.start → ai.status → ai.stop, with a reviewer-owned slow loopback TCP fixture at 127.0.0.1:19983 preventing round/input progress; no private state writes. `request-marker.log` records the intact stored message, omitted immediate status, omitted post-stop conversation row, and a literal-JSON assertion failure (exit101). The restored draft does show the full prompt; that does not repair the already saved conversation row/status.

   Fix: determine display prose from the actual authored request / explicit known input association, rather than a syntactically valid suffix alone. Preserve user-authored delimiters and JSON through submitting and pre-input cancellation, and continue hiding only the truly appended machine state. Strengthen the existing marker regression (`reading_tests.rs:415`) to require the exact literal JSON; currently it only checks the earlier sentence, so it passes while losing the suffix.

2. **Oversized explicitly typed messages still use an old content-based range that can hide the whole message.**

   Location: `crates/engine/src/companion/storage/pagination.rs:952-956`, consumed by the new fragment presentation at `crates/ui-egui/src/ai_companion/reading.rs:14-16`. The range calculator is unchanged in this commit, but the whole21 implementation relies on it and does not meet its long-message acceptance for this trigger.

   Trigger: save a supported v2 message >256KiB with explicit `role=assistant`, `display_kind=assistant`, whose authored text starts `Actual execution feedback: this is quoted by the user.` followed by ordinary readable prose. The same issue occurs with explicit `role=user`, `display_kind=request`. `text_range` ignores role/kind and assigns an empty visible span based solely on the text prefix. `slice` then skips that record entirely; no fragment metadata or new typed classification can recover it. A literal `\nActual photo state:` anywhere in long prose is also cropped at the first occurrence by this same heuristic.

   Effect: a long reply that was visible during streaming can disappear from persisted long-history reading; an entire stored typed request/reply may have no visible row or default status text. The actual `text.delta` caller accepts up to1MiB at `ai_companion.rs:770`, and `message.completed` stores explicit assistant kind at `:788`, so the assistant example fits the current supported flow. The independent saved-message fixture stays below that1MiB limit. This is current v2 paging, not future10 giant legacy-root/PNG/staged migration.

   Independent reproduction: `21/reviewer-21/giant-answer.rs` uses public Session.open_library_in/save_companion_tail → Headless ai.open → public historyLoading/status → painted output/full-record read. The message has528KiB of ordinary prose and explicit assistant kind. `giant-answer.log` records no initial authored sentence in paint/status, exact full stored record preserved, then the readable-sentence assertion fails (exit101). Corrected `giant-request.rs` proves the same for explicit request kind. These are public caller-seam observations, not private scanner tests.

   Fix: make oversized presentation honor explicit role/kind and true machine-state association consistently with ordinary records; do not infer hidden prose from marker spelling. Account for already persisted range hints that were computed using the old heuristic, without deleting or rewriting model messages. Add a real v2 fragment-reading regression with an independently expected visible authored sentence and final continuation, and retain default tool privacy.

### Minor

None requiring action identified.

## Verification and evidence

Reviewer commands executed in the fixed checkout:

- `git status --porcelain`, `git rev-parse HEAD`: clean; exact supplied HEAD, before/after.
- `git merge-base --is-ancestor 32eebdbdc7a71456f7678230a85af8ae07ed8c4a 757ca191b357acc3b36708ecd7de2133d6af333e`: exit0.
- `git diff --stat`, `git diff --name-only`, `git diff`, `git log --oneline` for the exact fixed range: read-only inspection.
- `git diff --check 32eebdbdc7a71456f7678230a85af8ae07ed8c4a..757ca191b357acc3b36708ecd7de2133d6af333e`: exit0.
- `cargo fmt --all -- --check`: exit0.

Independent reproducer compilation used `rustc --edition=2024 SOURCE -L dependency=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21/debug/deps --extern lightcraft_ui_egui=.../liblightcraft_ui_egui-d39cdf481eb5cb8e.rlib --extern lightcraft_engine=.../liblightcraft_engine-3c6ca3fac6cd88e0.rlib --extern serde_json=.../libserde_json-8e761e4a3c8b5a46.rlib [--extern lightcraft_catalog=.../liblightcraft_catalog-ab434b14ddf9a322.rlib] -o OUTPUT`; SOURCE/OUTPUT are exclusively inside `/tmp/lightcraft-ux-execution-20261013/21/reviewer-21/`. Cached production libraries were read, not rebuilt or mutated. Read-only rustc metadata identified matching dependency artifacts. No Cargo build/test invoked against the worker target and no shared target lock acquired by the reviewer.

Exact corrected compile/run commands are saved in `21/reviewer-21/commands.txt`. Actual corrected source/executable/log names: `request-marker`, `giant-request`, `giant-answer`. All compiled exit0; each expected user-facing assertion failed exit101 as reported above. Setup attempts using mismatched cached dependencies or a refused/no-listener network endpoint are excluded as behavioral evidence. An initial giant fixture expected the final literal before paging; that expectation was corrected to the initial authored sentence, and only the corrected run is used. The separate `legacy-feedback` diagnostic passed: its oversized legacy feedback does not expose raw state because the existing empty span suppresses it. It is not a finding or claimed coverage of all legacy migration paths.

I inspected worker final logs rather than rerunning its complete suite. Verified recorded results:

| Worker command | Actual final evidence | Result |
|---|---|---|
| cargo test -p lightcraft-ui-egui ai_companion -- --nocapture |21/ai-final.log|148 passed/0 failed/0 ignored,76.09s |
| cargo test -p lightcraft-ui-egui reading_controls_and_input -- --nocapture |21/layout6.log|1 passed/0 failed,6.00s; latest test-only glyph strengthening |
| cargo test -p lightcraft-engine companion -- --nocapture |21/engine-companion.log|85 passed/0 failed/1 pre-existing helper ignored; three actual child invocations passed |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |21/strict-final.log|exit0 |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |21/wasm-final.log|exit0; seven warnings total in unchanged codecs/preview/devices/lightroom_import code, of which four are the reported ui/lightroom_import warnings |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |21/i18n.log|30 passed/0 failed; format catalogs included |
| cargo test -p lightcraft-ui-egui --test docking_workflows |21/docking-workflows.log|3 passed/0 failed |
| cargo test -p lightcraft-ui-egui --test shutdown |21/shutdown.log|1 passed/0 failed |
| cargo xtask layers |21/layers.log|exit0 |
| cargo xtask parity |21/parity2.log|exit0 |

Worker Cargo environment: CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21; CARGO_BUILD_JOBS=2; CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts. Ordinary test concurrency; no new skips. Superseded red/green fixtures are not promoted to valid behavior evidence; final combination includes the originally failing UX-12 and model-preview cancellation checks.

Direct pixel inspection covered12 of24 final screenshots, one for each locale/layout combination, with both input and return surfaces represented: return-normal-en; input-normal-zh-hans; return-narrow-en; input-narrow-zh-hans; input-float120-en; return-float120-zh-hans; input-float240-en; return-float240-zh-hans; return-stack120-en; input-stack120-zh-hans; return-stack240-en; input-stack240-zh-hans. Full labels/input glyphs in these captures are readable/reachable; normal rows show distinct roles/semibold/bullets/closed tool details. Compact bodies legitimately scroll to reveal each control. I read the actual twelve-layout test for all24 exports and literal glyph/control interactions. No physical compositor claim. New translation keys are static; no new dynamic-format pattern, and all existing format-catalog tests are in the passing i18n evidence.

## Recommendations

Resolve both Important findings at the existing client/storage reading seams, preserving exact saved/model history and default privacy. Run the corrected literal-marker and giant typed-message regressions first, then the full148-test affected combination plus layout glyph test and engine/strict/format/parity checks as affected. Provide a new fixed HEAD for review; this report applies only to757ca19.

## Declined to judge — each requires root ruling

- Future10 giant legacy whole-Root loading, raw PNG/original-reference selection and staged migration: separately assigned pending work; this review checks currently readable native v2, known-v1 JPEG and inline sources.10 must consume this single captured Attachments module after merge, not introduce a second GUI root loader. The two findings above are current paths and are not deferred.
- Future15 full quit/close/reopen/switch waiting certification for the new image producer: separately assigned all-exit work. Actual cancellation/Drop/guard ownership was inspected here;15 must include actual Attachments busy retirement and queued/cache retention alongside existing producers, and test every exit combination.
- Future19 paired historical before/after comparison, crop/orientation fidelity and comparison controls: separately assigned whole comparison path; it must use real own recorded references and this reader ownership, never current-render substitution.
- Future18 durable restart undo authority and its full fault matrix: independently assigned;21 retains accepted17 explicit authority/caller behavior and existing regression evidence but does not certify pending18 combinations.
- Future22 full cross-ticket acceptance/deployment fixture matrix: separately assigned integration certification; the current recorded affected combinations are assessed without calling them final22 acceptance.
- Unsupported custom Stores without an owned background reader: existing documented capability limit; local placeholders are intentional and avoid blocking frame I/O. This is not native FsStore or the supported legacy/inline source path.
- A new standalone known-v1 child-process OS-lock scenario: not executed in this evidence; factory/native guard ownership was read and existing actual09 child tests pass, but no new independent legacy cross-process certification is claimed.
- Full HTML, fenced-code, links, rich Markdown and a new message-copy feature: outside the approved basic bold/list requirement; not required to merge21.
- Physical OS wheel/compositor delivery, Windows/Linux runtime and browser backend runtime: not executed; macOS CPU Headless and successful WASM compilation do not certify those surfaces.
- Paid-provider quality/deployment load and arbitrary physical power-loss behavior: outside this local deterministic review/evidence; no claim from transport adapters or temporary Store tests.

## Assessment

**Ready to merge: No.**

The main architecture, bounded captured previews, scroll intent, per-photo restoration and current regressions are well supported, but two independent public-seam reproductions show authored text being omitted from current conversation reading. Fix both Important findings and review the new fixed range before integration.


---

> 2026-10-14 second review fix update: b7c3dfff was independently cleared for R1/explicitR2 but rejected for known assistant-without-display-kind omission. Clean7ee3545b6c1c2ff5f7c63e619157197c56000040 now fixes that accepted finding. Current exact evidence/ruling/153-test results are in `/tmp/lightcraft-ux-execution-20261013/21-fix-role.md`; original reviewer re-clear pending. Earlier candidate evidence below is historical, not current merge approval.

# UX-21 review fixes — fixed candidate awaiting fresh review
Clean retained base757ca191b357acc3b36708ecd7de2133d6af333e, approved integration32e. Read21-review.md, explicit root ruling, installed tdd/tests/mocking/codebase-design. R1/R2 are current supported paths, not deferred10. No new renderer features or private state hooks.

## R1 cycle — authored valid JSON during submit / pre-input cancellation
Already-approved seam: actual Headless consent/start/status/stop/paint and Session photo_companion retrieval; real owned slow loopback TCP adapter prevents round/input progress. Catches exact authored marker/JSON preserved in default status and stopped conversation row before generated state exists; misses paid-provider/physical compositor. Expected prompt literal independent, no private state writes. Owned server lifetime closes on Drop.
R1 genuine red: fix-r1-red.log exit101, exact prompt intact in Session but immediate status omitted authored literal JSON;1.06s. Minimal production adds authored_text_bytes only at actual prepare_input enrichment. Ordinary request/status no longer strips marker spelling; only typed request with explicit valid UTF-8 authored boundary and exact associated suffix is shortened (without parsing or allocating machine state). Model text remains full. Existing weak saved-marker test strengthened to require full literal JSON and complete status text. Subsequent genuine green recorded below.
R1 green: fix-r1-green.log exit0,1 passed; exact authored literal survives submitting and real pre-input stop, default status and conversation row. Next approved public seam check: actual consent/start + external test_connection transport/round.state(awaiting_input) + real preview worker/input request + Session saved messages. Catches actual generated state hidden while authored marker/JSON survives; misses paid provider and OS delivery. Literal prompt and independent exposure0.7 are expected; external transport adapter is the previously approved varying seam, no private state assertions.
R1 actual enriched-input check passed (fix-r1-enriched.log): real preview/input publication preserves literal authored JSON, hides independently expected exposure0.7 from prose, retains it in full saved model text. No new red claimed for this positive characterization.

## R2 cycle — current v2 typed giant text
Approved Session.save/read + Headless ai.history/status/ai.bottom/public loading/paint seams, real Mem fallback and owned Fs native adapters. Catches explicit user/request and assistant/assistant marker spelling retained in first fragment and final continuation, unchanged full saved records; misses arbitrary OS allocator instrumentation. Literal quoted prefix, authored JSON and final marker are independently expected; one test through actual path before implementation.
R2 genuine red: fix-r2-red.log exit101, explicit request's entire authored body absent (0.78s). R2 green: fix-r2-green.log exit0, all four real native/fallback × request/assistant variants preserve quoted prefix/authored JSON and reach literal final continuation with exact model records unchanged.
Implementation: shared caller-used engine authored_message_text owns association policy for UI and writer. New companion index ranges carry optional version1 (independent companion sidecar, core catalog untouched); typed prose is never classified by spelling. Existing unversioned hints retain their correct string start, discover the closing unescaped quote inside each <=256KiB slice, preserve UTF-8/complete escapes and never hydrate/rewrite giant records. Verified selected metadata annotates old empty heuristic hints only after full digest; explicit kinds override this legacy fallback, while genuine untyped legacy feedback stays closed on native/fallback intermediate/final frames.

Old-hint compatibility verification seam: public Session save/close/reopen/page and owned FsStore read/write. Independently reconstruct previously supported index version0 empty/cropped hints in an owned copied fixture, update index/manifest/HEAD hashes, leave exact record block bytes untouched. Real native and no-owned-reader fallback capabilities. Catches reading old persisted hints (not merely new writer output), complete final continuation and default legacy feedback privacy; misses all unknown outer-schema migration (assigned10/14). These are fixture-side Store operations, not private GUI state/setters or production test-only methods. No new red claim if already-compatible reader passes.
Old-hint verification exposed a genuine owned compatibility regression in fallback (12.10s): paging's final selected-field filter discarded the verified old-empty privacy annotation, so legacy raw state reached default status after scanning. Minimal fix preserves only the two newly real selected fields authored_text_bytes/legacy_hidden_text through that existing filter; no broader record hydration. Native selected map already kept them. Corrected rerun: fix-r2-old-hints-green.log exit0,1 passed/0failed in19.30s; all8 native/fallback cases passed. This is an actual new red, not an invalid fixture.

Review-stage boundedness correction: authored_message_text uses the explicit typed UTF-8 byte boundary and exact appended delimiter, not a full serde_json Value parse of machine state. Rendering therefore does not allocate the generated settings tree. The real prepare_input producer owns the association; content spelling alone remains insufficient. No new renderer or private adapter added. Full affected combination is running against this final production change.

## Final fixed-source combination evidence
Environment for all Cargo commands: CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21; CARGO_BUILD_JOBS=2; CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts. Normal test concurrency, no new skips or lint allowances.
- `UX21_SCREENSHOT_DIR=/tmp/lightcraft-ux-execution-20261013/21/fix-screens cargo test -p lightcraft-ui-egui ai_companion -- --nocapture`: fix-ai-final.log exit0,152 passed/0failed/0ignored,83.66s. Original pagination/cancellation/guard/result/revert/readonly-navigation assertions retained. Exact same-page literal and relative painted position, cursor196/epoch/count260/localpixels/follow remain required, not weakened. Real late-A producer is gated after independent saving; A red pixels absent/B blue pixels present and actual retirement checked.
-24 freshly exported actual CPU Headless frames under21/fix-screens, all viewed directly with view_image. Normal/narrow/120–240px floating/stacked × EN/ZH × return/input. Full control/draft glyph bounds verified through public widget controls and true delivered wheel/input events in the complete152 run. Compact bodies scroll to each control; no OS synthetic input, compositor claim or image editing. Whole21 original24 screens and independent reviewer12 remain separately recorded.
- `cargo fmt --all -- --check` and `git diff --check`: exit0 after all production/test/doc edits so far; final clean boundary repeats below.

## Consumer handoff additions after R1/R2
`companion::authored_message_text(&Value)->&str` is shared by current UI ordinary prose and companion span writer; explicit user/request + authored_text_bytes UTF-8 boundary + exact appended delimiter, zero settings-tree deserialization. Actual prepare_input creates this association; submitting/pre-input cancellation never infer it from literal JSON. Complete model text/history remains unchanged.
Companion index `Entry.text_range_version` is optional serde-default metadata: new spans1, absent old hints use recorded string start and bounded real closing-quote continuation; no migration or original record rewrite. Existing HistorySlice items/captured scans and09 retained readers continue to own order/loading/errors/lifetime. Verified old empty-span annotation is presentation-only after whole digest scan; explicit display_kind takes precedence, untyped legacy execution remains folded in intermediate/final status/paint. Both native and real fallback retain these selected fields. New production test-only interfaces: none.
All earlier attachments bounds/scope/guard/lifecycle interfaces remain as in21-implementation.md.10 must unify its approved legacy original references/PNG/staged header into this single consumer,15 all-exit waiting must include actual Attachments busy/queue/cache guards,19 own recorded references,18 restart undo,22 integration acceptance remain pending work; no unapproved branch imported and no claim of their completion.

Final additional commands (all affected sources above):
| Command | Log under21/ | Result |
|---|---|---|
| cargo test -p lightcraft-engine companion -- --nocapture |fix-engine.log|exit0;85 passed/0failed/1 pre-existing helper ignored;3 actual native child invocations also1/0;24.42s |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |fix-strict.log|exit0; no new allowances or warnings;1m19s including shared own-target wait |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |fix-wasm.log|exit0;3.82s;7 unchanged warnings in codecs/preview/engine devices/UI lightroom_import (4UI); compile only |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |fix-i18n.log|30 passed/0failed/0ignored;1.96s; all format catalogs |
| cargo test -p lightcraft-ui-egui --test docking_workflows |fix-docking.log|3 passed/0failed;3.06s |
| cargo test -p lightcraft-ui-egui --test shutdown |fix-shutdown.log|1 passed/0failed;0.92s |
| cargo xtask layers |fix-layers.log|exit0;31 crates/no layering violations |

Honest failed history: independent first review disproved initial757ca191 candidate via real exact authored JSON and giant typed-prefix fixtures. This fix does not reuse its green148 result as acceptance. New owned red→green cycles documented above are real behavioral failures. The old-hint fallback failure is real lost presentation privacy metadata, fixed without removing privacy assertions. Invalid initial whole21 fixture/source/link/listener attempts remain excluded in the initial implementation/review reports; no skipped failed original assertions, no test-only setters/production test interfaces. Physical native/WASM runtime and paid-provider delivery remain outside these Headless/loopback/compile checks.

| Final command | Log under21/ | Result |
|---|---|---|
| cargo xtask parity |fix-parity.log|exit0; every command/control/path reference exists; prior statuses/counts unchanged |
| cargo fmt --all -- --check |fix-fmt.log|exit0 at final production/test/doc boundary |
| git diff --check |fix-diff-check.log|exit0 at final boundary |

## Final fixed candidate and review boundary
- Original reviewed candidate:757ca191b357acc3b36708ecd7de2133d6af333e, superseded for acceptance by the Important fixes.
- Fixed candidate:b7c3dfff242a38ac5b1c3fd04452265167e31ef2 on codex/ux-21; new local fix commit `UX-21: Preserve authored text through submit and stored fragments`,9files400insertions/24deletions.
- Fresh fix range:757ca191b357acc3b36708ecd7de2133d6af333e..b7c3dfff242a38ac5b1c3fd04452265167e31ef2. Whole ticket range:32eebdbdc7a71456f7678230a85af8ae07ed8c4a..b7c3dfff242a38ac5b1c3fd04452265167e31ef2.
- Latest approved integration checked immediately before final commit remains32e; `git merge --no-edit 32eebdbdc7a71456f7678230a85af8ae07ed8c4a` after commit reports Already up to date; no combination source change requiring redundant repeat. `git merge-base --is-ancestor 32e... HEAD`, full-range `git diff --check`, empty `git status --porcelain` exit0. Retained worktree unchanged, no reset/foreign import/integration edit/tracker/push/PR.
- All accepted R1/R2 behavior and targeted/full affected checks pass. Independent reviewer re-clear is pending; this report is not its approval and does not mark22/all task acceptance complete.

Interpretation limit: records without an explicit authored boundary cannot safely establish generated photo-state association from marker spelling alone. Such typed text is preserved, including previously cropped authored tails. Verified untyped old execution feedback stays folded by its old empty hint; actual new input enrichment carries explicit boundary and hides only its associated state. No heuristic rewriting of unknown historical text or full giant model materialization is introduced.


---

# UX-21 read-only fix re-review

Date:2026-10-14. Separate re-review role only; no merger work.

- Fix range: `757ca191b357acc3b36708ecd7de2133d6af333e..b7c3dfff242a38ac5b1c3fd04452265167e31ef2`.
- Whole range: `32eebdbdc7a71456f7678230a85af8ae07ed8c4a..b7c3dfff242a38ac5b1c3fd04452265167e31ef2`.
- Checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-21/lightcraft`, branch codex/ux-21. Fixed HEAD verified clean before/after.32e ancestor check passed. Fix diff:9files400insertions/24deletions.
- Read21-fix.md, updated21-implementation.md, original21-review.md and every root ruling on its declined lines in review-rulings.md. Existing read-only senior-review/skill/repo instructions remain applicable. No subagents, checkout/index/HEAD/branch mutation, paid provider, .env, originals, or OS synthetic input.

## Strengths and original finding clearance

**R1 cleared.** Actual prepare_input now records authored_text_bytes before appending state at ai_companion.rs:1133. `companion::authored_message_text` checks an explicit user/request association, valid UTF-8 byte boundary and exact producer delimiter; it does not deserialize the generated settings tree. Ordinary unassociated authored JSON remains full prose and full model text remains unchanged. I recompiled and ran the unchanged original public UI-consent/start/status/stop reproducer against the new matching production libraries: exit0. Its log shows the entire literal marker/JSON in the immediate status, intact stored request, post-stop status and actual conversation row.

**R2's explicitly typed request/assistant cases cleared.** Shared authored span calculation plus optional companion-sidecar text_range_version1 removes the old spelling inference for explicit kinds. The bounded version0 path discovers the real string end rather than trusting old empty/cropped ends. No core catalog-format change or record rewrite is introduced. The original public giant-request and giant-answer reproducers independently compiled and passed exit0; first prose is present in status/paint and exact stored records remain unchanged.

I independently ran the current compiled production test artifact's complete `ai_companion::reading_tests` group:14 passed/0failed/0ignored in9.46s. This includes genuine input enrichment (independent exposure0.7 hidden from prose but retained in saved model text), four native/fallback typed giant combinations with UTF-8/authored JSON and final continuation, eight native/fallback old empty/cropped-hint reopen/source-byte cases, intermediate/final default privacy, real wheel/terminal preservation, actual late-A/B pixels, and twelve EN/ZH layout/control/glyph combinations. The old-hint test independently rebuilds index/manifest/HEAD checksums while preserving source blocks, and fallback preserves the verified privacy annotation. I read those tests and their callers; they do not obtain their expected literals from the implementation.

## Issues

### Critical

None found.

### Important — remaining supported-role inconsistency

**A legacy assistant role is still mistaken for local execution feedback when display_kind is absent.**

Locations: `crates/ui-egui/src/ai_companion/reading.rs:165-166`; `crates/engine/src/companion/storage/pagination.rs:970-973`; related old-hint presentation at `pagination.rs:1118-1123`.

Trigger: a supported saved message with explicit `role=assistant`, no optional display_kind, and prose beginning `Actual execution feedback: this is quoted by the user.`. An ordinary short record is rendered as Execution feedback with a generic result sentence and its authored text omitted; a giant record is assigned an empty version1 text span and disappears from default prose entirely. Old empty hints for the same assistant role also become legacy_hidden_text and are folded by the same role-blind compatibility fallback.

The assistant's role is known. Actual execution feedback is generated as role=user in the current callers (ai_companion.rs:652/849/1037/1109) and existing legacy fixture (history_tests.rs:156). Missing display_kind therefore does not justify treating an assistant's literal prose as local execution. This is ordinary supported-role/legacy reading, not pending10 giant legacy Root/PNG/staged migration. The approved preservation of ambiguous old user feedback can remain while honoring known assistant/system roles.

Independent evidence: reviewer-owned `21/reviewer-21-fix/legacy-answer-small.rs` and `legacy-answer.rs` use public Session.open_library_in/save_companion_tail → Headless ai.open/historyLoading/status/paint → full stored-record equality. They differ from the passing original explicit-kind assistant fixture only by removing optional display_kind (and small variant uses one prose repetition). Both compile exit0 and fail the independent authored-sentence assertion exit101. Logs show false for both default status and painted authored sentence; full-record equality passed before that failure. This reproduces the short and giant variants without private fields, changing original records, or relying on old parser internals.

Fix: limit legacy execution/hidden-text fallback to the actual legacy feedback role rather than applying it to every supported role. Make ordinary rows, new span writer and old verified-hint annotation agree. Add public small/giant assistant-without-kind and old-empty-hint reading checks; keep ambiguous old user execution feedback default-closed in native/fallback intermediate/final status and paint. This is a narrow current reading correction, not a renderer expansion.

### Minor

None requiring action found.

## Commands and results

Reviewer recompiled sources into `/tmp/lightcraft-ux-execution-20261013/21/reviewer-21-fix/` only. Production artifacts were read, not rebuilt or mutated. Matching dependency metadata was identified read-only with `RUSTC_BOOTSTRAP=1 rustc -Zls=all`.

Exact compile/run commands are in `21/reviewer-21-fix/commands.txt`. Matching production libraries:

- lightcraft_ui_egui-55d1fa1ba7ec1505.rlib
- lightcraft_engine-1aa5118180b91eca.rlib
- lightcraft_catalog-10c8ec57adb1fc38.rlib
- serde_json-2b34462646895b96.rlib

All are under `/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21/debug/deps`. No Cargo build was invoked by this reviewer against that target. The original three source reproducers were copied unchanged into the new reviewer directory and relinked with these new libraries.

| Reviewer command/fixture | Result |
|---|---|
| request-marker (actual consent/start/status/pre-input stop) |exit0; exact authored JSON restored in status/stored text/conversation |
| giant-request (original public explicit request fixture) |exit0; authored prose present, full records equal |
| giant-answer (original public explicit assistant fixture) |exit0; authored prose present, full records equal |
| legacy-answer-small (known assistant role, no optional kind) |exit101 readable-sentence assertion; current Important finding |
| legacy-answer (known assistant role, no optional kind,528KiB prose) |exit101 readable-sentence assertion; current Important finding |
| `/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21/debug/deps/lightcraft_ui_egui-d7241b0d37537103 ai_companion::reading_tests --nocapture` |14passed/0failed/0ignored;9.46s; own reading-tests.log |
| `git diff --check 32eebdbdc7a71456f7678230a85af8ae07ed8c4a..b7c3dfff242a38ac5b1c3fd04452265167e31ef2` |exit0 |
| `cargo fmt --all -- --check` |exit0 |
| `git merge-base --is-ancestor 32eebdbdc7a71456f7678230a85af8ae07ed8c4a b7c3dfff242a38ac5b1c3fd04452265167e31ef2` |exit0 |
| `git status --porcelain`; `git rev-parse HEAD` |empty porcelain; exact b7c3dfff before/after |

Read and verified recorded final worker evidence, rather than claiming another full reviewer build:

| Worker command | Log under21/ | Result |
|---|---|---|
| cargo test -p lightcraft-ui-egui ai_companion -- --nocapture |fix-ai-final.log|152/0/0;83.66s;24fix-screens exported |
| cargo test -p lightcraft-engine companion -- --nocapture |fix-engine.log|85/0/1existing helper ignored;3actual child passes |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |fix-strict.log|exit0 |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |fix-wasm.log|exit0;7unchanged warnings,4inUI |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |fix-i18n.log|30/0/0;format catalogs included |
| cargo test -p lightcraft-ui-egui --test docking_workflows |fix-docking.log|3/0 |
| cargo test -p lightcraft-ui-egui --test shutdown |fix-shutdown.log|1/0 |
| cargo xtask layers |fix-layers.log|exit0;31crates/no violations |
| cargo xtask parity |fix-parity.log|exit0;references valid |
| cargo fmt --all -- --check; git diff --check |fix-fmt.log/fix-diff-check.log|exit0 |

Worker uses separate ux-agent-21 target, jobs2, craft-fonts and ordinary test concurrency, no new skips/allowances. Reviewer independently inspected six fresh fix screenshots (return-normal-en,input-narrow-zh-hans,input-float120-en,return-float240-zh-hans,input-stack120-zh-hans,return-stack240-en) and the prior twelve locale/layout captures; full labels and draft glyphs are visible at their actual reachable scroll positions. The independent fourteen-test run also executes every twelve-layout glyph/control interaction. No changed renderer geometry or new i18n key in this fix.

## Recommendations

Keep the cleared R1 and explicit R2 implementation and all current green regressions. Resolve the remaining role-blind legacy fallback at the existing client/storage reading seams, then rerun supported-role and default-privacy combinations and provide a new fixed HEAD. Do not weaken legacy user feedback privacy or rewrite model records to make the assistant-role fixture pass.

## Declined to judge / retained root rulings

All ten original declined lines were explicitly ruled by root and remain retained; no additional behavior was set aside in this re-review:

1. Whole10 giant/rawPNG/staged migration remains required after approved single21 consumer integration; current native-v2/known-v1/inline paths are not waived.
2. Whole15 all-exit wait and actual image producer/queue/cache retirement remain required.
3. Whole19 actual recorded paired composition/ownership remains required, with no current-render substitution.
4. Whole18 durable restart/fault/certificate behavior remains required;21 preserves accepted17 authority/callers.
5. Whole22 cross-ticket native fixture/evidence remains required; current green checks are not final acceptance.
6. Unsupported custom Store without an owned reader may show a local unavailable placeholder without GUI I/O; supported sources still required.
7. No new standalone known-v1 child-process claim or extra21 prerequisite; actual09/10 native-lock evidence is retained for22.
8. HTML/fenced code/link/rich Markdown/new message-copy feature is outside approved bold/list21 scope.
9. Physical wheel/compositor, Windows/Linux runtime and browser backend runtime remain unexecuted limitations, not reported passes.
10. Paid-provider quality/deployment load/physical power loss is outside local deterministic evidence, with no claim.

**Additional declined lines: none.** The known assistant-role defect above is a current supported-path finding, not a declined legacy/10 behavior.

## Assessment

**Ready to merge: No.**

The original exact JSON and explicit giant-role defects are independently cleared, genuine generated context remains hidden, bounded old-hint compatibility and current regressions pass. One remaining Important known-role legacy reading defect still omits assistant prose; correct it and re-review before any separate merge authorization.


---

# UX-21 role-aware review fix — clean candidate awaiting independent re-clear
Retained clean b7c3dfff242a38ac5b1c3fd04452265167e31ef2, own codex/ux-21 checkout; read21-review-fix.md and explicit root accepted ruling. No imports/reset/integration/tracker/push. Installed TDD/codebase-design and original approved public seams remain applicable.

Before new test: public Session.open_library_in/save_companion_tail/page and Headless ai.open/status/historyLoading/paint/ai.bottom; real Fs native and no-owned-reader Mem fallback adapters. Catches known assistant role without optional display_kind in short/giant messages, literal quoted marker/JSON/first prose/final continuation and exact full records. Misses physical compositor, paid provider, unsupported outer legacy migration assigned10. Expected literals independent; no private scanner/state setter/new production test interface. One genuine red before minimal role-aware correction. Existing old-hint compatibility seam already approved: owned Fs save/copy/index hash fixture/reopen/record equality and unchanged source block bytes, native/fallback intermediate/final privacy.

Genuine red: role-red.log exit101,1failed0.79s; ordinary fallback known assistant/no-kind loses exact first marker in default status. Minimal shared companion::execution_feedback owns explicit-kind/legacy-user policy for ordinary UI and span writer; old verified annotation limited to user/no-kind. Authored byte-boundary helper unchanged. First role-green.log has correct literal status but its immediate previous-frame paint assertion fails; this delivery-timing fixture failure is excluded from feature red claims. Corrected waits for the SAME independent KNOWN_ASSISTANT_FIRST paint, not arbitrary content. role-green2.log exit0: all4 native/fallback × small/giant cases, exact marker/JSON/first paint/final continuation/full-record equality pass.
After green, extend existing owned old-hint reopen fixture with known assistant/no-kind empty/cropped cases (12cases total native/fallback), preserving all previous user privacy/source-byte/intermediate/final assertions. No new seam or production hook. Compatibility run passed: role-old-hints.log exit0,1/0 in6.84s; all12cases native/fallback including known assistant missing-kind empty/cropped hints, original source-byte/record equality and every intermediate/final user-privacy assertion retained.

## Exact root scope ruling
Root explicitly ruled: "discarded unmerged b7-generatedversion1 empty assistant span artifacts are not released/approved32e compatibility requirement. Finish narrow rolefix with preserved trustworthyversion1 authored association/privacy; existing released unversioned oldhint12cases required/covered. Do not blindly reinterpretversion1 emptyrange or invent extra schema/migration to support intermediate candidate fixtures."
Applied exactly: version1 authored association/byte boundaries remain trusted and unchanged; released unversioned hints still read to the bounded actual string end. No abandoned candidate fixture migration/schema, no supported current role omission deferred. The exploratory observation of discarded b7 version1 artifacts was reported before final source; no fake new red or additional implementation claimed for it. All original10 declined-line rulings/future work remain retained, not waived.

## Final command/evidence table
All Cargo commands use CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21, CARGO_BUILD_JOBS=2, CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts. Normal test concurrency, no new skips/lint allowances.
| Exact command | Log under21/ | Result |
|---|---|---|
| cargo test -p lightcraft-ui-egui known_assistant_without_display_kind -- --nocapture |role-red.log|genuine exit101;0/1,0.79s; literal absent from status |
| same targeted command after minimal correction and identical-paint delivery wait |role-green2.log|exit0;1/0,3.86s; four native/fallback small/giant variants |
| cargo test -p lightcraft-ui-egui old_v2_empty_and_cropped_hints -- --nocapture |role-old-hints.log|exit0;1/0,6.84s;12 actual owned reopen cases |
| UX21_SCREENSHOT_DIR=/tmp/lightcraft-ux-execution-20261013/21/role-screens cargo test -p lightcraft-ui-egui ai_companion -- --nocapture |role-ai-final.log|exit0;153/0/0,87.88s; all prior combination/assertions retained |
| cargo test -p lightcraft-engine companion -- --nocapture |role-engine.log|exit0;85/0/1 pre-existing helper ignored,28.43s;3 actual child invocations1/0 each |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |role-strict.log|exit0;17.42s; no new warning/allowance |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |role-wasm.log|exit0;3.69s;7 unchanged warnings in codecs/preview/devices/lightroom_import,4UI; compile-only |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |role-i18n.log|exit0;30/0/0,1.91s; format catalogs included |
| cargo xtask layers |role-layers.log|exit0;31crates/no layering violations |
| cargo xtask parity |role-parity.log|exit0;every command/control/path reference valid; existing counts/statuses retained |
| cargo fmt --all -- --check |role-fmt.log|exit0 at final source/test/doc boundary |
| git diff --check |role-diff.log|exit0 at final boundary |
No geometry/docking/exit-path change: prior fix-docking3/0 and fix-shutdown1/0 retained, not claimed as rerun here; complete153 includes actual affected draft/saved-page/tool/photo/reopen/cleanup/producer/readonly navigation paths. No unrelated broader test run.

## Preservation and real interfaces
`companion::execution_feedback(&Value)->bool` is shared by existing UI rows/status and companion span writer. Explicit display_kind still takes precedence; legacy marker/verified-hidden fallback only for role=user without optional kind. Known assistant/system prose remains literal. The verified old hint annotator applies the same role/type condition after whole source digest, not raw unverified fields. No UI cache setter/private scanner test/new test-only production interface; shared module behavior is called by real writer/UI consumers.
R1 authored_message_text/actual prepare_input byte association remains unchanged, with no generated-state tree parse. Complete legal model history, original record blocks, chronological slices, one bounded continuation,09 reader/producer/decoded cache guards, independent12 photo/library/editor/cursor/count/epoch/pixels/follow intent,16 exact cards/17 explicit Undo,20 displayed readonlyA and explicit-other-photo navigation all retained in153 affected regressions. Tool details and ambiguous actual legacy-user execution remain default-closed throughout intermediate/final frames; new role tests require first marker/JSON and final continuation without rewriting records.
No new i18n key or renderer feature.24 fresh role-screens exported by actual public twelve-layout interactions/glyph bounds. Directly inspected6 current frames: return-normal-en,return-normal-zh-hans,input-float120-en,input-float120-zh-hans,return-stack240-en,input-narrow-zh-hans. Whole initial24 and prior fix24 were each fully directly inspected, and original reviewers separately inspected their recorded sets; no new geometry. No physical compositor/OS synthetic input/image editing claim.

## Clean candidate / fixed ranges
- Candidate:7ee3545b6c1c2ff5f7c63e619157197c56000040, codex/ux-21, retained own checkout.
- New local commit: `UX-21: Preserve known assistant roles in legacy conversation reading`;5files88insertions/17deletions.
- Focused range:b7c3dfff242a38ac5b1c3fd04452265167e31ef2..7ee3545b6c1c2ff5f7c63e619157197c56000040. Whole21:32eebdbdc7a71456f7678230a85af8ae07ed8c4a..7ee3545b6c1c2ff5f7c63e619157197c56000040.
- Latest approved32e checked immediately before commit; post-commit merge reports Already up to date. Empty porcelain, exact HEAD,32e ancestor and full-range diff-check confirmed exit0. No source edits after green final checks, no foreign WIP/integration/tracker/PR/push/reset. No ongoing own sessions.
- Independent original reviewer re-clear required; this report is candidate evidence, not merge approval/final22 acceptance. Physical Windows/Linux/browser runtime, paid-provider/load/powerloss remain unexecuted. Future10 single-consumer rawPNG/staged/original-reference support,15 all-exit producer/queue/cache waits,19 own historical pair composition,18 durable undo fault matrix,22 combined native acceptance remain assigned and required as previously ruled; not certified by role fix.


---

# UX-21 final role-fix re-clear

Date:2026-10-14. Role: original whole21 reviewer, separate read-only second fix re-review. No merger action or authority exercised.

**Focused range:** `b7c3dfff242a38ac5b1c3fd04452265167e31ef2..7ee3545b6c1c2ff5f7c63e619157197c56000040`.

**Whole21 range:** `32eebdbdc7a71456f7678230a85af8ae07ed8c4a..7ee3545b6c1c2ff5f7c63e619157197c56000040`.

Checkout `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-21/lightcraft`, codex/ux-21. Exact7ee HEAD and empty porcelain checked before/after;32e ancestry and whole-range diff check passed. Focused fix:5files88insertions/17deletions. No checkout/index/HEAD/branch mutation, shared-target mutation/build, subagents, .env, paid providers, user originals, OS synthetic input or merge.

Read21-fix-role.md, updated21-implementation.md, prior21-review-fix.md and latest review-rulings.md including the intermediate version1 scope ruling. Original senior review/skill/repo contract remains applicable. Read the complete focused production/test/document diff and all current helper/writer/UI/default-status/verified-annotation callers; the prior whole21 review remains the baseline for unchanged paths.

## Strengths and findings cleared

- The accepted remaining Important finding is fixed at one shared caller-used `companion::execution_feedback` interface (companion.rs:415). Explicit kinds retain precedence; legacy marker/verified-hidden fallback requires the actual legacy user role. UI ordinary rows/fragments/default status and span writer agree. The verified old-hint annotation also requires user/no-kind after full source verification; known assistant/system prose is not reclassified by spelling.
- The original R1 authored UTF-8 association and helper are unchanged: actual input preparation records its authored byte boundary; unassociated literal JSON stays readable; genuinely appended state is hidden without settings-tree deserialization. Model message text remains full.
- The original explicit-role R2 behavior stays correct, and the added native/fallback small/giant known-assistant-without-kind test independently requires marker, JSON, first painted sentence, final continuation, correct role and unchanged records. The old-hint fixture now exercises twelve native/fallback reopen cases including known assistant empty/cropped released unversioned hints, while retaining all original user-feedback privacy/source-byte assertions.
- Captured preview/reader/worker guards, default tool privacy, chronology, per-photo cursor/epoch/count/pixels/follow, real delivered upward wheel/terminal and readonly navigation are unchanged and remain in the affected combination.

## Issues

**Critical:** none.

**Important:** none remaining.

**Minor:** none requiring action identified.

Original R1, explicit-kind R2 and the later known-assistant-role finding are all independently cleared at7ee. No new supported current-path regression found.

## Independent commands and observations

Recompiled original public sources with matching new production libraries into `/tmp/lightcraft-ux-execution-20261013/21/reviewer-21-role/` only. Read-only rustc metadata verified the library dependency set:

- liblightcraft_ui_egui-55d1fa1ba7ec1505.rlib
- liblightcraft_engine-1aa5118180b91eca.rlib
- liblightcraft_catalog-10c8ec57adb1fc38.rlib
- libserde_json-2b34462646895b96.rlib

All are in `/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21/debug/deps`. Cached artifacts were read, not rebuilt/mutated. Exact compile/run commands: `21/reviewer-21-role/commands.txt`. Reviewer linkage used at most two independent processes; Cargo test concurrency/flags were not changed.

| Public reviewer fixture | Compile/run | Result |
|---|---|---|
| request-marker |0/0|Original actual UI consent→start→status→pre-input stop preserves exact literal JSON in status/stored text/conversation |
| giant-request |0/0|Original explicit request first prose is present; full saved records equal |
| giant-answer |0/0|Original explicit assistant first prose is present; full saved records equal |
| legacy-answer-small |0/0|Previously failing known assistant/no-kind small fixture now retains authored sentence in default status and paint, labels Assistant, never Execution feedback |
| legacy-answer |0/0|Previously failing known assistant/no-kind giant fixture has the same correct prose/role/status; full records equal |

The first relinked small fixture's immediate previous-frame paint diagnostic was false while current status already contained the text. Before claiming painted clearance, I strengthened both role probes to wait for the same independently expected authored sentence and assert the actual Assistant label/no Execution feedback label. Corrected final logs both show status=true, paint=true, exit0; no stale-frame diagnostic is treated as a product failure or a pass claim. Original status/full-record expectations are unchanged.

Independently executed:

`/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-21/debug/deps/lightcraft_ui_egui-d7241b0d37537103 ai_companion::reading_tests --nocapture`

**15passed/0failed/0ignored,13.81s**, recorded in reviewer-21-role/reading-tests.log. This is the current compiled production test artifact, not a new build. It includes actual enriched-state hiding, four typed-giant combinations, four known-assistant-without-kind small/giant combinations, twelve released-old-hint reopen/native/fallback cases, unchanged source blocks/full model records, default intermediate/final legacy-user/tool privacy, Unicode/JSON strings/complete continuations, actual late-A/B native pixels, real upward wheel and terminal behavior, and all twelve EN/ZH glyph/control layouts.

Other reviewer commands:

- `git diff --check 32eebdbdc7a71456f7678230a85af8ae07ed8c4a..7ee3545b6c1c2ff5f7c63e619157197c56000040`: exit0.
- `cargo fmt --all -- --check`: exit0.
- `git merge-base --is-ancestor 32eebdbdc7a71456f7678230a85af8ae07ed8c4a 7ee3545b6c1c2ff5f7c63e619157197c56000040`: exit0.
- `git status --porcelain`; `git rev-parse HEAD`: empty; exact7ee, before/after.

Reviewed final worker logs:

| Exact worker command | Log under21/ | Result |
|---|---|---|
| cargo test -p lightcraft-ui-egui ai_companion -- --nocapture |role-ai-final.log|153/0/0;87.88s;24actual role-screens exported |
| cargo test -p lightcraft-engine companion -- --nocapture |role-engine.log|85/0/1existing helper ignored;28.43s;3actual child invocations1/0 |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |role-strict.log|exit0; no allowances added |
| cargo check -p lightcraft-engine -p lightcraft-ui-egui --target wasm32-unknown-unknown |role-wasm.log|exit0;7unchanged warnings,4inUI |
| cargo test -p lightcraft-ui-egui i18n -- --nocapture |role-i18n.log|30/0/0;format catalogs included |
| cargo xtask layers |role-layers.log|exit0;31crates/no violations |
| cargo xtask parity |role-parity.log|exit0;references valid |
| cargo fmt --all -- --check; git diff --check |role-fmt.log/role-diff.log|exit0 |

Worker environment: own ux-agent-21 target, CARGO_BUILD_JOBS=2, CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts; ordinary test concurrency, no new skips/allowances. Prior fix-docking3/0 and fix-shutdown1/0 remain relevant without a rerun claim; this role correction changes no docking/exit geometry. Independently inspected fresh role-screens return-normal-en and input-float120-zh-hans, both readable; prior original/fix screenshots and their recorded inspections remain retained. The independent fifteen-test run executes every twelve-layout glyph/control combination. No new i18n key or geometry/renderer feature.

## Recommendations

Proceed to the separately authorized merge/integration certification for this exact candidate. Preserve the shared role-aware helpers and the twelve released-old-hint cases when combining10/15/18/19; final22 acceptance remains separate. No further current21 fix required by this review.

## Declined to judge

All ten original declined lines and their explicit root rulings remain retained exactly as recorded in21-review.md,21-review-fix.md and review-rulings.md: required future10/15/19/18/22 combinations, supported/unsupported Store capability boundaries, honest known-v1 lock evidence, basic Markdown scope and physical-platform/provider/power-loss limits. None is silently dropped or newly broadened.

**Additional line:** Compatibility with empty assistant version1 artifacts generated only by the discarded, never released or approved b7 candidate is set aside under root's explicit intermediate-artifact ruling. Trustworthy version1 authored association/privacy remains unchanged; released unversioned empty/cropped hints and current supported known-role messages were judged and independently passed. No additional schema/migration is required for unmerged intermediate fixtures.

## Assessment

**Ready to merge: Yes, for whole UX-21 at7ee3545b6c1c2ff5f7c63e619157197c56000040.**

All three accepted authored-text/role findings are independently cleared, current public reading/privacy/compatibility checks pass, and the narrow shared implementation is consistent across actual callers. This is review clearance only; integration remains32e until a separate merger role is explicitly authorized and performs its checks.


---

# UX-21 merge and fresh integration certification

**Reviewed candidate merged cleanly; all16 assigned integration checks passed.** Root-owned tracker was not edited or resolved. No push, PR, original-checkout reset, worker cleanup, other WIP import or next-ticket implementation was performed.

- Reviewed clean candidate: `7ee3545b6c1c2ff5f7c63e619157197c56000040`, codex/ux-21, `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-21/lightcraft`.
- Whole reviewed range: `32eebdbdc7a71456f7678230a85af8ae07ed8c4a..7ee3545b6c1c2ff5f7c63e619157197c56000040`.
- Clean integration before: `32eebdbdc7a71456f7678230a85af8ae07ed8c4a`, codex/ai-companion-ux, `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
- Merge command: `git merge --no-ff 7ee3545b6c1c2ff5f7c63e619157197c56000040 -m 'UX-21: merge readable scoped long conversation history'`.
- **Integration after: `499e52b1da80187b4f2673066f751993211c6fc9`.** Exact parents: original32e and reviewed7ee. No conflicts or merge-only repairs.
- `git diff --exit-code 7ee3545b6c1c2ff5f7c63e619157197c56000040 HEAD`: exit0, no source difference. Worker remained7ee/clean; integration remained499e/clean after all checks.

The separately authorized merger followed merger-context.md, context.md, execution.json, applicable repository AGENTS/collaboration instructions and current review-rulings. Review reports21-review.md,21-review-fix.md and final21-review-role-fix.md retain the genuine failures, fix evidence and independent public re-clear. Only approved21 was imported; pending10/18/15 work was not consumed.

## Fresh checks on actual merged source

Environment: `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`, `CARGO_BUILD_JOBS=2`, `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`, `UX21_SCREENSHOT_DIR=/tmp/lightcraft-ux-execution-20261013/merger/21/screens`. Normal test concurrency and original cases retained; no skips/allowances/flag fingerprint churn. Port18999 reserved but unused because accepted UI execution used actual procedural Headless app/control frames and owned native filesystem/HTTP fixtures.

Exact orchestration, logs and machine-readable results: `/tmp/lightcraft-ux-execution-20261013/merger/21/run_checks.py`, `summary.json`, per-name `.log` files. All16commands exited0.

| Exact command | Actual result | Log |
|---|---|---|
| cargo fmt --all -- --check |Passed;2.67s |fmt.log |
| git diff --check 32eebdbdc7a71456f7678230a85af8ae07ed8c4a..HEAD |Passed |diff.log |
| cargo test -p lightcraft-ui-egui ai_companion:: --lib -- --nocapture |**153passed/0failed/0ignored**;83.14s test execution,227.21s including affected compilation |ui.log |
| cargo test -p lightcraft-ui-egui tests_companion:: --lib |**15passed/0failed/0ignored**;0.16s |commands.log |
| cargo test -p lightcraft-engine companion --lib -- --nocapture |**85passed/0failed**,10.84s;87.93s including compilation. Outer existing child helper ignored, but supervising test actually ran **3 real child probes**, each1passed/0failed |engine.log |
| cargo test -p lightcraft-ui-egui i18n:: --lib |**30passed/0failed/0ignored**;1.79s, format catalogs included |i18n.log |
| cargo test -p lightcraft-engine tests::reset_edits_restores_a_raws_import_defaults --lib -- --exact |**1passed**, actual import-default Reset |reset.log |
| cargo test -p lightcraft-engine tests::versions_history_and_reset --lib -- --exact |**1passed**, ordinary versions/History/Reset |history.log |
| cargo test -p lightcraft-engine tests::undo_and_redo_show_the_photo_they_change --lib -- --exact |**1passed**, ordinary Undo/Redo selection behavior |undo-redo.log |
| cargo test -p lightcraft-ui-egui --test docking_workflows |**3passed/0failed**;2.80s,68.35s including compilation |docking.log |
| cargo test -p lightcraft-ui-egui --test shutdown |**1passed/0failed**;0.74s |shutdown.log |
| cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings |Passed;19.07s, no lint exemption |clippy.log |
| cargo check -p lightcraft |Passed;10.84s, actual native host/dependency path |native.log |
| cargo check --target wasm32-unknown-unknown -p lightcraft-ui-egui -p lightcraft-web |Passed;9.42s, affected UI/engine dependency and Web initializer |wasm.log |
| cargo xtask layers |Passed;**31crates/no violations** |layers.log |
| cargo xtask parity |Passed;**566rows/1235references**, all actual command/control/path references exist.103.86s includes its nested CLI registry build |parity.log |

Fresh affected compilation/checks are reported, not a blanket workspace/full-platform rebuild. WASM retains7recorded warnings in unchanged codecs/jxl_encode, preview/pool, engine/devices and UI/lightroom_import code (4UI); no strict WASM lint or browser runtime claim. No failure was waived or skipped.

## Current whole21 behavior and preserved combinations

- Fresh153includes chronological user/assistant/system/local feedback rows, basic semibold/bullets and numbered prose, default folded tools and unverified fragments, bounded native/fallback selected metadata, actual visible-only own recorded JPEG/inline previews and isolated bad-image placeholders, immutable full-header/epoch/generation/descriptor cache ownership and real late-A/red→B/blue pixel separation.
- Original R1 is fixed in actual prepare_input at ai_companion.rs:1133: authored_text_bytes associates only truly appended state; submitting/pre-input cancellation preserves exact user-authored marker/JSON. Actual enriched preview/input keeps generated exposure0.7 in full model text while hiding it from prose. No complete settings-tree parse in the display helper.
- Original R2 and the later legacy known-role finding are fixed through companion.rs:400/415 helpers shared by UI ordinary/fragment/default status and span writer. Explicit roles/kinds remain literal; legacy user feedback keeps default privacy. New optional companion-sidecar span version1 does not alter core catalog. Twelve released-unversioned empty/cropped native/fallback reopen cases preserve exact record/source-block bytes, first authored JSON/UTF-8 prose and final continuation.
- Real pointer-then-single-wheel delivery immediately stops following from a genuinely scrolled history; actual round.ended preserves settled pixels/follow intent. Explicit bottom reaches the real final oversized fragment. Normal initial last32 pagination remains distinct from explicit last-record bottom.
- Existing12 independent photo draft/cursor196/count260/epoch/localpixels/follow restoration retains the same painted literal/relative position through tool/photo/reopen. Existing11 complete legal model projection/budgets remain separate from presentation. Existing16 frozen card values and17 explicit bound Undo,20 readonly original-A/default-history scope and explicit-other-photo semantics are freshly retained in153and15command tests; ordinary History/UndoRedo/Reset passed separately.
- Actual09native lock/reader/view retirement supervision ran3real child processes, and fallback/native captured guard tests remain in153/85. Preview cancellation does not pretend the actual producer retired; queued/cache decoded views retain their guards. Required later15 exit waiting and13 obsolete descriptor/clear retirement remain separate assignments.

## Fresh inspected UI pixels

The actual integration153run exported **24fresh CPU Headless PNGs** under `/tmp/lightcraft-ux-execution-20261013/merger/21/screens/`: EN/ZH × normal/narrow/floating120/240/stacked120/240 × return/input. Tests independently require full return-label and typed Chinese draft glyph bounds through actual widget/wheel/input interactions; compact bodies scroll to expose controls.

Direct merger pixel inspections:

- [return-normal-en.png](/tmp/lightcraft-ux-execution-20261013/merger/21/screens/return-normal-en.png): visible You/Assistant/System rows, semibold Natural colors, bullet Preserve sky, closed Tool process and Back to bottom; ordinary canvas/filmstrip and composer controls remain readable.
- [input-float120-zh-hans.png](/tmp/lightcraft-ux-execution-20261013/merger/21/screens/input-float120-zh-hans.png): complete Chinese draft visible in the120px floating Tools pane after actual scrolling, with normal photo/library surface preserved.
- [return-stack240-zh-hans.png](/tmp/lightcraft-ux-execution-20261013/merger/21/screens/return-stack240-zh-hans.png): complete Chinese 回到底部 label and reachable history controls inside stacked Tools, ordinary Library remains present.

Original worker/reviewer screenshot inspections remain recorded separately. These are fresh merged-source Headless pixels, not a physical window/compositor claim.

## Retained scope and limits

Whole10 giant/rawPNG/staged migration and original references must integrate with this approved single captured preview consumer; no unapproved10 code was imported.15all-exit actual producer/queue/cache wait,13confirmed clear/obsolete task/view/revert retirement,18durable authority/fault matrix,19real paired historical composition, and22final paired native acceptance remain required. No assigned future requirement was waived by this merge.

All original ten Declined-to-judge root rulings remain retained. The additional explicit ruling remains: discarded never-released/approved b7version1 empty assistant spans are not a new compatibility requirement; trustworthy version1 authored association/privacy stays intact, while released unversioned hints/current supported messages were judged and passed. Unsupported custom Stores with no owned reader may locally show unavailable previews without blocking GUI I/O; supported native/legacy/inline paths retain current coverage.

No paid-provider/model-quality/deployment-load, physical power-loss, physical OS wheel/compositor, Windows/Linux packaged runtime or browser backend runtime certification is claimed. No .env or user originals were accessed. This report certifies assigned21on local merged source; root owns tracker resolution, cleanup and next-ticket scheduling.
