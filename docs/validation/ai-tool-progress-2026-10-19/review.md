# Independent final code review

Baseline: `73c005ecf3780f7a59808a29cbef58920ccde9d3`.
Initial review tip: `6323d801`; independently rechecked source tip: `d590386ae4937e1f2fe7bc00f8532678c221a4de`.
The source was unchanged by the subsequent documentation commit.

## Standards

### Initial review


Reviewed `git diff 73c005ecf3780f7a59808a29cbef58920ccde9d3...HEAD` at `6323d801`, including all changed production and test files. Sources: `AGENTS.md`, `docs/collaboration.md`, `docs/architecture.md`, README, and the supplied smell baseline. No hard documented-standard breaches found. Tooling-enforced matters are excluded. Scope/isolation, existing egui/i18n reuse, procedural fixtures, and recorded targeted verification follow the repository rules. The referenced craftrules checkout was absent and its never-crash raw GitHub URL returned 404; the relevant never-crash rules are reproduced in AGENTS.md.

The following are **P3 heuristic judgement calls**, not hard violations:

1. **Possible Repeated Switches** — `crates/ui-egui/src/ai_companion/reading/progress.rs:300–345`. Four new functions independently classify the same tool names: `category`, `planned`, `completed`, and `failure_label`. Concrete added hunks include `"get_photo_state" => "Preparing to view photo state"`, `"get_photo_state" => "Viewed photo state"`, and `"get_photo_state" => "Reading photo state failed"`. Adding a category now requires coordinated edits across those mappings. A single AI-private descriptor lookup containing category and label keys would keep the mapping together without changing the protocol format.

2. **Possible Primitive Obsession** — `reading/progress.rs:77–94,285–297`. `write_label` returns user-facing English text that `compact_write(label: &str)` then treats as a state discriminator: `"Adjustments applied; awaiting confirmation" => "Applied; confirming"`. Copy edits can silently fall through `_ => label`, dropping the compact caption needed in narrow panes. A small presentation status type with full/compact label accessors would keep display wording separate from status identity; persisted JSON can stay unchanged.

3. **Possible Duplicated Code** — `reading.rs:188–195` and `reading/progress.rs:103–109` repeat the boundary shape `fragment.is_none()` plus user / `execution_feedback(message)` / nonempty assistant text checks. The outer check additionally accepts a changed round. Reuse the common boundary predicate and layer the round condition outside it so completeness/counting boundaries cannot drift between the two paths.

Summary: 0 hard violations; 3 optional heuristic findings. No tests were rerun for this read-only review.

### Final independent recheck


Pinned HEAD: `d590386ae4937e1f2fe7bc00f8532678c221a4de`. Reviewed the actual six-file `git diff 6323d801...d590386ae4937e1f2fe7bc00f8532678c221a4de`, against the original review and main baseline `73c005ecf3780f7a59808a29cbef58920ccde9d3`. Read the fix notes and validation dispositions separately; conclusions below come from the source diff.

All three original P3 heuristic findings are resolved:

1. **Repeated Switches:** `reading/progress.rs` now has one `descriptor(name)` match producing `ToolDescriptor { photo_information, planned, completed, failure }`. The four consumers use that lookup, with existing unknown-tool fallbacks preserved. The tool-name facts no longer require four coordinated switches.

2. **Primitive Obsession:** private `WriteStatus` represents the write presentation states. `labels()` owns full/compact pairs; callers use `status.full_label()` and `status.compact_label()`. The old `compact_write(label: &str)` text-discrimination match is removed. Persisted/protocol state strings remain at the adapter boundary.

3. **Duplicated Code:** the outer range proof calls `progress::boundary(previous)`. Its additional `previous.fragment.is_none() && previous.round().is_some_and(...)` rule remains separate. `Row::round()` requires a message, so this preserves the original message-presence guard; the adjacent-page gap condition also remains intact.

The explicit local `not_executed` metadata, activity-labelled details, and new native reopen/input regression remain within the AI module and approved seams. The two changed interruption assertions require the new metadata while retaining execution and elapsed-time checks. No additional documented-standard breach or actionable baseline smell found in the follow-up diff.

**Unresolved: 0 hard violations, 0 heuristic findings.** No source edits or test reruns were performed. The original report is retained.

## Spec

### Initial review


Reviewed `73c005ecf3780f7a59808a29cbef58920ccde9d3...6323d801`, the originating Q31–Q38/state table/TP01–TP11, canonical client spec, tracker and delivery evidence. Source inspection was read-only; no competing build or real model/data call.

#### Implemented incorrectly / partial requirement

- **[P2] Unexecuted calls lose their actual outcome inside expanded details.** Spec Q34 says “区分失败、停止和未执行”; the state table says “工具明细使用可读活动及实际结果” (originating spec lines 39 and 70). New `crates/ui-egui/src/ai_companion/reading/progress.rs:273–276` recognizes unexecuted results only through `display_outcome == "not_executed"`. However the real termination producer, `ai_companion.rs:1893`, writes `executed:false` inside the retained tool text and no `display_outcome`. Consequently every synthetic closure falls through to generic `tool_activity`, such as “Read photo histogram” / “读取照片直方图”, rather than “Tool was not executed”. For knowledge calls, lines 187–191 additionally call the synthetic cancellation a “Photography guidance response recorded”. Reproduce with the existing stop fixture: emit `message.completed` containing `get_photo_histogram` (or `skill`), stop before its tool request/result, then expand both outer process and inner tool segment. The segment has the stop cause, but its individual activity cannot distinguish an unexecuted call from an actual read. Preserve the structured unexecuted fact in local display metadata (without changing original text/model history), and assert the expanded inner row through the public UI seam. Existing interruption tests open only the outer process, so miss this branch.

#### Missing requirements / scope

No other concrete missing requirement or unrequested behavior identified within the incremental scope. Fixed execution timing, conservative page summaries, category deduplication, actual publication state and normal final-answer/card placement are supported by the inspected implementation. These conclusions do not claim independent execution of the delivery tests.

### Final independent recheck


**Unresolved Spec findings: 0.** Read-only recheck at fixed HEAD `d590386ae4937e1f2fe7bc00f8532678c221a4de`, using `git diff 6323d801...HEAD`; original review preserved.

The P2 is resolved in production code. `ai_companion.rs:1893` adds only local `display_outcome:"not_executed"` to synthetic closures; the original protocol text, IDs, round/stage/model association and call/result order remain intact. `reading/progress.rs` checks that fact before write-state rendering and before generic activity rendering. Expanded photo, knowledge and write rows now identify the activity alongside its explicit unexecuted outcome. Knowledge-only cancellation uses the unexecuted summary rather than “response recorded”; an unexecuted write cannot claim application. These changes implement Q34 “区分失败、停止和未执行” and the state-table requirement “工具明细使用可读活动及实际结果”. Model projection still builds an explicit wire object and excludes display metadata.

The new public native regression actually sends the call event, stops before dispatch/results, saves and reopens FsStore, expands both UI containers, checks activity/outcome glyphs in Chinese and English at both widths, and verifies the next network input retains original paired result text/order without display metadata. It covers mixed photo/knowledge/write and knowledge-only batches. This directly covers the previously missed nested-detail branch.

No new Spec deviation found in the three refactors. ToolDescriptor preserves the same six counted categories, existing planned/completed/failure strings and unknown-name fallback. WriteStatus preserves full/compact labels and publication-versus-feedback distinctions. The shared boundary predicate remains complete-row-only. The outer changed-round boundary still requires a nonfragment row; adjacency gap checks, start/end-known Range fields and category gating on proven complete segments remain unchanged. Thus partial pages cannot acquire invented whole-segment counts or times.

I inspected source and the revised evidence; I did not rerun suites or builds while the merger was validating. The README accurately separates the initial 109-pass/2-failure run from the corrected five-test rerun instead of claiming one clean 111-case run. No additional missing requirement or scope creep identified within this corrective diff.

Initial findings: Standards 0 hard violations / 3 P3 heuristic suggestions; Spec 1 P2 issue. All were addressed. Final unresolved findings: Standards 0; Spec 0.
