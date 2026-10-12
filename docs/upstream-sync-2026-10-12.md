# UPSTREAM-070：合并上游 0.7.0

从 `storytold/lightcraft` 的 `25ef65c81eef81833a50ff5b61f8676d80611b06` 合并到 fork 的 `dev`，
起点为 `3f6183c3`。保留上游新增能力与 Photo Companion；同类功能优先使用上游实现。

## 合并与解耦

- 目录模型、操作类型、日志格式与版本沿用上游，核心目录当前仍为 v7。
- AI 对话独立保存在图库的 `companion/state.json`，格式 `lightcraft-companion`、版本 1。
  预览保存在同目录的 `preview-<photo>-<hash>.bin`。AI 存储变更不占用核心目录版本号。
- 依用户在预开发阶段的决定，旧 AI 数据不迁移：旧快照字段由上游读取器忽略；
  旧日志中 CRC 校验通过的 AI 操作转换为已有的空 Batch，保留操作顺序、序号及全部照片操作。
  清理是原子且可重复的；未知未来操作或更新目录格式不改写。旧附件不作为新会话引用。
- AI 存储与附件的校验、加载、原子写入及失败重试集中在 Companion 模块。
  引擎仅在图库加载、保存和照片修改处提供必要衔接。坏或更新的 AI 文件不妨碍普通照片编辑，
  也不会被覆盖；未保存的 AI 数据会报告并重试，不能被一次成功的照片目录快照掩盖。
- 保留上游缓存失效、撤销重试、侧车文件修复和图库切换失败保护，同时保留照片修改版本追踪。
- 普通蒙版使用上游参数裁剪；AI 严格校验留在 Companion 模块。AI 颜色取样复用上游探针及转换。
- AI 白平衡按上游规则转换、记录 Kelvin 单位；预览发布适配 `RenderResult.gpu_fallback`。
- AI 使用独立窗口，通过“窗口 → AI Photo Companion”打开；编辑面板保持上游原样，避免挤占降噪等控件。
- 上游面板布局命令和 AI 菜单全部保留。AI 工作时允许面板布局、打开或新建图库；
  切换前保存、结束旧图库的 AI 轮次，目标检查失败时轮次仍继续。
- MCP 取消测试及路径同步测试优先采用上游实现。保留上游重命名后的功能清单并重新生成统计。

## Windows 验证发现

纯上游提交 `25ef65c8` 的引擎测试复现了合并版本出现的全部 20 项路径/侧车失败，
另外出现一个后台人脸扫描时序失败。文件身份判断存在实质问题：`.xmp` 与 `.XMP` 在
大小写不敏感文件系统上不能对称识别，可能阻止修复后的元数据保存。本次修正该判断，
保留大小写敏感文件系统上的独立文件，并增加双向身份回归测试。
其他相关失败来自测试假文件系统的分隔符、故障注入未识别物理别名及期望路径拼接；
仅修正测试可移植性。仅供 Unix 测试的锁描述符复制方法限定为 Unix。

## 验证

- `cargo fmt --all --check` 及本次兼容修改的 `git diff --check` 通过。
  整体合并暂存区仅报告 `docs/raw/adobe-cameras.tsv:1380` 的末尾空列分隔符；
  `git diff 3f6183c3 25ef65c8 --check -- docs/raw/adobe-cameras.tsv` 复现同一报告，
  该文件与上游完全一致，保留 TSV 空字段。
- `cargo check --locked -p lightcraft-ui-egui -p lightcraft-mcp` 通过；
  `cargo build --locked -p lightcraft -p lightcraft-cli` 通过。
- 目录测试 191 项通过、1 项忽略；引擎最终全量单元测试 792 项通过、5 项忽略；
  UI 最终单元测试 751 项通过、2 项忽略。测试使用 `lightcraft-engine/denoise` 功能。
  引擎最终运行已重新编译的测试程序 `lightcraft_engine-1a5735fbff262cac.exe --test-threads=4`；
  Windows 故障修复相关三组测试另经 Cargo 复测：文件历史 8 项、重命名 13 项、侧车修复 4 项通过。
- MCP 单元测试 28 项、端到端测试 6 项、面板布局集成测试 3 项、退出保存测试 1 项通过。
  需要外部照片或模型的三项引擎集成测试按上游要求忽略；引擎文档测试通过（0 项）。
- `cargo xtask parity --write`、`cargo xtask layers`、`cargo xtask assets` 通过。
- 四个受影响 crate 的 `cargo clippy --locked -p lightcraft-catalog -p lightcraft-engine
  -p lightcraft-ui-egui -p lightcraft-mcp --all-targets --features lightcraft-engine/denoise
  -- -D warnings -A clippy::chunks_exact_to_as_chunks -A clippy::missing_const_for_thread_local`
  通过，无新增警告。严格 `-D warnings` 的两类失败已在纯上游 `25ef65c8` 上以
  `cargo clippy --locked -p lightcraft-catalog -p lightcraft-pipeline --all-targets -- -D warnings`
  复现：Rust 1.99 对 `pipeline/spots.rs:250` 的固定大小分块和
  `catalog/rules.rs:422,576`、`catalog/safe_file.rs:34` 的 `thread_local!` 报错。
  本次仅豁免这两项既有 lint，没有修改核心上游代码或放宽仓库检查配置。
- 实际启动桌面程序，使用独立配置与临时图库执行 33 次控制协议调用，验证 AI 窗口、
  对话独立存储、曝光编辑及撤销、打开无效目标后的原图库保护、新建及重新打开图库、
  核心快照 v7 且无 AI 字段；截图已检查，未发送云端请求。运行日志无无窗口渲染回退。
- 独立代码审查完成，持久化失败及 Windows 文件别名问题已修复；最终审查无阻断问题。

验证日志及桌面截图保存在本次工作区的 `target/upstream-*.log` 和
`target/upstream-native/`，属于忽略的验证产物，不进入提交。
