# AI companion UX 验收记录

日期：2026-10-15。第 01–21 项已逐项审查、合入并验证；第 22 项正在执行最终工作区检查和两轴审查。本记录不把进行中的检查记为通过。

环境：macOS / Apple M1 Pro，Rust 1.99、Go 工具链，实际桌面客户端及实际 Go HTTP/SSE 服务。桌面以独立临时 Fs 图库运行，图片由脚本生成，供应商仅为本机 loopback 可控接口；没有实际凭据或用户照片。运行时配置 `{}`，保持默认 32 客户端、4 执行、40 排队。字体来自已许可的 craft-fonts。

可复跑脚本位于相邻 photo-companion 工程 `scripts/verify-ux.py`、`scripts/verify-ux-capacity.py`，均接受 `--backend`、`--desktop`、全新 `--output`。前者 `--mode multi` 验证整组编辑；默认模式验证曝光流程。桌面授权均点击实际 checkbox，命令参数无法取得同意。控制通道不是 OS 自动输入。

`native-cold.json`：真实两次桌面进程，正常退出后重开保留结果及撤回资格，不恢复活动任务；后续手动修改使旧轮不能撤回。历史两幅实际 JPEG 均为 640×400，查看和返回不重放效果。`native-barrier.json`：六次真实工具/模型往返包含 WB 6100K、曲线、裁剪、颜色采样、蒙版创建和结构修正；第七次模型响应由 Event 屏障暂扣。分析期间真实 SSE 心跳正常，停止到可编辑为 0.5405 秒，释放旧响应后手动曝光 0.6 保留，未新增成功结果卡。

`capacity.json`：31 个独立 HTTP/SSE 客户端实际占满 4 个输入等待轮次和 40 个队列位置，桌面为第 32 个客户端；客户端 33 和轮次 45 被实际 429 拒绝。31 个占位客户端及桌面均确认真实 ping 后应答；无模型文字。队列拒绝后可编辑；释放一个队列位置后桌面真实排队并锁定照片，撤销 checkbox 后 0.5827 秒恢复编辑。输入屏障没有释放到模型。

| 验收 | 已执行证据与公共边界 |
| --- | --- |
| A01 | 实际桌面 `ai.start consent=true` 拒绝且模型调用为零；admission_tests 命令/实际 checkbox |
| A02 | 默认容量下实际排队撤权；实际分析响应屏障停止；network_tests 请求发送取消及 stop 保留通道；cancellation_tests 实际发布前后撤权及储存屏障 |
| A03 | admission_tests 单实例、多选/不可用照片；引擎 mutation guard 的实际目标及整批保护，独立照片可编辑 |
| A04 / Q29 | 实际队列明确拒绝解锁、排队锁定、响应屏障停止后迟到结果不能覆盖；admission_tests 提交即锁定，disconnect_tests 失效连接候选拒绝 |
| A05 | 实际曝光/整组工具/修正 JPEG；card_tests 有效 WB 单位、失败目标/保留结果、结构参数；实际执行反馈被拒的保留结果回归 |
| A06 / Q30 | 实际进程重开保留资格、后续编辑旧撤回拒绝；18 项 35 个持久化故障测试和 tests_companion 整轮撤回、部分 Undo/Redo、后续相等不复活；写失效失败不执行编辑、重试不重放 |
| A07 | 实际原生 Fs 配对与返回；comparison_tests 实际裁剪/方向、修正失败保留最近发布、独立 JPEG 像素比对、缺图及异照片拒绝 |
| A08 | comparison_tests 草稿/滚动冻结恢复、全屏同查看器、返回/切照片/工具；reading_tests 长内容恢复 |
| A09 | companion_clear 公共 Session 的 8 个提交/确认故障组合：保留照片、普通 History、草稿，失效旧资格和旧上下文 |
| A10 | companion_recovery 8 个保护/诊断反例；archive/pagination/migration 工具合法投影、坏记录部分回看、原始备份；完整历史不可用拒绝新任务 |
| A11 | catalog edited_tests、grid/filmstrip/筛选回归；05 项实际中英文图标截图，Reset/WB/默认外观/蒙版/复制同步统一比较 |
| A12 | health_task_tests、task_entry 和工具/菜单公共交互；实际桌面面板外完成入口，不抢照片选择 |
| A13 | comparison_tests 实际 Missing/Deleted 可读历史；engine companion_comparison 原子拒绝不可用目标；完整诊断/发送保护 |
| A14 | 实际正常进程重开和无文字心跳；lifecycle_tests 不续跑、所有退出/切图库保存；原生 Fs migration retirement 屏障；pagination 原生多照片历史超过 64MiB；recovery 故障隔离 |
| A15 | 实际桌面中文完成、英文对比/排队；公共 Headless 窄布局和阅读/结果卡；i18n 动态键回归；截图人工查看 |

详细先前故障注入、独立审查和合入检查记录保留在本目录 `ticket-*.md`；它们描述各项当时的结果，后续组合改进以最终工作区检查及本记录为准。正常 Fs、实际 native 进程、公共 Headless/Mem 故障注入分别标明，不将 Mem 故障测试称为真实断电。

截图 `completed-zh.png`、`multi-completed-zh.png`、`historical-en.png`、`queued-en.png` 来自实际桌面控制截图。macOS compositor 请求超时后明确回退到软件 offscreen，不能当作物理屏幕/GPU 合成验收。历史 JPEG 和实时修正附件的 CPU 纹理桥接经公共截图像素反例先失败后修复。`comparison-narrow-zh.png`、`reading-narrow-zh.png`、`interrupted-zh.png` 来自公共 Headless；字体和文字已人工查看。

限制：未验证真实断电、Windows/Linux 原生运行、浏览器运行或付费模型。WASM 编译仅代表构建通过。观测的 native-cold 渲染约 10.2ms；整组编辑观测 16.3ms，最大 update 包含导入/控制/截图，可达 800ms；不能由这些样本宣称所有交互达到 16ms。失败截图与错误前提的试跑未作为成功证据归档。

最终检查状态：进行中。`cargo xtask ci` 首次停止于 pipeline 的 Rust 1.99 `chunks_exact_to_as_chunks` lint；剩余检查独立执行，最终结果及既有失败逐项核对后补录。工单 22 尚未关闭。
