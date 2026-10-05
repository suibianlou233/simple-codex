# Simple Code 工作台

使用 @dtinsight/molecule 1.3.6 的 SplitPane 和 Tabs 组件组合工作台，Monaco Editor 0.31.1 替换 CodeMirror。按组件引入，保留原有文件树、审批和原生桥接，不初始化 Molecule 的全局文件/扩展服务。AI 侧栏使用与 Simple 相同的 assistant-ui 对话组件。

代码模式首次打开时加载工作台，之后保持挂载以保留未保存缓冲。文件栏可拖动调整；标签标识未保存状态，关闭脏文件仍需确认。保存继续携带 expected SHA，外部变化仍须显式处理。项目与任务草稿、IndexedDB 恢复、选区偏移、Ctrl+S / Ctrl+K / F12 / Shift+F12 和本地 TypeScript 导航保留。

Monaco worker 使用 Vite 本地打包。Molecule 的安装脚本仅检查 Node 版本，已配置 allowBuilds=false，使用包内预构建文件。两个依赖均保留许可副本于 public/licenses。

验证环境：React 19、Vite 7、Windows Edge 浏览器和本地 IPC fixtures。原生安装包/WebView2 打包仍须按原项目发布流程验证；本次不修改 Rust 内核。
