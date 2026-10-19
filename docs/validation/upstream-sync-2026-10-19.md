# UPSTREAM-091: upstream sync on 2026-10-19

## Scope

- Local `dev` started at `73c005ec` and fast-forwarded to `origin/dev` at `b95df329`.
- Merge upstream `storytold/lightcraft` `main` at `d999e382` (workspace version 0.9.1).
- Preserve the local untracked `.scratch/` directory.
- The sibling backend `photo-companion` `dev` was already current at `3a9d801`.

## Conflict resolution

- `Cargo.lock`: retain the fork's AI dependencies (`base64ct`, `crc32fast`,
  `getrandom`) together with upstream's `craft-docpath`; keep the upstream
  workspace version changes.
- `crates/ui-egui/locales/pt-br.json`: retain the fork entries and all six new
  upstream translations.
- `docs/parity-checklist.md`: retain both revision histories and the fork's
  measured summary. The merged rows still count 419 complete, 61 partial,
  59 missing and 35 out of scope (539 in scope).

Automatic merges retain the companion mutation guards, persistence hooks,
offline-import protections, AI menu entry and separate candidate validation.
An independent read-only review found no actionable integration defects.

Upstream's opaque PNG export now uses RGB rather than RGBA. The AI history
budget regression's 170×170 fixture consequently shrank to 87,290 bytes and
failed its 100–128 KiB setup assertion. Increase the random opaque fixture to
192×192 and update its seed stride; keep the request-limit and early-read-stop
assertions unchanged. Independent review confirmed this integration adaptation.

## Verification

- `cargo fmt --all -- --check`: passed.
- `git diff --cached --check`: passed.
- Portuguese catalog parses successfully and contains both the fork AI entry
  and upstream's selected-photo translation.
- `cargo test --locked -p lightcraft-ui-egui --lib ai_companion:: -- --test-threads=1`:
  243 passed, one failed at the PNG fixture setup assertion described above
  (203.49 s). No other failing test was observed.
- `cargo test --locked -p lightcraft-engine --lib companion:: -- --test-threads=1`:
  86 passed, one ignored child-process helper (23.24 s). The parent test
  separately executed that helper in three child processes, all passing.
- `cargo test --locked -p lightcraft-ui-egui --lib i18n::tests:: -- --test-threads=1`:
  37 passed (3.16 s).
- After the fixture adaptation,
  `cargo test --locked -p lightcraft-ui-egui --lib ai_companion::history_tests:: -- --test-threads=1`:
  all 13 passed (2.63 s), including the previously failing budget regression.
  Together with the unchanged 243 passing cases above, all 244 distinct AI
  cases are covered; this is not claimed as a single clean full-suite run.
- `cargo check --locked -p lightcraft`: passed (37.36 s).
- `cargo xtask parity`: passed; all 574 rows and 1,443 references are valid,
  with no duplicate gaps. Refreshed the generated summary with
  `cargo xtask parity --write`; measured counts remain unchanged.
- Final formatting and staged/unstaged whitespace checks: passed.

Per `docs/collaboration.md`, upstream-only codec/raw/printing/network behavior
reuses upstream verification; these suites are not claimed as locally tested.
No whole-workspace or extreme-capacity tests are run. Live backend/model calls,
browser runtime and manual desktop interaction are outside this sync check.
