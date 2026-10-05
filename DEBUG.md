# 修复记录

## 2026-09-19：9 月 18 日独立源码包的测试和许可证修复

范围：`E:/快速开发/simple-review-20260918-185822/simple-code`。原工作目录产品源码和原始 ZIP 未覆盖。

现象与证据：结束回合已改用 TurnExecution，但部分 bridge/UI 测试仍要求 Reasoning；另有工作台欢迎页测试未显式进入新任务、附件测试对多条消息使用单元素断言。Molecule 1.3.6 的 npm 包及 gitHead a114a2adc3c88bdc12e8b0693d09386a1d199ea7 的 LICENSE 都仅含 MIT，不是打包截断。

根因：界面演进后部分断言未同步；原收集脚本只收集上游文件，未提供标识符许可的补充文本。

修复：更新结束状态、折叠内容、最终回复可见性及交互断言；保留运行态 Reasoning 检查；修正两处工作台测试定位。许可证保留上游原文，附包作者 DTStack Corporation、固定提交和标准 MIT 条款；明确区分补充文本与上游原文，不编造版权年份。发布收集脚本纳入补充文件，以版本及 SHA-256 校验防止回退到标识符。增加缺文件、截断、版本变化的保护测试。

验证：Vitest 21 个文件、177 项测试通过；选定 Playwright 34 项（editor、queue、reasoning、turn-execution、workbench），首轮 32 通过，修复另两项后复验 2 通过；许可证保护 2 项通过；应用和 UI TypeScript 检查通过；前端生产构建成功，dist 中许可证与源文件一致；收集脚本语法检查通过。

遗留风险：未执行全量 UI、Rust 测试、完整 Cargo 发布许可证收集及 Windows 原生安装包验证。构建存在大于 500 kB 的 chunk 提示。上游没有完整版权行，当前使用原始 MIT 声明、已声明作者和明确标注的标准条款补充，未声称上游已补齐许可文件。

## 2026-09-19：第一批冗余清理

范围：9 月 18 日独立修复目录；按审计建议第 1～4 类清理。原工作目录产品源码未替换。

现象/证据：执行记录旧状态和回调无人消费；executionGroups、两个交互辅助函数仅测试引用；旧 InspectorPanel 无调用方；CodeMirror、旧检查器/队列/执行列表样式失去组件入口。

根因：组件替换后旧状态、转导出、接口、样式和测试夹具未同步退役。

修复：删除 WorkspacePanels.tsx 和 RunInstructions.tsx；直接导入当前 TerminalPanel/ComposerQueue；移除旧执行状态、空回调、分组函数、toggleInspector/emptyConversationLabel；清除 readProjectFile/read_project_file 前后端专用接口及 TS 类型（保留 editor_read/editor_save 共用的 Rust BackendInspectorFile）。移除对应旧 CSS，同时拆分保留终端和文件/diff 共用规则。同步清除旧模拟接口与 5 个已经没有界面入口的检查器 UI 用例，保留当前编辑器回归；更新 frontend-fixes 中两项结束状态断言。

结果：18 个代码/样式/测试文件，新增 12 行、删除 309 行，净减 297 行；源码原件已留存 cleanup-before.zip，差异见 cleanup.diff。主 CSS 构建体积由约 90.85 kB 减为 86.14 kB（gzip 16.62 → 15.84 kB）。

验证：21 个单元测试文件、174 项测试通过（移除 3 项只测试旧函数的用例）；6 个 UI 文件、39 项回归通过；应用/UI TypeScript 检查通过；前端生产构建通过；cargo check -p local-agent-desktop --lib --locked --offline 通过；已删符号在 src/src-tauri/tests 中无引用。

遗留风险/范围：保留第 5 类旧终端接口供单独清理，保留 PTY、Git diff、历史执行引擎、共享文件返回类型和全部依赖。没有重构沙箱 SID 或 AI 请求取消机制。未做 Windows 安装包运行验证；Vite 仍有大 chunk 提示。

## 2026-09-19：完成可直接删除的冗余清理

范围：继续在 9 月 18 日独立修复版操作，未替换根目录产品源码。

现象/证据：当前 TerminalPanel 使用 pty_*，但旧 openTerminal/runTerminalCommand 的类型、两个 bridge 实现、Tauri 注册命令、terminal_sessions 状态和结果结构仍存在；TerminalPanel 的 bridge 属性未读取；旧终端表单样式及 CodeMirror tooltip 无组件使用。

根因：从命令表单迁移到交互式 PTY 时，只替换了 UI，没有同步撤销旧接口。

修复：移除上述整个旧终端接口链及专属状态/类型，删除不再需要的 validate_command_request 导入（共享执行器本身保留）；移除 TerminalPanel 空转 bridge 属性和调用方传参，删除无入口的旧终端输出/表单/CodeMirror tooltip 样式。

结果：本批 9 个文件净减 286 行；两批合计 19 个代码/样式/测试路径净减 583 行，其中 2 个文件已删除。第一批和第二批分别有 cleanup-before.zip、cleanup-second-before.zip 原件备份；总差异为 cleanup-total.diff。

验证：174 项 Vitest 测试通过；应用与 UI TypeScript 检查通过；workbench/editor 共 22 项 UI 测试通过；cargo test -p local-agent-desktop --lib interactive_terminal::tests --locked --offline 通过 2 项测试，涵盖尺寸校验、ConPTY 变量状态、resize、Ctrl+C 中断后继续执行；前端构建成功。TypeScript 导出引用复查返回空候选，删除的旧终端符号在 src/src-tauri/tests 内无残留。Rust 测试编译仅有链接器输出提示，无编译失败。主 CSS 现为约 85.52 kB（gzip 15.73 kB）。

保留判断：Windows SID 两份实现及 AI 取消逻辑都有真实调用，属于需重构合并的重复实现，不是可直接删的死代码；历史内核脚本仍可手动调用且对应迁移用途，未擅自取消；QueueJournal.notes 属于已保存队列的兼容字段，本轮不改变其持久化格式；历史执行/恢复/审批链、PTY、Git diff、共享执行器和依赖均保留。这里的“清理完成”指已确认能直接删除的冗余，不是声称全项目不存在任何可重构之处。

遗留风险：未验收原生安装包；前端大 chunk 提示仍存在；没有验证仓库外私有 IPC 消费者，依赖已删除旧命令的外部集成需改用支持的入口。没有执行用户数据迁移或删除。

## 2026-10-05：能力读取失败时记忆面板被连带隐藏

范围：线上线下源码统一前的本地审查版。

现象与证据：Agent 设置中的内核能力请求失败后，界面显示“暂时无法读取能力”，但同一对话的本地记忆面板也消失；全量 Playwright 用例 `memory remains readable when native capabilities fail to load` 可稳定复现。

根因：`AgentSettings` 将记忆面板错误地放在 `capabilities` 成功返回的条件分支内。能力目录和本地记忆读取是两个独立请求，前者失败不应阻止后者。

修复：能力请求尚未成功时仍独立挂载 `ProjectMemoryPanel`；只有内核明确返回 `memoryUnavailableReason` 时才隐藏不受支持的记忆入口。同步移除 README 中已经失效的已知问题说明。

验证：项目记忆专项回归通过；全量 75 项 Playwright UI 测试、174 项前端单元测试、TypeScript 检查及生产构建通过。记录不包含用户记忆、项目路径或模型凭据。

遗留风险：能力失败时记忆面板会单独发起本地读取；若记忆读取本身也失败，仍按原有设计显示独立错误和刷新入口，不把失败伪装成空记忆。
