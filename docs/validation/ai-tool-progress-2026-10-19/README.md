# AI tool progress acceptance — 2026-10-19

This delivery covers the October 19 tool-process addition (Q31–Q38), separately
from the October 15 client UX delivery and October 18 process container. The
implementation starts at `73c005ecf3780f7a59808a29cbef58920ccde9d3`.
The originating acceptance matrix is
[`ai-companion-tool-progress-review-2026-10-19.md`](../../../../docs/ai-companion-tool-progress-review-2026-10-19.md)
in the parent project. The new five-ticket tracker lives in the sibling
`photo-companion/.scratch/ai-companion-tool-progress/` directory.

## Public acceptance seams

The approved seams are the actual Headless/control/actions interface, HTTP/SSE
network adapter, Session/Store save/read/reopen, and the public FileLoader
adapter. Procedural demo photos and a temporary generated 32 × 24 PNG exercise
real dispatch, rendering and publication. No user library, model credential,
paid model request, or operating-system input automation is used.

The existing egui CollapsingHeader, AI-private translations, Instant timing,
Session archive and existing bounded history reader are reused.
Category/outcome translation is application-specific; no external dependency
improves this mapping. Display metadata remains local and never enters model
history. The necessary engine change only retains bounded scalar fields in
its existing history-display allowlist.

## TP01–TP11 evidence

All test names below are in `crates/ui-egui/src/ai_companion/`, except the
engine pagination cases. Assertions cross the approved seams; existing tests
are reused instead of duplicating each branch in a second suite.

| Acceptance | Concrete behavior tests |
|---|---|
| TP01 six read descriptions | `reading_tests::six_read_categories_deduplicate_and_new_body_preserves_separate_guidance_activity`; `photo_read_progress_replaces_paired_records_and_waits_truthfully_for_model` |
| TP02 distinct successful categories | The six-category test performs eight actual reads, including different color samples and an empty mask list; `write_progress_tests::continuous_read_and_write_progress_keeps_success_count_during_prepare_apply_and_publish` excludes writes and gives the subsequent review its own count. |
| TP03 waiting and prose boundaries | The same live tests verify stopped tool execution, continuing model analysis, stable contiguous segments, and a real published edit followed by new review prose/read. |
| TP04 knowledge and pure answers | The six-category test keeps guidance separate; `interruption_progress_tests::explicit_knowledge_failure_expands_without_stopping_round_or_claiming_fallback`; `redesign_tests::live_process_expands_then_auto_collapses_preserving_final_answer_and_reopening_history`. |
| TP05 measured/unknown duration | `native_history_tests::timing_tests::real_read_timing_survives_reopen_and_replay_without_entering_model_input`; `historical_summary_uses_complete_measured_calls_and_rejects_mismatched_facts`; `complete_tool_batches_at_page_boundaries_never_claim_the_visible_subtotal_is_the_segment_total`. |
| TP06 truthful write stages | All eight `write_progress_tests` cover preparation, gated actual rendering, publication, rejection, correlated feedback failure and positive backend confirmation. The continuous journey also reviews the actual published result and preserves its final card. |
| TP07 attempted failures | `interruption_progress_tests::attempted_read_failure_expands_inline_without_inventing_success_or_fallback`; `partial_read_failure_retains_information_and_reason_outside_manually_folded_process`; `rejected_read_guard_is_not_an_attempted_tool_failure`. |
| TP08 terminal causes | `stopped_timed_out_and_disconnected_reads_keep_successes_without_failed_unexecuted_claims` covers all three causes with zero/partial successful reads and checks the actual unchanged-photo notice. |
| TP09 outer folding and final answer | The continuous journey and six-category test check normal collapse and final answer/card retention. The knowledge-error test checks one-shot failure expansion, manual folding across termination, and successful final answer after a recoverable error. |
| TP10 durable bounded history | All six timing tests; `native_history_tests::native_stop_and_failure_reopen_next_input_with_every_true_tool_result_in_original_order`; the affected paging and network suites. Original tool text/order is retained; duplicate/late/mismatched records cannot invent counts or elapsed totals. |
| TP11 language, width, ownership, scroll | `photo_tool_single_line_stays_within_bilingual_narrow_panel`; the continuous journey's actual glyph/rectangle checks in English/Chinese at 1000/1400 px; `reading_tests::upward_reading_survives_stream_completion_until_explicit_back_to_bottom`; `timing_tests::paged_result_fragments_and_photo_switch_do_not_invent_complete_segment_time`; affected paging tests. |

## Verification results

The following table records the initial delivery at `cb825780`. Post-review
source changes and their final verification are recorded in Final review below.

Commands use `CRAFT_FONTS_DIR` pointing to the existing craft-fonts checkout and
an isolated `CARGO_TARGET_DIR=target/agent-tool-progress-05`. Rust compiler:
`rustc 1.99.0 (b940084d7 2026-09-28)`, macOS Apple Silicon.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | Passed. |
| `cargo clippy -j 2 -p lightcraft-ui-egui -p lightcraft-engine --all-targets -- -D warnings` | Passed, no warning exemption. |
| `cargo build -p lightcraft --bin lightcraft` | Passed, native desktop binary built. |
| `cargo check -j 2 -p lightcraft-web --target wasm32-unknown-unknown` | Passed; seven warnings in unchanged source files, detailed below. |
| Actual native UI test binary, eleven affected AI test families, `--test-threads=2` | **110 passed, 0 failed**, 126.41 s. Families: reading, write progress, interruption progress, native history (including six timing tests), paging, redesign, network, knowledge, disconnect, identity and result cards. |
| Final fixture-related rerun after live-state correction | **37 passed, 0 failed**, 17.43 s (reading, interruption progress and write progress). The other 73 unchanged cases reuse the combined run above. |
| `cargo test -j 2 -p lightcraft-engine --lib companion::pagination_tests::oversized -- --nocapture` | **2 passed, 0 failed**, 0.45 s: bounded readable fragments and escaped text without raw-record exposure. |
| Final screenshot-specific public behavior run | **4 passed, 0 failed**, 6.43 s; these repeat four cases from the 110, rather than adding four distinct tests. |
| `git diff --check` | Passed. |

The native test binary was built with
`cargo test -j 2 -p lightcraft-ui-egui --lib --no-run`; direct invocation uses
normal libtest substring filters for the families above. It does not select
the whole UI crate or workspace. Logs and exact argument arrays are retained
in `/tmp/ai-tool-progress-05/`, including `combined-ui.log`, `native-build.log`,
`wasm-check.log` and `final-results.json`.

WASM warnings concern `codecs/src/jxl_encode.rs` (unused imports),
`preview/src/pool.rs` (unused constant), `engine/src/devices.rs` (unused mut),
and `ui-egui/src/lightroom_import.rs` (four unused import/dead-code warnings),
all under `crates/`. A diff against the fixed baseline confirms these four
files are unchanged. The old baseline's complete WASM build was not rerun;
these warnings are reported, rather than claimed as a separately reproduced
baseline check. WASM compilation does not establish browser runtime behavior.

The initial strict clippy run found a redundant `usize` cast in an existing AI
reading test, also present at baseline line 178. Removing that test-only cast
allowed the strict rerun to pass. The live reading fixture also now emits the
actual `analyzing_initial` event after completed input; this corrects evidence
that previously still displayed the uploading phase.

One concrete visual regression was fixed during acceptance: the English outer
title elided to `Processing history · contains to…` at 1000 px, hiding the tool
hint. A public Headless assertion on rendered glyphs failed first; shortening
the title to `Processing · tool activity` made that assertion pass in both
languages and widths. Chinese wording and existing folding behavior are
preserved. The real-render journey was then extended through a separate
review read and normal final answer/card completion, without changing the
execution model.

## Visual inspection

The final four-case public run generated **28 actual Headless PNGs** under
`/tmp/ai-tool-progress-05/screens/`, covering English and Simplified Chinese at
1000 × 1000 and 1400 × 1000. It refreshed the phase-correct live reading shots,
fixed measured-history summaries, actual prepare/apply/publish states,
partial failure and the completed read→apply→review journey. Screenshots are
temporary evidence, not committed product assets.

Inspected examples include `tool-progress-en-1000.png` (correct analysis
phase; outer tool hint visible), `tool-timing-en-1000.png` (count plus literal
2.1 s fully visible), `tool-mixed-applying-zh-hans-1000.png` (count plus actual
application stage visible), `interruption-partial-en-1400.png` (expanded real
failure and unchanged photo state), and both completed-journey locales
(normally folded process, final answer and actual result card visible).
The timing-history 2.1 s is an explicit saved test fixture, not an estimate
of an actual model run. Long secondary explanations can elide with a tooltip;
the essential count/time or count/write-stage and outer tool hint have actual
glyph assertions, not only source-text assertions.

Sixteen final `ui.inspect` observations reported frameMs **0.373–0.528 ms**.
Reading/history/failure fixtures used Apple M1 Pro Metal; the real edit journey
uses the existing CPU-render preference to gate actual file loading and
publication. These are observed UI frames, not a comparative benchmark or a
claim about cold rendering, model latency or all-machine performance.

## Scope and limitations

Only the AI modules, their immediate integration tests, and bounded engine
pagination are exercised. Unaffected upstream suites, whole-workspace checks,
extreme-capacity tests, real model quality, Windows/Linux runtime behavior and
browser runtime behavior are not claimed as locally verified. Native and WASM
build outcomes are listed above separately.

## Final review

Independent review at `6323d801` compared the delivery with the fixed baseline:
Standards found **zero hard violations and three P3 heuristic findings**;
Spec found **one P2 omission**. The implementation dispositions are:

| Finding | Disposition |
|---|---|
| P2 synthetic closure omitted the explicit unexecuted display outcome | `close_pending_tools` now adds local `display_outcome:not_executed`; original model-facing result text is unchanged. Expanded details retain each readable activity name and the explicit unexecuted fact, including pending photo reads, knowledge and writes. A knowledge-only segment cannot claim a response was recorded. |
| P3 repeated category/planned/completed/failure switches | One AI-private `ToolDescriptor` lookup owns those facts. Legacy generic fallbacks are retained. |
| P3 English copy used as write-status identity | Private `WriteStatus` has full and compact label accessors. Persisted state and protocol strings do not change; changing full copy cannot silently choose a different compact state. |
| P3 duplicate boundary predicates | Outer range proof reuses `progress::boundary` and separately retains the existing complete-row changed-round rule. Adjacent-page gaps, fragments, conservative Range proofs, folding and stable control IDs are unchanged. |

The new public native regression
`native_history_tests::unexecuted_photo_and_knowledge_tools_reopen_as_unexecuted_without_rewriting_model_results`
stops before dispatch, saves/reopens the native library, expands both containers,
and checks English/Chinese activity identity plus non-execution at both widths.
It then submits the original paired tool text/order to the next model input
and confirms local metadata is excluded. It covers a pending photo/knowledge/
write batch and a knowledge-only batch. No additional test seam was introduced.

The first behavioral red painted generic read labels rather than unexecuted
outcomes (4.94 s). After the minimal outcome fix passed, screenshot inspection
caught indistinguishable anonymous rows. Strengthening the same public test
failed on the missing activity names (6.77 s); details now combine the existing
human activity label and unexecuted result. These are actual UI regressions,
rather than compile failures or tests against private presentation helpers.
The three small refactors took place during review, after the first P2 green.

The first affected-suite run exposed two older interruption tests that still
required the local outcome to be absent. Those expectations were updated to
require explicit `not_executed` and no execution duration, while retaining all
their real UI/stop/timeout/disconnect and actual-result assertions. This is
distinct from changing or weakening the model-facing tool result.

| Post-review check | Actual result |
|---|---|
| Same eleven affected AI families, plus the new native case | First run: **109 passed, 2 failed**, 107.70 s. Both failures were exactly the old missing-outcome assertions described above; no other failure was observed. |
| Final `ai_companion::interruption_progress_tests::` after expectation correction | **5 passed, 0 failed**, 2.69 s. Combined with the unchanged 109 passing cases, all **111 distinct affected UI cases** are covered. This is not claimed as one clean 111-case command. |
| Strengthened native unexecuted regression | **1 passed, 0 failed**, 18.43 s after the activity-name fix, including eight actual screenshots, native reopen and next model input. It is included in the 109 above. |
| Four representative existing screenshot cases | **4 passed, 0 failed**, 8.88 s; existing timed/count/write/failure/complete snapshots and actual glyph checks refreshed. They are subsets of the affected UI coverage. |
| Incremental native desktop build | Passed, 1 m 05 s. |
| Incremental WASM check | Passed, 10.56 s. Warnings remain in the same unchanged files described above; browser runtime remains untested. |
| Final strict native clippy after expectation correction | Passed with `-p lightcraft-ui-egui -p lightcraft-engine --all-targets -- -D warnings`, 5.48 s; no warning exemption. Format and diff checks also passed. |

Post-review logs use `/tmp/ai-tool-progress-05/review-*.log`; exact build/lint
arguments are in `review-check-results.json`. The final evidence includes
**36 PNGs**: the original 28 refreshed plus eight named unexecuted activity
cases. Inspected new narrow English/Chinese three-tool details identify the
histogram, guidance and adjustment activity beside their unexecuted outcome;
English wraps naturally without eliding either fact. Knowledge-only details
likewise identify the reference activity and contain no recorded-response
claim. The existing mixed applying caption and measured Chinese history were
also inspected after refactoring. Twenty-four observed final frameMs samples
range **0.389–0.621 ms**, with the same non-benchmark limitation above.

The unchanged engine pagination source reuses its two passing checks above;
no whole-workspace, unrelated upstream or extreme-capacity suite was added.
Final independent rechecks at source commit `d590386a` confirmed all four
dispositions. Standards: 0 hard violations and all 3 P3 suggestions resolved.
Spec: the P2 unexecuted-detail gap resolved with no new deviation. Both axes
have **0 unresolved findings**. The [full two-axis report](review.md) retains
the initial reports and independent final source rechecks.

The merger separately reran the new native unexecuted regression (1 passed,
13.01 s) and interruption family (5 passed, 2.75 s) on the same source tree;
format and diff checks passed. Other unchanged-tree evidence above is reused.
