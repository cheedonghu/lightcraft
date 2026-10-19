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

Pending the parent implement-spec run's independent Standards and Spec review
against the fixed baseline above. This section does not imply those reviews
have already passed.
