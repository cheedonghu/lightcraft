2026-10-14：UX-20 整项已完成、独立审查及修正复审通过，并合入客户端 codex/ai-companion-ux，集成提交 32eebdbdc7a71456f7678230a85af8ae07ed8c4a。

服务检测只在首次打开或显式重试时异步执行，检测可达性，不自动发起 AI。面板外入口保留原任务照片、阶段和结果；原图不可用时可只读查看、停止原任务及返回当前，保留其他照片的选择、效果、草稿与阅读位置。错误文案按本机实际成功步骤描述效果及手动恢复，冷历史诊断可复制，心跳与模型文字分别观察。

独立审查发现并修复了只读 A 时无照片参数翻页误改 B 游标的问题；真实红绿和独立复审验证默认展示范围与显式指定范围。集成新跑 138 项 AI、15 项命令、85 项引擎、3 个真实读锁子进程、30 项语言、3 项普通编辑回归均通过，严格 lint、原生/Web/WASM、格式、分层和引用检查全部通过。36 张中英文布局截图均有实际控制验证，并独立检查了代表性图片。

保存、迁移、跨重启撤回、确认删除、历史对比及最终跨端验收仍由各自未完成工单承接；本项未宣称真实模型、物理剪贴板/窗口、Windows/Linux 运行或断电认证。以下保留实现、审查和合并证据，临时截图将由最终 22 项交付记录归档。


---

# UX-20 implementation evidence

**Whole UX-20 candidate complete; pending fresh independent review.** Clean fixed candidate `e20c530f1cbc5e37fa56f844d18da625a7b7668d`, branch `codex/ux-20`, worktree `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft`. Actual approved integration ancestor/review base `3e0d88e29a0e0bf559188606e89e1ebc364d282d` (UX-09/16/17 preserved). `git status --short` empty after commit. Candidate has not been merged back to integration, pushed, published or used to edit the tracker.

Result: asynchronous scoped first-open/explicit health GET with latest probe winning; original-photo/stage/truthful terminal global task entry outside Tools; explicit readonly unavailable-task navigation/Stop/Return preserving current selection/draft/reading/effects; actual acknowledged heartbeat separate from model silence and real-disconnect termination; localized actual-effect/manual recovery and verified scoped diagnostic copy including cold history. No automatic submit/reconnect/resume or selection/popup. Completed the whole ticket, not a type-only/interface-only slice.

Checks: original whole AI137/0, then independent-review fix whole AI138/0; final fixture-only stronger recovery1/0; engine85/0 plus three real reader-lock child probes; i18n30/0; strict UI lint/native host/fmt/whitespace/parity/layers/WASM28safe crates including Web all passed. Thirty-six latest procedural EN/ZH screenshots and all absolute paths are listed below. Every unsuccessful/harness attempt is identified rather than claimed passed. First independent review found an omitted-photo ai.history isolation bug; the genuine public red and minimal fix are documented below and in20-fix-note.md. Fixed candidate remains pending independent re-review.

Environment for Cargo checks: `CARGO_BUILD_JOBS=2`, `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-20`, `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`; normal Rust test parallelism. Real owned HTTP/SSE fixtures use bounded ephemeral loopback listeners; no user service/config secrets or paid models.

Base: 72717addc30f439d753989512674c0e800866cbc. Own branch codex/ux-20.
Installed tdd and codebase-design plus support files read. Existing agreed seams applied. No new product decisions or external/payed models.

## Cycle 1: first open service feedback
Seam: actual `ai.open`, full headless frames/widgets and `ai.status`. Catches first-open health feedback and actionable retry in real shared Tools; misses other-platform OS compositor and live provider readiness. Genuine red: expected visible Retry widget missing in current implementation. Minimal green asynchronously probes only HTTP reachability; consent/submit unchanged.

Planned module depth: health owns parsed loopback ServiceEndpoint, cancellation/worker bound, latest probe generation and observable result; its caller uses request/poll/view. Shared endpoint parsing is used by both actual connection and HTTP health adapter. Host Services configuration can supply endpoint for an embedding host or owned ephemeral fixture; otherwise existing PHOTO_COMPANION_ADDR then127.0.0.1:18080 fallback. No JSON address setter, run identity/consent, model-ready claim or retry submission.
Global entry uses an independent top strip below existing chrome so narrow search/menu/right icons do not overlap. Original taskscope/name remains after terminal Job removal, original stage+truthful terminal remain visible, viewing/new task clears only unviewed reminder. Explicit navigation validates current library generation and source photo before selection/revealing Tools. The strip cannot initiate a model call.
Cycle 1 actual red1 failed/green1 passed. Logs20/cycle1-{red,green}.log. First build4m49s; green separate compile.

## Cycle 2: visible global task navigation after background completion
Seam: actual consent checkbox, ai.start, injected owned HTTP/SSE adapter, full frames, original photo selection via Session command, ai.status and real global click. Catches correct original ownership, completion retention after Job removal, no auto-popup/selection, explicit navigation and reminder clearing. Misses physical OS compositor and live provider execution. Expected red: global task entry and ai.status task data are absent even when active/background terminal.
Cycle 2 actual red: original task is active but hidden Tools/other selected photo has no `button:aiTaskEntry`. Corrected an unreachable fixture event spelling before green (`text.completed`→real `message.completed` with preceding `text.delta`), not a product change.
Cycle 2 green1 passed: active entry visible while Tools hidden/B selected; terminalcompleted retains A and unread; original selection never changes until actual widget click, which opens A's AI conversation and clears reminder; no HTTP request caused by navigation. Logs20/cycle2-{red,green}.log.

## Cycle 3: failure wording and trace copy
Seam: actual consent/start, external SSE terminal event, full visible panel/status and real clipboard button. Catches structured reason presentation, truthful untouched starting settings, usable manual recovery, non-primary technical reason and copying actual available trace. Misses provider-quality decisions and physical OS clipboard integration. Expected genuine red: old terminal uses bare capability_unsupported and traceId text and has no copy control.
Cycle 3 genuine red bare reason_code/traceId; green1 passed clear unsupported edit cause + untouched starting settings + new request exit, no bare code in displayed text, actual trace copied to headless host clipboard. Logs20/cycle3-{red,green}.log. The local effect helper reads succeeded execution stages, never backend last-confirmed acknowledgement.

## Cycle 4: actual disconnect terminal
Seam: actual consent/start and owned transport disconnect event, full-frame ai.status/photo settings/global widgets. Catches immediate local disconnect termination, stopped fake processing, truthful original state and manual recovery; misses live provider and physical OS sockets (real native sockets added as verification). Genuine expected red: disconnect terminal is genericfailed and shows raw transport error rather than disconnected/manual recovery.
Cycle 4 actualred genericfailed/raw socket string; green1passed disconnected terminal, noStop control/no automatic request, original settings kept and service/manual recovery. Logs20/cycle4-{red,green}.log.

## Cycle 5: unavailable original task photo
Seam: actual consent/start/SSE completion, public Session delete/select, global widget click and visible readonly conversation/admission. Catches unavailable original ownership navigation without selecting unrelated same-ID/current photo, retained readable conversation, no send controls or callable admission in readonly mode. Misses OS file recovery and other-platform compositor. Expected genuine red: current global click silently leaves Tools closed for a deleted photo instead of showing readonly original conversation.
Cycle 5 actual red readonly entry missing; green passed retained original A answer, unchanged B selection, no send controls/common admission denied, explicit return restores B. Strengthened combination passes public saved B draft and nonzero reading-offset equality before/during/after readonly A. A first extension had a harness path typo using private editor module; corrected to public companion::EditorState, not a behavioral red. Normal background history load also follows scoped displayed A while readonly, avoiding loading B over A.

## Health actual HTTP verification
Seam: host Services configured loopback endpoint, actual ai.open/ai.health commands and frames against owned ephemeral TcpListener. Catches first-open asynchronous GET-only behavior, explicit newer probe replacing blocked older result and positive HTTP-only reachability with no automatic run identity/submit. Misses live provider, OS power loss and deployment readiness. This verifies the already implemented Cycle1 behavior, not a claimed additional red/green implementation cycle.
Health actual native verification1passed: blocked first GET does not block first-open feedback (<1s); retry GET2 becomes reachable; released older503 cannot replace newest success; no registration/round request.20/health-native-verification.log.

## Cycle 6: actual heartbeat vs silent model
Seam: host configured Services + actual consent/start/full frames across owned native HTTP registration/SSE/input/heartbeat fixture. Catches separate acknowledged heartbeat observability, continued analyzing for >30s with zero model text, native socket closure ending progression without auto-resume and no extra rounds. Misses paid-provider quality and deployment capacity. Expected genuine red: no independent heartbeat outcome is exposed to ai.status despite actual heartbeat HTTP success.
Cycle6 harness correction: the first red and attempted green had WouldBlock from a macOS listener-inherited nonblocking accepted socket. These runs do not count as behavior red/green. Preserved as20/cycle6-*-invalid-harness.log. Fixed adapter explicitly sets accepted streams to blocking+5s bounded reads/EOF assertions, reverted ALL new telemetry production changes to the preceding candidate, and now reruns the actual public missing-heartbeat red before restoring a minimal green. No successful>30s claim before verified run.
Cycle6 corrected genuine red passed handshake/input/analyzing and actual heartbeatHTTPsuccess, then publicstatus remained without separate heartbeat telemetry (1failed after5.82s;20/cycle6-red.log). Restored minimal separately acknowledged heartbeat event/telemetry and unchanged no-model-text-deadline behavior; public test now waits for exact observable heartbeat rather than racing request receipt against acknowledgement. Native priority-stop regression consumes/verifies1newHeartbeat event while retaining all9ordered request results.
Cycle6 corrected native green1passed: realGEThealth→registration→SSE→round→currentinput→acknowledgedheartbeat. Actual32s withzero modeltext remainsbusy/analyzing;≥6acknowledgedheartbeats; closingnativeSSEsocket givesdisconnected and exactly1roundPOST.20/cycle6-green.log. Telemetry is a production diagnostic/visible stage hint, separate heartbeat acknowledgement/time and model text age. No new model-response deadline. Connected localworker alive/closing is named connectionAlive, not model readiness.
## Cycle 7: asynchronous service layout must not invalidate a real consent click
Seam: owned blocked healthHTTP, actual last-frame checkbox rectangle, full-frame pointer down/up, ai.status and ai.start/observed external round request. Catches health completion between mouse down/up moving the checkbox or clipped paging labels; misses OS compositor. Existing full parallel regression red95passed/7failed showed actual consent clicks missed during asynchronous health layout growth plus narrow paging labels clipped. Two other assertions expected old raw reason/Chinese text (intentionally obsolete); one expected allbutton:ai insideTools (intentionally excludes the new global entry). Those test contracts will be updated precisely, not broadly skipped.
Cycle7 first minimal green attempt revealed the new test checked rectangle AFTER the consent action itself removed its normal admission notice (36px normal resulting layout change). Moved the geometry assertion to AFTER503/beforepointerup, then separately asserts real ai.start success/observed1roundPOST. Original32px health-only red remains valid. Precisely updated three existing contracts: typed external photo_busy fields + human wording/no bare code; actual disconnect English recovery; global button deliberately outsideTools while all othercomposer controls remain clipped/inside.
Approved16 combination checkpoint32858aa1 mergesactual03f8. Locale union preserved every key (one common Failed translation retains16失败). Source audit found the automatic merge retained an obsolete per-poll history cache traversal and the16 fallback card scanner still follows selectedB instead of readonlydisplayA; adaptation will preserve16's bounded scheduling and use the actual displayed scope.

## Cycle 8: explicit selection ends readonly old task conversation
Seam: actual task entry plus public Session/library.select/fullframes/editor read/status. Catches selectionC/multi/none exiting old readonlyA, restoring own draft/position without changing parameters or sending, and B editor untouched. Misses OS compositor and all source-format corpus. Expected genuine red: readonlyA persists after explicit selectionC and ai.status/view remainsA.
Cycle8 genuine red oldreadonlyA persisted after selectionC (photo2/statusviewed21). Green1passed C/multi/none, explicit sameBselection, new ai.returnCurrent command and real readonlyBack button. Each restores own draft/position, B0.7EV unchanged/no old-effect restore/noHTTP. Source16 adaptation uses actual displayed scope for fallback card scan/status qualification/loading; per-poll cache traversal removed again to preserve16. Real selection wrapper clearsmode even same-ID intent; directSession value change detected atcommand/poll/show. Navigation uses existing feedback deferral to avoid cold editor/history I/O during Stop first paint.

## Published-result verification
Seam: actual consent/start, observedinput and actual locally rendered edit/execution feedback, then backendfailed terminal with no last-confirmed acknowledgement, status/current parameters/global entry. Catches error wording keyed to locally published0.4EV and retained original task scope while another photo is selected; misses provider quality/real backend after-effect storage faults (09 owns durable fault recovery).
Published-result verification1passed: real rendered/published0.4EV, execution feedback notHTTP-confirmed, backendreviewfailed last_confirmed=null stillkeeps0.4, human current-result fact correct, hiddenTools/Bselection unaffected/globalAreminder retained, explicitentryclick opensA and actualtracecopies.20/published-result-verification.log.

## Cycle 9: diagnostics remain available in readonly original conversation
Seam: same actual locally published round and terminaltrace, public delete/select/globaltasknavigate, real clipboard widget. Catches unavailable-source readonly view still copying its own saved diagnostic, with B0.7EV untouched; misses physical OS clipboard. Expected genuine red: trace copy exists only in ordinary composer, absent when original task photo is readonly.
Cycle9 genuine red missing readonly trace widget; green1passed exact A diagnostic copied while deleted A shown and B0.7EV preserved. Logs20/cycle9-{red,green}.log.

## Cycle 10: readonly card navigation
Seam: same publicly published edit/trace, deleted A while selected B, actual result-card ordinary History button and ui.inspect. Catches A card mistakenly opening B's History; misses native OS input. Genuine red changed right panel to activity. Minimal green disables only ordinary History navigation for readonly/nonselected source; parameter folds remain enabled. Green1passed, A conversation stays open and B values unchanged. Logs20/cycle10-{red,green}.log.

## Cycle 11: readonly command mutations
Seam: public ai.clear/ai.undo and actual readonly frame. Catches default selected-B destructive/restoring commands while showing A; misses future UX-13 confirmation feature. Current footer already omits both actions, but public ai.clear still routes to selected B. Expected genuine red: ai.clear accepts readonly task view.

## Integration interfaces retained for remaining tickets
UX-15's later reliable unfinished summary must feed `task_entry::Task::new` with its verified captured PhotoScope/RoundIdentity for the original stored photo association and original filename, then retain the actual stage and interrupted terminal/unread state. Integrate this at the accepted recovery-summary application point; never infer scope from current selection and never reconnect/resubmit. Task display/navigation require the active Session library generation; UX-15 must supply the generation validated for its real captured recovery scope rather than blindly reusing a previous process's counter. UX-15 remains separate; its incomplete checkpoint is not consumed here. The runtime reminder is not a durable notification queue; durable round terminal/trace data remain persisted.
UX-13 clear confirmation should remove obsolete scoped task/readonly descriptors on successful conversation deletion without resurrecting old reminders; current readonly view conservatively refuses clear/undo. UX-19 can combine `ai.returnCurrent` with its current-vs-historical comparison interface: this command only exits the readonly task view, restores the selected photo's own draft/reading position, and never restores/replays an old edit.
Cycle11 genuine public red ai.clear accepted selected B while showing A; minimal common command guard rejects clear/undo until return. Green included in pre09 normal parallel full AI125passed0failed35.09s. No Clear/Undo footer is shown in readonly view.20/pre09-ai-regression.log.

## Cycle 12: readonly controls in supported short Tools
Seam: published-result scenario, actual float Tools120px, deleted A trace and real scroll/widget. Catches readonly trace/return controls clipped without a scrollable viewport; misses native OS compositor. Genuine red missing ai:composer while trying to reach the actual trace control. Minimal green gives the readonly controls/message view a compact scrolling body when height<200px, normal message scrolling otherwise. Readonly heading hover also uses displayed source scope.20/cycle12-red.log.

## UI verification
Actual procedural EN/ZH normal1280x800/narrow880x600/floating+stacked120/240px frames verify complete task stage and Retry text galleys inside their registered visible rectangles, real Retry does not create/reconnect a task, Stop is scroll-reachable, global strip remains outside Tools, hiding Tools/selecting B retains global A ownership and explicit click selects A.12screens20/screens/health-task-*.png created and normal EN, narrow ZH, float120EN and stack120ZH pixels inspected. They show readable labels/control hit areas and no topbar collision; partial compact body content is intentionally scrollable. Additional Retry-position/readonly-position screenshots and actual Return checks follow the Cycle12 compact correction.
Cycle12 green1passed exact readonly trace control reached/clicked in a real120px floating Tools viewport while B0.7EV stayed selected.20/cycle12-green.log.

Approved integration combination961a60ad merges actual c972764fc9c88ee7f0de46313bd6734b43077f5e (formal approved09+16). Only source conflict: adjacent module declarations; both recovery tests and task entry retained. Source audit confirms HistoryView::_retention in normal and error native completion, fallback HeaderView retention before scanning, native message retention transfer and actual Drop, scanner uses presented PhotoScope A rather than selected B, retirement/backoff and12viewport retained. Existing actual fallback/native retained-view outcomes are part of final AI regression; no test-only recovery setter or unapproved15/17 code imported.

Final combined first AI attempt had a fixture type typo Event::Closed(None) instead of the existing String interface; corrected to an actual disconnect diagnostic.20/final-ai-invalid-fixture.log is compile-only evidence and is not claimed as a behavioral red.

## Cycle 13: an unavailable active source still needs scoped Stop
Seam: owned procedural PNG/public import, actual consent/start and transport round identity/state, remove only that scratch file while Bselected, real global click/readonly Stop and observed cancel request. Catches losing Stop when original source disappears during an active round or targeting current B; misses native OS pointer delivery and live model/provider behavior. Expected genuine red: readonly original task body has no Stop widget. The source itself is an actual temporary PNG; file availability uses the existing public Session/files adapter, not a hidden setter or private task assertion.

## Native host compatibility correction
Actual cargo check -p lightcraft failed at apps/lightcraft/src/main.rs:67: existing macOS eframe::App::on_exit(self) lacked the Glow trait's Option<&Context> argument. c972 carries the same call; no baseline exemption taken. Root explicitly authorized the minimal on_exit(self,None) correction. This does not consume unfinished15 lifecycle code; its later merge retains the same compile fix plus its own ordering.20/final-native.log records actual error; corrected host check passed in20/final-native-fixed.log; final approved17 combination is checked separately.

Combined engine companion check:85passed/0failed/1built-inignored9.65s, plus3native reader-lock child probes eachpassed.20/final-engine.log. i18n30passed/0failed2.58s20/final-i18n.log. Strict UI clippy exposed two own style violations (collapsible_if in task entry, default field reassign in B editor fixture); fixed exactly, no allow/skip added.20/final-clippy.log is the failed lint run; subsequent fixed run required before final delivery.
Cycle13 initial fixture submitted before the real background file-availability check became known; it failed Checking photo availability rather than the intended Stop behavior. Preserved20/cycle13-invalid-availability.log; reverted ALL premature Stop production code and cancelled that attempted green. Fixed fixture to observe actual owned PNG existence before submit and actual native cached recheck after removing it (no private hook). The later genuine13a/13b results are recorded below. Additional readonly Return verification initially scrolled upward at top; corrected its procedure to scroll down to the below-notice control, no product change or claimed UI red.

## Cycle 16: local client plan failure presentation
Seam: approved public consent/start/full frames and actual transport edit-plan input, actual Session parameters and ai.status. Catches an invalid client plan retaining the real starting photo while producing localized cause, effect and explicit manual recovery; misses live provider quality and native OS pointer input. Source audit found fail_one and render failure still show a Chinese-only generic notice and omit manual recovery. Expected genuine red: the actual invalid capability plan ends with that legacy notice instead of a new-request/starting-settings explanation.

Cycle13a genuine red: actual deleted source task had a long filename whose selectable child labels intercepted the global entry click. Minimal nonselectable labels restored the real entry hit area; green passed. Cycle13b true missing-Stop red used an accepted round, observed input and analyzing_initial before removing only the owned procedural PNG and awaiting its real availability recheck. Minimal readonly Stop explicitly routes original A and retains the feedback save barrier. The first green asserted invented /cancel; corrected to existing /stop without protocol changes. Final green1passed15.65s (cycle13b-green-fixed-fixture.log), B selection/parameters preserved.

## Cycle 14: verified cold diagnostic trace
Seam: actual fresh Session/App and MemStore durable rounds, public frames/status/copy widget, both fallback and native scoped reads. Catches lost diagnostic IDs after restart, pure-answer rounds without editing cards, bounded scanning of large ignored fields, B/library scope isolation; misses physical OS clipboard and backend/provider diagnostics quality. Genuine red lacked Copy Diagnostic ID. Common selected-field DisplayScan now captures optional trace_id with a bounded decode then skips oversized values while retaining complete digest/owner verification. Captured HistoryView stores only a validated ≤512-byte diagnostic for locally referenced round IDs. Native and fallback readers retain original read/view ownership guards; no fullrecord hydration. Green1passed1.23s.

## Cycle 15: cold history cannot invalidate held consent
Seam: same real fresh durable history, actual public widget pointer down/up and fullframes while a3.24MB ignored request scans, observed real create request. Catches a newly appearing diagnostic button moving a held consent checkbox or changing subsequent widget identities; misses native OS input. Early fixture attempts settled before Down/used unregistered first-frame controls and are preserved as invalid harness evidence. Genuine red was26px checkbox movement between Down/up. Minimal reserved diagnostic row plus unconditional childUi allocation keeps geometry/identity stable. Copy is exercised before issuing a subsequent command that legitimately changes layout. Latest stable-identity green1passed1.59s (cycle15-stable-identities.log); real consent allowed exactly1roundPOST, same-ID new library/B had no A trace.

## Approved UX-17 combination
Owncheckpoint f47d484d; merge0bcd8961 combines approved actual integration3e0d88e29a0e0bf559188606e89e1ebc364d282d. Source conflict union retains UX-17 card-bound Undo and present_card runtime qualification/revert state/strict supplied photo parsing, no generic footer. UX-20 status/cards use displayed original photo, readonly A disables ordinary History and card Undo and rejects external clear/undo; ordinary current-photo cards preserve UX-17 behavior. Shared parser retains both trace_id and revert_state selectors. UX-09 actual view/read guards remain retained.

## UI receipt synchronization
The retained UX-09 recovery test genuinely failed when Session's engine save cleared before UI pending_local_saves feedback had flushed. Isolated reproduction (combined-recovery-rerun.log) showed the same current-photo scope, not stale readonly presentation. The fixture now waits for both engine unsaved=false and public ai.status.saveState=saved within its original6s bound, then keeps all original notice/revision/no-replay assertions. Parent accepted this public synchronization for fresh review; no product scheduling/backoff policy or assertion is waived. Full combined passing result remains required.

## Remaining-ticket diagnostic interface
UX-15 may extend the common bounded DisplayScan selected-field projection for its reliable accepted phase/association summary, then apply the real stored PhotoScope/RoundIdentity to the global descriptor; its consumer is not implemented here. UX-19 can reuse the captured verified diagnostic and navigation-only ai.returnCurrent. No new reconnect/replay behavior is introduced.

Cycle16 genuine public red1failed after an actual accepted/input plan path: invalid client capability stops with truefailed/unchanged starting params but legacy Chinese-only notice lacking manual recovery. First minimalgreen1passed2.45s (cycle16-green.log). First whole combination134passed/3failed72.34s: generic local cause dropped the existing authored incomplete-history/service-limit causes, and the09 fixture observed a new editor save receipt after its extra layout-transition frame. Minimal additive local_notice retains those authored localized causes and appends actual-effect/manual recovery; invalidplan cause maps to existing localized validation notice. Durable local_failure.message/terminal_notice are human recovery text, with the technical error separately bounded in local_failure.diagnostic. Storage-unsaved cause remains09's existing localized message.
Second whole combination136passed/1failed73.18s (final-combined-ai-fixed.log): both native preflight cases and09 recovery pass. The sole failure was a fixture capitalization expectation (Start vs the now-specific validation sentence's start). Updated to assert the precise could-not-be-validated cause plus case-insensitive manual new request, retaining starting-state and truefailed checks; not claimed as a product red.09 now additionally requires two consecutive actual saved frames+visible saved notice, within its original6s bound, proving convergence instead of masking perpetual layout/save feedback. Full final result follows.

## Final verification so far
Whole approved17 combination: `cargo test -p lightcraft-ui-egui ai_companion -- --nocapture`137passed/0failed/0ignored68.97s (20/final-combined-ai-verified.log). The production tree tested is the final implementation; afterward only the09 fixture's intended convergence counter was correctly inserted. The preceding declaration-only counter emitted unused warnings; those warnings are not claimed as strict-lint success. Strengthened actual09 convergence: `cargo test -p lightcraft-ui-egui public_ai_result_stays_visible_and_unsaved_recovery_only_writes_storage -- --nocapture`1passed/0failed2.14s (20/final-recovery-convergence.log). Both locales need two consecutive actual completed frames with engine unsaved=None, public UI saveState=saved and visible saved notice within the original6s bound. This distinguishes a one-time read-position editor receipt from failed history persistence; no perpetual feedback/save loop, ordinary storage/history policy, first-feedback I/O deferral or aggregate cleanup backoff was changed.

UI evidence directory `/tmp/lightcraft-ux-execution-20261013/20/screens/`:36latest procedural PNGs, all combinations of `health-task`, `retry`, `readonly` × `normal`, `narrow`, `float120`, `float240`, `stack120`, `stack240` × `en`, `zh-hans`. Every layout asserts complete global stage and Retry galleys inside visible hit areas, actual Retry/Stop/Return clicks and original ownership/navigation. Actual pixels inspected: health-task-normal-en.png, health-task-narrow-zh-hans.png, readonly-float120-en.png, retry-stack120-zh-hans.png, retry-float120-en.png, readonly-stack120-zh-hans.png, readonly-float240-zh-hans.png, retry-narrow-en.png. They show readable controls, complete primary labels and a scrollable short body. Dedicated actual cold-history/published-result fixtures independently prove Copy Diagnostic ID; the36layout fixtures use ordinary transport disconnects without an invented trace.

Limits: these are actual procedural UI/fullframes + owned Store/native HTTP/SSE/clipboard seams, not physical macOS/Windows pointer or accessibility/compositor acceptance. No live paid model/provider run, original user photo, backend capacity/deployment validation, forced OS crash or cross-platform packaged application runtime was used. HTTP health success means reachability only. Runtime unread reminder is intentionally not a persisted notification queue. UX-15 reliable interrupted summary and UX-19 historical comparison consumers remain separate; their interfaces are documented rather than claimed implemented. No unapproved15 source, hidden address/recovery setters, core Catalog schema or backend protocol was introduced.

Final affected engine companion:85passed/0failed/1built-inignored11.98s, and its named ignored native_reader_lock_child was actually exercised by three successful child probes (20/final-combined-engine.log). No ignored-test exemption added. Final i18n:30passed/0failed1.47s (20/final-combined-i18n.log). Final strict UI lint `cargo clippy -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` passed16.91s (20/final-combined-clippy.log), including the corrected convergence guard with no allowances/skips. Final fmt and whitespace checks passed (20/final-combined-fmt.log). Approved combination parity refs/counts and dependencies passed (20/final-combined-parity.log,20/final-combined-layers.log): every cited command/control/path exists,31crates/no layering violation. Native/Web compile results follow.

Final native host `cargo check -p lightcraft` passed13.59s on the combined candidate (20/final-combined-native.log). This includes the actual Services endpoint configuration/field initialization and the authorized main.on_exit(self,None) compatibility correction. It is not a claim that unfinished UX-15 exit/cancellation lifecycle is implemented.

## All36latest screenshot paths

| Layout/locale | Active task | Explicit Retry reached | Readonly Return reached |
|---|---|---|---|
| normal/en | health-task-normal-en.png (historical local run detail) | retry-normal-en.png (historical local run detail) | readonly-normal-en.png (historical local run detail) |
| normal/zh-hans | health-task-normal-zh-hans.png (historical local run detail) | retry-normal-zh-hans.png (historical local run detail) | readonly-normal-zh-hans.png (historical local run detail) |
| narrow/en | health-task-narrow-en.png (historical local run detail) | retry-narrow-en.png (historical local run detail) | readonly-narrow-en.png (historical local run detail) |
| narrow/zh-hans | health-task-narrow-zh-hans.png (historical local run detail) | retry-narrow-zh-hans.png (historical local run detail) | readonly-narrow-zh-hans.png (historical local run detail) |
| float120/en | health-task-float120-en.png (historical local run detail) | retry-float120-en.png (historical local run detail) | readonly-float120-en.png (historical local run detail) |
| float120/zh-hans | health-task-float120-zh-hans.png (historical local run detail) | retry-float120-zh-hans.png (historical local run detail) | readonly-float120-zh-hans.png (historical local run detail) |
| float240/en | health-task-float240-en.png (historical local run detail) | retry-float240-en.png (historical local run detail) | readonly-float240-en.png (historical local run detail) |
| float240/zh-hans | health-task-float240-zh-hans.png (historical local run detail) | retry-float240-zh-hans.png (historical local run detail) | readonly-float240-zh-hans.png (historical local run detail) |
| stack120/en | health-task-stack120-en.png (historical local run detail) | retry-stack120-en.png (historical local run detail) | readonly-stack120-en.png (historical local run detail) |
| stack120/zh-hans | health-task-stack120-zh-hans.png (historical local run detail) | retry-stack120-zh-hans.png (historical local run detail) | readonly-stack120-zh-hans.png (historical local run detail) |
| stack240/en | health-task-stack240-en.png (historical local run detail) | retry-stack240-en.png (historical local run detail) | readonly-stack240-en.png (historical local run detail) |
| stack240/zh-hans | health-task-stack240-zh-hans.png (historical local run detail) | retry-stack240-zh-hans.png (historical local run detail) | readonly-stack240-zh-hans.png (historical local run detail) |

## Final completion
`xtask wasm` passed all28selected wasm-safe crates, including lightcraft-ui-egui and lightcraft-web (20/final-combined-wasm.log). Invoked the task-owned built xtask with `CARGO_MANIFEST_DIR=/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft/xtask` so the checks use this actual worktree; it dispatches normal cargo check --target wasm32-unknown-unknown per selected crate. The earlier Web check exposed its explicit Services initializer missing companion_address; fixed with the actual None default, retaining runtime endpoint fallback. Earlier failed strict lint, native-host trait call, Web constructor and integration tests have no baseline exemption. All final affected checks passed.

Initial whole candidate commit before the first independent review `3d9872274ab6b1c37f8be0ae2e515435d616edef` (UX-20: retain human failure causes and truthful manual recovery). `git merge-base 3e0d88e29a0e0bf559188606e89e1ebc364d282d HEAD` returns the actual approved integration base unchanged. Previous scoped20 checkpoints/approved union: f47d484d and0bcd8961; no15WIP consumed. Final candidate is ready for the root's fresh independent standards/spec review. No further task implementation or integration merge was started from this worker.

## Cycle 17: first independent-review history scope correction
Review `/tmp/lightcraft-ux-execution-20261013/20-review.md` identified one Important: omitted ai.history in readonly A writes selected B reading state. Full pre-test approved-seam catches/misses, invalid fixture evidence, real red/green and final fixed checks are in `/tmp/lightcraft-ux-execution-20261013/20-fix-note.md`. Initial fixture's nonexistent ui.language is preserved as invalid harness; actual public app.language menu commands corrected it without production changes. Genuine red1failed1.04s on unchanged3d987: actual saved B cursor32→0 while A remains displayed.

Minimal production fix resolves only omitted-photo ai.history via existing displayed_photo (same as ai.status); explicitly supplied photo scope is preserved. No paging quota, hidden setter, schema, engine, host, paint geometry,04feedback deferral,09retention guard,12editor behavior or17cardboundUndo policy changed. Regression uses real Session/MemStore/transport admission and completion, deletion/selection, readonlyTask, history and Return; EN/ZH verify actual A cursor0 and a complete A history row physically inside the registered message viewport after actual move/scroll controls, unchanged B full editor/draft/selection/0.7EV and Return position. Explicit B navigation still intentionally writes B cursor0 without changing viewed A, B draft/effects/selection or submitting.

Targeted green1passed1.28s (20/fix1-green.log). Whole affected AI138passed/0failed/0ignored65.04s (20/fix1-ai-full.log), including all navigation/drafts/retained paging/control paths. Strict native UI clippy --all-targets --no-deps -Dwarnings passed8.57s (20/fix1-clippy.log); fmt/whitespace passed (20/fix1-fmt.log). Affected UI WASM check passed2.74s (20/fix1-wasm.log), with only the same previously recorded unrelated Lightroom import/codec/preview/device warnings; no strict WASM-lint claim or allowances. No paint changes: the same36linked screenshots remain applicable and their complete bilingual fullframe/control checks reran in138. Unchanged engine/locale/host/dependency/full28WASM source retains the already recorded checks rather than a redundant broad rerun.

Fixed clean whole candidate `e20c530f1cbc5e37fa56f844d18da625a7b7668d`. Actual approved integration ancestor/merge-base remains `3e0d88e29a0e0bf559188606e89e1ebc364d282d`; git status empty. Not merged back to integration. Ready for the root's independent fix review; no additional task started.


---

# UX-20 first independent review

Fixed range: `3e0d88e29a0e0bf559188606e89e1ebc364d282d..3d9872274ab6b1c37f8be0ae2e515435d616edef`.
Checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft`, branch `codex/ux-20`, clean HEAD confirmed before and after review. Review was read-only: no checkout, index, HEAD, branch, or source mutation; no subagents, merge, provider calls, `.env` reads, or OS synthetic input.

Read context/review-contract/skill-update, requesting-code-review reviewer template, both installed tdd/codebase-design skills, repository AGENTS.md and collaboration rules, whole issue 20 and the authoritative design/implementation/review documents. Inspected every changed production file and the relevant surrounding admission, cancellation, draft, archive, paging, publication, error, and host paths. Reviewed test changes and implementation evidence rather than accepting its completion assertion.

## Strengths

- Health is a bounded independent module: both health GET and round transport share the host-selected loopback endpoint; probe serials reject obsolete completion and library replacement cancels the old health object. It creates neither client nor round identity, and the status honestly says HTTP reachability only (`health.rs:39–95`, `network.rs:116–140`). Native and Web Services initializers contain the added field; the native on_exit signature correction is appropriately minimal.
- The global entry lives outside Tools, retains the original PhotoScope/RoundIdentity/name and last stage after Job removal, and changes selection only on explicit navigation. Missing/deleted A uses a separate displayed scope while B selection remains intact; Return performs navigation without restoring effects (`task_entry.rs:33–80`, `ai_companion.rs:234–253`). Readonly UI has scoped Stop, no input, disabled ordinary History/Undo cards, and common clear/undo/admission guards.
- Transport reports acknowledged heartbeat observations separately from model text and retains the existing absence of a model-silence deadline. Explicit disconnection ends the local Job/progress and retains actual published results. Human failure text derives actual effect from succeeded local stages, not last_confirmed_plan_id; backend/technical identifiers remain secondary (`errors.rs:3–41`, `network.rs:30–55`, `ai_companion.rs:606–680`).
- Cold diagnostics reuse digest-verified selected-field scanning and exact message-to-round association. Fallback/native retained views keep their HistoryRetention guards, including error views; fallback advances one bounded scan step per frame and oversized trace values are discarded while scanning/digest verification continues (`paging.rs:68–171,237–290`, `scan.rs:28–69,294–306`). The reserved normal diagnostic row preserves held consent geometry and subsequent widget identities.
- UX-09's revised recovery assertion retains its six-second deadline and now requires two consecutive completed frames with engine unsaved clear, public saveState saved, and visible saved notice. It does not skip assertions, manufacture success, or accept perpetual receipt loops (`recovery_tests.rs:145–166`). The implementation report distinguishes genuine behavior reds from invalid harness attempts and does not claim retroactive new-skill compliance.

## Issues

### Critical (Must Fix)

None found.

### Important (Should Fix)

1. **Omitted-photo history navigation writes selected B while readonly A is displayed.**
   - Location: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft/crates/ui-egui/src/ai_companion.rs:1778` (default target resolved at line 1701; mutation at lines 1783–1784). Actual persisted effect is in `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft/crates/ui-egui/src/ai_companion/drafts.rs:209` through line 216.
   - Trigger: finish A's task, make A unavailable/deleted, select B with its own saved reading cursor, and open A through `ai.task`. Then call the existing public `ai.history {record:0,offset:0}` without a photo argument.
   - Source-proven result: command resolves B from active selection, loads B, then navigate_page sets B's reading_cursor/epoch/count, sets follow_tail=false and restore_scroll=true, and calls Session.set_companion_editor for B. The readonly A descriptor and selection snapshot remain active, so the user still sees A while B's saved reading position has been changed. The request may also supersede the single paging worker's desired A request. This violates the current whole20 navigation/isolation requirement and changes what Return restores for B.
   - Why existing coverage misses it: readonly isolation tests exercise omitted-photo start, clear and undo, but only the UI history buttons pass A explicitly. `ai.status` already uses displayed_photo for its omitted-photo viewed conversation at line 1742, so this is an inconsistent command default introduced by the new readonly presentation, not a request to forbid explicit other-photo reads.
   - Fix: resolve omitted-photo ai.history against the displayed photo (or reject that omitted-photo operation before touching B while readonly), preserving explicitly requested photo semantics. Add a public command/full-frame regression with saved B reading state, readonly A, omitted-photo ai.history, A navigation, and Return; assert B editor/selection/develop state remain unchanged. This finding was traced through actual caller-used source, not executed with a newly injected reproducer in this read-only review.

### Minor (Nice to Have)

None with a concrete additional impact.

## Verification and evidence

- Independently reran the final candidate's already-built test executable, without rebuilding or writing an active Cargo target: `/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-20/debug/deps/lightcraft_ui_egui-d7241b0d37537103 ai_companion --nocapture`. **137 passed, 0 failed, 0 ignored, 74.53s.** Output: `/tmp/lightcraft-ux-execution-20261013/20-review-ai.log`. This includes real >30-second heartbeat/model-silence, missing-file Stop, held health/trace gestures, native/fallback retained histories, approved17 revert cards, and strengthened09 convergence. A passing existing suite does not cover the finding above.
- Independently ran `git diff --check` for the fixed range: passed. Rechecked clean worktree and unchanged HEAD after running tests.
- Inspected implementer final engine (85 pass/0 fail/1 built-in child helper ignored, separately exercised by three passing child probes), i18n (30 pass), strict clippy, native host, and WASM logs. These are reviewed implementer evidence, not independently rebuilt certification. No build was required for this review; consequently no shared or worker target was mutated by Cargo.
- Confirmed all 36 screenshot files listed in `20-implementation.md` exist. Independently inspected actual pixels in `health-task-narrow-zh-hans.png` and `readonly-float120-en.png`: global scope/stage remains legible, short readonly content scrolls, and Return is reachable. The public layout test independently reran all EN/ZH × normal/narrow/floating120/240/stacked120/240 cases. This is headless procedural evidence, not physical compositor certification.
- Sampled genuine red logs for first-open/global-entry/error/disconnect/readonly/heartbeats/trace/History/command isolation/short viewport/Stop/cold trace/held consent/local plan failure, plus final recovery evidence. Invalid fixture/socket/availability attempts remain explicitly marked as such; no new TDD claim was invented during review.

## Recommendations

Resolve the single omitted-photo history target inconsistency at the command seam and add the observable readonly A/B regression before merge. Retain the scoped UI actions, actual-effect errors, immutable history-retention guards and bounded scans; no architecture expansion or change to explicit other-photo read requests is needed.

## Declined to judge

- Reliable interrupted-task recovery and exit/cancellation lifecycle: explicitly assigned to UX-15; this review checks that UX-20 exposes scoped descriptors without consuming that unfinished implementation, not that cold restart invents a task reminder.
- Historical comparison/preview consumer: explicitly assigned to UX-19. The omitted-photo history finding above remains current UX-20 isolation scope and is not deferred.
- Confirmation before deleting a conversation and clearing obsolete task descriptors after confirmed deletion: explicitly assigned to UX-13; not waived for that ticket, not invented as a current20 requirement.
- Explicitly requested other-photo `ai.history {photo:B,...}`: no evidence that an intentional scoped read must be forbidden. The finding is limited to the omitted-photo operation while A is presented.
- Paid-model quality, physical OS pointer/clipboard/compositor behavior, packaged Windows/Linux/browser runtime and forced power loss: not exercised by the approved local fixture evidence; compile/source compatibility is not presented as those certifications.

## Assessment

**Ready to merge? No.**

Most of whole UX-20 is implemented with strong observable regression evidence and the approved09/12/16/17 combination passes. Fix the Important omitted-photo history isolation bug before merging; there are no Critical findings or additional speculative blockers.


---

# UX-20 independent-review fix

Base candidate3d9872274ab6b1c37f8be0ae2e515435d616edef; approved integration3e0d88e29a0e0bf559188606e89e1ebc364d282d. Finding: omitted-photo ai.history resolves selected B while readonly task A remains presented, overwriting B reading cursor/follow-tail.

TDD seam (already approved public seam): actual Session/MemStore saved history/editor/develop data, consent/start and owned transport completion, public selection/delete/ai.task/ai.history/ai.returnCurrent commands and full visible frames. Catches omitted history default targeting presented original A, actual A page content/cursor navigation, saved B draft/reading/effects/selection isolation and correct Return, while explicit photo requests retain intentional scope. Misses physical OS pointer/compositor, paid models and all source formats. Expected genuine red on actual3d987 candidate: saved B editor changes from record32 to0 after omitted ai.history while displayed photo/readonly descriptor remain A. Record outcome before minimal production correction; no new test-only setter or quota.

Initial fixture failed before the intended behavior because it used nonexistent ui.language. It is not a behavioral red (20/fix1-invalid-language-command.log). Corrected to the actual public app.language.english/app.language.simplifiedChinese menu commands; production is still unchanged3d987.

Genuine red1failed1.04s (20/fix1-red.log): readonly A descriptor/public viewedPhoto are A; omitted history changes actual saved B reading_cursor32→0 while draft/scroll/epoch/count remain the original B values. Minimal correction resolves only omitted-photo ai.history via the existing displayed_photo selector (same semantics as ai.status), preserving explicitly supplied photo handling and all page/load/editor/retention paths. The existing regression now additionally moves/scrolls through actual public controls and checks the full A row galley within its visible message viewport rather than offclip text.

Targeted green1passed1.28s (20/fix1-green.log), EN/ZH within the actual public regression. A record0 is presented as a complete visible row in the real message viewport; A owns its reading cursor, B full editor/draft/develop/selection are unchanged before/after Return. Explicit photo:B still intentionally updates B reading_cursor0 while public viewedPhoto remains A; no additional submission. Paint implementation/geometry was not changed, so the original36layout screenshot artifacts remain applicable; the whole suite reruns their exact bilingual fullframe/control coverage.

Shared-command affected regression: `cargo test -p lightcraft-ui-egui ai_companion -- --nocapture`138passed/0failed/0ignored65.04s (20/fix1-ai-full.log). Includes original09/12/16 paging/editor/navigation/retention,17card-bound Undo,04feedback/cancellation plus all EN/ZH normal/narrow/short layouts and the new default/explicit history scope test. Strict UI clippy --all-targets --no-deps -- -D warnings passed8.57s (20/fix1-clippy.log); fmt and git diff --check passed (20/fix1-fmt.log). No paint change, screenshot regeneration, new dependency/schema/engine/host code or baseline exemption. Existing36screens and original engine/nativehost/i18n/layers/parity/full28WASM evidence remain unchanged scope; the changed UI module is additionally checked for WASM below.

Affected `cargo check --target wasm32-unknown-unknown -p lightcraft-ui-egui` passed2.74s (20/fix1-wasm.log). Its four Lightroom-import-only UI warnings and codec/preview/device dependency warnings are exactly the previously recorded final-combined-wasm warnings, outside changed files; no strict WASM-lint success or new allowance is claimed. Native strict UI lint remains clean.

Fixed whole candidate: `e20c530f1cbc5e37fa56f844d18da625a7b7668d`, branch codex/ux-20, clean git status. Approved integration3e0d88e29a0e0bf559188606e89e1ebc364d282d remains the ancestor/merge-base. Commit UX-20: keep omitted history navigation on the displayed task photo. Candidate not merged to integration, pushed, published or used for tracker edits; root will independently review this fix.


---

# UX-20 first-review fix clearance

Whole fixed range: `3e0d88e29a0e0bf559188606e89e1ebc364d282d..e20c530f1cbc5e37fa56f844d18da625a7b7668d`.
Focused delta: `3d9872274ab6b1c37f8be0ae2e515435d616edef..e20c530f1cbc5e37fa56f844d18da625a7b7668d`.
Checkout: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft`, `codex/ux-20`. Clean fixed HEAD and unchanged integration merge-base confirmed. This is read-only fix review, not merger authorization: no checkout/index/HEAD/source mutation, Cargo rebuild, subagents, merge, provider calls, .env reads or OS synthetic input.

Read `20-fix-note.md`, updated `20-implementation.md`, root `review-rulings.md`, all three changed files and the caller-used displayed_photo/load/navigate_page/request_page/Return implementations. The prior whole20 review remains applicable except its single Important finding, resolved below.

## Strengths / accepted finding clearance

- **Important omitted-photo history isolation finding cleared.** `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft/crates/ui-egui/src/ai_companion.rs:1779` now resolves an omitted photo through the same displayed_photo selector as ai.status. Load, editor navigation and page request all receive that resolved A. It cannot silently write selected B while the readonly descriptor presents A. An explicitly supplied photo still takes the original command photo path; no blanket ban or new explicit-photo policy was introduced.
- The regression uses the approved public seam: Session/MemStore durable histories and editors, real consent/start with the existing external transport adapter, actual completion/delete/select/task/history/Return commands and full frames. EN/ZH both check unchanged B's full EditorState (including saved cursor32, draft, reading epoch/count, scroll and follow-tail), selected photo and develop settings. It proves A's cursor0 and a complete A row inside the real visible message viewport after public move/scroll controls, then checks B restoration on Return. Explicit B still intentionally changes its own cursor to0 while A remains displayed (`health_task_tests.rs:1052–1101`). Navigation creates no new round request.
- Installed-TDD evidence is genuine and narrow: `20/fix1-red.log` fails at the exact B state comparison (cursor32→0) on the preceding implementation. The nonexistent language-command attempt is explicitly excluded as invalid fixture evidence. The production change is solely the target selector, with a matching documentation sentence; no paging, feedback, retention, layout, storage schema or revert policy changes were hidden in the fix. No retroactive TDD claims were added.

## Issues

### Critical (Must Fix)

None.

### Important (Should Fix)

None remaining. The sole finding from `20-review.md` is cleared on this exact candidate.

### Minor (Nice to Have)

None with concrete additional impact.

## Independent verification

Ran the final already-built candidate executable directly, with no Cargo target mutation:
`/Users/liujinju/lightcraft-project/lightcraft/target/ux-agent-20/debug/deps/lightcraft_ui_egui-d7241b0d37537103`.

- New public readonly/default/explicit history regression: **1 passed, 0 failed, 0 ignored, 0.90s**, covering both EN and ZH. Log: `/tmp/lightcraft-ux-execution-20261013/20-review-fix-public.log`.
- Existing09 saved-feedback convergence: **1 passed**, 1.74s (`20-review-fix-preserve-0.log`). The two consecutive saved frames and original six-second bound remain intact.
- Actual fallback retained view through inactive scans and real retirement: **1 passed**, 1.16s (`20-review-fix-preserve-1.log`).
- Actual native completed view retaining its graph until real retirement: **1 passed**, 1.05s (`20-review-fix-preserve-2.log`).
- Existing12 native pending older intent across selection and reader retirement: **1 passed**, 4.02s (`20-review-fix-preserve-3.log`).
- Approved17 actual whole-round card revert after consent revocation: **1 passed**, 1.60s (`20-review-fix-preserve-4.log`).
- Fixed whole-range `git diff --check`: passed; clean status, candidate HEAD and approved base ancestor confirmed.

Inspected worker final `fix1-ai-full.log` (**138 passed, 0 failed, 0 ignored, 65.04s**), strict UI clippy, fmt and affected UI WASM check logs. Those are reviewed worker evidence, not independently rebuilt certification. Original whole20 independent137 run and source review plus these six focused runs support the unchanged09/12/16/17 combination; no layout regeneration is needed for a command-target-only correction, and the worker138 reran the existing bilingual layout coverage. Original 36 screenshot artifacts and their limitations remain applicable.

## Recommendations

No additional implementation correction is required for the reviewed whole20 candidate. Preserve the default displayed-conversation scope and intentional explicit-photo behavior when future19 integrates history navigation.

## Declined to judge

All original lines remain governed by the explicit root rulings, without requirement waivers:

- UX-15 reliable interrupted summary and full exit/cancellation lifecycle remain assigned and required there.
- UX-19 historical comparison/preview consumer remains assigned and required there; current20 omitted-photo isolation has now been judged and fixed.
- UX-13 confirmation and obsolete task/descriptor/permission retirement after successful deletion remain assigned and required there.
- Explicitly requested other-photo history navigation is allowed under the root ruling; preserving it is verified, not treated as a missing blanket prohibition.
- Physical compositor/OS clipboard, paid providers, packaged Windows/Linux/browser runtime and forced power loss remain unexecuted certification limits for final22. Local headless/source/compile evidence does not claim those executions.

## Assessment

**Ready to merge? Yes, for whole UX-20 at `e20c530f1cbc5e37fa56f844d18da625a7b7668d`.**

The accepted Important isolation bug is resolved by a minimal command-scope correction, with genuine failing public evidence and independently passing corrected behavior. No Critical, Important or concrete Minor findings remain; merger work requires the separate parent instruction and integration verification.


---

# UX-20 merge and fresh integration certification

**Clean merge and all assigned integration checks passed.** No tracker resolution, push, PR, original-checkout reset, other worker import or next-ticket work was performed.

- Reviewed whole candidate: `e20c530f1cbc5e37fa56f844d18da625a7b7668d`, branch `codex/ux-20`, clean worker `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-20/lightcraft`.
- Integration before merge: `3e0d88e29a0e0bf559188606e89e1ebc364d282d`, branch `codex/ai-companion-ux`, clean `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft`.
- Normal merge command: `git merge --no-ff e20c530f1cbc5e37fa56f844d18da625a7b7668d -m 'UX-20: merge service health and scoped global task navigation'`.
- Integration after merge: **`32eebdbdc7a71456f7678230a85af8ae07ed8c4a`**; parents are exactly original integration `3e0d88…` and reviewed candidate `e20c530…`. No conflicts or merge-only repairs.
- Source identity: `git diff --exit-code e20c530f1cbc5e37fa56f844d18da625a7b7668d HEAD` returned zero with no differences. Worker candidate remains unchanged and clean; integration status remained clean after all checks.

The separate merger role followed `merger-context.md`, current execution.json and applicable repository collaboration instructions. The initial whole20 review and fix clearance are recorded in `20-review.md` and `20-review-fix.md`. Only the finally reviewed candidate was merged; no unfinished15 or other worker WIP was consumed. Backend tracker is root-owned and untouched.

## Fresh checks on actual merged integration source

Environment: `CARGO_TARGET_DIR=/Users/liujinju/lightcraft-project/lightcraft/target/ux-integration`, `CARGO_BUILD_JOBS=2`, `CRAFT_FONTS_DIR=/Users/liujinju/lightcraft-project/craft-fonts`. Normal test parallelism and original cases were retained. Control port18999 remained reserved but was not used: accepted UI evidence came from procedural headless frames and worker screenshots, with actual owned ephemeral native HTTP/SSE and filesystem fixtures in the suite.

Exact executable orchestration and machine-readable results: `/tmp/lightcraft-ux-execution-20261013/merger/20/run_checks.py`, `summary.json`, and per-name `.log` files. **All 14 commands exited zero.**

| Command | Actual result | Log |
|---|---|---|
| `cargo fmt --all -- --check` | Passed, 2.59s | fmt.log |
| `git diff --check 3e0d88e29a0e0bf559188606e89e1ebc364d282d..HEAD` | Passed | diff.log |
| `cargo test -p lightcraft-ui-egui ai_companion:: --lib -- --nocapture` | **138 passed, 0 failed/ignored**, 72.42s test execution; 178.16s including fresh build | ui.log |
| `cargo test -p lightcraft-ui-egui tests_companion:: --lib` | **15 passed, 0 failed/ignored**, 0.13s test execution | commands.log |
| `cargo test -p lightcraft-engine companion --lib -- --nocapture` | **85 passed, 0 failed**, 11.54s; outer child helper remains marked ignored but supervising test actually ran **3 successful real child probes**, each1passed | engine.log |
| `cargo test -p lightcraft-ui-egui i18n:: --lib` | **30 passed, 0 failed/ignored**, 2.08s | i18n.log |
| `cargo test -p lightcraft-engine tests::reset_edits_restores_a_raws_import_defaults --lib -- --exact` | **1 passed**, actual raw import-default Reset smoke | reset.log |
| `cargo test -p lightcraft-engine tests::versions_history_and_reset --lib -- --exact` | **1 passed**, ordinary versions/History/Reset | history.log |
| `cargo test -p lightcraft-engine tests::undo_and_redo_show_the_photo_they_change --lib -- --exact` | **1 passed**, ordinary Undo/Redo selection behavior | undo-redo.log |
| `cargo clippy -p lightcraft-engine -p lightcraft-ui-egui --all-targets --no-deps -- -D warnings` | Passed, 15.72s; no allowances or lint exemptions | clippy.log |
| `cargo check -p lightcraft` | Passed, 131.19s; native host initialization and actual on_exit signature compiled | native.log |
| `cargo check --target wasm32-unknown-unknown -p lightcraft-ui-egui -p lightcraft-web` | Passed, 25.71s; actual changed UI/engine dependency and Web initializer compiled | wasm.log |
| `cargo xtask layers` | Passed; **31 crates, no layering violation** | layers.log |
| `cargo xtask parity` | Passed; **566 rows/1232 references**, every cited command/control/path exists | parity.log |

WASM compilation retains previously recorded warnings in unchanged `codecs/src/jxl_encode.rs`, `preview/src/pool.rs`, `engine/src/devices.rs`, and `ui-egui/src/lightroom_import.rs`. Native strict lint passed; no strict WASM-lint success is claimed. Unchanged other WASM-safe source retains the worker's original successful full28-crate evidence (`20/final-combined-wasm.log`); the fresh integration claim is specifically the affected UI/Web check above, not a fresh full28 run. No failed check was waived or skipped and no unrelated blanket workspace suite was substituted.

## Behavior, combination and source proof

- The freshly passing138 suite includes asynchronous first-open/explicit-only loopback health and newest-probe ownership, original-scope task/result navigation outside Tools, unavailable-original readonly Stop/Return, omitted-photo ai.history on presented A and intentional explicit-photo semantics, B editor/draft/reading/develop/selection isolation, >30-second acknowledged heartbeat with silent model and real disconnection, locally published-effect failures, bounded verified cold trace copy, held mouseDown/up stability, and the bilingual short-panel/control layouts.
- Original09/12/16/17 combination remains present and freshly tested. `HistoryView::_retention` and native ReadMessage retention transfer remain in normal/error completion; fallback captures before read and scans only the displayed scoped photo. Actual native child test verifies A lock exclusion while reader lives, after worker retirement while captured view still lives, and allowed reopen after actual view Drop. Two-saved-frame09 convergence retains its original six-second bound.04feedback deferral is unchanged.17card-bound actual revert and explicit round targeting remain in the138/UI-command suite, with ordinary History/UndoRedo smokes above.
- Host source proof: `/Users/liujinju/lightcraft-project/_worktrees/ai-companion-ux-integration/lightcraft/apps/lightcraft/src/main.rs:408` and `apps/lightcraft-web/src/web.rs:171` both initialize companion_address; native `main.rs:67` calls `eframe::App::on_exit(self, None)`. Services field is in `crates/ui-egui/src/lib.rs:173`; health and real connection share network service_address resolution. Native/Web checks compile these actual paths.
- Omitted ai.history resolves through displayed_photo at `crates/ui-egui/src/ai_companion.rs:1779`; explicit supplied photo takes the existing requested scope. The accepted Important is fixed in the merged source, not hidden by a test assertion or moved to future19.

## Inspected UI evidence

All latest36 procedural PNG paths are preserved in `20-implementation.md` under `/tmp/lightcraft-ux-execution-20261013/20/screens/`: EN/ZH × normal/narrow/floating120/240/stacked120/240 × task/retry/readonly. Paint/source is identical to the reviewed candidate; final command-scope fix changed no layout. The fresh integration suite reran their fullframe/control cases.

Actual pixels inspected again during merger role:

- health-task-normal-en.png (historical local run detail): readable global original-task stage outside Tools and visible Stop/Retry; stable service feedback.
- retry-stack120-zh-hans.png (historical local run detail): readable Chinese Retry within the short stacked Tools viewport, global task entry still outside it.
- readonly-stack240-zh-hans.png (historical local run detail): original unavailable task name, readonly notice, Return control and actual disconnected/manual-recovery text remain legible with B selected.

Earlier independent review additionally inspected narrow-ZH and readonly floating120-EN pixels. Screenshot artifacts are worker captures, not a claimed physical native-window run during this merge.

## Limits and retained assignments

This certifies whole assigned20 on the merged local source, with honest procedural UI/owned native HTTP/SSE/Store evidence. It is not live paid-provider quality, physical compositor/OS clipboard certification, packaged Windows/Linux/browser runtime or power-loss certification. Health success means HTTP reachability only. No automatic retry/submission/reconnect/resume, synthetic OS input, .env access or real user-photo fixture was introduced.

Root review-rulings retain required UX-15 reliable interrupted summary/full exit lifecycle, UX-19 historical comparison, UX-13 confirmation and retirement of old scoped task/view/revert permissions, UX-18 durable certificates/Q30, and final22 paired native acceptance. Those tasks are neither implemented nor waived by this20 merge. Tracker resolution and any cleanup remain root-owned; no next task was started.
