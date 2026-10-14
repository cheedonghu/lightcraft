## Spec follow-up: offline originals with smart previews

The approved design requires readonly edits when the original is missing, including when a valid smart proxy can render.

- Design §7, line 178: “原图丢失或Recently Deleted时仍可回看完好文字/预览、复制诊断信息；发送、修改和撤回禁用。”
- Design A13: “原图Missing/Recently Deleted” must have “发送/修改/撤回禁用”.
- Implementation plan line 124: “Missing/Deleted只禁发送/编辑/撤回”.
- Published ticket 19 expressly repeats “原图缺失/已删除时…发送/修改/撤回不可用”.

No valid-smart-proxy exception appears in these approved requirements. `tests_import::smart_previews_stand_in_for_offline_originals` physically renames the actual source directory and verifies `originalOnline == false`, then expects `develop.set` to succeed. That success expectation conflicts with the new approved readonly behavior; its failure is an intentional behavior change, not a baseline failure or a virtual-file fixture issue.

Keep the production guard. Update the public test to retain its assertions that a valid smart preview renders while the source is offline, assert that editing is rejected without changing settings or history, and retain the assertion that discarding the proxy makes rendering fail. Record the changed behavior explicitly. Do not permit a valid proxy to bypass AI submission or card-revert original-source verification.

The five separate tests using nonexistent paths as virtual available originals should explicitly supply their availability fixture while preserving their original functional assertions; they do not represent physically offline originals.

The unreadable-settings wrapper implements only the former Store surface. New bounded reads deliberately default to Unsupported; test-adapter forwarding can be extended, but it must honor the same unreadable-name gate and preserve exact original bytes/not-overwrite assertions. Do not reintroduce unbounded production reads as a compatibility fallback.
